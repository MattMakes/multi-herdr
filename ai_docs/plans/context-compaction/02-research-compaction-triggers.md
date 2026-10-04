# Brief 02: how to trigger compaction on each harness from outside the session

## GOAL
Write `ai_docs/reports/context-compaction/02-compaction-triggers.md`. It tells
horch exactly how the orchestrator makes a running session compact on each of
the 5 harnesses, and what happens around that action.

## CONTEXT
Read `ai_docs/plans/context-compaction/00-shared-context.md` first.
The only channel into a pane is terminal text injection (`horch tell` types text
and presses Enter; confirm in `crates/horch-core/src/paneshell.rs` and
`crates/horch/src/cmd/messaging.rs`). The planned flow is: the session writes a
whats-next handoff file, then it compacts, then it re-reads the handoff file
and continues. Brief 01 (context measurement) and brief 03 (horch/herdr
integration) run in parallel. Do not research their topics.

## FILES
- own: `ai_docs/reports/context-compaction/02-compaction-triggers.md` (create it).
- own (scratch only): `/tmp/horch-compact-research/` for probe files.
- do not touch: every other file. No source edits.

## PRIOR WORK
- `ai_docs/reports/env-research/claude-code.md` finding 4 (auto-compact levers).
- `ai_docs/reports/env-research/codex-opencode.md` and
  `ai_docs/reports/env-research/pi-ollama-prime.md` (config levers).
- `ai_docs/reports/env-research/compaction-benchmarks.md` section 2 (default
  triggers per harness).
- Installed binaries: Claude Code under `~/.local/share/claude/versions/`,
  Codex under `/opt/homebrew/Caskroom/codex/`, OpenCode `~/.opencode/bin/opencode`,
  pi `/opt/homebrew/bin/pi`, Prime `/opt/homebrew/bin/prime-agent`.
  Check the current versions first; they auto-update.

## STEPS
For each harness (Claude Code, Codex, OpenCode, pi, Prime Agent):
1. Name the manual compact command (for example `/compact`) and its argument
   syntax for custom instructions. Check: cite docs URL, source file, or
   binary strings.
2. State what happens when the command text arrives while the agent is BUSY
   mid-turn: queued until the turn ends, rejected, sent as a user message, or
   interrupts. This decides whether the orchestrator must wait for idle.
   Check: source code or docs. Mark UNVERIFIED if neither shows it.
3. State whether the harness can be told to compact from inside the model's
   own turn (a tool, a hook, or the model typing into its own pane via
   `horch tell` to itself). This matters for the orchestrator compacting itself.
4. State the config lever that lowers the automatic compact threshold to about
   300,000 tokens (env var, CLI flag, settings key, config.toml key), with exact
   names and allowed ranges. This is the backstop if the orchestrator misses.
5. State what survives compaction: system prompt, CLAUDE.md or AGENTS.md
   re-read, custom instructions, file references. State how the model knows to
   re-read `ai_docs/handoffs/<role>-whats-next.md` afterwards (custom compact
   instructions, a hook such as Claude `SessionStart` with `source=compact` or
   `PreCompact`, or a follow-up `horch tell`).
6. State how to detect that compaction finished (transcript marker, pane
   status, screen text).
7. Optional live check, Claude Code and Codex only, at most 2 tiny sessions in
   total: in a scratch dir under `/tmp/horch-compact-research/`, run a short
   interactive session, send `/compact keep only the word banana` and record
   the result. Use `env -u ANTHROPIC_API_KEY claude` for Claude. Use the
   cheapest model (`--model haiku` for Claude). Skip this step if a source
   answers steps 2 and 6 already.

## CONSTRAINTS
- Prefer source and binary evidence over live runs. Codex, OpenCode and pi are
  open source; read the source at the installed version tag.
- Do not change any global config (`~/.claude/settings.json`, `~/.codex/config.toml`,
  OpenCode or pi config). Pass flags or env on the command line only.
- Do not start a subagent. A live check in step 7 is the only allowed agent run.

## DONE WHEN
- The report has one section per harness answering steps 1-6, each fact cited
  or marked UNVERIFIED.
- The report ends with a table: harness | command | busy behavior | self-compact
  possible | threshold lever | post-compact re-read hook | completion signal.
- The report ends with a "Recommendation input" list.

## REPORT
- `horch note` at each harness finished.
- `horch tell orchestrator "QUESTION: ..."` if blocked. Continue other harnesses meanwhile.
- `horch done` summary: report path, the table rows in one line each, and the
  live checks you ran (or "none").
