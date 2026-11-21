# A herdr-restored pane may act only as its registered worker

After a herdr server restart, herdr resumes agent panes by itself (#125).
This is `resume_agents_on_restore` under `[session]` in herdr's config. It
is on by default, and herdr also resumes when no config file exists. The
config path can be set with `HERDR_CONFIG_PATH`. A resumed fleet agent runs
without `horch worker`: it has no `HORCH_ROLE`, `HORCH_RECORD_ID` or
`HORCH_WORKSPACE_ID`, no relay, and it starts from the herdr server's
environment, which horch does not scrub. `crates/horch/src/cmd/restore.rs`
warns when the setting is on, and lets a resumed worker pane act as its
registered role again, with the identity horch finds from the pane's
registration. The warning only names the forbidden variable and never
reads it.

## Consequences

- `horch fleet` and `horch doctor` warn while the restore is on, and tell
  the operator to set `resume_agents_on_restore = false` under `[session]`.
- `main` adopts the restored identity through
  `restore::adopt_restored_identity` for the commands that need it.
- The adoption gives the pane its role's identity from the workspace
  registry and its brief, and says once that the pane runs outside its
  supervisor.
- Only a pane that herdr shows running an agent counts. An operator's
  shell that got a stale pane id is not a worker.
- The orchestrator's role is never adopted, and nothing is adopted in the
  pane the workspace registers for the orchestrator: an untagged line from
  the orchestrator pane is read as the human operator.
- The e2e test `crates/horch-e2e/tests/restored_pane.rs` covers a resumed
  worker pane finding its registered identity.
