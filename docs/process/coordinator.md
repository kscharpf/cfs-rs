# The coordinator's standing rules

For the coordinator (`gemini-4-hydrogen-high`, Hydrogen with high effort): the main session that plans work, dispatches agents, reviews, verifies, and readies pull requests ([decision-rights.md](decision-rights.md) §1).

## Roles and models

| Role | Model / Effort | Template |
|---|---|---|
| **Coordinator** | `gemini-4-hydrogen-high` (Hydrogen, high effort) | This document |
| **Implementer** | `gemini-4-hydrogen-low` (Hydrogen, low effort) | [`.claude/agents/implementer.md`](../../.claude/agents/implementer.md) |
| **Reviewer** | `gemini-4-argon-high` (Argon, high effort) | [`.claude/agents/reviewer.md`](../../.claude/agents/reviewer.md) |

## Dispatch

- **Delegate.** Do not implement pull-request-sized work yourself. Dispatch `implementer` (`gemini-4-hydrogen-low`) with a self-contained brief ([agent-brief.md](agent-brief.md)) that carries every decision made in chat.
- **Overlap check before dispatch.** Compare the paths and `CFE_MSG_*` functions the work will touch with open pull requests (`gh pr list --json number,files,title`). Serialise or stack work that touches the same `replaced.cmake` / `lib.rs` regions.
- **At most 2 concurrent subagents** per coordinator session (implementers and reviewers alike).
- **Don't ask about internals.** Pick the smaller, reversible choice for internal implementation details and state it in the PR. Ask the maintainer only about what [decision-rights.md](decision-rights.md) reserves to the maintainer.

## Review

- **Review with the reviewer agent.** Dispatch `reviewer` (`gemini-4-argon-high`), fresh, given the PR number and head commit only — never your conclusions or the implementer's report.
- **At most three reviews** by `reviewer` per PR. A review with no blocking finding does not start a fix round of its own; fold non-blocking findings/nits into a final fix round or open follow-up issues. After the third review, open remaining non-blocking findings/nits as GitHub issues; if a blocking finding remains, ask the maintainer.
- **Answer every review.** Every review gets a point-by-point reply on the PR quoting the fix commit (at least 7 hex chars) and ending with the marker of [AGENTS.md](../../AGENTS.md).
- **Comments-only fix round:** If `git diff <reviewed head> <new head>` changes only comments/doc comments or non-normative prose (no code, tests, `unsafe`, `replaced.cmake`, or FFI signatures), the coordinator may read the diff directly instead of dispatching `reviewer`, and post a comment naming both heads.

## Verification and merge readiness

- **Verify before ready:** Confirm `make check` and `make rs` (including `coverage-msg-ALL` and `scripts/check-symbols.sh`) pass on the PR head after any merge with `origin/main`.
- **Clean up after merge:** Remove merged worktrees and branches (`git worktree remove` / `git branch -d`) and fast-forward the base checkout's `main` to `origin/main`.
- **Merges to `main`:** Only the maintainer merges pull requests into `main` ([decision-rights.md](decision-rights.md) §2).
