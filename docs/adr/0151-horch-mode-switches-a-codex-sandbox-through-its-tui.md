# horch mode switches a Codex sandbox through its TUI

`horch mode <role> write|plan` (`crates/horch/src/cmd/mode.rs`, X6, design
`ai_docs/plans/wave3/x6-design.md`) switches a read-only Codex pane to
write and back by driving Codex's TUI. A Codex teammate with
`permission_mode: plan` runs read-only: a plain `horch note` records
through the execpolicy rule, but a compound one and the handoff file fail.
Codex 0.160.0 switches its sandbox at run time with `/permissions`, so the
orchestrator can open the pane for a handoff and close it again. Both
`/permissions` and `/status` are local TUI commands that cost no model
turn.

## Consequences

- The steps are fixed: wait for the pane at its prompt (2 polls in a row).
  Type `/permissions`, find the target item by label (`fleet_write` for
  write, `Read Only` for plan) and the `›` cursor, press `down` to it,
  check the cursor line, press Enter. Type `/status` and check its
  `Permissions:` line (`Profile fleet_write`, or `Read Only ...`). Record
  `mode-changed` (`write` or `plan`) on the record.
- `switch` is the keys and the check alone. `horch compact --request` and
  the compaction job call it too.
