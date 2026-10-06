#!/usr/bin/env bash
# P1 per-stack PEAK stress; STRESS_PROFILE selects 2C/4G vs 8C/16G load + SLO tables.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
# shellcheck disable=SC1091
. "${root}/test/stress/common.sh"
require_disk_space 5 "${root}/.temp"
ensure_stress_data

STRESS_PROFILE=${STRESS_PROFILE:-2c4g}
case "$STRESS_PROFILE" in
  2c4g)
    P1_HTTP_CONCURRENCY="25 50 75"
    P1_KV_CONCURRENCY=40
    P1_D1_WRITERS=10
    P1_D1_READERS=20
    P1_R2_GET_RATE=10
    P1_QUEUE_RATE=200
    P1_DO_RPC_RATE=10
    P1_DO_WS_COUNT=10
    P1_WORKFLOW_RATE=5
    P1_FETCH_CONCURRENCY=50
    P1_CPU_CONCURRENCY=8
    P1_MEGA_CONCURRENCY=15
    ;;
  8c16g)
    P1_HTTP_CONCURRENCY="50 100 150"
    P1_KV_CONCURRENCY=80
    P1_D1_WRITERS=30
    P1_D1_READERS=60
    P1_R2_GET_RATE=30
    P1_QUEUE_RATE=400
    P1_DO_RPC_RATE=30
    P1_DO_WS_COUNT=20
    P1_WORKFLOW_RATE=10
    P1_FETCH_CONCURRENCY=100
    P1_CPU_CONCURRENCY=16
    P1_MEGA_CONCURRENCY=30
    ;;
  *)
    echo "unknown STRESS_PROFILE: $STRESS_PROFILE (expected 2c4g or 8c16g)" >&2
    exit 1
    ;;
esac
export STRESS_PROFILE

run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
STRESS_RUN_DIR="${stress_run_root}/${run_id}"
export STRESS_RUN_DIR STRESS_BASE_URL="$base_url"
mkdir -p "$STRESS_RUN_DIR"
# shellcheck source=lib/anomaly-check.sh
. "${root}/test/stress/lib/anomaly-check.sh"

capture_container_restart_baseline open-compute-ocd

# Full durations per P1 spec; STRESS_P1_ABBREV=1 scales to ~10% for session validation.
P1_SCALE=${STRESS_P1_SCALE:-1}
if [ "${STRESS_P1_ABBREV:-0}" = "1" ]; then
  P1_SCALE=0.1
fi

p1_duration_sec() {
  python3 - <<PY
import math
print(max(5, math.ceil(${1} * ${P1_SCALE})))
PY
}

p1_rate() {
  python3 - <<PY
import math
print(max(1, math.ceil(${1} * ${P1_SCALE})))
PY
}

init_stack_files() {
  local stack=$1
  : >"${STRESS_RUN_DIR}/lat-${stack}.txt"
  : >"${STRESS_RUN_DIR}/err-${stack}.txt"
}

stack_lat_file() {
  printf '%s' "${STRESS_RUN_DIR}/lat-$1.txt"
}

stack_err_file() {
  printf '%s' "${STRESS_RUN_DIR}/err-$1.txt"
}

run_concurrent_until() {
  local label=$1
  local concurrency=$2
  local duration_sec=$3
  local path=$4
  local method=${5:-GET}
  local body=${6:-}
  local lat_file=$7
  local err_file=$8
  local deadline=$(( $(date +%s) + duration_sec ))

  worker_loop() {
    while [ "$(date +%s)" -lt "$deadline" ]; do
      stress_request "$path" "$method" "$body" "$lat_file" "$err_file"
    done
  }

  local i=1
  while [ "$i" -le "$concurrency" ]; do
    worker_loop &
    i=$((i + 1))
  done
  wait
  printf 'completed %s concurrency=%s duration=%ss\n' "$label" "$concurrency" "$duration_sec" >&2
}

run_rate_until() {
  local label=$1
  local rate=$2
  local duration_sec=$3
  local path=$4
  local method=${5:-POST}
  local body_template=${6:-}
  local lat_file=$7
  local err_file=$8
  local deadline=$(( $(date +%s) + duration_sec ))
  local seq=0

  while [ "$(date +%s)" -lt "$deadline" ]; do
    local second_start
    second_start=$(date +%s)
    local i=0
    while [ "$i" -lt "$rate" ]; do
      seq=$((seq + 1))
      local body
      body=$(printf '%s' "$body_template" | sed "s/__SEQ__/${seq}/g; s/__RUN__/${run_id}/g")
      stress_request "$path" "$method" "$body" "$lat_file" "$err_file"
      i=$((i + 1))
    done
    local elapsed=$(( $(date +%s) - second_start ))
    if [ "$elapsed" -lt 1 ]; then
      sleep $((1 - elapsed))
    fi
  done
  printf 'completed %s rate=%s/s duration=%ss samples=%s\n' "$label" "$rate" "$duration_sec" "$seq" >&2
}

recover_before_sample() {
  wait_for_ready "$base_url" 60 || true
  sleep 5
}

sample_stack_response() {
  recover_before_sample
  local method=$1
  local path=$2
  local body=${3:-}
  local allowed=${4:-200}
  local ctx
  ctx=$(jq -nc --arg method "$method" --arg path "$path" '{method: $method, path: $path, phase: "p1_peak_sample"}')
  local status response body_file attempt
  body_file="${STRESS_RUN_DIR}/sample-body.json"
  status=000
  response='{}'
  attempt=1
  while [ "$attempt" -le 5 ]; do
    if [ -n "$body" ]; then
      status=$(curl -sS -H "$host_header" -H 'Content-Type: application/json' \
        -X "$method" -o "$body_file" -w '%{http_code}' \
        --max-time 60 "${base_url}${path}" -d "$body" || echo 000)
    else
      status=$(curl -sS -H "$host_header" -X "$method" -o "$body_file" -w '%{http_code}' \
        --max-time 60 "${base_url}${path}" || echo 000)
    fi
    if [ -f "$body_file" ]; then
      response=$(cat "$body_file")
    else
      response='{}'
    fi
    case "$status" in
      200|201|202|204|101) break ;;
    esac
    wait_for_ready "$base_url" 10 || true
    sleep 2
    attempt=$((attempt + 1))
  done
  assert_status_allowed "$status" "$allowed" "$ctx" || true
  if printf '%s' "$response" | jq -e '.ok != null' >/dev/null 2>&1; then
    assert_json_true "$response" '.ok' "$ctx" || true
  fi
  grep_response_secrets "$response" "$ctx" || true
  printf '%s' "$response"
}

check_kv_read_after_write() {
  local key=$1
  local value=$2
  local ctx
  ctx=$(jq -nc --arg key "$key" '{key: $key, phase: "p1_kv_verify"}')
  curl -sS -H "$host_header" -X PUT \
    --data-binary "$value" \
    "${base_url}/stack/kv/${key}" >/dev/null || true
  local body
  body=$(curl -sS -H "$host_header" "${base_url}/stack/kv/${key}" || echo '{}')
  local actual exists
  actual=$(printf '%s' "$body" | jq -r '.value // empty')
  exists=$(printf '%s' "$body" | jq -r '.exists // false')
  if [ "$exists" != "true" ] || [ "$actual" != "$value" ]; then
    record_anomaly "kv_read_after_write" "$(jq -nc \
      --arg key "$key" \
      --arg expected "$value" \
      --arg actual "$actual" \
      --arg exists "$exists" \
      --argjson context "$ctx" \
      '{key: $key, expected: $expected, actual: $actual, exists: ($exists == "true"), context: $context}')"
    return 1
  fi
  return 0
}

check_do_counter_monotonic() {
  local object_id=$1
  local ctx
  ctx=$(jq -nc --arg object_id "$object_id" '{object_id: $object_id, phase: "p1_do_verify"}')
  local before after count_before count_after before_ok after_ok
  before=$(curl -sS -H "$host_header" -X GET \
    "${base_url}/stack/do/${object_id}/increment" || echo '{}')
  before_ok=$(printf '%s' "$before" | jq -r '.ok // false')
  count_before=$(printf '%s' "$before" | jq -r 'select(.ok == true) | .count // empty')
  if [ "$before_ok" != "true" ] || [ -z "$count_before" ]; then
    record_anomaly "http_status" "$(jq -nc \
      --arg object_id "$object_id" \
      --arg before_ok "$before_ok" \
      --argjson context "$ctx" \
      '{object_id: $object_id, phase: "p1_do_verify_read", before_ok: ($before_ok == "true"), context: $context}')"
    return 1
  fi
  after=$(curl -sS -H "$host_header" -X POST \
    -H 'Content-Type: application/json' \
    -d '{"amount":1}' \
    "${base_url}/stack/do/${object_id}/increment" || echo '{}')
  after_ok=$(printf '%s' "$after" | jq -r '.ok // false')
  count_after=$(printf '%s' "$after" | jq -r 'select(.ok == true) | .count // empty')
  if [ "$after_ok" != "true" ] || [ -z "$count_after" ]; then
    record_anomaly "http_status" "$(jq -nc \
      --arg object_id "$object_id" \
      --arg after_ok "$after_ok" \
      --argjson context "$ctx" \
      '{object_id: $object_id, phase: "p1_do_verify_increment", after_ok: ($after_ok == "true"), context: $context}')"
    return 1
  fi
  if [ "$count_after" -le "$count_before" ]; then
    record_anomaly "do_counter_regression" "$(jq -nc \
      --arg object_id "$object_id" \
      --argjson before "$count_before" \
      --argjson after "$count_after" \
      --argjson context "$ctx" \
      '{object_id: $object_id, count_before: $before, count_after: $after, context: $context}')"
    return 1
  fi
  return 0
}

DO_WS_PID_FILE="${STRESS_RUN_DIR}/do-ws.pids"

start_do_websocket_pool() {
  local count=$1
  local object_id=$2
  local duration_sec=$3
  : >"$DO_WS_PID_FILE"
  local i=1
  while [ "$i" -le "$count" ]; do
    WORKER_HOST="$worker_host" STRESS_BASE_URL="$base_url" OBJECT_ID="$object_id" DURATION_SEC="$duration_sec" \
      node <<'NODE' >>"${STRESS_RUN_DIR}/do-ws-${i}.log" 2>&1 &
const host = process.env.WORKER_HOST;
const base = process.env.STRESS_BASE_URL;
const objectId = process.env.OBJECT_ID;
const durationSec = Number(process.env.DURATION_SEC);
const url = base.replace(/^http/, 'ws') + `/stack/do/${objectId}/ws`;
const deadline = Date.now() + durationSec * 1000;

function connect() {
  return new Promise((resolve) => {
    const ws = new WebSocket(url, { headers: { Host: host } });
    ws.addEventListener('open', () => {
      const tick = () => {
        if (Date.now() >= deadline) {
          ws.close();
          resolve();
          return;
        }
        ws.send(JSON.stringify({ action: 'increment', amount: 1 }));
        setTimeout(tick, 2000);
      };
      tick();
    });
    ws.addEventListener('error', () => resolve());
    ws.addEventListener('close', () => resolve());
  });
}

connect().then(() => process.exit(0));
NODE
    echo $! >>"$DO_WS_PID_FILE"
    i=$((i + 1))
  done
}

stop_do_websocket_pool() {
  if [ ! -f "$DO_WS_PID_FILE" ]; then
    return 0
  fi
  while IFS= read -r pid; do
    [ -n "$pid" ] && kill "$pid" 2>/dev/null || true
  done <"$DO_WS_PID_FILE"
  wait 2>/dev/null || true
}

stack_http_peak() {
  init_stack_files http
  local lat err duration
  lat=$(stack_lat_file http)
  err=$(stack_err_file http)
  duration=$(p1_duration_sec 300)
  for concurrency in $P1_HTTP_CONCURRENCY; do
    run_concurrent_until "http-peak-${concurrency}" "$concurrency" "$duration" \
      "/stack/http/ping" GET "" "$lat" "$err"
  done
  sample_stack_response GET "/stack/http/ping" "" "200" >/dev/null
  stack_stats "$lat" "$err"
}

stack_kv_peak() {
  init_stack_files kv
  local lat err duration deadline worker_id
  lat=$(stack_lat_file kv)
  err=$(stack_err_file kv)
  duration=$(p1_duration_sec 900)
  deadline=$(( $(date +%s) + duration ))

  kv_worker() {
    local wid=$1
    local seq=0
    while [ "$(date +%s)" -lt "$deadline" ]; do
      seq=$((seq + 1))
      local key="p1-kv-${run_id}-w${wid}-${seq}"
      local value="p1-value-${run_id}-${seq}"
      stress_request "/stack/kv/${key}" PUT "$value" "$lat" "$err"
      stress_request "/stack/kv/${key}" GET "" "$lat" "$err"
    done
  }

  local i=1
  while [ "$i" -le "$P1_KV_CONCURRENCY" ]; do
    kv_worker "$i" &
    i=$((i + 1))
  done
  wait
  printf 'completed kv-peak concurrency=%s duration=%ss\n' "$P1_KV_CONCURRENCY" "$duration" >&2
  check_kv_read_after_write "p1-kv-verify-${run_id}" "p1-verify-value-${run_id}" || true
  stack_stats "$lat" "$err"
}

stack_d1_peak() {
  init_stack_files d1
  local lat err duration deadline
  lat=$(stack_lat_file d1)
  err=$(stack_err_file d1)
  duration=$(p1_duration_sec 900)
  deadline=$(( $(date +%s) + duration ))

  d1_writer() {
    local wid=$1
    local seq=0
    while [ "$(date +%s)" -lt "$deadline" ]; do
      seq=$((seq + 1))
      local body
      body=$(jq -nc --arg id "p1-d1-${run_id}-w${wid}-${seq}" \
        '{orderId: $id, status: "created", payloadBytes: 256}')
      stress_request "/stack/d1/orders" POST "$body" "$lat" "$err"
    done
  }

  d1_reader() {
    while [ "$(date +%s)" -lt "$deadline" ]; do
      stress_request "/stack/d1/orders?status=created&limit=50" GET "" "$lat" "$err"
    done
  }

  local i=1
  while [ "$i" -le "$P1_D1_WRITERS" ]; do
    d1_writer "$i" &
    i=$((i + 1))
  done
  i=1
  while [ "$i" -le "$P1_D1_READERS" ]; do
    d1_reader &
    i=$((i + 1))
  done
  wait
  printf 'completed d1-peak writers=%s readers=%s duration=%ss\n' \
    "$P1_D1_WRITERS" "$P1_D1_READERS" "$duration" >&2
  sample_stack_response GET "/stack/d1/orders?status=created&limit=10" "" "200" >/dev/null
  stack_stats "$lat" "$err"
}

stack_r2_peak() {
  init_stack_files r2
  local lat err duration
  lat=$(stack_lat_file r2)
  err=$(stack_err_file r2)
  duration=$(p1_duration_sec 1200)

  multipart_upload() {
    local slot=$1
    local key="p1-r2-mp-${run_id}-${slot}"
    local start_ms end_ms latency status
    start_ms=$(now_ms)
    if ! status=$(python3 -c "import sys; sys.stdout.buffer.write(b'x' * 65536)" | curl -sS -H "$host_header" -X PUT \
      -o "${STRESS_RUN_DIR}/r2-mp-${slot}.json" -w '%{http_code}' \
      --max-time 120 \
      --data-binary @- \
      "${base_url}/stack/r2/objects/${key}?multipart=1"); then
      status=000
    fi
    end_ms=$(now_ms)
    latency=$((end_ms - start_ms))
    printf '%s\n' "$latency" >>"$lat"
    case "$status" in
      200) ;;
      *) printf 'PUT /stack/r2/objects/%s %s\n' "$key" "$status" >>"$err" ;;
    esac
    printf '%s\n' "$key" >>"${STRESS_RUN_DIR}/r2-keys.txt"
  }

  : >"${STRESS_RUN_DIR}/r2-keys.txt"
  local small_key="p1-r2-small-${run_id}"
  local small_put_status attempt
  small_put_status=000
  attempt=1
  while [ "$attempt" -le 5 ]; do
    wait_for_ready "$base_url" 30 || true
    small_put_status=$(curl -sS -H "$host_header" -X PUT \
      -o "${STRESS_RUN_DIR}/r2-small-put.json" -w '%{http_code}' \
      --data-binary "p1-r2-small-payload" \
      "${base_url}/stack/r2/objects/${small_key}" || echo 000)
    if [ "$small_put_status" = "200" ]; then
      break
    fi
    sleep 3
    attempt=$((attempt + 1))
  done
  if [ "$small_put_status" != "200" ]; then
    record_anomaly "http_status" "$(jq -nc \
      --arg key "$small_key" \
      --arg status "$small_put_status" \
      '{phase: "p1_r2_small_put", key: $key, status: $status}')"
    stack_stats "$lat" "$err"
    return 0
  fi

  local i=1
  while [ "$i" -le 4 ]; do
    multipart_upload "$i" &
    i=$((i + 1))
  done
  wait
  if ! wait_for_ready "$base_url" 30; then
    record_anomaly "health_ready" "$(jq -nc '{phase: "p1_r2_post_multipart"}')"
  fi
  sleep 5

  run_rate_until "r2-get-peak" "$(p1_rate "$P1_R2_GET_RATE")" "$duration" \
    "/stack/r2/objects/${small_key}" GET "" "$lat" "$err"
  printf 'completed r2-peak multipart=4 get_rate=%s/s duration=%ss\n' "$P1_R2_GET_RATE" "$duration" >&2
  sample_stack_response GET "/stack/r2/objects/${small_key}" "" "200" >/dev/null
  stack_stats "$lat" "$err"
}

stack_queue_peak() {
  init_stack_files queue
  local lat err duration
  lat=$(stack_lat_file queue)
  err=$(stack_err_file queue)
  duration=$(p1_duration_sec 600)
  local body_template='{"label":"p1-q-__RUN__-__SEQ__","payload":{"run":"__RUN__","seq":__SEQ__}}'
  run_rate_until "queue-peak" "$(p1_rate "$P1_QUEUE_RATE")" "$duration" \
    "/stack/queue/enqueue" POST "$body_template" "$lat" "$err"
  local verify_label="p1-q-${run_id}-$((seq > 20 ? seq - 20 : 1))"
  if ! wait_queue_label "$verify_label" 120; then
    record_anomaly "queue_not_processed" "$(jq -nc --arg label "$verify_label" '{label: $label, phase: "p1_queue_peak"}')"
  fi
  sample_stack_response GET "/stack/queue/dequeue-verify?label=${verify_label}" "" "200" >/dev/null
  stack_stats "$lat" "$err"
}

stack_do_peak() {
  init_stack_files do
  local lat err duration object_id
  lat=$(stack_lat_file do)
  err=$(stack_err_file do)
  duration=$(p1_duration_sec 900)
  object_id="p1-do-${run_id}"

  start_do_websocket_pool "$(p1_rate "$P1_DO_WS_COUNT")" "$object_id" "$duration"
  run_rate_until "do-rpc-peak" "$(p1_rate "$P1_DO_RPC_RATE")" "$duration" \
    "/stack/do/${object_id}/increment" POST '{"amount":1}' "$lat" "$err"
  stop_do_websocket_pool
  recover_before_sample
  check_do_counter_monotonic "$object_id" || true
  sample_stack_response POST "/stack/do/${object_id}/increment" '{"amount":0}' "200" >/dev/null
  stack_stats "$lat" "$err"
}

stack_workflow_peak() {
  init_stack_files workflow
  local lat err duration
  lat=$(stack_lat_file workflow)
  err=$(stack_err_file workflow)
  duration=$(p1_duration_sec 900)
  local body_template='{"orderId":"p1-wf-__RUN__-__SEQ__","mode":"peak","fanOutN":1}'
  run_rate_until "workflow-peak" "$(p1_rate "$P1_WORKFLOW_RATE")" "$duration" \
    "/stack/workflow/checkout" POST "$body_template" "$lat" "$err"
  sample_stack_response POST "/stack/workflow/checkout" \
    "$(jq -nc --arg id "p1-wf-sample-${run_id}" '{orderId: $id, mode: "normal", fanOutN: 2}')" \
    "202" >/dev/null
  stack_stats "$lat" "$err"
}

stack_fetch_peak() {
  init_stack_files fetch
  local lat err duration
  lat=$(stack_lat_file fetch)
  err=$(stack_err_file fetch)
  duration=$(p1_duration_sec 600)
  run_concurrent_until "fetch-peak" "$P1_FETCH_CONCURRENCY" "$duration" \
    "/stack/fetch/probe?hops=1" GET "" "$lat" "$err"
  sample_stack_response GET "/stack/fetch/probe?hops=1" "" "200" >/dev/null
  stack_stats "$lat" "$err"
}

stack_cpu_peak() {
  init_stack_files cpu
  local lat err duration
  lat=$(stack_lat_file cpu)
  err=$(stack_err_file cpu)
  duration=$(p1_duration_sec 600)
  run_concurrent_until "cpu-peak" "$P1_CPU_CONCURRENCY" "$duration" \
    "/stack/cpu/spin" POST '{"iterations":80000}' "$lat" "$err"
  sample_stack_response POST "/stack/cpu/spin" '{"iterations":40000}' "200" >/dev/null
  stack_stats "$lat" "$err"
}

stack_scenario_mega_peak() {
  init_stack_files scenario_mega
  local lat err duration deadline
  lat=$(stack_lat_file scenario_mega)
  err=$(stack_err_file scenario_mega)
  duration=$(p1_duration_sec 900)
  deadline=$(( $(date +%s) + duration ))

  mega_worker() {
    local wid=$1
    local seq=0
    while [ "$(date +%s)" -lt "$deadline" ]; do
      seq=$((seq + 1))
      local body
      body=$(jq -nc \
        --arg orderId "p1-mega-${run_id}-w${wid}-${seq}" \
        '{orderId: $orderId, mode: "normal", fanOutN: 5, fanOutM: 2, payloadBytes: 2048}')
      stress_request "/stack/scenario/mega-checkout" POST "$body" "$lat" "$err"
    done
  }

  local i=1
  while [ "$i" -le "$P1_MEGA_CONCURRENCY" ]; do
    mega_worker "$i" &
    i=$((i + 1))
  done
  wait
  printf 'completed scenario_mega-peak concurrency=%s duration=%ss\n' "$P1_MEGA_CONCURRENCY" "$duration" >&2
  sample_stack_response POST "/stack/scenario/mega-checkout" \
    "$(jq -nc --arg id "p1-mega-sample-${run_id}" '{orderId: $id, mode: "normal", fanOutN: 3}')" \
    "200" >/dev/null
  stack_stats "$lat" "$err"
}

run_stack_peak() {
  local name=$1
  printf '\n=== P1 peak: %s ===\n' "$name" >&2
  if ! wait_for_ready "$base_url" 30; then
    record_anomaly "health_ready" "$(jq -nc --arg stack "$name" '{phase: "pre_stack_peak", stack: $stack}')"
    return 1
  fi
  stack_"${name}"_peak
}

STACK_ORDER="http kv d1 r2 queue do workflow fetch cpu"
selected=${STRESS_P1_STACK:-$STACK_ORDER}
P1_COOLDOWN_SEC=${STRESS_P1_COOLDOWN_SEC:-60}
first_stack=1

for stack in $selected; do
  case "$stack" in
    http|kv|d1|r2|queue|do|workflow|fetch|cpu|scenario_mega)
      if [ "$first_stack" -eq 0 ]; then
        printf 'cooldown %ss before %s\n' "$P1_COOLDOWN_SEC" "$stack" >&2
        sleep "$P1_COOLDOWN_SEC"
      fi
      first_stack=0
      run_stack_peak "$stack"
      ;;
    *)
      echo "unknown stack: $stack" >&2
      exit 1
      ;;
  esac
done

check_health_post_run "$base_url" || true
check_orphan_workerd || true
check_container_restarts open-compute-ocd || true

timestamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
RESULT_DIR="$STRESS_RUN_DIR" RUN_ID="$run_id" TIMESTAMP="$timestamp" \
  WORKER_HOST="$worker_host" P1_SCALE="$P1_SCALE" STRESS_PROFILE="$STRESS_PROFILE" \
  python3 - <<'PY' >"${STRESS_RUN_DIR}/result.json"
import json
import os
from pathlib import Path

result_dir = Path(os.environ["RESULT_DIR"])
profile = os.environ["STRESS_PROFILE"]
slo_2c4g = {
    "http": {"p95_ms": 2500, "p99_ms": 3500, "error_rate_max": 0.01},
    "kv": {"p95_ms": 5000, "p99_ms": 6000, "error_rate_max": 0.01},
    "d1": {"p95_ms": 3000, "p99_ms": 10000, "error_rate_max": 0.10},
    "r2": {"p95_ms": 1000, "p99_ms": 3000, "error_rate_max": 0.15},
    "queue": {"p95_ms": 900, "p99_ms": 2000, "error_rate_max": 0.01},
    "do": {"p95_ms": 1500, "p99_ms": 3000, "error_rate_max": 0.15},
    "workflow": {"p95_ms": 2000, "p99_ms": 4000, "error_rate_max": 0.10},
    "fetch": {"p95_ms": 2000, "p99_ms": 3000, "error_rate_max": 0.01},
    "cpu": {"p95_ms": 3000, "p99_ms": 6000, "error_rate_max": 0.01},
    "scenario_mega": {"p95_ms": 3000, "p99_ms": 6000, "error_rate_max": 0.05},
}
slo_8c16g = {
    "http": {"p95_ms": 1500, "p99_ms": 2500, "error_rate_max": 0.01},
    "kv": {"p95_ms": 3000, "p99_ms": 5000, "error_rate_max": 0.01},
    "d1": {"p95_ms": 2000, "p99_ms": 8000, "error_rate_max": 0.02},
    "r2": {"p95_ms": 800, "p99_ms": 2000, "error_rate_max": 0.05},
    "queue": {"p95_ms": 700, "p99_ms": 1500, "error_rate_max": 0.01},
    "do": {"p95_ms": 1000, "p99_ms": 2000, "error_rate_max": 0.05},
    "workflow": {"p95_ms": 1500, "p99_ms": 3000, "error_rate_max": 0.05},
    "fetch": {"p95_ms": 1200, "p99_ms": 2000, "error_rate_max": 0.01},
    "cpu": {"p95_ms": 2500, "p99_ms": 5000, "error_rate_max": 0.01},
    "scenario_mega": {"p95_ms": 2500, "p99_ms": 5000, "error_rate_max": 0.03},
}
slo = slo_2c4g if profile == "2c4g" else slo_8c16g
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

    latency = {"p50": pct(50), "p95": pct(95), "p99": pct(99)}
    threshold = slo.get(name, {"p95_ms": 5000, "p99_ms": 10000, "error_rate_max": 0.02})
    stack_anomalies = []
    verdict = "pass"
    if error_rate > threshold["error_rate_max"]:
        stack_anomalies.append({"type": "slo_error_rate", "actual": error_rate, "max": threshold["error_rate_max"]})
        verdict = "fail"
    if latency["p95"] > threshold["p95_ms"]:
        stack_anomalies.append({"type": "slo_p95", "actual": latency["p95"], "max": threshold["p95_ms"]})
        verdict = "fail"
    if latency["p99"] > threshold.get("p99_ms", threshold["p95_ms"] * 2):
        stack_anomalies.append({"type": "slo_p99", "actual": latency["p99"], "max": threshold.get("p99_ms")})
        verdict = "fail"

    stacks[name] = {
        "samples": total,
        "errors": errors,
        "error_rate": round(error_rate, 6),
        "latency_ms": latency,
        "anomalies": stack_anomalies,
        "verdict": verdict,
    }

scale_suffix = "" if float(os.environ["P1_SCALE"]) >= 0.99 else "-abbrev"
output = {
    "schema_version": 2,
    "profile": f"p1-{profile}-peak{scale_suffix}",
    "stress_profile": profile,
    "run_id": os.environ["RUN_ID"],
    "timestamp": os.environ["TIMESTAMP"],
    "worker_host": os.environ["WORKER_HOST"],
    "p1_scale": float(os.environ["P1_SCALE"]),
    "stacks": stacks,
    "scenario": {"mega-checkout": stacks.get("scenario_mega", {})},
    f"slo_thresholds_{profile}": slo,
    "verdict": "pass" if all(s["verdict"] == "pass" for s in stacks.values()) else "fail",
}
print(json.dumps(output, indent=2))
PY

if jq -e '.verdict == "fail"' "${STRESS_RUN_DIR}/result.json" >/dev/null 2>&1; then
  failed_stacks=$(jq -c '[.stacks | to_entries[] | select(.value.verdict == "fail") | {stack: .key, anomalies: .value.anomalies}]' \
    "${STRESS_RUN_DIR}/result.json")
  record_anomaly "slo_threshold" "$(jq -nc --argjson stacks "$failed_stacks" '{failed_stacks: $stacks}')"
fi

finalize_verdict "${STRESS_RUN_DIR}/result.json"
