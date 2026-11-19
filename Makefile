# Entry points. `check` runs on the host; the cFS builds run in a Linux container.
CONTAINER ?= podman
IMAGE     ?= cfs-rs-build
RUN        = $(CONTAINER) run --rm --sysctl fs.mqueue.msg_max=64 -v "$(CURDIR)":/work -v cfs-rs-cargo:/cargo $(IMAGE)

.PHONY: check miri image stock rs node realtime shell clean

# Rust only: format, lints, unit tests.
check:
	cargo fmt --all --check
	cargo clippy --locked --workspace --all-targets -- -D warnings
	cargo test --locked --workspace

# Miri UB checks on the unsafe FFI crates (`cfe` and `cfe-msg-ffi`).
miri:
	cargo +$$(cat tooling-nightly) miri test --locked -p cfe -p cfe-msg-ffi

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

# Realtime build with sch_lab and link_lab on rs MSG, with UDP ground verification.
realtime:
	$(RUN) scripts/cfs-realtime.sh

shell:
	$(CONTAINER) run --rm -it -v "$(CURDIR)":/work -v cfs-rs-cargo:/cargo $(IMAGE) bash

clean:
	rm -rf build target
