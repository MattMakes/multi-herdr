# Briefing changes carry the incident that caused them

Each sanctioned change to the orchestrator or worker briefing is a pinned
block in `crates/horch-core/tests/golden_prompts.rs`, and each block exists
because of a named incident or fact. The briefing text states the rule. The
test records why it is there, so the rule is not dropped as noise. The
worker briefing differs from its golden in exactly 9 places.

## Consequences

- The worker briefing describes the orchestrator as a separate *agent*
  session, not a Claude session. `horch fleet codex` puts a Codex
  orchestrator in that pane, and OpenCode and pi are coming, so naming 1
  CLI is wrong for most fleets that read it. A worker only needs to know
  the orchestrator is a SEPARATE session whose typed lines are
  instructions. Which agent is behind it changes nothing the worker does.
  The fixed `orchestration` recipe still says "Claude ... running Fable" in
  `orchestration-worker.md`, because there the orchestrator really is
  pinned to 1 agent and 1 model.
- Workers end their turn when they need a reply or are blocked, instead of
  waiting or polling. A worker once polled a wait tool for 32 minutes. The
  worker briefing (ninth sanctioned change) says the reply arrives as the
  worker's next message. The orchestrator briefing (seventeenth sanctioned
  change) says blocked workers end their turn rather than WAIT.
- The orchestrator briefing has a stale-skill guard (its tenth change). 2
  `~/.claude/skills` symlinks, `herdr-orchestrator` and `herdr-worker`,
  describe an older horch and trigger on the words the briefing is full of.
  Claude teammates switch them off by name. The briefing tells the
  orchestrator not to load them, which covers a pane whose operator
  settings horch does not control.
- Tell-retry block (ported from fe9c5db6): a failed `horch tell` lost a
  worker's COMPACT-READY report. A worker tries `horch tell` 1 more time,
  then records the message with `horch note`, which the orchestrator reads.
- Ledger-worktree block (#202): a ledger worker made a worktree with the
  plain `horch worktree new`, which the ledger does not record, so `finish`
  could not check or land it. In a ledger run the section names only 1
  worktree command, `horch ledger worktree new`.
- Ledger read and run-alone block (#181): a Codex execpolicy allow rule
  lifts the sandbox only for a command that is the whole exec call. So the
  briefing tells workers to run ledger read commands alone, and those read
  commands are allowed by name.
- Codex execpolicy rules render exactly as before, so an existing
  `~/.codex/execpolicy` is not duplicated on upgrade. The 3 old blocks stay
  unchanged. 2 additions are sanctioned and pinned in the test. The
  `horch skills read` rule (SKL-09) has no golden, so its block is pinned
  there. Second (#181): the 1 broad `["horch", "ledger"]` allow rule let a
  worker run any check command outside its sandbox. It is replaced by 1
  allow rule per worker subcommand and 3 forbidden rules for the
  subcommands that take a check command.
- Context hooks block (2026-11-21, operator request; ADR 0197): Claude Code
  panes get the context ask and the handoff file from hooks. Both briefings
  say so in 1 block each, so a worker does not wait for a line from the
  orchestrator and does not read its handoff file a second time
  (`ctx_hooks_worker_block`, `ctx_hooks_orchestrator_block`).
