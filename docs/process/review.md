# Independent review

Protocol for independent review of pull requests on **cfs-rs**. Referenced by [`.claude/agents/reviewer.md`](../../.claude/agents/reviewer.md) (`gemini-4-hydrogen-xhigh`, Hydrogen with extra-high effort) and [AGENTS.md](../../AGENTS.md).

## What to check

Do not spend review effort re-linting what `cargo fmt`, `cargo clippy`, and `make check` already enforce. Focus on what automated checks do not catch, **prioritizing type safety and compile-time guarantees of correctness**:

1. **Type safety & compile-time guarantees (top priority):**
   - Prefer strong, domain-specific Rust types (e.g., `aldis` types, bounded newtypes, enums, fixed-size arrays `[u8; N]`) over raw primitives (`u8`/`u16`/`usize`/slices) wherever invalid states can be made unrepresentable at compile time.
   - Prefer `const fn`, exhaustive `match`, and compile-time layout/size assertions (`const { assert!(...) }`) for `#[repr(C)]` FFI structs over runtime checks or implicit assumptions.
   - Flag missed opportunities where a runtime check, sentinel value, or loosely typed parameter/return in `crates/cfe-msg` could be enforced by the Rust type system at compile time.
2. **C ABI equivalence (`crates/cfe-msg-ffi` vs `cfs/`):**
   - Signatures, parameter types, and return types match `cfs/cfe/modules/core_api/fsw/inc/cfe_msg.h` and `cfe_msg_api_typedefs.h` exactly.
   - Null-pointer checks, out-param initialisation, and `CFE_Status_t` return values match the C implementation in `cfs/cfe/modules/msg/fsw/src/` for every error and boundary case.
3. **`unsafe` soundness:**
   - Every `unsafe` block is in `crates/cfe-msg-ffi`, is minimal, and has an accurate `// SAFETY:` comment.
   - No `unsafe` in `crates/cfe-msg` (`#![forbid(unsafe_code)]`).
4. **Aldis & Rust core (`crates/cfe-msg`):**
   - Space Packet / time-code behaviour reuses `aldis` where applicable rather than reimplementing CCSDS bit layouts.
   - No reachable panic paths (`no_std`, `panic = "abort"`).
   - Unit tests cover edge cases and compare against the C behaviour.
5. **Symbol replacement (`modules/msg/replaced.cmake`):**
   - Every `#[unsafe(no_mangle)]` function in `crates/cfe-msg-ffi` is listed in `modules/msg/replaced.cmake` (and vice versa).
   - `cfs/` is completely unmodified.
6. **Verification evidence:**
   - The PR includes passing `make check` and `make rs` (`coverage-msg-ALL` + `scripts/check-symbols.sh`) output for the current head.

## Grounding rule

Every statement about the implementation must cite `path:line` and quote the relevant code when a defect depends on it. Do not make claims from memory.

## Reporting findings

- **Blocking:** failing checks, a broken rule of `AGENTS.md`, a C ABI mismatch, an unsound `unsafe` block, a missing/wrong `CFE_Status_t` or edge-case behaviour against `cfs/`, or a missing entry in `replaced.cmake`.
- **Non-blocking:** anything else worth fixing or tracking.
- **Nits:** at most 5 per review, each as exact replacement text. Drop any beyond 5.
- Every finding must state its concrete consequence (e.g., UB on null pointer, wrong status code, C symbol not replaced).

## Review rounds

- **Review 1:** Full review of all changed files.
- **Reviews 2 and 3:** Check only (i) whether earlier findings were fixed and (ii) defects in lines changed since the previous reviewed head.

## Method and posting

- Read the diff file by file: `gh pr diff <n> --name-only`, then `git diff origin/main...<head> -- <path>` for each file.
- Before posting, check `gh pr view <n> --json reviews` to avoid duplicate reviews of the same head.
- Post with `gh pr review <n> --comment` (marking each finding **blocking** or **non-blocking** in the body). End the body with:
  `🤖 Generated with Gemini`
- Return a short summary to the coordinator: findings ranked by severity with `path:line`, and the nit batch.
