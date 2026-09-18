## Summary

Planning and prompt work from a research fleet run on 2026-09-18. Two commits: one changes the base teammate prompts, the other adds the plan, spec, briefs, and research reports. No Rust behaviour changes; the only Rust edit is a golden-test pin.

### Applied: Simplified Technical English for agent-to-agent messages

`teammates/_base/fleet-worker.md` and `fleet-orchestrator.md` gain a "Message style" section. Every `horch tell`, `horch assign`, `horch done`, and `[role]` line now follows STE: one fact per sentence, 20-word cap, active voice, one word per thing, no idioms or hedges, verbatim identifiers, flat lists, a fixed opening keyword (`ready`, `DONE:`, `BLOCKED:`, `NOTE:`, `QUESTION:`), digits with units, warnings before actions. Each file has a role-tagged example. The golden prompt test pins the section.

### Planned: `ai_docs/plans_to_improve.md`

Top-10 env vars and settings per harness (Claude Code 2.1.276, Codex 0.154.0, OpenCode 1.18.2, pi 0.85.1 with Ollama 0.32.15, Prime Agent 0.9.4), each verified against the installed binary, that version's docs, or `--help`. Highlights:

- **Compact earlier.** Claude Code compacts the 1M models at about 967k tokens. Third-party MRCR v2 shows Opus 5 dropping below 90% of its short-context score in the 128K–256K bin and Sonnet 5 in the 32K–64K bin; Anthropic's own evals compact at 200k. The plan sets 200k for Opus workers, 150k for Sonnet tiers, 300k for the orchestrator, and matching values for Codex, pi, and Prime, with a compaction-instructions block.
- **Effort split.** Every Claude teammate runs `xhigh`; by the CLI's own cost index that is 2.41x `high` on Sonnet 5. Grunt tiers and implementers move to `high`; judgment roles keep `xhigh`.
- **Nine bugs found**, including: pi cannot start on the shell's node version; OpenCode silently drops the effort horch passes; `--pure` keeps the operator's Playwright MCP on free workers; Codex's private home symlinks the real config; Prime's auto-refine rewrites the global harness dir from inside a worker.
- **39 background model calls inventoried** (section 3.6) with a verified off switch or a verified "none exists" for each: prompt suggestions, session titles, subagent summaries, recaps, title agents, auto-refine.
- **Measured:** four Codex flags cut input tokens per request by about 35%.
- **Skipped options** grouped by reason: quality policy, drops tuned settings, or not a real saving.
- A measured rollout order and eight open questions for the operator.

### Specced: `ai_docs/plans/tiling-manager-spec.md`

A tree-driven pane placer for `horch spawn`: orchestrator always leftmost, workers fill top then bottom then next column, tab 1 holds 2x2 beside the orchestrator, overflow tabs hold 2x3, freed slots are reused, broken tabs get a repair that moves panes. Grounded in `ai_docs/reports/layout-survey.md`.

## Test plan

- [x] `cargo test -p horch-core` passes (138 tests)
- [x] `horch teammates --check` exits 0
- [x] No credentials in `ai_docs/` (grep for key and token patterns)
- [ ] Reviewer reads sections 2.3 and 7 of the plan and answers the open questions
