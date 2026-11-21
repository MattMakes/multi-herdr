# Repository scripts pin the environment that git and horch read

The gate and the `justfile` do not let an inherited environment decide
which repository git writes to or which roster horch reads.
`scripts/phase-gate.sh` unsets every `GIT_*` variable that can aim a git
call at a repository: `GIT_ALTERNATE_OBJECT_DIRECTORIES`,
`GIT_CEILING_DIRECTORIES`, `GIT_COMMON_DIR`, `GIT_CONFIG`,
`GIT_CONFIG_COUNT`, `GIT_CONFIG_PARAMETERS`, `GIT_DIR`, `GIT_GRAFT_FILE`,
`GIT_IMPLICIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_INTERNAL_SUPER_PREFIX`,
`GIT_NAMESPACE`, `GIT_NO_REPLACE_OBJECTS`, `GIT_OBJECT_DIRECTORY`,
`GIT_PREFIX`, `GIT_REPLACE_REF_BASE`, `GIT_SHALLOW_FILE`, `GIT_WORK_TREE`
and every numbered `GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>`. These
are the names `REPO_ENV` in `crates/horch-marketplace/src/git.rs` scrubs
from every git child horch starts.

## Consequences

- A gate run under `git rebase -x` once wrote test fixture commits into
  the real repository. Rebase first, then run the gate. Never run it under
  `git rebase -x`.
- The `justfile` exports `HORCH_TEAMMATES_DIR` to this checkout's
  `teammates/` for every recipe. Every briefing an agent reads comes from a
  file in there. `horch` otherwise falls back to a compiled-in copy of the
  roster, and a fleet launched from another directory would silently
  ignore edits to `teammates/`.
