## Summary

Planning and prompt work from a research fleet run on 2026-09-18 and 2026-09-19, plus three small behaviour changes to `horch`. Eight commits.

### Applied: Simplified Technical English for agent-to-agent messages (317f8e7)

`teammates/_base/fleet-worker.md` and `fleet-orchestrator.md` gain a "Message style" section. Every `horch tell`, `horch assign`, `horch done`, and `[role]` line now follows STE: one fact per sentence, 20-word cap, active voice, one word per thing, no idioms or hedges, verbatim identifiers, flat lists, a fixed opening keyword (`ready`, `DONE:`, `BLOCKED:`, `NOTE:`, `QUESTION:`), digits with units, warnings before actions. Each file has a role-tagged example. The golden prompt test pins the section.

### Applied: orchestrator delegates through fleet workers only (741ddea)

The orchestrator base prompt's "Protect your context" section now says: use fleet workers only, no subagents, Agent tool, background tasks, or in-session delegation. Every delegated piece of work goes through `horch spawn` or `horch assign` so the ledger and the grid show it. Both orchestrator flavours inherit it.

### Applied: claude.ai-synced skills are off in every fleet Claude pane (d4710ef, 179a36d)

The 11 `anthropic-skills:*` entries synced from claude.ai cost about 2,300 tokens of listing per pane and none serves a fleet worker or the orchestrator. Claude Code 2.1.276 has a single setting, `syncClaudeAiSkills: false`, that hides them for a launch without moving files. `horch` now emits it in its per-launch settings overlay for every Claude pane unless the teammate sets `inherit_claudeai_skills: true`. A new per-teammate `disabled_skills` list renders into `skillOverrides`, merged per key with the operator's own overrides (the existing `explain-diff-notion: off` was verified to survive). Live check on the orchestrator overlay: 21 skills listed before, 10 after. Documented in `teammates/_template.md`; research in `ai_docs/reports/claudeai-synced-skills.md`.

### Applied: fleet workspace named after the project folder (3debb5b, be4d135)

`horch fleet` labelled every workspace "Herdr Fleet". It now uses the final component of the working directory, resolving relative paths such as `.` and `..` first, and falls back to the old label only for a root or empty path. The fixed 5-pane orchestration recipe keeps its label. Note: the resolver follows symlinks, so a symlinked project dir gets its target's name.

### Planned: `ai_docs/plans_to_improve.md` (c99f5c1, 1166917)

Top-10 env vars and settings per harness (Claude Code 2.1.276, Codex 0.154.0, OpenCode 1.18.2, pi 0.85.1 with Ollama 0.32.15, Prime Agent 0.9.4), each verified against the installed binary, that version's docs, or `--help`. Highlights:

- **Compact earlier.** Claude Code compacts the 1M models at about 967k tokens. Third-party MRCR v2 shows Opus 5 dropping below 90% of its short-context score in the 128K–256K bin and Sonnet 5 in the 32K–64K bin; Anthropic's own evals compact at 200k. The plan sets 200k for Opus workers, 150k for Sonnet tiers, 300k for the orchestrator, and matching values for Codex, pi, and Prime, with a compaction-instructions block.
- **Effort split.** Every Claude teammate runs `xhigh`; by the CLI's own cost index that is 2.41x `high` on Sonnet 5. Grunt tiers and implementers move to `high`; judgment roles keep `xhigh`.
- **Nine bugs found**, including: pi cannot start on the shell's node version; OpenCode silently drops the effort horch passes; `--pure` keeps the operator's Playwright MCP on free workers; Codex's private home symlinks the real config; Prime's auto-refine rewrites the global harness dir from inside a worker.
- **39 background model calls inventoried** (section 3.6) with a verified off switch or a verified "none exists" for each: prompt suggestions, session titles, subagent summaries, recaps, title agents, auto-refine.
- **Measured:** four Codex flags cut input tokens per request by about 35%.
- **Skipped options** grouped by reason: quality policy, drops tuned settings, or not a real saving.
- A measured rollout order and eight open questions for the operator.

### Specced: `ai_docs/plans/tiling-manager-spec.md` (c99f5c1)

A tree-driven pane placer for `horch spawn`: orchestrator always leftmost, workers fill top then bottom then next column, tab 1 holds 2x2 beside the orchestrator, overflow tabs hold 2x3, freed slots are reused, broken tabs get a repair that moves panes. Grounded in `ai_docs/reports/layout-survey.md`.

## Test plan

- [x] `cargo test -p horch-core` passes (133 unit + 5 golden)
- [x] `cargo test -p horch` passes (40)
- [x] `cargo build` with 0 warnings; `rustfmt --check` clean on every touched Rust file
- [x] `horch teammates --check` exits 0 (22 teammates)
- [x] Live launch with the new overlay: settings parse, synced skills absent, operator's own `skillOverrides` intact
- [x] No credentials in `ai_docs/`
- [ ] Reviewer reads sections 2.3 and 7 of the plan and answers the open questions
