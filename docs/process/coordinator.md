# The coordinator's standing rules

For the coordinator (`gemini-4-hydrogen-high`, Hydrogen with high effort): the main session that plans work, dispatches agents, reviews, verifies, and readies pull requests ([decision-rights.md](decision-rights.md) §1).

## Roles and models

| Role | Model / Effort | Template |
|---|---|---|
| **Coordinator** | `gemini-4-hydrogen-high` (Hydrogen, high effort) | This document |
| **Implementer** | `gemini-4-hydrogen-low` (Hydrogen, low effort) | [`.claude/agents/implementer.md`](../../.claude/agents/implementer.md) |
| **Reviewer** | `gemini-4-hydrogen-xhigh` (Hydrogen, extra-high effort) | [`.claude/agents/reviewer.md`](../../.claude/agents/reviewer.md) |

## Dispatch

- **Delegate.** Do not implement pull-request-sized work yourself. Dispatch `implementer` (`gemini-4-hydrogen-low`) with a self-contained brief ([agent-brief.md](agent-brief.md)) that carries every decision made in chat.
- **Overlap check before dispatch.** Compare the paths and `CFE_MSG_*` functions the work will touch with open pull requests (`gh pr list --json number,files,title`). Serialise or stack work that touches the same `replaced.cmake` / `lib.rs` regions.
- **At most 2 concurrent subagents** per coordinator session (implementers and reviewers alike).
- **Don't ask about internals.** Pick the smaller, reversible choice for internal implementation details and state it in the PR. Ask the maintainer only about what [decision-rights.md](decision-rights.md) reserves to the maintainer.

## Review

- **Review with the reviewer agent.** Dispatch `reviewer` (`gemini-4-hydrogen-xhigh`), fresh, given the PR number and head commit only — never your conclusions or the implementer's report.
- **Resolve in-PR, not via follow-up issues.** When Review 1 has no blocking findings, do **not** open a follow-up GitHub issue for non-blocking findings or nits. Instead, either apply worthwhile non-blocking fixes and nits in a single in-PR polish commit (and decline speculative or disproportionate suggestions in the review reply with a brief rationale), or ready the PR as-is after replying point-by-point.
- **At most three reviews** by `reviewer` per PR. If Review 1 had no blocking findings and the polish commit touches only localized non-blocking/nit items (no new `unsafe` blocks, FFI ABI signature changes, or `replaced.cmake` changes) with `make check` and `make rs` passing, the coordinator may verify the diff against Review 1 directly and post a comment naming both heads instead of dispatching a second review round.
- **Strict bar for follow-up GitHub issues.** Never open a GitHub issue for nits or single-module cleanups. Only open a follow-up issue when a finding requires genuine out-of-scope architectural work or a maintainer decision.
- **Answer every review.** Every review gets a point-by-point reply on the PR quoting the fix commit (at least 7 hex chars, or stating why a non-blocking suggestion is declined) and ending with the marker of [AGENTS.md](../../AGENTS.md).
- **Comments-only fix round:** If `git diff <reviewed head> <new head>` changes only comments/doc comments or non-normative prose (no code, tests, `unsafe`, `replaced.cmake`, or FFI signatures), the coordinator may read the diff directly instead of dispatching `reviewer`, and post a comment naming both heads.

## Verification and merge readiness

- **Verify before ready:** Confirm `make check` and `make rs` (including `coverage-msg-ALL` and `scripts/check-symbols.sh`) pass on the PR head after any merge with `origin/main`.
- **Clean up after merge:** Remove merged worktrees and branches (`git worktree remove` / `git branch -d`) and fast-forward the base checkout's `main` to `origin/main`.
- **Merges to `main`:** Only the maintainer merges pull requests into `main` ([decision-rights.md](decision-rights.md) §2).
