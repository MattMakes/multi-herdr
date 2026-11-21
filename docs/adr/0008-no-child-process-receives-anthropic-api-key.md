# No child process horch starts receives ANTHROPIC_API_KEY

Nothing in a fleet uses `ANTHROPIC_API_KEY`. This is the operator's rule
in `CLAUDE.md`. Every `claude` run signs in with the operator's claude.ai
subscription login. The shell that starts a fleet can export the key, so
every agent CLI launch removes it: `FORBIDDEN_ENV` in
`crates/horch-core/src/harness/launch.rs` lists the variables no agent CLI
ever receives. `child_env` and the launch command in
`crates/horch-core/src/execution/lifecycle.rs` remove the key from the
agent's child even when the teammate's `env` block names it.

## Consequences

- The brief's transport variables, the workspace and the PATH reach the
  agent's command and nothing else. The test of this leaves the test
  process's own environment the same afterwards.
- `crates/horch-marketplace/src/git.rs` removes the key from every git
  child through its own `FORBIDDEN_ENV`. Git never needs it, and the
  operator's shell can carry an invalid, billed key.
- `fake-claude` (and `fake-antigravity`) in `crates/horch-e2e` record a
  violation when the key is present: nothing horch starts carries it.
