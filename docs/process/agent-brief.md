# Briefing an agent

For the coordinator dispatching an `implementer` (`gemini-4-hydrogen-low`, Hydrogen with low effort), and for the implementer itself.

## The brief

Every brief must state:
1. **Goal:** which `CFE_MSG_*` functions or change the task implements.
2. **Base & branch:** the base branch (`origin/main`) and target branch name (e.g., `feat/msg-pri-hdr`).
3. **Decisions:** any design or ABI choices already made in chat.
4. **Done criteria:** PR opened (or updated in a fix round), `make check` and `make rs` passing on the pushed head, and the final report returned.
5. **Boundaries:** do not edit `cfs/`, do not touch unrelated open PRs, do not merge to `main`.

## Conventions for the implementer

1. **Worktree:** Work in an isolated git worktree/branch created from `origin/main` (e.g., `.claude/worktrees/<branch>`).
2. **Verify before pushing:** Run `make check` on the host and `make rs` in the container on the final commit before pushing.
3. **Pushes & PR:** Confirm the PR is open (`gh pr view <n> --json state`) before pushing to an existing PR branch. Use conventional commit PR titles (e.g., `feat(msg): implement CFE_MSG_GetSize and CFE_MSG_SetSize`).
4. **Clean up build artifacts if needed:** Do not commit `build/`, `target/`, or `*.log` files.
5. **Boundaries:** Stop at an open, pushed PR. Merging is the maintainer's.

## The reply to a review (fix rounds)

In a fix round, after pushing the fix commit:
- Post a point-by-point reply on the PR (`gh pr comment <n>`) quoting the pushed head commit (at least 7 hex chars, e.g., `Fix round, head abc1234`) and ending with:
  `🤖 Generated with Gemini`

## The report

End your turn once with:
- PR URL and pushed `HEAD` SHA (`gh pr view <n> --json headRefOid`)
- Summary of changes (`crates/cfe-msg`, `crates/cfe-msg-ffi`, `modules/msg/replaced.cmake`)
- Verification evidence (`make check` and `make rs` / `check-symbols.sh` results)
- **Judgement calls:** any local decisions made under [decision-rights.md](decision-rights.md)
- **For the maintainer:** any questions requiring maintainer input (with a recommended option), or `none`
