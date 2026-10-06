# Build context contains native Linux release executables:
#   ocd.linux-x64, ocd.linux-arm64
# Use the same libc baseline as the Linux release builder; workerd is not a static ELF.
FROM ubuntu:24.04 AS base
ARG TARGETARCH
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl gettext-base libgcc-s1 login \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -g 65532 open-compute \
    && useradd -u 65532 -g 65532 -M -s /usr/sbin/nologin open-compute
COPY ocd.linux-x64 ocd.linux-arm64 /tmp/
RUN mkdir -p /opt/open-compute \
    && case "${TARGETARCH}" in \
      amd64) cp /tmp/ocd.linux-x64 /opt/open-compute/ocd ;; \
      arm64) cp /tmp/ocd.linux-arm64 /opt/open-compute/ocd ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && chmod 0555 /opt/open-compute/ocd \
    && rm -f /tmp/ocd.linux-x64 /tmp/ocd.linux-arm64

FROM base AS runtime
LABEL org.opencontainers.image.title="open-compute" \
      org.opencontainers.image.url="https://open-compute.dev"
USER 65532:65532
WORKDIR /var/lib/open-compute
VOLUME ["/var/lib/open-compute"]
ENTRYPOINT ["/opt/open-compute/ocd"]
CMD ["--system", "run"]

FROM base AS init
COPY init-volume.sh /usr/local/bin/init-volume.sh
RUN chmod 0555 /usr/local/bin/init-volume.sh
ENTRYPOINT ["/usr/local/bin/init-volume.sh"]
