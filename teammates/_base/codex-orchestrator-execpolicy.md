---
name: codex-orchestrator-execpolicy
description: >
  The worker rules in _base/codex-execpolicy.md are not enough for an
  orchestrator: it never runs `horch note` or `horch done`, and it needs the
  fleet-building commands instead. Every command _base/fleet-orchestrator.md
  tells an orchestrator to run must appear here, or a codex orchestrator comes
  up able to think and unable to act.
rules:
  - pattern: '"horch", "spawn"'
    justification: create a worker pane, fresh or resumed
  - pattern: '"horch", "assign"'
    justification: record a task on the ledger and deliver it to a worker
  - pattern: '"horch", "tell"'
    justification: message any worker in the fleet
  - pattern: '"horch", "inbox"'
    justification: list the live, reachable roles
  - pattern: '"horch", "sessions"'
    justification: read the project session ledger before spawning
  - pattern: '"horch", "layout"'
    justification: report the worker grid and the next split
---
Each rule is rendered into this launch's own `rules/horch.rules`, inside a
private `CODEX_HOME` that lives only as long as the pane, as:

    prefix_rule(
        pattern = [{pattern}],
        decision = "allow",
        justification = "herdr-fleet: {justification}",
    )
