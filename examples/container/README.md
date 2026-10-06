# Container notes

This directory ships:

- `Dockerfile` — minimal production runtime image (`ocd --system run` as UID 65532)
- `Dockerfile.init` — one-shot compose bootstrap (not a release artifact)
- `docker-compose.yml` — local object storage stack
- `docker-compose.external-s3.yml` — overlay for an existing S3-compatible bucket

The runtime image runs one `ocd` daemon for the explicitly registered instances. Each running instance
owns its own verified workerd child and data directory; the daemon owns the shared listener and
Gateway. The build context contains one native Linux release file named `ocd`; use the matching
CPU architecture. The Ubuntu 24.04 base matches the CI Linux release builder; workerd requires
glibc, so this image cannot use `scratch` or Alpine/musl.

## Quick start (local storage)

```bash
cd examples/container
cp .env.example .env
# edit tokens in .env

make up
curl -fsS http://127.0.0.1:8787/health/live
```

`make up` downloads the pinned release binary into the build context, builds the runtime/init
images, and starts compose.

## External S3 (shared RustFS / MinIO)

Use a non-overlapping bucket prefix per instance. Example with an existing Docker network:

```bash
cp .env.example .env
# set:
#   OC_STORAGE_BACKEND=s3
#   OC_EXTERNAL_NETWORK=1panel-network
#   OC_S3_ENDPOINT=http://1Panel-rustfs-R3pn:9000
#   OC_S3_ACCESS_KEY_ID=...
#   OC_S3_SECRET_ACCESS_KEY=...
#   OC_S3_BUCKET=open-compute

docker compose -f docker-compose.yml -f docker-compose.external-s3.yml up -d --build
```

To run beside another stack that already uses `8787`, change both `OC_PUBLIC_PORT` and
`OC_PUBLIC_BIND` before the first `up`. The listener is written into the data volume during
bootstrap and is not changed by a later env edit.

## Published images

GitHub Actions publishes multi-arch images to GHCR on pushes to `main` and on version tags:

- `ghcr.io/<owner>/open-compute:<version>`
- `ghcr.io/<owner>/open-compute-init:<version>`

Point compose at published images:

```env
OC_IMAGE=ghcr.io/konghayao/open-compute:0.2.4
```

and replace the `build:` sections with `image:` for `init`/`ocd` if desired.

## Runtime contract

- Run as non-root (`USER 65532`). Pre-provision `/var/lib/open-compute` and every external instance
  data directory for that UID with mode 0700.
- PID 1 is `ocd`; it owns and drains its children. There is no shell or runtime sidecar.
- Mount a writable, executable filesystem at `/var/lib/open-compute` (`OCD_DIR`). It holds
  `ocd.toml`, shared keys, Gateway state, locks, and the verified embedded runtime package. Do not
  mount it `noexec` or read-only.
- `ocd.toml` lists only each instance's `config` path and `autostart`. Each `compute.toml` must
  explicitly set `[data].path`; `instances/` is merely a convenient default location, never
  scanned. Data inside OCD_DIR must be strictly below `instances/`; external data paths are allowed.
- Supply credentials with environment variables or private files referenced by the appropriate
  config. Never bake credentials into the executable or image.
- Keep the image root read-only. Expose a non-loopback public listener only with an explicit
  admin authentication reference or a separate loopback-only admin listener.
- Local object bytes stay under each instance's data directory. S3 is optional; when instances
  share one bucket, configure non-overlapping system and R2 prefixes for each.
- Restart on process exit or `/health/live` failure, never on readiness 503.
- An image build is a deployment operation, not an automatic local validation command.

The image must use the formally pinned workerd embedded in the release executable. See
[`packages/runtime/workerd.lock.json`](../../packages/runtime/workerd.lock.json) for its identity.
