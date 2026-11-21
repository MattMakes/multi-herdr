# Codex panes escape the sandbox only by an exact execpolicy prefix

A Codex pane runs every command in Codex's sandbox unless an execpolicy
`allow` rule matches it. The sandbox has no network, so herdr's socket and
the channel relay are unreachable from it. A rule lifts the sandbox only for
a command that matches the rule's prefix on its own, as the whole exec call
(#181). A `horch spawn` at the
end of a script with a heredoc, or with inline variables, still runs
sandboxed. Codex sets `CODEX_SANDBOX` in every command it runs sandboxed.
`main::codex_sandbox_hint` turns a herdr failure under `CODEX_SANDBOX` into
a hint that ends with the exact plain command to run alone. Without it a
Codex orchestrator reported the fleet BLOCKED, and a worker that set the
fleet variables inline lost its report (W2 C1).

## Consequences

- When several execpolicy `prefix_rule`s match a command, codex takes the
  strictest decision, so a `forbidden` rule in `ExecRule`
  (`crates/horch-core/src/roster/teammate.rs`) beats any `allow` rule,
  whatever their order.
- Codex refuses any command its execpolicy does not name, and an
  orchestrator runs a different command set than a worker.
  `recipes::exec_rules_for` installs the orchestrator rule set for a Codex
  orchestrator pane and the worker set otherwise, not both.
- An allow rule runs a Codex worker's `horch ledger <sub>` outside its
  sandbox, and `horch ledger finish` runs a task's check command there
  (#181). So no allow-listed ledger subcommand may accept `--check`, and
  every allow rule names a real subcommand. The test
  `no_allowed_ledger_subcommand_takes_a_check_command` reads this from the
  clap definition, so a new `--check` on an allowed subcommand fails it.
- `messaging::require_env` names what sets a missing `HORCH_*` value and
  tells the worker never to set it inline. A Codex worker that does so runs
  the command sandboxed, where its report cannot get out (W2 C1).
- A Codex shell environment policy can drop an inherited variable.
  `recipes::child_env` sets the verify profile variable explicitly, because
  the orchestrator's `horch ledger run new` reads it.
- A sandboxed caller (a Codex pane) cannot write the task ledger.
  `worktree::triage_rows` then falls back to
  `taskledger::open_existing_read_only` and skips the bytes cache (T-5)
  instead of failing.
- The worktree steward has no codex fallback (Fix B, T-5), because the
  codex sandbox cannot write the task ledger. When the claude pool is
  exhausted, `horch fleet` starts no steward and says why.
- `spawn::sweep_state` skips the state sweep when a sandboxed worker runs
  `horch spawn`, because the sandbox denies removal of skill bundles
  (ticket 28). A skipped sweep writes no stamp, so the next spawn outside a
  sandbox sweeps on time.
