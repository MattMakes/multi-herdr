# Teammate paths and env values expand tilde against the launch home

Teammate files are committed, so their paths are written with `~` and are
not bound to 1 home directory. horch expands them against the launch's home
before the CLI sees them (`crates/horch-core/src/harness/launch.rs`). This
covers plugin paths and the values of a teammate's `env` block
(`teammate_env`). The agent CLI starts without a shell, so nothing else
expands them.

## Consequences

- Only a leading `~/` expands. With no home the value stays as written.
- asc refuses a relative `ASC_CONFIG_PATH`, so an unexpanded value would
  break it.
- `teammate_env` builds the environment from the teammate's
  `subagent_model` and its `env` block, the latter winning, then the `env`
  of its harness defaults for the keys it does not set. The caller gives it
  to the child command with `runtime::process::inherit_env`, so a value the
  builder set on the command still wins, as it did when this was exported
  into the parent's environment.
- An `env` value that is exactly `{path_of:<tool>}` becomes that host
  tool's executable (`LaunchEnv::host_tool`). When the tool is not found,
  the variable is left out, so the launch still works and the CLI keeps
  what it inherits.
