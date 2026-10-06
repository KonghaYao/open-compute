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

if [ "${OC_STORAGE_BACKEND}" = "s3" ]; then
  COMPUTE_TEMPLATE=/etc/open-compute/compute.s3.toml.template
else
  COMPUTE_TEMPLATE=/etc/open-compute/compute.local.toml.template
fi

mkdir -p "${ROOT}/keys" "${ROOT}/instances/default" "${ROOT}/tmp"
chmod 700 "${ROOT}" "${ROOT}/tmp" "${ROOT}/keys"

printf '%s\n' "${ADMIN_TOKEN}" > "${ROOT}/keys/admin.token"
chmod 600 "${ROOT}/keys/admin.token"

envsubst < /etc/open-compute/ocd.toml.template > "${ROOT}/ocd.toml"
chmod 600 "${ROOT}/ocd.toml"

CONFIG="${ROOT}/instances/default/compute.toml"
envsubst < "${COMPUTE_TEMPLATE}" > "${CONFIG}"
chmod 600 "${CONFIG}"

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
  ${OCD} --system --config '${CONFIG}' config check"

su -s /bin/sh open-compute -c "\
  OPEN_COMPUTE_DEPLOYER_TOKEN='${DEPLOYER_TOKEN}' \
  OPEN_COMPUTE_READ_ONLY_TOKEN='${READONLY_TOKEN}' \
  OC_S3_ACCESS_KEY_ID='${OC_S3_ACCESS_KEY_ID:-}' \
  OC_S3_SECRET_ACCESS_KEY='${OC_S3_SECRET_ACCESS_KEY:-}' \
  ${OCD} --system instance add --config '${CONFIG}'"

touch "${MARKER}"
chown open-compute:open-compute "${MARKER}"
echo "init-volume: bootstrap complete"
