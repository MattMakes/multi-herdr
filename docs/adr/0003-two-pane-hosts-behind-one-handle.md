# Two pane hosts behind one `PaneHost` handle

`horch` drives one of two pane hosts per fleet — the `herdr` TUI server
(`herdr`) or `herdr-core` plus the Herdr desktop app (`hc`) — through a single
`PaneHost` handle (`crates/horch-core/src/workspace/host.rs`). The backend is
resolved once when `RuntimeContext` is built (`--backend`, else `HORCH_BACKEND`,
else whichever single pane identity `HERDR_PANE_ID` or `HC_TERMINAL_ID` is set,
refusing both as ambiguous, else `herdr`), travels to panes in the brief, and
is recorded on every execution record and task-ledger worker row. Code that
acts on its own pane constructs `PaneHost::of(ctx)`; code handed a workspace or
record uses `PaneHost::for_workspace` or `PaneHost::for_backend`; machine-wide
listings and sweeps use `Hosts::reachable(ctx)`. A verdict that a workspace or
pane is gone comes only from the backend recorded for it; when that host does
not answer, state is kept untouched. Both hosts can run side by side on one
machine without colliding.

## Consequences

- **No layout engine on `hc`.** The Herdr desktop app owns pane geometry and
  exposes no rects, move, or resize. `horch` places each worker once at spawn
  (`workspace::place`: up to 4 workers in 2 columns beside the orchestrator on
  tab 1, then overflow tabs named `workers <N>` holding up to 6 workers in 3
  columns); `horch tile` and `horch balance` are no-ops on `hc`, and
  `horch layout` prints a sizeless space/tab/terminal tree.
- **`pane_send_text` is refused on `hc`.** `hc send` pastes and submits with
  Enter only; there is no type-without-Enter primitive, so delivery uses only
  the `agent_prompt` path on `hc`.
- **Harness gate on `hc`.** `horch` relies on OSC 7501 status transitions to
  know when a pane is at its prompt. `horch spawn` on `hc` refuses any real
  harness not in `HC_STATUS_HARNESSES` (`claude`, `codex`, `antigravity`)
  before creating a pane.
- **A fleet stays on one host.** The two hosts have no bridge, so a running
  fleet cannot move between `herdr` and `hc`.
- **Weakened Bash sandbox on `hc`.** A sandboxed Claude agent on `hc` gets the
  `herdr-core` and `herdr-app` sockets in `allowUnixSockets` so `horch spawn`,
  `horch tell`, and `horch done` work from the pane. Because the `herdr-core`
  socket can run programs and write files outside the sandbox (`machine.exec`,
  `file.write`), Claude's Bash sandbox is no longer an isolation boundary on
  `hc`. `horch doctor` prints a note stating this trade-off.
