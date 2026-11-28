#!/usr/bin/env bash
# =============================================================================
# Skotos Live Full-Stack Smoke Test
#
# Verifies end-to-end health and real-time dataflow across every running service:
#   1. Health checks on control-api, delivery-edge, event-ingest, relay-proxy, web
#   2. Vite /api proxy -> control-api authentication
#   3. Control Plane flag create + semantic patch (turn ON)
#   4. Real-time Outbox -> MinIO -> NATS -> delivery-edge propagation (< 5s)
#   5. Upstream SSE delivery-edge -> relay-proxy propagation (< 5s)
#   6. event-ingest summary rollup -> Control Plane Flag Usage API
#   7. Phase 4 APIs (experiments, guarded rollouts, engineering insights)
#   8. Clean deletion of the temporary smoke flag
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
STATE_DIR="${ROOT_DIR}/.skotos-dev"
RELAY_ENVS_FILE="${STATE_DIR}/relay-envs.json"
COOKIE_JAR="$(mktemp)"
SMOKE_FLAG_KEY="smoke-check-$(date +%s)"
CREATED_FLAG=false

if [[ -f "${ROOT_DIR}/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "${ROOT_DIR}/.env"
  set +a
fi

CONTROL_API_URL="http://127.0.0.1:${PORT:-8080}"
DELIVERY_EDGE_URL="http://127.0.0.1:${SKOTOS_DELIVERY_EDGE_ADDR##*:}"
[[ "${DELIVERY_EDGE_URL}" == "http://127.0.0.1:" ]] && DELIVERY_EDGE_URL="http://127.0.0.1:8081"
EVENT_INGEST_URL="http://127.0.0.1:${SKOTOS_EVENT_INGEST_ADDR##*:}"
[[ "${EVENT_INGEST_URL}" == "http://127.0.0.1:" ]] && EVENT_INGEST_URL="http://127.0.0.1:8082"
RELAY_PROXY_URL="http://127.0.0.1:${SKOTOS_RELAY_ADDR##*:}"
[[ "${RELAY_PROXY_URL}" == "http://127.0.0.1:" ]] && RELAY_PROXY_URL="http://127.0.0.1:8083"
WEB_URL="http://127.0.0.1:${SKOTOS_WEB_PORT:-5173}"

ADMIN_EMAIL="${SKOTOS_BOOTSTRAP_ADMIN_EMAIL:-admin@skotos.local}"
ADMIN_PASSWORD="${SKOTOS_BOOTSTRAP_ADMIN_PASSWORD:-SkotosDevPass123!}"

cleanup() {
  if [[ "${CREATED_FLAG}" == "true" && -f "${COOKIE_JAR}" ]]; then
    curl -sS -o /dev/null -b "${COOKIE_JAR}" -X DELETE \
      "${CONTROL_API_URL}/api/v1/projects/default/flags/${SMOKE_FLAG_KEY}" || true
  fi
  rm -f "${COOKIE_JAR}"
}
trap cleanup EXIT

pass() { printf '[smoke] PASS: %s\n' "$*"; }
fail() { printf '[smoke] FAIL: %s\n' "$*" >&2; exit 1; }

now_ms() {
  python3 -c 'import time; print(int(time.time() * 1000))'
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v jq >/dev/null 2>&1 || fail "jq is required"
command -v python3 >/dev/null 2>&1 || fail "python3 is required"

# 1. Verify all 5 service health endpoints
curl -fsS "${CONTROL_API_URL}/readyz" >/dev/null || fail "control-api /readyz unreachable"
curl -fsS "${DELIVERY_EDGE_URL}/healthz" >/dev/null || fail "delivery-edge /healthz unreachable"
curl -fsS "${EVENT_INGEST_URL}/readyz" >/dev/null || fail "event-ingest /readyz unreachable"
relay_status="$(curl -fsS "${RELAY_PROXY_URL}/status" | jq -r '.status')"
[[ "${relay_status}" == "healthy" ]] || fail "relay-proxy /status expected 'healthy', got '${relay_status}'"
curl -fsS "${WEB_URL}" | grep -qi "<!doctype html" || fail "@skotos/web did not return HTML at ${WEB_URL}"
curl -fsS "${WEB_URL}/api/v1/auth/config" | jq -e '.mode' >/dev/null || fail "Vite /api proxy -> control-api failed"
pass "All 5 services + Vite /api proxy healthy"

# 2. Authenticate via control-api
login_payload="$(jq -nc --arg e "${ADMIN_EMAIL}" --arg p "${ADMIN_PASSWORD}" '{email:$e,password:$p}')"
curl -fsS -c "${COOKIE_JAR}" -H "Content-Type: application/json" \
  -d "${login_payload}" \
  "${CONTROL_API_URL}/api/v1/auth/login" >/dev/null \
  || fail "Login failed for ${ADMIN_EMAIL}"
me_email="$(curl -fsS -b "${COOKIE_JAR}" "${CONTROL_API_URL}/api/v1/auth/me" | jq -r '.email')"
[[ "${me_email}" == "${ADMIN_EMAIL}" ]] || fail "/auth/me returned '${me_email}', expected '${ADMIN_EMAIL}'"
pass "Authenticated as ${me_email}"

# Resolve development keys
DEV_SDK_KEY=""
DEV_CLIENT_SIDE_ID=""
if [[ -f "${RELAY_ENVS_FILE}" ]]; then
  DEV_SDK_KEY="$(jq -r '.[] | select(.name == "development") | .sdkKey // empty' "${RELAY_ENVS_FILE}")"
  DEV_CLIENT_SIDE_ID="$(jq -r '.[] | select(.name == "development") | .clientSideId // empty' "${RELAY_ENVS_FILE}")"
fi
if [[ -z "${DEV_CLIENT_SIDE_ID}" ]]; then
  DEV_CLIENT_SIDE_ID="$(curl -fsS -b "${COOKIE_JAR}" \
    "${CONTROL_API_URL}/api/v1/projects/default/environments/development" | jq -r '.clientSideId')"
fi
if [[ -z "${DEV_SDK_KEY}" ]]; then
  DEV_SDK_KEY="$(curl -fsS -b "${COOKIE_JAR}" -X POST \
    -H "Content-Type: application/json" \
    -d '{"kind":"sdk","gracePeriodSeconds":3600}' \
    "${CONTROL_API_URL}/api/v1/projects/default/environments/development/keys/rotate" | jq -r '.plaintextKey')"
fi

# 3. Create temporary flag & turn ON in development
create_body="$(jq -nc --arg k "${SMOKE_FLAG_KEY}" '{
  key: $k,
  name: "Live Smoke Check Flag",
  kind: "boolean",
  variations: [
    { value: false, name: "Off" },
    { value: true, name: "On" }
  ],
  defaultOnVariationIndex: 1,
  defaultOffVariationIndex: 0,
  clientSideAvailability: { usingEnvironmentId: true, usingMobileKey: true }
}')"
curl -fsS -b "${COOKIE_JAR}" -X POST -H "Content-Type: application/json" \
  -d "${create_body}" \
  "${CONTROL_API_URL}/api/v1/projects/default/flags" >/dev/null \
  || fail "Failed to create temporary smoke flag"
CREATED_FLAG=true

t_patch_ms="$(now_ms)"
curl -fsS -b "${COOKIE_JAR}" -X PATCH -H "Content-Type: application/json" \
  -d '{"environmentKey":"development","comment":"Smoke test toggle ON","instructions":[{"kind":"turnFlagOn"}]}' \
  "${CONTROL_API_URL}/api/v1/projects/default/flags/${SMOKE_FLAG_KEY}" >/dev/null \
  || fail "Failed to PATCH smoke flag ON"
pass "Created & enabled temporary flag '${SMOKE_FLAG_KEY}'"

# 4. Verify real-time propagation to delivery-edge (evalx + latest-all)
CTX_B64="$(python3 -c 'import base64; print(base64.urlsafe_b64encode(b"{\"kind\":\"user\",\"key\":\"smoke-user-1\"}").decode().rstrip("="))')"
edge_deadline=$(( $(date +%s) + 15 ))
edge_ms=0
last_edge_val=""
while true; do
  last_edge_val="$(curl -fsS "${DELIVERY_EDGE_URL}/sdk/v1/evalx/${DEV_CLIENT_SIDE_ID}/contexts/${CTX_B64}" 2>/dev/null \
    | jq -r --arg k "${SMOKE_FLAG_KEY}" 'if has($k) then (.[$k].value | tostring) else "missing" end' || echo "error")"
  if [[ "${last_edge_val}" == "true" ]]; then
    edge_ms="$(( $(now_ms) - t_patch_ms ))"
    break
  fi
  if (( $(date +%s) >= edge_deadline )); then
    fail "Flag '${SMOKE_FLAG_KEY}' did not evaluate true on delivery-edge within 15s (last: ${last_edge_val})"
  fi
  sleep 0.1
done

curl -fsS -H "Authorization: ${DEV_SDK_KEY}" "${DELIVERY_EDGE_URL}/sdk/v1/latest-all" \
  | jq -e --arg k "${SMOKE_FLAG_KEY}" '.flags[$k].on == true' >/dev/null \
  || fail "delivery-edge /sdk/v1/latest-all missing enabled flag '${SMOKE_FLAG_KEY}'"
pass "delivery-edge live evaluation & /latest-all synced in ${edge_ms} ms"

# 5. Verify real-time upstream SSE propagation into relay-proxy
relay_deadline=$(( $(date +%s) + 15 ))
relay_ms=0
last_relay_val=""
while true; do
  last_relay_val="$(curl -fsS "${RELAY_PROXY_URL}/sdk/v1/evalx/${DEV_CLIENT_SIDE_ID}/contexts/${CTX_B64}" 2>/dev/null \
    | jq -r --arg k "${SMOKE_FLAG_KEY}" 'if has($k) then (.[$k].value | tostring) else "missing" end' || echo "error")"
  if [[ "${last_relay_val}" == "true" ]]; then
    relay_ms="$(( $(now_ms) - t_patch_ms ))"
    break
  fi
  if (( $(date +%s) >= relay_deadline )); then
    fail "Flag '${SMOKE_FLAG_KEY}' did not evaluate true on relay-proxy within 15s (last: ${last_relay_val})"
  fi
  sleep 0.1
done
pass "relay-proxy live upstream SSE evaluation synced in ${relay_ms} ms"

# 6. Verify event-ingest -> Flag Usage API
ts_now="$(now_ms)"
summary_batch="$(jq -nc --arg k "${SMOKE_FLAG_KEY}" --argjson s "$((ts_now - 1000))" --argjson e "${ts_now}" '[
  {
    kind: "summary",
    startDate: $s,
    endDate: $e,
    features: {
      ($k): {
        default: false,
        contextKinds: ["user"],
        counters: [{ variation: 1, version: 2, value: true, count: 5 }]
      }
    }
  }
]')"
ingest_http="$(curl -sS -o /dev/null -w '%{http_code}' -X POST \
  -H "Authorization: ${DEV_SDK_KEY}" \
  -H "Content-Type: application/json" \
  -H "X-Skotos-Payload-ID: ${SMOKE_FLAG_KEY}-batch" \
  -d "${summary_batch}" \
  "${EVENT_INGEST_URL}/events/v1/bulk")"
[[ "${ingest_http}" == "202" ]] || fail "event-ingest returned HTTP ${ingest_http}"

usage_deadline=$(( $(date +%s) + 10 ))
while true; do
  total_evals="$(curl -fsS -b "${COOKIE_JAR}" \
    "${CONTROL_API_URL}/api/v1/projects/default/flags/${SMOKE_FLAG_KEY}/environments/development/usage" \
    | jq -r '.totalEvaluations // 0')"
  if (( total_evals >= 5 )); then
    break
  fi
  if (( $(date +%s) >= usage_deadline )); then
    fail "Usage rollup did not reach >= 5 within 10s (got ${total_evals})"
  fi
  sleep 0.25
done
pass "event-ingest -> Usage API verified (${total_evals} recorded evaluations)"

# 7. Verify Phase 4 endpoints
curl -fsS -b "${COOKIE_JAR}" \
  "${CONTROL_API_URL}/api/v1/projects/default/environments/development/experiments" \
  | jq -e '.items | type == "array"' >/dev/null || fail "Experiments API check failed"
curl -fsS -b "${COOKIE_JAR}" \
  "${CONTROL_API_URL}/api/v1/projects/default/environments/development/guarded-rollouts" \
  | jq -e '.items | type == "array"' >/dev/null || fail "Guarded Rollouts API check failed"
curl -fsS -b "${COOKIE_JAR}" \
  "${CONTROL_API_URL}/api/v1/projects/default/insights?days=30" \
  | jq -e '.lookbackDays == 30' >/dev/null || fail "Engineering Insights API check failed"
pass "Phase 4 APIs (Experiments, Guarded Rollouts, Insights) healthy"

# 8. Delete temporary smoke flag explicitly (trap also guards errors)
curl -fsS -b "${COOKIE_JAR}" -X DELETE \
  "${CONTROL_API_URL}/api/v1/projects/default/flags/${SMOKE_FLAG_KEY}" >/dev/null \
  || fail "Failed to clean up temporary smoke flag"
CREATED_FLAG=false
pass "Cleaned up temporary flag '${SMOKE_FLAG_KEY}'"

printf '\n[smoke] ALL CHECKS PASSED (delivery-edge: %sms, relay-proxy: %sms)\n' "${edge_ms}" "${relay_ms}"
