#!/usr/bin/env bash
# Per-stack P0 stress for 2C/4G compose profile with per-stack result.json breakdown.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
# shellcheck disable=SC1091
. "${root}/test/stress/common.sh"
ensure_stress_data

run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
STRESS_RUN_DIR="${stress_run_root}/${run_id}"
export STRESS_RUN_DIR STRESS_BASE_URL="$base_url"
mkdir -p "$STRESS_RUN_DIR"
# shellcheck source=lib/anomaly-check.sh
. "${root}/test/stress/lib/anomaly-check.sh"

capture_container_restart_baseline open-compute-ocd

STACK_NAMES="http kv d1 r2 queue do workflow fetch cpu service scenario_mega"
for stack in $STACK_NAMES; do
  : >"${STRESS_RUN_DIR}/lat-${stack}.txt"
  : >"${STRESS_RUN_DIR}/err-${stack}.txt"
done

run_stack_load() {
  stack=$1
  concurrency=$2
  path=$3
  method=${4:-GET}
  body=${5:-}
  i=1
  while [ "$i" -le "$concurrency" ]; do
    stress_request "$path" "$method" "$body" \
      "${STRESS_RUN_DIR}/lat-${stack}.txt" \
      "${STRESS_RUN_DIR}/err-${stack}.txt" &
    i=$((i + 1))
  done
  wait
  printf 'completed %s concurrency=%s\n' "$stack" "$concurrency"
}

for concurrency in 10 25 50; do
  run_stack_load http "$concurrency" "/stack/http/ping"
done

kv_key="p0-kv-${run_id}"
run_stack_load kv 20 "/stack/kv/${kv_key}" PUT "p0-value-${run_id}"
run_stack_load kv 20 "/stack/kv/${kv_key}" GET

d1_body='{"status":"created","payloadBytes":128}'
run_stack_load d1 12 "/stack/d1/orders" POST "$d1_body"

i=1
while [ "$i" -le 10 ]; do
  run_stack_load r2 1 "/stack/r2/objects/p0-r2-${run_id}-${i}" PUT "p0-r2-payload-${i}" &
  i=$((i + 1))
done
wait

queue_body='{"batch":[{"label":"p0-q1","payload":{"run":"'"${run_id}"'"}},{"label":"p0-q2","payload":{"run":"'"${run_id}"'"}}]}'
run_stack_load queue 8 "/stack/queue/enqueue" POST "$queue_body"

do_id="p0-do-${run_id}"
run_stack_load do 10 "/stack/do/${do_id}/increment" POST '{"amount":1}'

workflow_body='{"mode":"normal","fanOutN":2}'
run_stack_load workflow 6 "/stack/workflow/checkout" POST "$workflow_body"

run_stack_load fetch 15 "/stack/fetch/probe?hops=1"
run_stack_load cpu 4 "/stack/cpu/spin" POST '{"iterations":40000}'
run_stack_load service 10 "/stack/service/call?mode=rpc"

mega_body='{"mode":"normal","fanOutN":5,"fanOutM":2,"payloadBytes":2048}'
run_stack_load scenario_mega 6 "/stack/scenario/mega-checkout" POST "$mega_body"

check_health_post_run "$base_url" || true
check_orphan_workerd || true
check_container_restarts open-compute-ocd || true

timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
RESULT_DIR="$STRESS_RUN_DIR" RUN_ID="$run_id" TIMESTAMP="$timestamp" WORKER_HOST="$worker_host" python3 - <<'PY' >"${STRESS_RUN_DIR}/result.json"
import json
import os
from pathlib import Path

result_dir = Path(os.environ["RESULT_DIR"])
stacks = {}
for lat_file in sorted(result_dir.glob("lat-*.txt")):
    name = lat_file.name.replace("lat-", "").replace(".txt", "")
    err_file = result_dir / f"err-{name}.txt"
    total = sum(1 for _ in lat_file.open())
    errors = sum(1 for _ in err_file.open()) if err_file.exists() else 0
    error_rate = (errors / total) if total else 1.0

    def pct(p: int) -> int:
        if total == 0:
            return 0
        rank = (total * p + 99) // 100
        lines = sorted(int(line.strip()) for line in lat_file.open() if line.strip())
        return lines[min(rank, len(lines)) - 1]

    stacks[name] = {
        "samples": total,
        "errors": errors,
        "error_rate": round(error_rate, 6),
        "p50": pct(50),
        "p95": pct(95),
        "p99": pct(99),
    }

output = {
    "schema_version": 2,
    "profile": "p0-2c4g",
    "run_id": os.environ["RUN_ID"],
    "timestamp": os.environ["TIMESTAMP"],
    "worker_host": os.environ["WORKER_HOST"],
    "stacks": stacks,
    "scenario": {"mega-checkout": stacks.get("scenario_mega", {})},
    "slo_thresholds_2c4g": {
        "http": {"p95_ms": 800, "p99_ms": 1500, "error_rate_max": 0.01},
        "kv": {"p95_ms": 600, "error_rate_max": 0.01},
        "d1": {"p95_ms": 1200, "error_rate_max": 0.01},
        "r2": {"p95_ms": 1000, "error_rate_max": 0.01},
        "queue": {"p95_ms": 900, "error_rate_max": 0.01},
        "do": {"p95_ms": 1000, "error_rate_max": 0.01},
        "workflow": {"p95_ms": 2000, "error_rate_max": 0.01},
        "fetch": {"p95_ms": 1200, "error_rate_max": 0.01},
        "cpu": {"p95_ms": 3000, "error_rate_max": 0.01},
        "service": {"p95_ms": 800, "error_rate_max": 0.01},
        "scenario_mega": {"p95_ms": 3000, "error_rate_max": 0.01},
    },
}
print(json.dumps(output, indent=2))
PY

finalize_verdict "${STRESS_RUN_DIR}/result.json"
