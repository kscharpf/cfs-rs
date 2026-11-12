# cfs-rs

Rust replacements for modules of NASA's [core Flight System](https://github.com/nasa/cFS) (cFS), one module at a time, behind the unchanged C API. The first module is cFE **MSG**, the message header accessors.

This is a learning project. It is not flight software and has no flight heritage.

## How it works

cFS is not forked or patched. The `cfs` directory is a git submodule at an unmodified release of the cFS bundle (v7.0.1), and the cFS CMake build stays the top-level build.

- **Module override.** The cFS build searches the directories of `CFS_APP_PATH` before its own. With `CFS_APP_PATH` set to `modules/`, it finds `modules/msg` here in place of `cfe/modules/msg`.
- **One function at a time.** `modules/msg` builds the stock C module, then, for each function listed in [`modules/msg/replaced.cmake`](modules/msg/replaced.cmake), compiles the C implementation under the name `<function>_c` and links the Rust one under the real name. A function that is not listed is still the C one.
- **The C tests are the oracle.** The MSG unit tests of cFE run unchanged against the mixed library. The renamed C functions stay linked, for differential tests later.

## Layout

| Path | Content |
|---|---|
| `crates/cfe-msg` | Safe Rust core, `no_std`, no `unsafe`: the behaviour of each `CFE_MSG_*` function. Uses [Aldis](https://github.com/kscharpf/aldis) for Space Packet behaviour. |
| `crates/cfe-msg-ffi` | The static library: `extern "C"` exports under the cFE names. All `unsafe` goes here. The header is cFE's own `cfe_msg.h`. |
| `modules/msg` | CMake glue that the cFS build uses in place of `cfe/modules/msg`. |
| `cfs` | The cFS bundle, a submodule. Do not edit it. |
| `scripts`, `Containerfile`, `Makefile` | The build environment and entry points. |

## Build and test

Clone with `git clone --recurse-submodules`. cFS builds on Linux, so the cFS builds run in a container (`podman` by default; `make CONTAINER=docker …` for Docker).

| Command | Where | What it does |
|---|---|---|
| `make check` | host | `cargo fmt`, `clippy`, and the Rust unit tests |
| `make image` | host | Builds the container image, once |
| `make stock` | container | Builds cFS as released and runs its MSG unit tests: the baseline |
| `make rs` | container | Builds cFS with the Rust MSG module and runs the same tests |

## Moving a function to Rust

1. Implement the behaviour in `crates/cfe-msg`, with a test against the C behaviour.
2. Export it in `crates/cfe-msg-ffi` with the name and signature of `cfe_msg.h`.
3. Add the name to `modules/msg/replaced.cmake`.
4. Run `make check` and `make rs`.

## Licence

Apache-2.0, the licence of cFS; see [LICENSE](LICENSE) and [NOTICE](NOTICE).
