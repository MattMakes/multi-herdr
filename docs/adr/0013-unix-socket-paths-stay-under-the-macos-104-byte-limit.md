# Unix socket paths stay under the macOS 104-byte limit

A Unix socket path holds at most 104 bytes on macOS (`sun_path`,
`SUN_LEN`), the last one a NUL. horch keeps every socket path it makes
inside that limit and refuses a longer one by name rather than cutting it.
`prime::MAX_SOCKET_BYTES` (`crates/horch-core/src/harness/prime.rs`) is
103, the longest path `Daemon::install` accepts. horch puts its relay,
report and hc sockets under its state dir, so a long state root breaks the
socket bind.

## Consequences

- `Daemon::install` names the launch dir with 12 hex digits and no role
  slug, so the Prime daemon socket path stays short.
- `messaging::channel::prepare_socket_path` refuses a relay socket path of
  104 bytes or more by name, before it creates anything. `unix_address`
  refuses a path that overflows its own field instead of cutting it.
- The default macOS temp folder is long. Tests that bind a socket put
  their folder under `/tmp` with a short prefix
  (`crates/horch-core/src/messaging/report.rs`,
  `crates/horch-core/tests/context_prime.rs`,
  `crates/horch-core/tests/harness.rs`).
- The e2e tests that use the relay, report and hc sockets keep their
  scratch and state paths short, with short temp prefixes and short labels
  (`tests/pane_host_contract.rs`, `tests/relay_first_delivery.rs`,
  `tests/skill_intake_spawn.rs`). The relay tests use a harness whose state
  dir is short. A spawn test uses a short label, so the report socket path
  under the harness state root stays under the 104-byte `SUN_LEN`.
- The e2e harness root (`crates/horch-e2e/src/harness.rs`) is
  `/tmp/horch-e2e-<name>-<pid>-<n>` on unix, with `<name>` cut to its first
  12 characters and `<n>` in hex. When the process cannot write there,
  `TMPDIR` is used only if Prime's longest daemon socket path stays below
  the 104-byte limit.
