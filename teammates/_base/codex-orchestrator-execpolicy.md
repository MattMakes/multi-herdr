---
name: codex-orchestrator-execpolicy
description: >
  The worker rules in _base/codex-execpolicy.md are not enough for an
  orchestrator: it never runs `horch note` or `horch done`, and it needs the
  commands of the chat persona instead: read the fleet, and run an operator
  command when the operator tells it to. Every command _base/fleet-orchestrator.md
  tells an orchestrator to run must appear here, or a codex orchestrator comes
  up able to think and unable to act.
rules:
  - pattern: '"horch", "tell"'
    justification: message any worker in the fleet
  - pattern: '"horch", "inbox"'
    justification: list the live, reachable roles
  - pattern: '"horch", "sessions"'
    justification: read the project session ledger
  - pattern: '"horch", "layout"'
    justification: report the worker grid and the next split
  - pattern: '"horch", "quota"'
    justification: read the usage pools
  - pattern: '"horch", "route"'
    justification: see what the usage-limit gate decides for a persona
  - pattern: '"horch", "context"'
    justification: report the context size of every session
  - pattern: '"horch", "compact"'
    justification: compact a session at a stopping point
  - pattern: '"horch", "mode"'
    justification: switch a read-only Codex worker to write and back
  - pattern: '"horch", "ledger"'
    justification: read the task ledger, and stop, fall back or waive for the operator
  - pattern: '"horch", "ideas"'
    justification: read each idea and the needs-you list
  - pattern: '"horch", "idea"'
    justification: show an idea, or add, amend, revise, park or cancel one for the operator
  - pattern: '"horch", "task"'
    justification: requeue or cancel a task for the operator
  - pattern: '"horch", "feature"'
    justification: abandon a feature for the operator
---
Each rule is rendered into this launch's own `rules/horch.rules`, inside a
private `CODEX_HOME` that lives only as long as the pane, as:

    prefix_rule(
        pattern = [{pattern}],
        decision = "allow",
        justification = "herdr-fleet: {justification}",
    )
