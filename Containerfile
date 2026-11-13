# Build environment for cFS with the Rust MSG module: cFS builds on Linux, not on macOS.
FROM docker.io/library/ubuntu:24.04

RUN apt-get update \
 && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        build-essential ca-certificates cmake curl git \
 && rm -rf /var/lib/apt/lists/*

# The toolchain and components of rust-toolchain.toml, installed here so that no build downloads
# them: keep the two in step. CARGO_HOME is a volume so Cargo's cache is kept between runs.
ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/cargo \
    PATH=/opt/rustup-bin:/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | CARGO_HOME=/opt/rustup-init sh -s -- -y --no-modify-path --profile minimal --default-toolchain 1.100.0 \
          --component clippy,rustfmt \
 && mkdir -p /opt/rustup-bin \
 && ln -s /opt/rustup-init/bin/* /opt/rustup-bin/

WORKDIR /work
