# The state root is a codex pane's 1 extra writable root

`Codex::prepare` in `crates/horch-core/src/harness/codex.rs` makes the
state root the pane's 1 extra writable root. `horch note` and `horch done`
lock and replace the ledger directly in it, and `horch tell` spools long
messages there. An execpolicy `allow` lifts the sandbox only for a command
that matches its prefix, so `horch note x 2>&1; echo $?` ran sandboxed and
failed on the lock (live check B2). A write or plan pane gets the root
inside `FLEET_WRITE_PROFILE`. Otherwise how the root is passed depends on
the launch sandbox, chosen by `launch_sandbox_args`.

## Consequences

- Only a `workspace-write` launch (or codex's default) gets the root as
  `--add-dir`. Codex 0.160.0 exits 1 when `--add-dir` meets a read-only
  sandbox ("the effective permissions do not allow additional writable
  roots").
- A `read-only` launch gets the root as a
  `sandbox_workspace_write.writable_roots` pair instead. Codex reads it
  only after a switch to workspace-write, and starts normally with it
  (X6 repro 2).
- Full access needs no extra root.
- `launch_has_network` judges whether a codex launch can have the network
  from the teammate file and the harness defaults only. The operator's
  `config.toml` is not read, and it can turn the network on, so the answer
  fails closed: only a read-only sandbox, or a workspace-write sandbox
  whose args or forced harness defaults turn
  `sandbox_workspace_write.network_access` off, has none. A default
  without `force` yields to `config.toml`.
