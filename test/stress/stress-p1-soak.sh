#!/usr/bin/env bash
# P1 soak: rotate stacks every 5 min, restart container every 15 min, track recovery.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
# shellcheck disable=SC1091
. "${root}/test/stress/common.sh"
cleanup_old_stress_runs
kill_orphan_workerd_preflight
ensure_stress_data

run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
STRESS_RUN_DIR="${stress_run_root}/${run_id}"
export STRESS_RUN_DIR STRESS_BASE_URL="$base_url"
mkdir -p "$STRESS_RUN_DIR"
# shellcheck source=lib/anomaly-check.sh
. "${root}/test/stress/lib/anomaly-check.sh"

capture_container_restart_baseline open-compute-ocd

export STRESS_BINDINGS_ALIGN_SEC=${STRESS_BINDINGS_ALIGN_SEC:-60}

if [ "${STRESS_SOAK_ABBREV:-0}" = "1" ]; then
  SOAK_TOTAL_SEC=900
  ROTATE_SEC=120
  SOAK_INJECT_RESTART=0
  RESTART_INTERVAL_SEC=$((SOAK_TOTAL_SEC + 1))
  SOAK_HTTP_CONCURRENCY=10
  SOAK_KV_CONCURRENCY=8
  SOAK_D1_CONCURRENCY=5
  SOAK_R2_RATE=5
  SOAK_QUEUE_RATE=10
  SOAK_DO_RATE=8
  SOAK_WORKFLOW_RATE=3
  SOAK_FETCH_CONCURRENCY=10
  SOAK_MEGA_CONCURRENCY=3
  SOAK_RECOVER_SETTLE_SEC=20
  printf 'Soak abbreviated mode (STRESS_SOAK_ABBREV=1): 15 min total, no container restart\n' >&2
else
  SOAK_TOTAL_SEC=3600
  ROTATE_SEC=300
  SOAK_INJECT_RESTART=1
  RESTART_INTERVAL_SEC=900
  SOAK_HTTP_CONCURRENCY=25
  SOAK_KV_CONCURRENCY=20
  SOAK_D1_CONCURRENCY=10
  SOAK_R2_RATE=10
  SOAK_QUEUE_RATE=20
  SOAK_DO_RATE=15
  SOAK_WORKFLOW_RATE=3
  SOAK_FETCH_CONCURRENCY=20
  SOAK_MEGA_CONCURRENCY=6
  SOAK_RECOVER_SETTLE_SEC=15
fi

STACK_NAMES="http kv d1 r2 queue do workflow fetch scenario_mega"
for stack in $STACK_NAMES; do
  : >"${STRESS_RUN_DIR}/lat-${stack}.txt"
  : >"${STRESS_RUN_DIR}/err-${stack}.txt"
done

SOAK_LOG="${STRESS_RUN_DIR}/soak-events.jsonl"
: >"$SOAK_LOG"

log_soak_event() {
  jq -nc \
    --arg event "$1" \
    --argjson elapsed "$(($(date +%s) - soak_start))" \
    --arg details "${2:-}" \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{event: $event, elapsed_sec: $elapsed, details: $details, timestamp: $ts}' \
    >>"$SOAK_LOG"
}

run_stack_soak() {
  stack=$1
  duration=$2
  recover_before_sample "$SOAK_RECOVER_SETTLE_SEC"
  case "$stack" in
    http)
      run_duration_concurrent http "$SOAK_HTTP_CONCURRENCY" "$duration" "/stack/http/ping" GET "" \
        "${STRESS_RUN_DIR}/lat-http.txt" "${STRESS_RUN_DIR}/err-http.txt"
      ;;
    kv)
      run_duration_concurrent kv "$SOAK_KV_CONCURRENCY" "$duration" "/stack/kv/soak-kv-${run_id}" PUT "soak-value" \
        "${STRESS_RUN_DIR}/lat-kv.txt" "${STRESS_RUN_DIR}/err-kv.txt"
      ;;
    d1)
      run_duration_concurrent d1 "$SOAK_D1_CONCURRENCY" "$duration" "/stack/d1/orders" POST '{"status":"created"}' \
        "${STRESS_RUN_DIR}/lat-d1.txt" "${STRESS_RUN_DIR}/err-d1.txt"
      ;;
    r2)
      curl -sS -H "$host_header" -X PUT \
        --data-binary "soak-r2-payload-${run_id}" \
        "${base_url}/stack/r2/objects/soak-r2-${run_id}" >/dev/null || true
      run_rate_load r2 "$SOAK_R2_RATE" "$duration" "/stack/r2/objects/soak-r2-${run_id}" GET "" \
        "${STRESS_RUN_DIR}/lat-r2.txt" "${STRESS_RUN_DIR}/err-r2.txt"
      ;;
    queue)
      run_rate_load queue "$SOAK_QUEUE_RATE" "$duration" "/stack/queue/enqueue" POST \
        "{\"label\":\"soak-q-${run_id}-$(date +%s)\"}" \
        "${STRESS_RUN_DIR}/lat-queue.txt" "${STRESS_RUN_DIR}/err-queue.txt"
      ;;
    do)
      run_rate_load do "$SOAK_DO_RATE" "$duration" "/stack/do/soak-do-${run_id}/increment" POST '{"amount":1}' \
        "${STRESS_RUN_DIR}/lat-do.txt" "${STRESS_RUN_DIR}/err-do.txt"
      ;;
    workflow)
      run_rate_load workflow "$SOAK_WORKFLOW_RATE" "$duration" "/stack/workflow/checkout" POST '{"mode":"normal","fanOutN":2}' \
        "${STRESS_RUN_DIR}/lat-workflow.txt" "${STRESS_RUN_DIR}/err-workflow.txt"
      ;;
    fetch)
      run_duration_concurrent fetch "$SOAK_FETCH_CONCURRENCY" "$duration" "/stack/fetch/probe?hops=1" GET "" \
        "${STRESS_RUN_DIR}/lat-fetch.txt" "${STRESS_RUN_DIR}/err-fetch.txt"
      ;;
    scenario_mega)
      run_duration_concurrent scenario_mega "$SOAK_MEGA_CONCURRENCY" "$duration" "/stack/scenario/mega-checkout" POST \
        "$(jq -nc --arg orderId "soak-mega-${run_id}-$(date +%s)" '{orderId: $orderId, mode: "normal"}')" \
        "${STRESS_RUN_DIR}/lat-scenario_mega.txt" "${STRESS_RUN_DIR}/err-scenario_mega.txt"
      ;;
  esac
  recover_before_sample "$SOAK_RECOVER_SETTLE_SEC"
}

post_restart_verify() {
  local restart_num=$1
  local ready_start ready_ms
  ready_start=$(now_ms)
  if ! wait_for_worker_ready 90; then
    record_anomaly "health_ready" "$(jq -nc --arg phase "soak_restart_${restart_num}" '{phase: $phase, reason: "worker_ready_timeout"}')"
    log_soak_event "restart_ready_fail" "restart=${restart_num}"
    return 1
  fi
  ready_ms=$(($(now_ms) - ready_start))
  log_soak_event "restart_ready_ok" "restart=${restart_num} recovery_ms=${ready_ms}"

  local attempt=0
  while [ "$attempt" -lt 2 ]; do
    if STRESS_BINDINGS_ALIGN_SEC="${STRESS_BINDINGS_ALIGN_SEC:-60}" \
      bash "${root}/test/stress/reconcile.sh" >/dev/null 2>&1; then
      log_soak_event "restart_reconcile_ok" "restart=${restart_num} attempt=$((attempt + 1))"
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 5
  done
  record_anomaly "reconcile_consistency" "$(jq -nc --arg phase "soak_restart_${restart_num}" '{phase: $phase, reason: "reconcile_failed"}')"
  log_soak_event "restart_reconcile_fail" "restart=${restart_num}"
  return 1
}

inject_restart() {
  local restart_num=$1
  log_soak_event "restart_begin" "restart=${restart_num}"
  kill_orphan_workerd_preflight
  if command -v docker >/dev/null 2>&1; then
    docker restart open-compute-ocd >/dev/null 2>&1 || true
    sleep 10
    post_restart_verify "$restart_num" || true
    recover_before_sample "$SOAK_RECOVER_SETTLE_SEC"
    capture_container_restart_baseline open-compute-ocd
  else
    log_soak_event "restart_skipped" "docker unavailable"
  fi
  check_orphan_workerd || true
}

soak_start=$(date +%s)
soak_end=$((soak_start + SOAK_TOTAL_SEC))
next_restart=$((soak_start + RESTART_INTERVAL_SEC))
restart_count=0
stack_idx=0
stack_list="http kv d1 r2 queue do workflow fetch scenario_mega"

log_soak_event "soak_start" "total_sec=${SOAK_TOTAL_SEC} rotate_sec=${ROTATE_SEC}"

while [ "$(date +%s)" -lt "$soak_end" ]; do
  stack=$(printf '%s\n' $stack_list | sed -n "$((stack_idx % 9 + 1))p")
  stack_idx=$((stack_idx + 1))
  remaining=$((soak_end - $(date +%s)))
  phase_duration=$ROTATE_SEC
  if [ "$phase_duration" -gt "$remaining" ]; then
    phase_duration=$remaining
  fi
  if [ "$phase_duration" -le 0 ]; then
    break
  fi

  log_soak_event "stack_begin" "stack=${stack} duration=${phase_duration}"
  run_stack_soak "$stack" "$phase_duration"
  log_soak_event "stack_end" "stack=${stack}"

  if [ "${SOAK_INJECT_RESTART:-0}" = "1" ] \
    && [ "$(date +%s)" -lt "$soak_end" ] \
    && [ "$(date +%s)" -ge "$next_restart" ]; then
    restart_count=$((restart_count + 1))
    inject_restart "$restart_count"
    next_restart=$((next_restart + RESTART_INTERVAL_SEC))
  fi

  check_health_post_run "$base_url" || true
done

log_soak_event "soak_end" "restarts=${restart_count} anomalies=$(anomaly_count)"

check_orphan_workerd || true
check_container_restarts open-compute-ocd || true

timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
RESULT_DIR="$STRESS_RUN_DIR" RUN_ID="$run_id" TIMESTAMP="$timestamp" \
  WORKER_HOST="$worker_host" SOAK_LOG="$SOAK_LOG" RESTART_COUNT="$restart_count" \
  python3 - <<'PY' >"${STRESS_RUN_DIR}/result.json"
import json
import os
from pathlib import Path

result_dir = Path(os.environ["RESULT_DIR"])
soak_events = []
soak_file = Path(os.environ.get("SOAK_LOG", ""))
if soak_file.is_file():
    soak_events = [json.loads(line) for line in soak_file.read_text().splitlines() if line.strip()]

stacks = {}
for lat_file in sorted(result_dir.glob("lat-*.txt")):
    name = lat_file.name.replace("lat-", "").replace(".txt", "")
    err_file = result_dir / f"err-{name}.txt"
    lines = [int(line.strip()) for line in lat_file.read_text().splitlines() if line.strip()]
    total = len(lines)
    errors = sum(1 for _ in err_file.open()) if err_file.exists() else 0
    error_rate = (errors / total) if total else 0.0

    def pct(p: int) -> int:
        if not lines:
            return 0
        rank = (total * p + 99) // 100
        return sorted(lines)[min(rank, total) - 1]

    stacks[name] = {
        "samples": total,
        "errors": errors,
        "error_rate": round(error_rate, 6),
        "latency_ms": {"p50": pct(50), "p95": pct(95), "p99": pct(99)},
        "anomalies": [],
        "verdict": "pass",
    }

recovery_times = [
    e.get("details", "")
    for e in soak_events
    if e.get("event") == "restart_ready_ok"
]
print(json.dumps({
    "schema_version": 2,
    "profile": "p1-soak" if os.environ.get("STRESS_SOAK_ABBREV") != "1" else "p1-soak-abbrev",
    "run_id": os.environ["RUN_ID"],
    "timestamp": os.environ["TIMESTAMP"],
    "worker_host": os.environ["WORKER_HOST"],
    "soak": {
        "container_restarts_injected": int(os.environ.get("RESTART_COUNT", "0")),
        "recovery_events": recovery_times,
        "events": soak_events,
    },
    "stacks": stacks,
    "scenario": {"mega-checkout": stacks.get("scenario_mega", {})},
    "verdict": "pass",
}, indent=2))
PY

finalize_verdict "${STRESS_RUN_DIR}/result.json"
