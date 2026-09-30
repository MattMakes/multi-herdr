# Brief 04: design the early-compaction feature and write the implementation plan

## GOAL
Write two files:
- `ai_docs/designs/context-compaction.md`: the design.
- `ai_docs/plans/context-compaction/05-implementation.md`: an implementation plan
  split into units with disjoint file ownership, which Opus, Sonnet or Codex
  workers can execute step by step.

## CONTEXT
Read `ai_docs/plans/context-compaction/00-shared-context.md` first. It holds the
operator's request. Then read the three research reports:
- `ai_docs/reports/context-compaction/01-context-measure.md` (how to read current context per harness)
- `ai_docs/reports/context-compaction/02-compaction-triggers.md` (how to compact per harness)
- `ai_docs/reports/context-compaction/03-horch-integration.md` (where it plugs into horch; change map; test baseline)

## FILES
- own: `ai_docs/designs/context-compaction.md`, `ai_docs/plans/context-compaction/05-implementation.md`.
- do not touch: every other file. No source edits in this brief.

## PRIOR WORK
The orchestrator adds research gotchas here before spawning you:
Gotchas from research that the design must handle:
- Round 1 left `output_tokens` out of the Claude formula. Report 01 corrects it:
  input + cache_creation + cache_read + output of the last non-sidechain,
  non-synthetic assistant message.
- THRESHOLD CONFLICT: Codex's own auto-compact fires at 244,800 of a 258,400
  window (report 01), which is below 300,000. A fixed 300,000 threshold never
  fires on Codex. Decide a per-harness rule, for example
  `min(300000, a fraction of the harness trigger)`, and justify it.
- Tail-read transcripts. Drop the first partial line. Codex `compacted` lines
  reach 4.6 MB, so the tail window must grow until it holds one full line.
  A full parse of a 160 MB rollout takes 0.31 s; a tail read takes 0.03 s.
- pi and Prime are after a compaction: the value is null until the next
  response. Treat null as "just compacted", not as 0 tokens.
- The orchestrator pane has no `HORCH_*` env vars and no ledger record, so
  `horch note` and `horch done` fail there (report 03 section 1). herdr
  `pane get` gives its `agent_session.value` for Claude and Codex; horch parses
  that in `herdr.rs:48`.
- herdr gives `agent_status` idle/working/blocked/done/unknown. horch's `Pane`
  struct (`herdr.rs:27-41`) does not parse it yet. pi, OpenCode and Prime
  status is not verified live.
- `horch tell ROLE '/compact ...'` already sends a raw line (no prefix,
  send-text plus 2 Enter keys).
- The handoff skill is in all 4 phase catalogs and reaches both orchestrators.
  Its default path `ai_docs/handoffs/whats-next.md` collides between workers
  in the shared tree. Use a per-role path.
- Teammate `env:` reaches every CLI, so a Claude backstop through
  `CLAUDE_CODE_AUTO_COMPACT_WINDOW` needs no Rust change.
- Out of scope, but note in the design: `read_pi` counts error lines and
  toolResult usage; the `usage.rs:360` comment about duplicate token_count
  lines is wrong (report 01 section 9).
- Build and test with `CARGO_TARGET_DIR=/tmp/<role>-target`. The shared `target/`
  has a stale build-script path. Baseline: 340 of 340 tests pass;
  `horch teammates --check` passes with 25 teammates.
- Report 02: busy behavior differs per harness (Claude queues, Codex rejects,
  OpenCode ends the turn, pi aborts, Prime queues). Report 02 recommends that
  horch compact only an idle pane. Codex and OpenCode accept only a bare
  `/compact`; `/compact <text>` becomes a model prompt there. No harness starts
  a new turn after a manual compaction, so horch must send a follow-up message
  that names the handoff file. Claude was checked live once (haiku); Codex was
  not checked live.
- Orchestrator self-compaction: report 02 shows Claude queues a
  `/compact` that `horch tell` types into its own pane, but the self-address
  path is UNVERIFIED, and Codex rejects a self-send. Design for both
  orchestrator flavors (`orchestrator`, `orchestrator-codex`).
<!-- ORCHESTRATOR-NOTES -->

## THE DESIGN MUST DECIDE (with the reason for each decision)
1. Measurement: how horch computes current context tokens per harness, and the
   command that reports it for every live worker and for the orchestrator
   (for example `horch context [--json]`). Output must show role, harness,
   model, context tokens, window, percent, and last-compacted time.
2. Threshold: 300,000 tokens is the default. Where it is configured (a
   default constant, an override per teammate frontmatter, an env var). State
   whether a lower harness auto-compact threshold is also set as a backstop,
   and at what value, per harness. The backstop must not fire before horch's
   own flow has a chance (for example backstop at 400,000).
3. Watch loop: who polls and when. Options include: the orchestrator runs
   `horch context` at each natural checkpoint in its loop (on every DONE,
   every worker message, before every spawn); a horch hook; a background
   watcher. The briefing forbids the orchestrator from running background
   agents; a plain non-agent watcher process may be acceptable. Pick one and
   justify it. Keep it simple.
4. Natural stopping point: define it concretely. For a worker: between plan
   steps, after a `horch note`, when idle waiting for an answer, never mid-edit
   or mid-test-run. For the orchestrator: after it has answered every pending
   worker message and no spawn is half-done. Say who decides (the session
   itself, told by the orchestrator) and how the orchestrator asks
   (for example `horch tell <role> "NOTE: context is 312000 tokens. At your next
   stopping point, run the handoff skill, then send COMPACT-READY."`).
5. whats-next integration: the handoff skill (`skills/handoff/SKILL.md`) writes
   `ai_docs/handoffs/<role>-whats-next.md` (decide the path). It must be
   available to every worker in every phase and to the orchestrator. Say how.
   Say whether the handoff also goes into the ledger (`horch note`).
6. Compaction per harness: the exact action horch performs for each of Claude
   Code, Codex, OpenCode, pi, Prime, from report 02. Include custom compact
   instructions that point the model to its whats-next file. Include the
   harnesses where compaction is impossible or unsafe, and the fallback (for
   example: `horch done` + fresh spawn with the handoff file as PRIOR WORK).
7. Self-compaction of the orchestrator: how the orchestrator compacts itself,
   given report 02's answer about busy-state injection and self-triggering.
8. Verification after compaction: how horch confirms it worked (context number
   dropped, transcript marker) and how the session resumes work.
9. Surface: new or changed horch subcommands, Rust modules, teammate prose,
   `_base` fragments, skills catalog, golden files, docs (`README.md`,
   `docs/phase-skills.md`). Respect: prompts are data, Rust only substitutes
   placeholders.
10. Out of scope: say what v1 does not do. Do not implement
   `ai_docs/designs/telemetry-and-balancing.md`; reuse its ideas only where
   they are needed (for example the orchestrator as a ledger record) and say so.

## THE DESIGN MUST BE SELF-CONTAINED AND CLOUD-VERIFIABLE
The operator may give this work to a remote Claude Code cloud session
(claude.ai/code). Write the design for that reader.
- Self-contained: a cloud session clones the GitHub repository only. The
  research reports and plans under `ai_docs/` are untracked today, so the cloud
  session may not have them. Put every harness fact the implementation needs
  in the design itself: file paths, JSON field names, formulas, commands,
  markers, thresholds, config keys. Cite the report too, but do not depend on it.
- Assume the cloud sandbox has: a Linux container, a fresh clone, the Rust
  toolchain (state the install step if it is missing), Claude Code itself, and
  restricted network. Assume it does NOT have: a herdr server, Codex, OpenCode,
  pi, Prime or Ollama, any logged-in harness other than itself, the operator's
  `~/.claude` or `~/.codex` history, or macOS.
- Add a section `## Verification` with 2 tiers:
  1. Cloud-verifiable. Every check runs in that sandbox with no herdr and no
     other harness. Design the code so this tier is large: checked-in fixture
     transcripts for all 5 harnesses (including a compaction marker and a
     pre/post pair), a home-directory or transcript-path override so
     `horch context` runs against fixtures, unit tests for the per-harness
     formulas and threshold logic, golden-prompt tests for the prose, and
     `horch teammates --check`. For each check give the exact command and the
     exact expected output or pass condition. State how to build fixtures from
     the formats in this design without access to real transcripts.
  2. Local-only. Checks that need herdr and the real harnesses on the
     operator's Mac: live `horch context` against a Claude and a Codex worker,
     a real compaction round trip (worker writes whats-next, compacts, context
     drops, work resumes), the orchestrator self-compaction. Give each as a
     numbered manual procedure with the expected observation.
- Add a section `## Handing this to a cloud session` with: the files the cloud
  session must have (and a note to commit the design first), the order of
  units, and the rule that the session never uses `ANTHROPIC_API_KEY`.

## IMPLEMENTATION PLAN REQUIREMENTS
- Use the `horch:create-plan` skill format if it is available to you.
- Split into units. Each unit lists: GOAL, FILES own / do not touch, STEPS with
  a check per step, DONE WHEN, and the tests to run. Two units must never own
  the same file. Mark which units can run in parallel and which are serialized.
- Name a suggested teammate per unit (opus for judgement-heavy Rust, sonnet or
  codex-terra for mechanical prose and golden updates).
- Every unit keeps `cargo test --workspace` and `horch teammates --check` green.
  State the golden-file update procedure.
- Include one final validation unit that runs the cloud-verifiable tier, and
  list the local-only tier as a separate operator checklist.

## CONSTRAINTS
- Plain exact English. Cite report sections for every harness fact.
- If a research report leaves a decision open, decide and write the reason, or
  send `horch tell orchestrator "QUESTION: ..."` if it needs the operator.

## DONE WHEN
- Both files exist and cover every numbered item above.
- The design has the `## Verification` section (both tiers, exact commands and
  expected results) and the `## Handing this to a cloud session` section.

## REPORT
- `horch tell orchestrator "QUESTION: ..."` for decisions that need the operator.
- `horch done` summary: both paths, the 10 decisions in one line each, the unit
  list with suggested teammates and parallel groups.
