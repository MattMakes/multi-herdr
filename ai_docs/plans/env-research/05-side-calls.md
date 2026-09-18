# Brief: verify every background or side model call per harness and its off switch

Read `/Users/mascott/projects/multi-herdr/ai_docs/plans/env-research/00-shared-context.md` FIRST (verification rules, hygiene, versions).
Then read section 3.6 of `/Users/mascott/projects/multi-herdr/ai_docs/plans_to_improve.md`. That table is your deliverable: you EDIT IT IN PLACE, replacing every row marked TO VERIFY with a verified finding, and adding rows for any side call you discover that is not listed.

## Goal
The operator requires that every harness feature which makes a model call the worker did not ask for (recap, prompt suggestion, session title, away summary, narration, memory extraction, thread classifier, background summaries, "small model" tasks) is OFF in fleet panes. For each one you must find: whether it exists in the installed version, whether it is on today, and the exact switch that turns it off, with the landing place (teammate `env:`, teammate `args:`, horch settings overlay, `OPENCODE_CONFIG_CONTENT`, pi or prime settings.json).

## Steps
1. Claude Code (2.1.276 at `~/.local/share/claude/versions/2.1.276`, also check 2.1.274): trace the gates for `agent_summary`, `agent_classifier`, `rolling_compact`, and any other `querySource` values in the binary (`strings <bin> | grep -oE 'querySource:"[a-z_]+"' | sort -u` and similar). For each querySource: what triggers it, is it reachable from a plain interactive pane, and what env var or settings key disables it. Also find whether a client-side switch exists for background memory extraction beyond `autoMemoryEnabled`. Prior evidence is in `ai_docs/reports/env-research/claude-code.md` Headline 7 and the Unverified list.
2. Codex (0.154.0, source at git tag `rust-v0.154.0`; a checkout may already be at `/tmp/codex-src`): search for session title generation, thread naming, summaries, "recap", `memories`, and any request that is not the main turn (grep `summar`, `title`, `recap`, `classif` in `codex-rs/core/src`). For each: on by default? config key or `--disable <feature>` to turn it off? Verify with `codex features list` and `codex debug prompt-input` where possible. Do NOT write to `~/.codex/config.toml`.
3. OpenCode (1.18.2, bundle at `~/.opencode/bin/opencode`): find every use of `getSmallModel` and `agent.title` in the bundle; find whether the title agent can be disabled (look for `agent.title.disable`, `title: false`, `OPENCODE_DISABLE_TITLE`, or a permission that blocks it). Also list any other background agents (`summarize`, `compaction` agent model, `general`, `explore`) and which model each runs on by default. Do not make live free-tier calls.
4. pi (0.85.1) and Prime (0.9.4): search the bundles under `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/` and `/opt/homebrew/lib/node_modules/prime-agent/dist/bundle/` for session naming or titling, branch summaries, goal refinement, daemon-side summaries, and any model call outside the main turn and compaction. Check the shipped `docs/*.md` for settings that disable them. Prime's `refine` and `goal` built-in skills are removed by `--no-skills` already; confirm nothing else calls the model in the background.
5. Edit section 3.6 of `plans_to_improve.md` in place: replace each TO VERIFY row, add any new rows, keep the table format. Add one line under the table listing which switches need a horch Rust change (settings overlay keys) versus teammate-file changes only.
6. Write a short evidence appendix to `/Users/mascott/projects/multi-herdr/ai_docs/reports/env-research/side-calls.md` (binary strings, doc URLs, code lines) so each table row is traceable.

## Out of scope
No changes to teammate files, settings, config, or Rust. No live model calls on free-tier endpoints. Do not touch any other section of `plans_to_improve.md`.

## Done looks like
Section 3.6 has zero TO VERIFY rows, every row names an exact switch or says "does not exist in <version>", the appendix exists. Reply with `horch done` giving the two paths and the count of rows changed or added.
