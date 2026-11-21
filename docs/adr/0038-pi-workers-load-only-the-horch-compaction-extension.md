# pi workers load only the horch compaction extension

A pi launch is `pi --model <pattern> --thinking <level> -- "<prompt>"`
(`crates/horch-core/src/harness/pi.rs`). The only extension it loads is
horch's compaction extension, passed with `--extension`. Nothing else is
discovered: a fleet worker with 1 narrow job should not inherit a
project's extensions or the operator's skills (`push_isolation_args`). The
operator's `~/.pi/agent` is not read or written. pi's `--session-id`
creates the session if it is missing, so horch mints the id and nothing is
discovered after launch.

## Consequences

- The Rust build of Prime (0.10.0) refuses every tool selection flag and
  `--no-extensions`, `-e` and `--extension` with "Unknown option" and exit
  1. Prime 0.10.0 loads no extensions anyway. So `push_tool_args` and
  `push_isolation_args` emit those flags only for pi, and the roster check
  rejects `tools:` and `disallowed_tools` on a Prime teammate.
