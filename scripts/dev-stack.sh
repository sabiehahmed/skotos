#!/usr/bin/env bash
# =============================================================================
# Skotos Local Full-Stack Orchestrator
#
# Starts/stops the complete Skotos stack locally:
#   - Backing infra (Docker Compose): PostgreSQL 16, NATS JetStream, MinIO,
#     plus optional ClickHouse (--analytics)
#   - Rust services: control-api (:8080), delivery-edge (:8081),
#     event-ingest (:8082), relay-proxy (:8083)
#   - Management Dashboard SPA: @skotos/web (:5173)
#
# Usage:
#   scripts/dev-stack.sh up [--analytics] [--seed] [--smoke] [--no-build] [--foreground]
#   scripts/dev-stack.sh down [--infra] [--clean]
#   scripts/dev-stack.sh restart [--analytics] [--seed] [--smoke] [--no-build]
#   scripts/dev-stack.sh status
#   scripts/dev-stack.sh logs [control-api|delivery-edge|event-ingest|relay-proxy|web]
#   scripts/dev-stack.sh seed
#   scripts/dev-stack.sh smoke
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
STATE_DIR="${ROOT_DIR}/.skotos-dev"
PID_DIR="${STATE_DIR}/pids"
LOG_DIR="${STATE_DIR}/logs"
RELAY_ENVS_FILE="${STATE_DIR}/relay-envs.json"
COOKIE_JAR="${STATE_DIR}/session.cookie"

# Colors when attached to a TTY
if [[ -t 1 ]]; then
  C_RESET=$'\033[0m'
  C_BOLD=$'\033[1m'
  C_GREEN=$'\033[32m'
  C_YELLOW=$'\033[33m'
  C_RED=$'\033[31m'
  C_BLUE=$'\033[34m'
  C_DIM=$'\033[2m'
else
  C_RESET=""
  C_BOLD=""
  C_GREEN=""
  C_YELLOW=""
  C_RED=""
  C_BLUE=""
  C_DIM=""
fi

info()  { printf '%s[skotos]%s %s\n' "${C_BLUE}${C_BOLD}" "${C_RESET}" "$*"; }
ok()    { printf '%s[  ok  ]%s %s\n' "${C_GREEN}${C_BOLD}" "${C_RESET}" "$*"; }
warn()  { printf '%s[ warn ]%s %s\n' "${C_YELLOW}${C_BOLD}" "${C_RESET}" "$*" >&2; }
err()   { printf '%s[ fail ]%s %s\n' "${C_RED}${C_BOLD}" "${C_RESET}" "$*" >&2; }
die()   { err "$*"; exit 1; }

usage() {
  cat <<'EOF'
Skotos Local Full-Stack Launcher

Usage:
  scripts/dev-stack.sh up [options]       Start infra + all services + web dashboard
  scripts/dev-stack.sh down [options]     Stop app services (optionally Docker infra/volumes)
  scripts/dev-stack.sh restart [options]  Restart app services (passes options to `up`)
  scripts/dev-stack.sh status             Show health status of infra and all services
  scripts/dev-stack.sh logs [service]     Tail logs (default: all services)
  scripts/dev-stack.sh seed               Seed realistic demo flags, experiments & telemetry
  scripts/dev-stack.sh smoke              Run live end-to-end verification across all services

Options for `up` / `restart`:
  --analytics    Also start ClickHouse (:8123) and enable CLICKHOUSE_URL backend
  --seed         Run scripts/seed-demo.sh after the stack is healthy
  --smoke        Run scripts/smoke-test.sh after the stack is healthy
  --no-build     Skip `cargo build` if binaries already exist under target/debug
  --foreground   Keep attached and gracefully stop all app services on SIGINT/SIGTERM

Options for `down`:
  --infra        Also stop Docker Compose backing containers (preserves volumes)
  --clean        Stop Docker containers, remove volumes (-v), and wipe .skotos-dev/
EOF
}

require_cmd() {
  local cmd="$1"
  command -v "${cmd}" >/dev/null 2>&1 || die "Required tool not found on PATH: ${cmd}"
}

ensure_env() {
  cd "${ROOT_DIR}"
  if [[ ! -f "${ROOT_DIR}/.env" ]]; then
    cp "${ROOT_DIR}/.env.example" "${ROOT_DIR}/.env"
    info "Created .env from .env.example"
  fi
  set -a
  # shellcheck disable=SC1091
  source "${ROOT_DIR}/.env"
  set +a

  export CONTROL_API_PORT="${PORT:-8080}"
  export DELIVERY_EDGE_PORT="${SKOTOS_DELIVERY_EDGE_ADDR##*:}"
  export EVENT_INGEST_PORT="${SKOTOS_EVENT_INGEST_ADDR##*:}"
  export RELAY_PROXY_PORT="${SKOTOS_RELAY_ADDR##*:}"
  export WEB_PORT="${SKOTOS_WEB_PORT:-5173}"

  export CONTROL_API_URL="http://127.0.0.1:${CONTROL_API_PORT}"
  export DELIVERY_EDGE_URL="http://127.0.0.1:${DELIVERY_EDGE_PORT}"
  export EVENT_INGEST_URL="http://127.0.0.1:${EVENT_INGEST_PORT}"
  export RELAY_PROXY_URL="http://127.0.0.1:${RELAY_PROXY_PORT}"
  export WEB_URL="http://127.0.0.1:${WEB_PORT}"
}

pid_is_running() {
  local pid="$1"
  [[ -n "${pid}" ]] && kill -0 "${pid}" 2>/dev/null
}

read_pid() {
  local name="$1"
  local file="${PID_DIR}/${name}.pid"
  if [[ -f "${file}" ]]; then
    tr -d '[:space:]' < "${file}"
  fi
}

service_running() {
  local name="$1"
  local pid
  pid="$(read_pid "${name}")"
  if [[ -n "${pid}" ]] && pid_is_running "${pid}"; then
    return 0
  fi
  return 1
}

# Print PIDs of every descendant of the given PID (children, grandchildren, ...).
# Uses `pgrep -P`, available on both Linux (procps) and macOS.
descendant_pids() {
  local parent="$1"
  local child
  command -v pgrep >/dev/null 2>&1 || return 0
  for child in $(pgrep -P "${parent}" 2>/dev/null || true); do
    descendant_pids "${child}"
    printf '%s\n' "${child}"
  done
}

# Send a signal to a service: its process group (when it leads one), every
# descendant, and the PID itself. Covering descendants matters for wrappers such
# as `pnpm exec vite`, where the real server is a child that survives if only the
# wrapper is signalled.
signal_service_tree() {
  local sig="$1"
  local pid="$2"
  shift 2
  local p
  kill "-${sig}" -- "-${pid}" 2>/dev/null || true
  for p in "$@" "${pid}"; do
    kill "-${sig}" "${p}" 2>/dev/null || true
  done
}

any_pid_running() {
  local p
  for p in "$@"; do
    if pid_is_running "${p}"; then
      return 0
    fi
  done
  return 1
}

stop_service() {
  local name="$1"
  local file="${PID_DIR}/${name}.pid"
  local pid
  pid="$(read_pid "${name}")"
  if [[ -z "${pid}" ]]; then
    rm -f "${file}"
    return 0
  fi
  if ! pid_is_running "${pid}"; then
    rm -f "${file}"
    return 0
  fi

  info "Stopping ${name} (pid ${pid})..."
  # Snapshot descendants before signalling: once the parent exits they are
  # re-parented to init/launchd and can no longer be found via the parent PID.
  local descendants=()
  local d
  for d in $(descendant_pids "${pid}"); do
    descendants+=("${d}")
  done

  signal_service_tree TERM "${pid}" ${descendants[@]+"${descendants[@]}"}

  local waited=0
  while any_pid_running "${pid}" ${descendants[@]+"${descendants[@]}"} && (( waited < 40 )); do
    sleep 0.25
    waited=$((waited + 1))
  done
  if any_pid_running "${pid}" ${descendants[@]+"${descendants[@]}"}; then
    warn "${name} did not exit cleanly within 10s; sending SIGKILL"
    signal_service_tree KILL "${pid}" ${descendants[@]+"${descendants[@]}"}
  fi
  rm -f "${file}"
}

# Fallback for a dev server left behind by an older dev-stack.sh, or one whose
# PID file is gone: stop whatever listens on the port, but only when that
# process runs from this repository (so another project's server is untouched).
stop_port_listener_from_repo() {
  local name="$1"
  local port="$2"
  local pid cwd
  command -v lsof >/dev/null 2>&1 || return 0
  for pid in $(lsof -nP -t -iTCP:"${port}" -sTCP:LISTEN 2>/dev/null || true); do
    cwd="$(lsof -a -p "${pid}" -d cwd -Fn 2>/dev/null | sed -n 's/^n//p' | head -n1)"
    case "${cwd}" in
      "${ROOT_DIR}"|"${ROOT_DIR}"/*)
        info "Stopping leftover ${name} listener on :${port} (pid ${pid})..."
        kill -TERM "${pid}" 2>/dev/null || true
        local waited=0
        while pid_is_running "${pid}" && (( waited < 40 )); do
          sleep 0.25
          waited=$((waited + 1))
        done
        if pid_is_running "${pid}"; then
          kill -KILL "${pid}" 2>/dev/null || true
        fi
        ;;
      *)
        warn "Port ${port} is held by pid ${pid} outside this repo (${cwd:-unknown cwd}); leaving it alone"
        ;;
    esac
  done
}

start_detached() {
  local name="$1"
  shift
  local log_file="${LOG_DIR}/${name}.log"
  local pid_file="${PID_DIR}/${name}.pid"

  if service_running "${name}"; then
    ok "${name} already running (pid $(read_pid "${name}"))"
    return 0
  fi

  : > "${log_file}"
  if command -v setsid >/dev/null 2>&1; then
    setsid "$@" </dev/null >>"${log_file}" 2>&1 &
  else
    # macOS has no setsid(1). Enable job control so the background job gets its
    # own process group (pgid == pid), which stop_service can then signal whole.
    set -m
    nohup "$@" </dev/null >>"${log_file}" 2>&1 &
    set +m
  fi
  local pid=$!
  printf '%s\n' "${pid}" > "${pid_file}"
}

wait_for_http() {
  local name="$1"
  local url="$2"
  local timeout_secs="${3:-60}"
  local deadline=$(( $(date +%s) + timeout_secs ))

  while true; do
    if ! service_running "${name}"; then
      err "${name} exited unexpectedly during startup! Last 40 lines of ${LOG_DIR}/${name}.log:"
      tail -n 40 "${LOG_DIR}/${name}.log" >&2 || true
      exit 1
    fi
    if curl -fsS --max-time 2 "${url}" >/dev/null 2>&1; then
      ok "${name} healthy (${url})"
      return 0
    fi
    if (( $(date +%s) >= deadline )); then
      err "Timed out waiting for ${name} at ${url}. Last 40 lines of ${LOG_DIR}/${name}.log:"
      tail -n 40 "${LOG_DIR}/${name}.log" >&2 || true
      exit 1
    fi
    sleep 0.3
  done
}

provision_relay_keys() {
  # Checks whether cached .skotos-dev/relay-envs.json works against live delivery-edge;
  # otherwise logs into control-api and rotates SDK/mobile keys for development & production.
  if [[ -f "${RELAY_ENVS_FILE}" ]]; then
    local cached_dev_sdk
    cached_dev_sdk="$(jq -r '.[] | select(.name == "development") | .sdkKey // empty' "${RELAY_ENVS_FILE}" 2>/dev/null || true)"
    if [[ -n "${cached_dev_sdk}" ]]; then
      local code
      code="$(curl -sS -o /dev/null -w '%{http_code}' -H "Authorization: ${cached_dev_sdk}" "${DELIVERY_EDGE_URL}/sdk/v1/latest-all" || echo "000")"
      if [[ "${code}" == "200" ]]; then
        ok "Reusing valid local SDK/Relay keys from .skotos-dev/relay-envs.json"
        return 0
      fi
    fi
  fi

  info "Provisioning local SDK, Mobile, and Client-Side keys for Relay Proxy & smoke tests..."
  rm -f "${COOKIE_JAR}"
  local email="${SKOTOS_BOOTSTRAP_ADMIN_EMAIL:-admin@skotos.local}"
  local password="${SKOTOS_BOOTSTRAP_ADMIN_PASSWORD:-SkotosDevPass123!}"

  local login_payload
  login_payload="$(jq -nc --arg e "${email}" --arg p "${password}" '{email:$e,password:$p}')"
  curl -fsS -c "${COOKIE_JAR}" -H "Content-Type: application/json" \
    -d "${login_payload}" \
    "${CONTROL_API_URL}/api/v1/auth/login" >/dev/null \
    || die "Failed to sign in to control-api as ${email}"

  local envs_json
  envs_json="$(curl -fsS -b "${COOKIE_JAR}" "${CONTROL_API_URL}/api/v1/projects/default/environments")"

  local relay_entries="[]"
  local env_key
  for env_key in development production; do
    local csid
    csid="$(printf '%s' "${envs_json}" | jq -r --arg k "${env_key}" '.items[] | select(.key == $k) | .clientSideId // empty')"
    if [[ -z "${csid}" ]]; then
      warn "Environment '${env_key}' not found in default project; skipping for relay-proxy"
      continue
    fi
    local sdk_rot mob_rot sdk_key mob_key
    sdk_rot="$(curl -fsS -b "${COOKIE_JAR}" -X POST \
      -H "Content-Type: application/json" \
      -d '{"kind":"sdk","gracePeriodSeconds":3600}' \
      "${CONTROL_API_URL}/api/v1/projects/default/environments/${env_key}/keys/rotate")"
    sdk_key="$(printf '%s' "${sdk_rot}" | jq -r '.plaintextKey')"

    mob_rot="$(curl -fsS -b "${COOKIE_JAR}" -X POST \
      -H "Content-Type: application/json" \
      -d '{"kind":"mobile","gracePeriodSeconds":3600}' \
      "${CONTROL_API_URL}/api/v1/projects/default/environments/${env_key}/keys/rotate")"
    mob_key="$(printf '%s' "${mob_rot}" | jq -r '.plaintextKey')"

    relay_entries="$(printf '%s' "${relay_entries}" | jq \
      --arg name "${env_key}" \
      --arg sdkKey "${sdk_key}" \
      --arg clientSideId "${csid}" \
      --arg mobileKey "${mob_key}" \
      '. + [{name:$name, sdkKey:$sdkKey, clientSideId:$clientSideId, mobileKey:$mobileKey}]')"
  done

  umask 077
  printf '%s\n' "${relay_entries}" > "${RELAY_ENVS_FILE}"
  chmod 600 "${RELAY_ENVS_FILE}"
  rm -f "${COOKIE_JAR}"

  # Wait until OutboxWorker -> MinIO -> NATS propagates the newly rotated SDK key to delivery-edge
  local dev_sdk
  dev_sdk="$(jq -r '.[] | select(.name == "development") | .sdkKey' "${RELAY_ENVS_FILE}")"
  local deadline=$(( $(date +%s) + 15 ))
  while true; do
    local code
    code="$(curl -sS -o /dev/null -w '%{http_code}' -H "Authorization: ${dev_sdk}" "${DELIVERY_EDGE_URL}/sdk/v1/latest-all" || echo "000")"
    if [[ "${code}" == "200" ]]; then
      ok "Provisioned & synced local environment keys to delivery-edge"
      return 0
    fi
    if (( $(date +%s) >= deadline )); then
      die "Timed out waiting for rotated SDK key to propagate to delivery-edge (last HTTP ${code})"
    fi
    sleep 0.25
  done
}

wait_for_relay_healthy() {
  local timeout_secs="${1:-30}"
  local deadline=$(( $(date +%s) + timeout_secs ))
  while true; do
    if ! service_running "relay-proxy"; then
      err "relay-proxy exited unexpectedly! Last 40 lines of ${LOG_DIR}/relay-proxy.log:"
      tail -n 40 "${LOG_DIR}/relay-proxy.log" >&2 || true
      exit 1
    fi
    local status_val
    status_val="$(curl -fsS --max-time 2 "${RELAY_PROXY_URL}/status" 2>/dev/null | jq -r '.status // empty' || true)"
    if [[ "${status_val}" == "healthy" ]]; then
      ok "relay-proxy healthy and streaming from delivery-edge (${RELAY_PROXY_URL})"
      return 0
    fi
    if (( $(date +%s) >= deadline )); then
      err "relay-proxy did not become healthy within ${timeout_secs}s. Status output:"
      curl -sS "${RELAY_PROXY_URL}/status" >&2 || true
      echo "" >&2
      tail -n 40 "${LOG_DIR}/relay-proxy.log" >&2 || true
      exit 1
    fi
    sleep 0.3
  done
}

cmd_up() {
  local with_analytics=false
  local with_seed=false
  local with_smoke=false
  local no_build=false
  local foreground=false

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --analytics) with_analytics=true ;;
      --seed) with_seed=true ;;
      --smoke) with_smoke=true ;;
      --no-build) no_build=true ;;
      --foreground) foreground=true ;;
      -h|--help) usage; exit 0 ;;
      *) die "Unknown option for 'up': $1" ;;
    esac
    shift
  done

  require_cmd docker
  require_cmd cargo
  require_cmd pnpm
  require_cmd curl
  require_cmd jq

  mkdir -p "${PID_DIR}" "${LOG_DIR}"
  ensure_env

  # 1. Start Docker Compose backing infrastructure
  local compose_args=()
  if [[ "${with_analytics}" == "true" ]]; then
    compose_args=(--profile analytics)
    export CLICKHOUSE_URL="${CLICKHOUSE_URL:-http://127.0.0.1:${CLICKHOUSE_HTTP_PORT:-8123}}"
    export CLICKHOUSE_DATABASE="${CLICKHOUSE_DATABASE:-skotos}"
    export CLICKHOUSE_USER="${CLICKHOUSE_USER:-default}"
    info "Starting Docker backing infra (PostgreSQL 16, NATS JetStream, MinIO, ClickHouse)..."
  else
    info "Starting Docker backing infra (PostgreSQL 16, NATS JetStream, MinIO)..."
  fi
  docker compose "${compose_args[@]}" up -d --wait --wait-timeout 90
  ok "Docker infrastructure healthy"

  # 2. Ensure JS workspace dependencies are installed
  if [[ ! -d "${ROOT_DIR}/node_modules" ]]; then
    info "Installing JS workspace dependencies (pnpm install --frozen-lockfile)..."
    (cd "${ROOT_DIR}" && pnpm install --frozen-lockfile)
    ok "JS dependencies installed"
  fi

  # 3. Build Rust service binaries once (avoiding concurrent cargo locks)
  local bin_dir="${ROOT_DIR}/target/debug"
  local need_build=true
  if [[ "${no_build}" == "true" ]] \
    && [[ -x "${bin_dir}/control-api" ]] \
    && [[ -x "${bin_dir}/delivery-edge" ]] \
    && [[ -x "${bin_dir}/event-ingest" ]] \
    && [[ -x "${bin_dir}/relay-proxy" ]]; then
    need_build=false
    ok "Skipping cargo build (--no-build; existing binaries found)"
  fi
  if [[ "${need_build}" == "true" ]]; then
    info "Building Rust services (control-api, delivery-edge, event-ingest, relay-proxy)..."
    (cd "${ROOT_DIR}" && cargo build \
      --bin control-api \
      --bin delivery-edge \
      --bin event-ingest \
      --bin relay-proxy)
    ok "Rust service binaries built"
  fi

  # 4. Launch Rust services in dependency order
  info "Starting control-api (${CONTROL_API_URL})..."
  start_detached "control-api" "${bin_dir}/control-api"
  wait_for_http "control-api" "${CONTROL_API_URL}/readyz" 60

  info "Starting delivery-edge (${DELIVERY_EDGE_URL})..."
  start_detached "delivery-edge" "${bin_dir}/delivery-edge"
  wait_for_http "delivery-edge" "${DELIVERY_EDGE_URL}/healthz" 30

  info "Starting event-ingest (${EVENT_INGEST_URL})..."
  start_detached "event-ingest" "${bin_dir}/event-ingest"
  wait_for_http "event-ingest" "${EVENT_INGEST_URL}/readyz" 30

  provision_relay_keys

  info "Starting relay-proxy (${RELAY_PROXY_URL})..."
  local relay_envs_compact
  relay_envs_compact="$(jq -c '.' "${RELAY_ENVS_FILE}")"
  SKOTOS_RELAY_ENVS="${relay_envs_compact}" \
  SKOTOS_UPSTREAM_STREAM_URL="${DELIVERY_EDGE_URL}" \
  SKOTOS_UPSTREAM_EVENTS_URL="${EVENT_INGEST_URL}" \
    start_detached "relay-proxy" "${bin_dir}/relay-proxy"
  wait_for_relay_healthy 30

  # 5. Launch Management Dashboard SPA (@skotos/web)
  info "Starting @skotos/web dashboard (${WEB_URL})..."
  VITE_API_PROXY_TARGET="${CONTROL_API_URL}" \
    start_detached "web" pnpm --dir "${ROOT_DIR}" --filter @skotos/web exec vite --host 0.0.0.0 --port "${WEB_PORT}" --strictPort
  wait_for_http "web" "${WEB_URL}" 45

  # 6. Optional seed & smoke test
  if [[ "${with_seed}" == "true" ]]; then
    "${SCRIPT_DIR}/seed-demo.sh"
  fi
  if [[ "${with_smoke}" == "true" ]]; then
    "${SCRIPT_DIR}/smoke-test.sh"
  fi

  print_summary "${with_analytics}"

  if [[ "${foreground}" == "true" ]]; then
    info "Running in foreground mode. Press Ctrl+C to stop all Skotos app services."
    trap 'echo ""; info "Caught signal; stopping Skotos app services..."; cmd_down; exit 0' INT TERM
    while true; do
      sleep 2
    done
  fi
}

print_summary() {
  local with_analytics="${1:-false}"
  local email="${SKOTOS_BOOTSTRAP_ADMIN_EMAIL:-admin@skotos.local}"
  local password="${SKOTOS_BOOTSTRAP_ADMIN_PASSWORD:-SkotosDevPass123!}"
  cat <<EOF

${C_GREEN}${C_BOLD}Skotos Local Full Stack is UP${C_RESET}
────────────────────────────────────────────────────────────────────────
  Dashboard (@skotos/web)  ${WEB_URL}
  Control Plane API        ${CONTROL_API_URL}   (/readyz, /api/v1/...)
  Flag Delivery Edge       ${DELIVERY_EDGE_URL}   (DB-free SSE / evalx)
  Event Ingestion          ${EVENT_INGEST_URL}   (/events/v1/bulk)
  Relay Proxy              ${RELAY_PROXY_URL}   (/status, /stream/v1/all)
  MinIO Console            http://127.0.0.1:${MINIO_CONSOLE_PORT:-9001}
  NATS Monitor             http://127.0.0.1:${NATS_MONITOR_PORT:-8222}
EOF
  if [[ "${with_analytics}" == "true" ]] || [[ -n "${CLICKHOUSE_URL:-}" ]]; then
    printf '  ClickHouse HTTP          http://127.0.0.1:%s\n' "${CLICKHOUSE_HTTP_PORT:-8123}"
  fi
  cat <<EOF
────────────────────────────────────────────────────────────────────────
  Bootstrap Login          ${email}  /  ${password}
  Logs                     ${LOG_DIR}/<service>.log
  Useful Commands          make dev-status | make seed-demo | make smoke | make dev-down
EOF
}

cmd_down() {
  local stop_infra=false
  local clean_all=false

  while [[ $# -gt 0 ]]; do
    case "$1" in
      --infra) stop_infra=true ;;
      --clean) stop_infra=true; clean_all=true ;;
      -h|--help) usage; exit 0 ;;
      *) die "Unknown option for 'down': $1" ;;
    esac
    shift
  done

  for svc in web relay-proxy event-ingest delivery-edge control-api; do
    stop_service "${svc}"
  done
  stop_port_listener_from_repo "web" "${WEB_PORT:-${SKOTOS_WEB_PORT:-5173}}"

  if [[ "${stop_infra}" == "true" ]]; then
    cd "${ROOT_DIR}"
    if [[ "${clean_all}" == "true" ]]; then
      info "Stopping Docker infra and removing volumes..."
      docker compose --profile analytics down -v
    else
      info "Stopping Docker infra (preserving volumes)..."
      docker compose --profile analytics down
    fi
  fi

  if [[ "${clean_all}" == "true" ]]; then
    info "Removing ${STATE_DIR}..."
    rm -rf "${STATE_DIR}"
  fi

  ok "Skotos local stack stopped"
}

check_endpoint() {
  local label="$1"
  local url="$2"
  local pid_name="${3:-}"
  local pid_info="-"

  if [[ -n "${pid_name}" ]]; then
    local pid
    pid="$(read_pid "${pid_name}")"
    if [[ -n "${pid}" ]] && pid_is_running "${pid}"; then
      pid_info="pid ${pid}"
    else
      pid_info="stopped"
    fi
  fi

  if curl -fsS --max-time 2 "${url}" >/dev/null 2>&1; then
    printf '  %-20s %sUP%s      %-12s %s\n' "${label}" "${C_GREEN}${C_BOLD}" "${C_RESET}" "${pid_info}" "${url}"
  else
    printf '  %-20s %sDOWN%s    %-12s %s\n' "${label}" "${C_RED}${C_BOLD}" "${C_RESET}" "${pid_info}" "${url}"
  fi
}

cmd_status() {
  ensure_env
  printf '%sDocker Backing Infrastructure%s\n' "${C_BOLD}" "${C_RESET}"
  (cd "${ROOT_DIR}" && docker compose --profile analytics ps)
  echo ""
  printf '%sApplication Services%s\n' "${C_BOLD}" "${C_RESET}"
  check_endpoint "control-api" "${CONTROL_API_URL}/readyz" "control-api"
  check_endpoint "delivery-edge" "${DELIVERY_EDGE_URL}/healthz" "delivery-edge"
  check_endpoint "event-ingest" "${EVENT_INGEST_URL}/readyz" "event-ingest"
  check_endpoint "relay-proxy" "${RELAY_PROXY_URL}/healthz" "relay-proxy"
  check_endpoint "web (@skotos/web)" "${WEB_URL}" "web"
}

cmd_logs() {
  local svc="${1:-}"
  if [[ ! -d "${LOG_DIR}" ]]; then
    die "No logs directory found at ${LOG_DIR} (has the stack been started?)"
  fi
  if [[ -n "${svc}" ]]; then
    local target="${LOG_DIR}/${svc}.log"
    [[ -f "${target}" ]] || die "No log file found for '${svc}' at ${target}"
    tail -n 100 -f "${target}"
  else
    tail -n 40 -f "${LOG_DIR}"/*.log
  fi
}

main() {
  local subcmd="${1:-help}"
  shift || true
  case "${subcmd}" in
    up) cmd_up "$@" ;;
    down) cmd_down "$@" ;;
    restart)
      cmd_down
      cmd_up "$@"
      ;;
    status) cmd_status ;;
    logs) cmd_logs "$@" ;;
    seed) exec "${SCRIPT_DIR}/seed-demo.sh" "$@" ;;
    smoke) exec "${SCRIPT_DIR}/smoke-test.sh" "$@" ;;
    -h|--help|help) usage ;;
    *) die "Unknown command: ${subcmd}. Run 'scripts/dev-stack.sh --help' for usage." ;;
  esac
}

main "$@"
