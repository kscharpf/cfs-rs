# AGENTS.md

Rules for coding agents working on **cfs-rs**. Mechanics and process details are in [docs/process/coordinator.md](docs/process/coordinator.md), [docs/process/review.md](docs/process/review.md), [docs/process/agent-brief.md](docs/process/agent-brief.md), and [docs/process/decision-rights.md](docs/process/decision-rights.md).

## Sources of truth

1. **cFS (`cfs/`)** is the reference for C signatures, types, status codes, and behaviour (`cfs/cfe/modules/core_api/fsw/inc/cfe_msg.h`, `cfs/cfe/modules/msg/fsw/src/`). **Do not edit anything inside `cfs/`**.
2. **Aldis** (`aldis` dependency in `Cargo.toml`) provides CCSDS Space Packet and time-code primitives. Reuse Aldis types rather than reimplementing CCSDS packet logic; implement here only what cFS adds on top.
3. **cFE MSG unit tests** (`coverage-msg-ALL`, run via `make rs`) are the behavioural oracle for the C ABI.

## Crate and module boundaries

| Path | Role | Rules |
|---|---|---|
| `crates/cfe-msg` | Safe Rust core | `#![no_std]`, `#![forbid(unsafe_code)]`. Pure Rust types and functions; no raw pointers or C types. |
| `crates/cfe-msg-ffi` | C ABI exports | `#![cfg_attr(not(test), no_std)]` staticlib. `#[unsafe(no_mangle)] pub extern "C"` matching `cfe_msg.h` exactly. All `unsafe` lives here, each block with a `// SAFETY:` comment. |
| `modules/msg/replaced.cmake` | Symbol switch | Lists every `CFE_MSG_*` function exported by `cfe-msg-ffi`. Keep 1:1 with `#[unsafe(no_mangle)]` exports. |
| `cfs/` | cFS v7.0.1 submodule | Read-only reference. Never modify. |

## Code rules

- **Panic-free:** `cfe-msg` and `cfe-msg-ffi` link with `panic = "abort"` into C. Workspace lints deny `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, and `arithmetic_side_effects`. Do not add panic paths.
- **No silent lint suppressions:** Only use `#[expect(clippy::<lint>, reason = "…")]` on the smallest item with a true reason.
- **Null-pointer & bounds safety in FFI:** Validate every pointer (`is_null()`) and output parameter before dereferencing in `crates/cfe-msg-ffi`, returning the exact `CFE_Status_t` error code that cFE returns (check the C implementation in `cfs/cfe/modules/msg/fsw/src/`).
- **Tests:** Every function added to `crates/cfe-msg` must have unit tests covering edge cases and matching the C reference behaviour.

## Checks

Run before committing or reporting a round done:

1. `make check` on the host (`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`).
2. `make rs` in the container (builds cFS with `modules/msg`, runs `ctest -R coverage-msg`, and runs `scripts/check-symbols.sh`).

**Always commit your work before reporting back** (on your branch/worktree for implementers, or on the target branch when asked to make changes directly).

## Process & roles

- **Worktrees & PRs:** All implementation work happens in a dedicated git worktree and feature branch (e.g. `.claude/worktrees/<branch>`). Never implement directly on `main`. **Standing rule for every implementation:** after `make check` and `make rs` pass and changes are committed, always push the branch, open a PR, and run an independent review (`docs/process/review.md`) before reporting the task complete.
- **Coordinator** (`gemini-4-hydrogen-high`): plans work, dispatches agents, runs checks/PRs, answers reviews.
- **Implementer** (`gemini-4-hydrogen-low`): implements one task/PR or fix round in its own worktree/branch and reports back.
- **Reviewer** (`gemini-4-hydrogen-xhigh`): independent read-only review of one PR; posts findings on GitHub.

End every GitHub PR comment or review posted by an agent with:
`🤖 Generated with Gemini`
