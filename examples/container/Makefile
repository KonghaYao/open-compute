# Local helpers for the container example. CI downloads release binaries automatically.
VERSION ?= v0.2.4
TARGET ?= linux-arm64
RELEASE_REPO ?= elliothux/open-compute

.PHONY: fetch-ocd fetch-ocd-all image init-image compose-config up

fetch-ocd:
	curl -fsSL --http1.1 -o ocd.$(TARGET) \
		"https://github.com/$(RELEASE_REPO)/releases/download/$(VERSION)/ocd-$(VERSION)-$(TARGET)"
	chmod 0555 ocd.$(TARGET)

fetch-ocd-all:
	$(MAKE) fetch-ocd TARGET=linux-x64
	$(MAKE) fetch-ocd TARGET=linux-arm64

image: fetch-ocd-all
	docker build -t open-compute:local -f Dockerfile .

init-image: fetch-ocd-all
	docker build -t open-compute-init:local -f Dockerfile.init .

compose-config:
	docker compose config

up: image init-image
	docker compose up -d --build
