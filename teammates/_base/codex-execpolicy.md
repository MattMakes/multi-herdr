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
  - pattern: '"horch", "tell"'
    justification: report to the orchestrator or message another fleet worker
  - pattern: '"horch", "note"'
    justification: record progress in the session ledger
  - pattern: '"horch", "skills", "read"'
    justification: read a catalog skill that the bundle does not hold
  - pattern: '"horch", "done"'
    justification: mark session done and close own pane
  # The task ledger is outside the sandbox's writable roots, so each ledger
  # command a worker runs needs its own allow rule. Only the worker's own
  # subcommands are allowed (#181). `add`, `add-map` and `reopen` take any
  # `--check` command, which `finish` would then run outside the sandbox, so
  # they are forbidden: a forbidden rule beats any allow rule. `finish` still
  # runs the task's check and its git commands outside the sandbox, in a tree
  # and a .git the worker writes. On Windows every codex session shares 1
  # rules file, so a codex orchestrator's broad ledger rule reaches workers.
  - pattern: '"horch", "ledger", "claim"'
    justification: take the next task for this worker
  - pattern: '"horch", "ledger", "worktree"'
    justification: make, scratch or drop the worker's own worktree
  - pattern: '"horch", "ledger", "progress"'
    justification: record a milestone of the worker's task
  - pattern: '"horch", "ledger", "status"'
    justification: read the board of the run
  - pattern: '"horch", "ledger", "show"'
    justification: read 1 task or worker with its history
  - pattern: '"horch", "ledger", "steps"'
    justification: read the finish steps of the worker's task
  - pattern: '"horch", "ledger", "finish"'
    justification: run the finish steps, then land and push the worker's work
  - pattern: '"horch", "ledger", "done"'
    justification: finish the worker's task and close its pane
  - pattern: '"horch", "ledger", "add"'
    decision: forbidden
    justification: a worker never queues a task, and its check command would run unsandboxed
  - pattern: '"horch", "ledger", "add-map"'
    decision: forbidden
    justification: a worker never queues tasks, and their check commands would run unsandboxed
  - pattern: '"horch", "ledger", "reopen"'
    decision: forbidden
    justification: a worker never reopens a task, and its check command would run unsandboxed
---
Each rule is rendered into this launch's own `rules/horch.rules`, inside a
private `CODEX_HOME` that lives only as long as the pane, as:

    prefix_rule(
        pattern = [{pattern}],
        decision = "{decision}",
        justification = "herdr-fleet: {justification}",
    )

`decision` is `allow` when the rule does not set it. Codex takes the
strictest decision of every rule that matches a command. An allow rule
lifts the sandbox only for a command that is the whole exec call: a
`horch ledger` command after `;` or `&&`, or with a pipe, runs in the
sandbox and cannot open the ledger.
