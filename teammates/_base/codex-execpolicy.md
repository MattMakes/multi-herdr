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
  # From Phase 5 a worker's ledger commands run in its sandbox: they read
  # the ledger read-only and write the outbox in the task folder, a
  # writable root (FM-17). So no rule lets `horch ledger` leave the sandbox
  # (#181). `add`, `add-map` and `reopen` take any `--check` command, so
  # they stay forbidden: a forbidden rule beats any allow rule. On Windows
  # every codex session shares 1 rules file, so a codex orchestrator's
  # broad ledger rule reaches workers.
  - pattern: '"horch", "ledger", "add"'
    decision: forbidden
    justification: a worker never queues a task, and its check command would run unsandboxed
  - pattern: '"horch", "ledger", "add-map"'
    decision: forbidden
    justification: a worker never queues tasks, and their check commands would run unsandboxed
  - pattern: '"horch", "ledger", "reopen"'
    decision: forbidden
    justification: a worker never reopens a task, and its check command would run unsandboxed
  # Only fleetd makes or removes a worktree (FM-26). A worker that makes a
  # tree anyway is found by the reconciler as an unowned tree. On Windows the
  # shared rules file gets no forbidden rule, so there the reconciler alone
  # catches it. `git worktree list` is a read and runs in the sandbox.
  - pattern: '"git", "worktree", "add"'
    decision: forbidden
    justification: only fleetd makes a worktree; use horch ledger worktree new
  - pattern: '"git", "worktree", "remove"'
    decision: forbidden
    justification: only fleetd removes a worktree; horch ledger finish and drop remove it
  - pattern: '"git", "worktree", "prune"'
    decision: forbidden
    justification: only fleetd prunes worktree records
  # SR-3: no agent pushes, merges a PR or makes a release; fleetd pushes
  # the feature branch. The pre-push hook of the agent hooks path also
  # refuses a push; this rule also stops `git push --no-verify`.
  - pattern: '"git", "push"'
    decision: forbidden
    justification: fleet agents do not push; fleetd pushes the feature branch
  - pattern: '"gh", "pr", "merge"'
    decision: forbidden
    justification: fleet agents do not merge a pull request
  - pattern: '"gh", "release"'
    decision: forbidden
    justification: fleet agents do not make a release
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
`horch tell` command after `;` or `&&`, or with a pipe, runs in the
sandbox.
