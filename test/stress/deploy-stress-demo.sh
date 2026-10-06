#!/bin/sh
# Bootstrap resources and deploy examples/stress-demo to a running compose stack.
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
container_env="${root}/examples/container/.env"
deploy_env="${root}/.temp/stress-run/.deploy_env"
port=${OC_PUBLIC_PORT:-8788}

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
CF_SEND_TELEMETRY=false node "${root}/node_modules/cf/bin/cf" kv namespaces create --title stress-demo-kv >/dev/null 2>&1 || true
CF_SEND_TELEMETRY=false node "${root}/node_modules/cf/bin/cf" d1 create --name stress-demo-db >/dev/null 2>&1 || true
CF_SEND_TELEMETRY=false node "${root}/node_modules/cf/bin/cf" r2 buckets create --name stress-demo-bucket >/dev/null 2>&1 || true
CF_SEND_TELEMETRY=false node "${root}/node_modules/cf/bin/cf" queues create --queue-name stress-demo-events >/dev/null 2>&1 || true

sh "${root}/test/stress/generate-data.sh"

bun run build
CF_SEND_TELEMETRY=false DO_NOT_TRACK=1 bun run deploy:local

queue_id=$(CF_SEND_TELEMETRY=false node "${root}/node_modules/cf/bin/cf" queues list 2>/dev/null \
  | python3 -c 'import json,sys; rows=json.load(sys.stdin); print(next(r["queue_id"] for r in rows if r.get("queue_name")=="stress-demo-events"))' 2>/dev/null || true)
if [ -n "$queue_id" ]; then
  CF_SEND_TELEMETRY=false node "${root}/node_modules/cf/bin/cf" queues consumers create "$queue_id" \
    --script-name stress-demo --type worker >/dev/null 2>&1 || true
fi

printf 'stress-demo deployed to %s (account %s)\n' "$STRESS_BASE_URL" "$STRESS_ACCOUNT_ID"
