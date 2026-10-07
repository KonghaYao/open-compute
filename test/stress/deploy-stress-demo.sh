#!/bin/sh
# Bootstrap resources and deploy examples/stress-demo to a running compose stack.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
container_env="${root}/examples/container/.env"
deploy_env="${root}/.temp/stress-run/.deploy_env"
config_file="${root}/examples/stress-demo/cloudflare.config.ts"
config_backup="${root}/.temp/stress-run/cloudflare.config.ts.bak"
port=${OC_PUBLIC_PORT:-8788}
cf_cli="node ${root}/node_modules/cf/bin/cf"

mkdir -p "${root}/.temp/stress-run"

admin_token=$(grep '^OPEN_COMPUTE_ADMIN_TOKEN=' "$container_env" | cut -d= -f2-)
account_id=$(curl -fsS -H "Authorization: Bearer ${admin_token}" \
  "http://127.0.0.1:${port}/client/v4/accounts" | python3 -c 'import json,sys; p=json.load(sys.stdin); r=p["result"] if isinstance(p,dict) and p.get("success") else p; print(r[0]["id"])')
deployer_token=$(docker run --rm -v open-compute_ocd-data:/data:ro alpine cat /data/instances/default/data/keys/deployer.token)

cat >"$deploy_env" <<EOF
export CLOUDFLARE_API_BASE_URL=http://127.0.0.1:${port}/client/v4
export CLOUDFLARE_ACCOUNT_ID=${account_id}
export CLOUDFLARE_API_TOKEN=${deployer_token}
export STRESS_BASE_URL=http://127.0.0.1:${port}
export STRESS_ACCOUNT_ID=${account_id}
EOF
chmod 600 "$deploy_env"

set -a
# shellcheck disable=SC1090
. "$deploy_env"
set +a

cd "${root}/examples/stress-demo"
CF_SEND_TELEMETRY=false $cf_cli kv namespaces create --title stress-demo-kv >/dev/null 2>&1 || true
CF_SEND_TELEMETRY=false $cf_cli d1 create --name stress-demo-db >/dev/null 2>&1 || true
CF_SEND_TELEMETRY=false $cf_cli r2 buckets create --name stress-demo-bucket >/dev/null 2>&1 || true
CF_SEND_TELEMETRY=false $cf_cli queues create --queue-name stress-demo-events >/dev/null 2>&1 || true

kv_namespace_id=$(CF_SEND_TELEMETRY=false $cf_cli kv namespaces list \
  | python3 -c 'import json,sys; rows=json.load(sys.stdin); print(next(r["id"] for r in rows if r.get("title")=="stress-demo-kv"))')
d1_database_id=$(CF_SEND_TELEMETRY=false $cf_cli d1 list \
  | python3 -c 'import json,sys; rows=json.load(sys.stdin); print(next(r["uuid"] for r in rows if r.get("name")=="stress-demo-db"))')

cat >>"$deploy_env" <<EOF
export STRESS_KV_NAMESPACE_ID=${kv_namespace_id}
export STRESS_D1_DATABASE_ID=${d1_database_id}
EOF

worker_exists=$(curl -fsS -H "Authorization: Bearer ${deployer_token}" \
  "${CLOUDFLARE_API_BASE_URL}/accounts/${account_id}/workers/scripts" \
  | python3 -c 'import json,sys; payload=json.load(sys.stdin); rows=payload.get("result") or []; print("true" if any(row.get("id")=="stress-demo" for row in rows) else "false")')

cp "$config_file" "$config_backup"
restore_config() {
  cp "$config_backup" "$config_file"
}
trap restore_config EXIT INT TERM

patch_stress_config() {
  include_service=$1
  STRESS_KV_NAMESPACE_ID=$kv_namespace_id \
  STRESS_D1_DATABASE_ID=$d1_database_id \
  STRESS_OUTBOUND_URL="http://127.0.0.1:${port}/health/live" \
  STRESS_INCLUDE_SERVICE=$include_service \
  STRESS_CONFIG_PATH=$config_file \
  python3 - <<'PY'
import os
import re
from pathlib import Path

path = Path(os.environ["STRESS_CONFIG_PATH"])
text = path.read_text()
kv_id = os.environ["STRESS_KV_NAMESPACE_ID"]
d1_id = os.environ["STRESS_D1_DATABASE_ID"]
outbound_url = os.environ["STRESS_OUTBOUND_URL"]
include_service = os.environ["STRESS_INCLUDE_SERVICE"] == "true"

text = re.sub(
    r'bindings\.kv\(\{ id: "[^"]+" \}\)',
    f'bindings.kv({{ id: "{kv_id}" }})',
    text,
    count=1,
)
text = re.sub(
    r'(bindings\.d1\(\{\s*id: ")[^"]+(")',
    rf'\g<1>{d1_id}\2',
    text,
    count=1,
)
text = re.sub(
    r'bindings\.text\("http://127\.0\.0\.1:[0-9]+/health/live"\)',
    f'bindings.text("{outbound_url}")',
    text,
    count=1,
)
if not include_service:
    text = text.replace(
        """      SERVICE: bindings.worker({
        worker: "stress-demo",
        exportName: "InternalApi",
      }),
""",
        "",
    )
path.write_text(text)
PY
}

deploy_stress_demo() {
  bun run build
  CF_SEND_TELEMETRY=false DO_NOT_TRACK=1 bun run deploy:local
}

sh "${root}/test/stress/generate-data.sh"

if [ "$worker_exists" = "false" ]; then
  patch_stress_config false
  deploy_stress_demo
fi

patch_stress_config true
deploy_stress_demo

queue_id=$(CF_SEND_TELEMETRY=false $cf_cli queues list 2>/dev/null \
  | python3 -c 'import json,sys; rows=json.load(sys.stdin); print(next(r["queue_id"] for r in rows if r.get("queue_name")=="stress-demo-events"))' 2>/dev/null || true)
if [ -n "$queue_id" ]; then
  CF_SEND_TELEMETRY=false $cf_cli queues consumers create "$queue_id" \
    --script-name stress-demo --type worker >/dev/null 2>&1 || true
fi

trap - EXIT INT TERM
restore_config

printf 'stress-demo deployed to %s (account %s, kv=%s, d1=%s)\n' \
  "$STRESS_BASE_URL" "$STRESS_ACCOUNT_ID" "$kv_namespace_id" "$d1_database_id"
