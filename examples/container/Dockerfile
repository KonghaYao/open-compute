# Build context contains native Linux release executables:
#   ocd.linux-x64, ocd.linux-arm64
# Use the same libc baseline as the Linux release builder; workerd is not a static ELF.
FROM ubuntu:24.04
ARG TARGETARCH
LABEL org.opencontainers.image.title="open-compute" \
      org.opencontainers.image.url="https://open-compute.dev"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libgcc-s1 \
    && rm -rf /var/lib/apt/lists/*
COPY ocd.linux-x64 ocd.linux-arm64 /tmp/
RUN case "${TARGETARCH}" in \
      amd64) cp /tmp/ocd.linux-x64 /opt/open-compute/ocd ;; \
      arm64) cp /tmp/ocd.linux-arm64 /opt/open-compute/ocd ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && chmod 0555 /opt/open-compute/ocd \
    && rm -f /tmp/ocd.linux-x64 /tmp/ocd.linux-arm64
USER 65532:65532
WORKDIR /var/lib/open-compute
VOLUME ["/var/lib/open-compute"]
ENTRYPOINT ["/opt/open-compute/ocd"]
CMD ["--system", "run"]
