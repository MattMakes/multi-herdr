# Child processes get a scrubbed environment

Every child horch starts gets an environment cleaned of what it must not
carry. `scrub_child_env` in `crates/horch-core/src/runtime/process.rs`
removes every `FORBIDDEN_ENV` variable
(`crate::harness::launch::FORBIDDEN_ENV`) and every git repository
variable: `REPO_ENV` (`horch_marketplace::git::REPO_ENV`) and the numbered
`GIT_CONFIG_KEY_<n>` and `GIT_CONFIG_VALUE_<n>`. A horch started under
`git rebase -x` or a git hook has `GIT_DIR` set, and an agent or judge that
inherited it would run its git in that repository.

## Consequences

- `scrub_child_env` must be called after any `env` call on the command.
- `Inherited` in `crates/horch-core/src/runtime/context.rs` holds only
  named fields. horch never keeps a map of the whole process environment,
  so a `FORBIDDEN_ENV` name such as `ANTHROPIC_API_KEY` is never captured
  and passed on. `DEFAULT_ENV_YIELD` lists the env names a non-Claude
  harness default may yield to, and each one is a named field of
  `Inherited`.
- A detached child gets none of the pane identity of the process that
  starts it (#218): `PANE_IDENTITY_ENV` (`HORCH_ROLE`, `HORCH_RECORD_ID`,
  `HORCH_WORKER_ID`, `HORCH_WORKSPACE_ID`, `HERDR_PANE_ID`,
  `HC_TERMINAL_ID`, `HC_SCOPE`, `HORCH_BACKEND`). The child outlives that
  pane and serves the whole project, so it must not act as that pane.
- `scrub_pane_identity` also drops a `TMPDIR` under `<state_root>/tmp/`.
  That is the starting pane's private temp dir, which the pane removes when
  it ends. A key the caller set or removed on the command itself is left as
  the caller wants it.
- The tests check pane identity against `PANE_KEYS`, the keys #218 and
  HCB-02 name, written out. They do not read `PANE_IDENTITY_ENV`, because a
  loop over the list under test passes when a key is dropped from it or
  when the list is empty.
- `path_with_own_dir` puts the running horch binary's own directory first
  on a child's PATH, and returns `None` when nothing needs to change. Every
  worker briefing tells the agent that `horch` is on its PATH, and the agent
  calls it by bare name from its own shell tool. The bash launchers made
  that true with `export PATH="$root/bin:$PATH"`. Without the equivalent, a
  `horch` run from a build directory would leave every worker unable to
  reach the orchestrator, its only channel.
