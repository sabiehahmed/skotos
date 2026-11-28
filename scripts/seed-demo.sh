#!/usr/bin/env bash
# =============================================================================
# Skotos Demo Data Seeder (Idempotent)
#
# Populates a running local Skotos stack with realistic Phase 1-4 data:
#   - Segment: beta-testers
#   - Flags: checkout-redesign (targeted boolean), search-ranking-v2 (boolean),
#     orders-db-migration (6-stage migration flag), pricing-tier-banner (string)
#   - Metrics, Funnel & Mutual-Exclusion Layer
#   - Running Bayesian A/B Experiment (exp-checkout-redesign) with 160 exposures
#   - Active Guarded Rollout on search-ranking-v2 with healthy telemetry
#   - Migration telemetry (shadow reads, consistency rate, old vs new latency)
#   - Summary usage rollups via live event-ingest
#
# Safe to run repeatedly; existing resources (HTTP 409) are reused cleanly.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
STATE_DIR="${ROOT_DIR}/.skotos-dev"
RELAY_ENVS_FILE="${STATE_DIR}/relay-envs.json"
COOKIE_JAR="$(mktemp)"
trap 'rm -f "${COOKIE_JAR}"' EXIT

if [[ -f "${ROOT_DIR}/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "${ROOT_DIR}/.env"
  set +a
fi

CONTROL_API_URL="http://127.0.0.1:${PORT:-8080}"
EVENT_INGEST_URL="http://127.0.0.1:${SKOTOS_EVENT_INGEST_ADDR##*:}"
[[ "${EVENT_INGEST_URL}" == "http://127.0.0.1:" ]] && EVENT_INGEST_URL="http://127.0.0.1:8082"

ADMIN_EMAIL="${SKOTOS_BOOTSTRAP_ADMIN_EMAIL:-admin@skotos.local}"
ADMIN_PASSWORD="${SKOTOS_BOOTSTRAP_ADMIN_PASSWORD:-SkotosDevPass123!}"

info() { printf '[seed] %s\n' "$*"; }
ok()   { printf '[seed] OK: %s\n' "$*"; }
die()  { printf '[seed] ERROR: %s\n' "$*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v jq >/dev/null 2>&1 || die "jq is required"

api_post_idempotent() {
  # Sends POST; accepts 200, 201, or 409 (already exists). Fails on other statuses.
  local label="$1"
  local path="$2"
  local body="$3"
  local resp_file
  resp_file="$(mktemp)"
  local code
  code="$(curl -sS -o "${resp_file}" -w '%{http_code}' \
    -b "${COOKIE_JAR}" \
    -H "Content-Type: application/json" \
    -X POST -d "${body}" \
    "${CONTROL_API_URL}${path}")"
  if [[ "${code}" == "200" || "${code}" == "201" ]]; then
    ok "Created ${label}"
  elif [[ "${code}" == "409" ]]; then
    info "Already exists (skipped): ${label}"
  else
    cat "${resp_file}" >&2 || true
    rm -f "${resp_file}"
    die "POST ${path} for '${label}' returned HTTP ${code}"
  fi
  rm -f "${resp_file}"
}

info "Signing in to ${CONTROL_API_URL} as ${ADMIN_EMAIL}..."
login_payload="$(jq -nc --arg e "${ADMIN_EMAIL}" --arg p "${ADMIN_PASSWORD}" '{email:$e,password:$p}')"
curl -fsS -c "${COOKIE_JAR}" -H "Content-Type: application/json" \
  -d "${login_payload}" \
  "${CONTROL_API_URL}/api/v1/auth/login" >/dev/null \
  || die "Login failed (is control-api running at ${CONTROL_API_URL}?)"

# Obtain development SDK key (from dev-stack cache or by rotating one)
DEV_SDK_KEY=""
if [[ -f "${RELAY_ENVS_FILE}" ]]; then
  DEV_SDK_KEY="$(jq -r '.[] | select(.name == "development") | .sdkKey // empty' "${RELAY_ENVS_FILE}" 2>/dev/null || true)"
fi
if [[ -z "${DEV_SDK_KEY}" ]]; then
  DEV_SDK_KEY="$(curl -fsS -b "${COOKIE_JAR}" -X POST \
    -H "Content-Type: application/json" \
    -d '{"kind":"sdk","gracePeriodSeconds":3600}' \
    "${CONTROL_API_URL}/api/v1/projects/default/environments/development/keys/rotate" \
    | jq -r '.plaintextKey')"
fi

# 1. Segment: beta-testers
api_post_idempotent "segment 'beta-testers'" \
  "/api/v1/projects/default/environments/development/segments" \
  '{
    "key": "beta-testers",
    "name": "Beta Testers",
    "description": "Internal staff and early-access customers",
    "included": ["user-alice", "user-bob"],
    "rules": [{
      "id": "staff-email-rule",
      "clauses": [{
        "contextKind": "user",
        "attribute": "email",
        "op": "endsWith",
        "values": ["@skotos.local"],
        "negate": false
      }]
    }]
  }'

# 2. Feature flags
api_post_idempotent "flag 'checkout-redesign'" \
  "/api/v1/projects/default/flags" \
  '{
    "key": "checkout-redesign",
    "name": "One-Click Checkout Redesign",
    "description": "Streamlined single-page checkout flow under Bayesian A/B experimentation",
    "kind": "boolean",
    "tags": ["checkout", "experiment"],
    "variations": [
      { "value": false, "name": "Control (Legacy)" },
      { "value": true, "name": "Treatment (One-Click)" }
    ],
    "defaultOnVariationIndex": 1,
    "defaultOffVariationIndex": 0,
    "clientSideAvailability": { "usingEnvironmentId": true, "usingMobileKey": true }
  }'

api_post_idempotent "flag 'search-ranking-v2'" \
  "/api/v1/projects/default/flags" \
  '{
    "key": "search-ranking-v2",
    "name": "Semantic Search Ranking v2",
    "description": "Hybrid BM25 + vector search ranker protected by an automated error-rate guardrail",
    "kind": "boolean",
    "tags": ["search", "guarded-rollout"],
    "variations": [
      { "value": false, "name": "Baseline Ranker" },
      { "value": true, "name": "Hybrid Ranker v2" }
    ],
    "defaultOnVariationIndex": 1,
    "defaultOffVariationIndex": 0,
    "clientSideAvailability": { "usingEnvironmentId": true, "usingMobileKey": true }
  }'

api_post_idempotent "flag 'orders-db-migration'" \
  "/api/v1/projects/default/flags" \
  '{
    "key": "orders-db-migration",
    "name": "Orders Ledger Storage Migration",
    "description": "6-stage zero-downtime migration from legacy monolith table to partitioned ledger",
    "kind": "boolean",
    "tags": ["migration", "platform"],
    "variations": [
      { "value": false, "name": "Legacy Store" },
      { "value": true, "name": "Partitioned Ledger" }
    ],
    "defaultOnVariationIndex": 1,
    "defaultOffVariationIndex": 0,
    "clientSideAvailability": { "usingEnvironmentId": false, "usingMobileKey": false }
  }'

api_post_idempotent "flag 'pricing-tier-banner'" \
  "/api/v1/projects/default/flags" \
  '{
    "key": "pricing-tier-banner",
    "name": "Pricing Tier Banner Variant",
    "description": "Multivariate banner copy for self-serve vs enterprise pricing tiers",
    "kind": "string",
    "tags": ["growth", "web"],
    "variations": [
      { "value": "standard", "name": "Standard Copy" },
      { "value": "enterprise-promo", "name": "Enterprise Promo" },
      { "value": "founders-club", "name": "Founders Club" }
    ],
    "defaultOnVariationIndex": 1,
    "defaultOffVariationIndex": 0,
    "clientSideAvailability": { "usingEnvironmentId": true, "usingMobileKey": true }
  }'

# Enable flags & configure targeting rule on checkout-redesign if not yet enabled
for fk in checkout-redesign search-ranking-v2 orders-db-migration pricing-tier-banner; do
  flag_json="$(curl -fsS -b "${COOKIE_JAR}" "${CONTROL_API_URL}/api/v1/projects/default/flags/${fk}")"
  is_on="$(printf '%s' "${flag_json}" | jq -r '.environments.development.on // false')"
  if [[ "${is_on}" != "true" ]]; then
    if [[ "${fk}" == "checkout-redesign" ]]; then
      has_rule="$(printf '%s' "${flag_json}" | jq -r '(.environments.development.rules // []) | any(.id == "beta-testers-early-access")')"
      if [[ "${has_rule}" == "true" ]]; then
        patch_body='{"environmentKey":"development","comment":"Seed: enable in development","instructions":[{"kind":"turnFlagOn"}]}'
      else
        on_var_id="$(printf '%s' "${flag_json}" | jq -r '.variations[1].id')"
        patch_body="$(jq -nc --arg vid "${on_var_id}" '{
          environmentKey: "development",
          comment: "Seed: turn on with beta-testers targeting rule",
          instructions: [
            { kind: "turnFlagOn" },
            {
              kind: "addRule",
              id: "beta-testers-early-access",
              clauses: [{
                contextKind: "user",
                attribute: "key",
                op: "segmentMatch",
                values: ["beta-testers"],
                negate: false
              }],
              variationId: $vid
            }
          ]
        }')"
      fi
    else
      patch_body='{"environmentKey":"development","comment":"Seed: enable in development","instructions":[{"kind":"turnFlagOn"}]}'
    fi
    curl -fsS -b "${COOKIE_JAR}" -X PATCH \
      -H "Content-Type: application/json" \
      -d "${patch_body}" \
      "${CONTROL_API_URL}/api/v1/projects/default/flags/${fk}" >/dev/null
    ok "Enabled '${fk}' in development"
  else
    info "Already enabled in development (skipped): '${fk}'"
  fi
done

# 3. Configure 6-stage Migration Flag on orders-db-migration
curl -fsS -b "${COOKIE_JAR}" -X PUT \
  -H "Content-Type: application/json" \
  -d '{
    "template": "six_stage",
    "currentStage": "shadow",
    "checkRatio": 1,
    "readExecutionOrder": "parallel"
  }' \
  "${CONTROL_API_URL}/api/v1/projects/default/flags/orders-db-migration/environments/development/migration" >/dev/null
ok "Configured 6-stage migration flag on 'orders-db-migration' (stage: shadow)"

# 4. Metrics, Metric Group (Funnel), and Mutual-Exclusion Layer
api_post_idempotent "metric 'checkout-conversion'" \
  "/api/v1/projects/default/metrics" \
  '{
    "key": "checkout-conversion",
    "name": "Checkout Conversion Rate",
    "kind": "conversion",
    "eventKey": "checkout_completed",
    "successDirection": "higher_is_better",
    "unit": "%"
  }'

api_post_idempotent "metric 'api-error-rate'" \
  "/api/v1/projects/default/metrics" \
  '{
    "key": "api-error-rate",
    "name": "API Error Rate",
    "kind": "error_rate",
    "eventKey": "api_error",
    "successDirection": "lower_is_better",
    "unit": "%"
  }'

api_post_idempotent "metric group 'checkout-funnel'" \
  "/api/v1/projects/default/metric-groups" \
  '{
    "key": "checkout-funnel",
    "name": "Checkout Conversion & Reliability Funnel",
    "kind": "funnel",
    "metricKeys": ["checkout-conversion", "api-error-rate"]
  }'

api_post_idempotent "mutual-exclusion layer 'checkout-surface'" \
  "/api/v1/projects/default/layers" \
  '{
    "key": "checkout-surface",
    "name": "Checkout Surface Layer",
    "description": "Isolates concurrent checkout experiments into disjoint hash buckets"
  }'

# 5. Bayesian A/B Experiment
api_post_idempotent "experiment 'exp-checkout-redesign'" \
  "/api/v1/projects/default/environments/development/experiments" \
  '{
    "key": "exp-checkout-redesign",
    "name": "One-Click Checkout Conversion Lift",
    "hypothesis": "Reducing checkout from 3 steps to 1 click lifts completed checkouts by >= 8%",
    "flagKey": "checkout-redesign",
    "mode": "ab_test",
    "engine": "bayesian",
    "controlVariationIndex": 0,
    "primaryMetricKey": "checkout-conversion",
    "secondaryMetricKeys": ["api-error-rate"],
    "metricGroupKey": "checkout-funnel",
    "layerKey": "checkout-surface",
    "layerReservationBuckets": 25000,
    "allocations": [
      { "variationIndex": 0, "weight": 50000 },
      { "variationIndex": 1, "weight": 50000 }
    ],
    "alpha": 0.05
  }'

exp_status="$(curl -fsS -b "${COOKIE_JAR}" \
  "${CONTROL_API_URL}/api/v1/projects/default/environments/development/experiments/exp-checkout-redesign" \
  | jq -r '.status')"
if [[ "${exp_status}" == "draft" ]]; then
  curl -fsS -b "${COOKIE_JAR}" -X POST \
    "${CONTROL_API_URL}/api/v1/projects/default/environments/development/experiments/exp-checkout-redesign/start" >/dev/null
  ok "Started experiment 'exp-checkout-redesign'"
fi

# 6. Guarded Rollout on search-ranking-v2 (only create if none exists for this flag yet)
existing_gr="$(curl -fsS -b "${COOKIE_JAR}" \
  "${CONTROL_API_URL}/api/v1/projects/default/environments/development/guarded-rollouts" \
  | jq '[.items[] | select(.flagKey == "search-ranking-v2")] | length')"
if [[ "${existing_gr}" == "0" ]]; then
  api_post_idempotent "guarded rollout on 'search-ranking-v2'" \
    "/api/v1/projects/default/environments/development/guarded-rollouts" \
    '{
      "flagKey": "search-ranking-v2",
      "name": "Search Ranking v2 Error-Rate Guardrail",
      "controlVariationIndex": 0,
      "treatmentVariationIndex": 1,
      "currentTrafficPct": 25,
      "autoRollback": true,
      "guardrails": [{
        "metricKey": "api-error-rate",
        "maxRegressionPct": 5.0,
        "minSampleSize": 30
      }]
    }'
else
  info "Already exists (skipped): guarded rollout on 'search-ranking-v2'"
fi

# 7. Seed realistic live telemetry through event-ingest (idempotent via X-Skotos-Payload-ID)
info "Sending demo exposures, conversions, rollups, and migration telemetry to ${EVENT_INGEST_URL}..."
now_ms="$(( $(date +%s) * 1000 ))"
start_ms="$(( now_ms - 60000 ))"

telemetry_payload="$(jq -nc --argjson now "${now_ms}" --argjson start "${start_ms}" '
  [
    {
      kind: "summary",
      startDate: $start,
      endDate: $now,
      features: {
        "checkout-redesign": {
          default: false,
          contextKinds: ["user"],
          counters: [
            { variation: 0, version: 2, value: false, count: 80 },
            { variation: 1, version: 2, value: true, count: 80 }
          ]
        },
        "search-ranking-v2": {
          default: false,
          contextKinds: ["user"],
          counters: [
            { variation: 0, version: 2, value: false, count: 60 },
            { variation: 1, version: 2, value: true, count: 40 }
          ]
        },
        "pricing-tier-banner": {
          default: "standard",
          contextKinds: ["user"],
          counters: [
            { variation: 0, version: 2, value: "standard", count: 45 },
            { variation: 1, version: 2, value: "enterprise-promo", count: 55 }
          ]
        }
      }
    }
  ]
  + [range(0; 80) as $i |
      {
        kind: "feature",
        creationDate: ($now - 10000),
        key: "checkout-redesign",
        variation: 0,
        value: false,
        version: 2,
        contextKeys: { user: ("seed-ctrl-" + ($i | tostring)) }
      },
      (if $i < 12 then {
        kind: "custom",
        creationDate: ($now - 9000),
        key: "checkout_completed",
        contextKeys: { user: ("seed-ctrl-" + ($i | tostring)) },
        metricValue: 1.0
      } else empty end),
      (if $i < 2 then {
        kind: "custom",
        creationDate: ($now - 8500),
        key: "api_error",
        contextKeys: { user: ("seed-ctrl-" + ($i | tostring)) },
        metricValue: 1.0
      } else empty end),
      {
        kind: "feature",
        creationDate: ($now - 10000),
        key: "checkout-redesign",
        variation: 1,
        value: true,
        version: 2,
        contextKeys: { user: ("seed-trt-" + ($i | tostring)) }
      },
      (if $i < 34 then {
        kind: "custom",
        creationDate: ($now - 9000),
        key: "checkout_completed",
        contextKeys: { user: ("seed-trt-" + ($i | tostring)) },
        metricValue: 1.0
      } else empty end),
      (if $i < 1 then {
        kind: "custom",
        creationDate: ($now - 8500),
        key: "api_error",
        contextKeys: { user: ("seed-trt-" + ($i | tostring)) },
        metricValue: 1.0
      } else empty end)
    ]
  + [range(0; 40) as $i |
      {
        kind: "feature",
        creationDate: ($now - 10000),
        key: "search-ranking-v2",
        variation: 0,
        value: false,
        version: 2,
        contextKeys: { user: ("seed-search-ctrl-" + ($i | tostring)) }
      },
      {
        kind: "feature",
        creationDate: ($now - 10000),
        key: "search-ranking-v2",
        variation: 1,
        value: true,
        version: 2,
        contextKeys: { user: ("seed-search-trt-" + ($i | tostring)) }
      },
      (if $i == 0 then {
        kind: "custom",
        creationDate: ($now - 8500),
        key: "api_error",
        contextKeys: { user: ("seed-search-ctrl-" + ($i | tostring)) },
        metricValue: 1.0
      } else empty end),
      (if $i == 0 then {
        kind: "custom",
        creationDate: ($now - 8500),
        key: "api_error",
        contextKeys: { user: ("seed-search-trt-" + ($i | tostring)) },
        metricValue: 1.0
      } else empty end)
    ]
  + [range(0; 25) as $i |
      {
        kind: "migration_op",
        creationDate: ($now - 5000),
        operation: "read",
        key: "orders-db-migration",
        stage: "shadow",
        oldInvoked: true,
        newInvoked: true,
        consistent: ($i < 24),
        oldLatencyMs: 18.4,
        newLatencyMs: 6.2,
        oldError: false,
        newError: false
      }
    ]
')"

ingest_code="$(curl -sS -o /dev/null -w '%{http_code}' \
  -X POST \
  -H "Authorization: ${DEV_SDK_KEY}" \
  -H "Content-Type: application/json" \
  -H "X-Skotos-Payload-ID: skotos-seed-demo-v1" \
  -d "${telemetry_payload}" \
  "${EVENT_INGEST_URL}/events/v1/bulk")"
[[ "${ingest_code}" == "202" ]] || die "event-ingest returned HTTP ${ingest_code}"
ok "Demo telemetry ingested (HTTP 202; idempotent payload ID 'skotos-seed-demo-v1')"

printf '\n[seed] Demo environment ready! Sign in at http://127.0.0.1:%s with %s / %s\n' \
  "${SKOTOS_WEB_PORT:-5173}" "${ADMIN_EMAIL}" "${ADMIN_PASSWORD}"
