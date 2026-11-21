# Git children select their repository only by the -C directory

Every git child that horch or its tests start removes the
repository-selecting environment variables, so the `-C` directory alone
selects the repository and its config. `REPO_ENV` in
`crates/horch-marketplace/src/git.rs` lists them, and `scrub_repo_env`
removes them. A caller inside a git hook, or under `git rebase --exec`, has
`GIT_DIR` set. On 2026-10-03 a gate run under `git rebase -x` inherited
`GIT_DIR`, and test fixtures that `git init` and commit wrote into the real
repository (`core.bare=true`, `HEAD=main`, 2 commits).

## Consequences

- `REPO_ENV` holds every name that `git rev-parse --local-env-vars` prints,
  plus `GIT_NAMESPACE` and `GIT_CEILING_DIRECTORIES`. A test checks the
  list against the host git.
- `scrub_repo_env` also removes the numbered `GIT_CONFIG_KEY_<n>` and
  `GIT_CONFIG_VALUE_<n>`.
- `scrub_repo_env` removes both the variables this process inherited and
  those set on the command. It must be called after any `Command::env`
  call, or a later call can reintroduce `GIT_DIR` and the others.
- `GitRunner::output` passes the directory again as an absolute `-C` path,
  so the repository never depends on how git searches upward from its
  working directory.
- Test fixtures that run git, such as those in
  `crates/horch-marketplace/tests/marketplace.rs`, call `scrub_repo_env`
  too. A `GIT_DIR` from a hook or `git rebase -x` would aim fixture commits
  at the real repository.
