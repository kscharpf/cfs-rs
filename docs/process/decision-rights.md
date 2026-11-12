# Decision rights

Who decides what in **cfs-rs**, so routine implementation choices don't block on the maintainer, and questions that reach the maintainer arrive with a recommended answer.

## 1. Roles

| Role | Model / Effort | Scope |
|---|---|---|
| **Maintainer** | Human (project owner) | Scope, ABI departures, dependencies, releases, merges to `main` |
| **Coordinator** | `gemini-4-hydrogen-high` | Planning, dispatching, review orchestration, PR readiness |
| **Implementer** | `gemini-4-hydrogen-low` | Implementing a work package / fix round in its branch |
| **Reviewer** | `gemini-4-hydrogen-xhigh` | Independent read-only PR review |

## 2. Who decides what

- **C ABI & `cfs/` behaviour:** `cfe_msg.h` and `cfs/cfe/modules/msg/fsw/src/` govern. Any intentional departure from cFE's C behaviour or ABI is **Maintainer only**. Matching cFE's documented/observed behaviour is decided by the **Implementer** / **Coordinator**.
- **Internal Rust design:** Module layout, private helpers, test structure, and how to wrap `aldis` types inside `crates/cfe-msg` are decided by the **Implementer** (recorded under **Judgement calls**).
- **Dependencies & tooling:** Adding new crates, changing `Cargo.toml` lints, or changing `rust-toolchain.toml` is **Maintainer only**.
- **Review findings:** Fixing findings that change no C ABI or cFE-compatible behaviour is decided by the **Coordinator** / **Implementer**. Rejecting a reviewer finding that the reviewer holds after reply, or changing behaviour/ABI, goes to the **Maintainer**.
- **Merges, releases, visibility:** **Maintainer only**.
