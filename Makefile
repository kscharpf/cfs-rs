# Entry points. `check` runs on the host; the cFS builds run in a Linux container.
CONTAINER ?= podman
IMAGE     ?= cfs-rs-build
RUN        = $(CONTAINER) run --rm --sysctl fs.mqueue.msg_max=64 -v "$(CURDIR)":/work -v cfs-rs-cargo:/cargo $(IMAGE)

.PHONY: check image stock rs node shell clean

# Rust only: format, lints, unit tests.
check:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

image:
	$(CONTAINER) build -t $(IMAGE) -f Containerfile .

# cFS as released, with its MSG unit tests: the baseline.
stock:
	$(RUN) scripts/cfs-build.sh stock

# cFS with the Rust MSG module, with the same MSG unit tests.
rs:
	$(RUN) scripts/cfs-build.sh rs

# Node build with link_lab and step_lab on stock and rs MSG, with checks.
node:
	$(RUN) scripts/cfs-node.sh

shell:
	$(CONTAINER) run --rm -it -v "$(CURDIR)":/work -v cfs-rs-cargo:/cargo $(IMAGE) bash

clean:
	rm -rf build target
