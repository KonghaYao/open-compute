#!/bin/sh
# One-shot volume bootstrap for docker compose. Modeled after test/gateway/runtime-smoke.sh.
set -eu

OCD=/opt/open-compute/ocd
ROOT=/var/lib/open-compute
MARKER="${ROOT}/.compose-initialized"
ADMIN_TOKEN="${OPEN_COMPUTE_ADMIN_TOKEN:?set OPEN_COMPUTE_ADMIN_TOKEN}"
DEPLOYER_TOKEN="${OPEN_COMPUTE_DEPLOYER_TOKEN:?set OPEN_COMPUTE_DEPLOYER_TOKEN}"
READONLY_TOKEN="${OPEN_COMPUTE_READ_ONLY_TOKEN:?set OPEN_COMPUTE_READ_ONLY_TOKEN}"

if [ -f "${MARKER}" ]; then
  echo "init-volume: already initialized"
  exit 0
fi

export OC_PUBLIC_PORT="${OC_PUBLIC_PORT:-8787}"
export OC_PUBLIC_BIND="${OC_PUBLIC_BIND:-0.0.0.0:${OC_PUBLIC_PORT}}"
export OC_PUBLIC_BASE_DOMAIN="${OC_PUBLIC_BASE_DOMAIN:-open-compute.dev}"
export OC_STORAGE_BACKEND="${OC_STORAGE_BACKEND:-local}"
export OC_S3_ENDPOINT="${OC_S3_ENDPOINT:-http://127.0.0.1:9000}"
export OC_S3_REGION="${OC_S3_REGION:-us-east-1}"
export OC_S3_BUCKET="${OC_S3_BUCKET:-open-compute}"
export OC_S3_PREFIX="${OC_S3_PREFIX:-open-compute/system/}"
export OC_S3_R2_PREFIX="${OC_S3_R2_PREFIX:-open-compute/tenant/r2/}"

write_ocd_config() {
  envsubst <<EOF >"${ROOT}/ocd.toml"
[server]
public_bind = "${OC_PUBLIC_BIND}"
admin_auth = { file = "${ROOT}/keys/admin.token" }
EOF
  chmod 600 "${ROOT}/ocd.toml"
}

write_compute_config() {
  CONFIG="${ROOT}/instances/default/compute.toml"
  if [ "${OC_STORAGE_BACKEND}" = "s3" ]; then
    envsubst <<EOF >"${CONFIG}"
[instance]
name = "default"

[auth]
deployer_auth = { env = "OPEN_COMPUTE_DEPLOYER_TOKEN" }
read_only_auth = { env = "OPEN_COMPUTE_READ_ONLY_TOKEN" }

[data]
path = "${ROOT}/instances/default/data"
master_key_file = "${ROOT}/instances/default/data/keys/master.key"
sqlite_busy_timeout_ms = 5000
free_space_soft_bytes = 1073741824
free_space_hard_bytes = 268435456

[storage]
backend = "s3"
endpoint = "${OC_S3_ENDPOINT}"
region = "${OC_S3_REGION}"
bucket = "${OC_S3_BUCKET}"
force_path_style = true
prefix = "${OC_S3_PREFIX}"
r2_prefix = "${OC_S3_R2_PREFIX}"
access_key_id_env = "OC_S3_ACCESS_KEY_ID"
secret_access_key_env = "OC_S3_SECRET_ACCESS_KEY"

[runtime]
startup_timeout_ms = 20000
shutdown_grace_ms = 10000

[artifacts]
public_origin = "http://127.0.0.1:${OC_PUBLIC_PORT}"

[dashboard]
enabled = true

[public_gateway]
base_domain = "${OC_PUBLIC_BASE_DOMAIN}"

[observability]
external_control_origin = "http://127.0.0.1:${OC_PUBLIC_PORT}"

[metrics]
enabled = true
EOF
  else
    envsubst <<EOF >"${CONFIG}"
[instance]
name = "default"

[auth]
deployer_auth = { env = "OPEN_COMPUTE_DEPLOYER_TOKEN" }
read_only_auth = { env = "OPEN_COMPUTE_READ_ONLY_TOKEN" }

[data]
path = "${ROOT}/instances/default/data"
master_key_file = "${ROOT}/instances/default/data/keys/master.key"
sqlite_busy_timeout_ms = 5000
free_space_soft_bytes = 1073741824
free_space_hard_bytes = 268435456

[storage]
backend = "local"
prefix = "system/"
r2_prefix = "tenant/r2/"
free_space_soft_bytes = 1073741824
free_space_hard_bytes = 268435456
partial_grace_ms = 3600000

[runtime]
startup_timeout_ms = 20000
shutdown_grace_ms = 10000

[artifacts]
public_origin = "http://127.0.0.1:${OC_PUBLIC_PORT}"

[dashboard]
enabled = true

[public_gateway]
base_domain = "${OC_PUBLIC_BASE_DOMAIN}"

[observability]
external_control_origin = "http://127.0.0.1:${OC_PUBLIC_PORT}"

[metrics]
enabled = true
EOF
  fi
  chmod 600 "${CONFIG}"
}

mkdir -p "${ROOT}/keys" "${ROOT}/instances/default" "${ROOT}/tmp"
chmod 700 "${ROOT}" "${ROOT}/tmp" "${ROOT}/keys"

printf '%s\n' "${ADMIN_TOKEN}" > "${ROOT}/keys/admin.token"
chmod 600 "${ROOT}/keys/admin.token"

write_ocd_config
CONFIG="${ROOT}/instances/default/compute.toml"
DATA="${ROOT}/instances/default/data"

chown -R open-compute:open-compute "${ROOT}"

log="${ROOT}/tmp/init-daemon.log"
su -s /bin/sh open-compute -c "${OCD} --system run" >"${log}" 2>&1 &
daemon=$!

cleanup() {
  kill -TERM "${daemon}" 2>/dev/null || true
  wait "${daemon}" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

count=0
until su -s /bin/sh open-compute -c "${OCD} --system status --json" 2>/dev/null | grep -F '"state":"running"' >/dev/null; do
  if ! kill -0 "${daemon}" 2>/dev/null; then
    echo "init-volume: ocd exited during bootstrap" >&2
    cat "${log}" >&2 || true
    exit 1
  fi
  count=$((count + 1))
  if [ "${count}" -ge 180 ]; then
    echo "init-volume: timed out waiting for ocd" >&2
    cat "${log}" >&2 || true
    exit 1
  fi
  sleep 1
done

su -s /bin/sh open-compute -c "\
  OPEN_COMPUTE_DEPLOYER_TOKEN='${DEPLOYER_TOKEN}' \
  OPEN_COMPUTE_READ_ONLY_TOKEN='${READONLY_TOKEN}' \
  OC_S3_ACCESS_KEY_ID='${OC_S3_ACCESS_KEY_ID:-}' \
  OC_S3_SECRET_ACCESS_KEY='${OC_S3_SECRET_ACCESS_KEY:-}' \
  ${OCD} --system instance setup \
    --config '${CONFIG}' \
    --data-dir '${DATA}' \
    --yes --autostart true --start true"

printf '%s\n' "${DEPLOYER_TOKEN}" > "${DATA}/keys/deployer.token"
printf '%s\n' "${READONLY_TOKEN}" > "${DATA}/keys/read-only.token"
chmod 600 "${DATA}/keys/deployer.token" "${DATA}/keys/read-only.token"
chown open-compute:open-compute "${DATA}/keys/deployer.token" "${DATA}/keys/read-only.token"

if [ "${OC_STORAGE_BACKEND}" = "s3" ]; then
  write_compute_config
fi

su -s /bin/sh open-compute -c "\
  OPEN_COMPUTE_DEPLOYER_TOKEN='${DEPLOYER_TOKEN}' \
  OPEN_COMPUTE_READ_ONLY_TOKEN='${READONLY_TOKEN}' \
  OC_S3_ACCESS_KEY_ID='${OC_S3_ACCESS_KEY_ID:-}' \
  OC_S3_SECRET_ACCESS_KEY='${OC_S3_SECRET_ACCESS_KEY:-}' \
  ${OCD} --system --config '${CONFIG}' config check"

touch "${MARKER}"
chown open-compute:open-compute "${MARKER}"
echo "init-volume: bootstrap complete"
