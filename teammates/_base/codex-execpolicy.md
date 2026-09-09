---
name: codex-execpolicy
description: >
  Codex loads execpolicy rules at startup and refuses anything outside its
  sandbox that is not explicitly allowed. Every command a codex teammate is
  told to run in _base/fleet-worker.md must appear here, or the worker launches
  mute: unable to report, unable to close its own pane. These rules reach only
  the pane that needs them - horch gives each codex launch a private CODEX_HOME
  rather than adding to the operator's shared rules file.
rules:
  - pattern: '"horch", "tell", "orchestrator"'
    justification: report results to the fleet orchestrator
  - pattern: '"horch", "note"'
    justification: record progress in the session ledger
  - pattern: '"horch", "done"'
    justification: mark session done and close own pane
---
Each rule is rendered into this launch's own `rules/horch.rules`, inside a
private `CODEX_HOME` that lives only as long as the pane, as:

    prefix_rule(
        pattern = [{pattern}],
        decision = "allow",
        justification = "herdr-fleet: {justification}",
    )
