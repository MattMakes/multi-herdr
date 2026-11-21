# Codex gets fleet variables through shell_environment_policy.set

A codex launch passes the fleet variables (`HORCH_*`, `HERDR_*`) as
`shell_environment_policy.set` pairs (`crates/horch-core/src/harness/codex.rs`).
These are the variables `horch tell`, `note` and `done` read in the
agent's shell tool, given as `PrepareRequest.shell_env`
(`crates/horch-core/src/harness/mod.rs`). In W2 C2 the operator's
`shell_environment_policy.inherit = "core"` stripped `HORCH_*` and
`HERDR_*` from the agent's shell commands, so `horch tell`, `note` and
`done` failed with `HORCH_RECORD_ID is unset`. `inherit` does not filter
`set`.

## Consequences

- `shell_env_args` writes 1 `shell_environment_policy.set` pair per valid
  variable, each value encoded with `serde_json::to_string`. A JSON string,
  newlines escaped too, is a valid TOML basic string, which is what codex's
  `-c` parser expects.
- A name that is not a plain variable name is skipped.
