# Brief: switch claude.ai-synced skills off by default in every fleet Claude pane

Repo: /Users/mascott/projects/multi-herdr, branch `fleet-efficiency-plan` (confirm with `git branch --show-current`). Claude Code installed: 2.1.276 at `/Users/mascott/.local/share/claude/versions/2.1.276`.

## Goal
The operator's Claude Code lists 11 skills synced from claude.ai under the `anthropic-skills:` prefix (anki-learning-builder, docs, docx, import-memory, morning, pdf, persona, pptx, skill-creator, treasures-talk-writer, xlsx; about 2,300 tokens of listing in total). None is useful to a fleet worker or the orchestrator. They must be OFF by default in every Claude pane horch launches (workers and the orchestrator), without editing the operator's `~/.claude/settings.json`, and without dropping any other operator setting.

Known facts:
- Claude Code stores a per-skill off switch in the settings key `skillOverrides`, format `{"<skill-name>": "off"}`. The operator's user settings already have `{"explain-diff-notion": "off"}`. The 11 synced skills are NOT in there: the toggle the operator used was not persisted to that file.
- horch builds a `--settings` JSON overlay per Claude launch in `crates/horch-core/src/launch.rs` around lines 288-318 (`enabledPlugins` off-map when `inherit_plugins: false`, plus `statusLine`). Settings merge per key, so adding a `skillOverrides` object there disables only the named skills. A teammate that sets its own `settings:` file replaces the overlay entirely; that stays as-is but must be documented.
- Every Claude teammate, including `teammates/orchestrator.md`, goes through this launch builder and has `inherit_plugins: false`.

## Steps
1. Research first (30 minutes max), write findings to `ai_docs/reports/claudeai-synced-skills.md`:
   a. Is there a single settings key or env var in 2.1.276 that disables ALL claude.ai-synced skills at once? Grep the binary: `strings <bin> | grep -iE 'claudeai.?skill|synced.?skill|skill.?sync|disableClaudeAi' | sort -u`, and the settings schema descriptions (`strings <bin> | grep -E '^(When|Whether|Disable|Enable).*skill' | head -50`). The operator already uses `disableClaudeAiConnectors: true`; look for a sibling for skills.
   b. Where are the synced skills cached locally? Look under `~/.claude/` for a directory holding the 11 names (try `grep -rl "treasures-talk-writer" ~/.claude --include=*.md --include=*.json -l 2>/dev/null | head`). Record the path pattern and how a skill's listed name (`anthropic-skills:<name>`) maps to it.
   c. Confirm the exact `skillOverrides` value that turns a skill off (`"off"`) and that the key merges with the operator's own overrides rather than replacing them, by launching `claude -p '/skills' --settings '{"skillOverrides":{"anthropic-skills:docx":"off"}}'` or by reading the settings-merge code. If `-p` mode cannot list skills, read the merge code in the binary (search `skillOverrides`).
2. Implement, choosing by what 1a and 1b found:
   - If a single global switch exists (1a): set it in the overlay whenever `inherit_plugins: false`. Add a teammate boolean `inherit_claudeai_skills: false` (default false) that, when true, leaves the switch out.
   - Otherwise: add `fn operator_synced_skills() -> Vec<String>` next to `operator_enabled_plugins()` in launch.rs that discovers the synced skill names from the cache found in 1b, and emit `skillOverrides: {<each>: "off"}` in the overlay whenever `inherit_plugins: false`. Also add a teammate list field `disabled_skills: Vec<String>` (default empty) whose entries are added to the same `skillOverrides` map, so an operator can switch off any further skill per teammate. Add `inherit_claudeai_skills: bool` (default false) to opt a teammate back in.
   - In both cases the overlay must still merge with the operator's own `skillOverrides` from user settings. Verify with a live launch that the operator's `explain-diff-notion: off` survives (check `/skills` output or the resolved settings).
3. Wire the new field(s) through `crates/horch-core/src/teammates.rs` (struct, default, the field table used by `horch teammates`, and the roster check), and document them in `teammates/_template.md` under the skills section, with one sentence: "claude.ai-synced skills are off in every fleet pane; set `inherit_claudeai_skills: true` to keep them."
4. Tests: unit tests in launch.rs for the overlay JSON (synced skills off, a `disabled_skills` entry added, opt-in leaves them out, the map is absent when `inherit_plugins: true`). Use the existing launch test style around `launch.rs:752` (orchestrator launch test). Do not depend on the operator's real cache in tests: inject the list.
5. Live check: `horch spawn sonnet "type /skills, read the list, then run: horch done 'skills listed: N, anthropic-skills entries shown as off: M'"` from a pane inside this workspace is NOT required; instead run `claude --model sonnet --settings '<the overlay horch would build>' -p 'Reply OK'` once from the repo root with `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false` to confirm the settings parse without error. Do not open extra herdr panes.
6. `rustfmt --edition 2021 --check` on touched files only (5 files in `crates/horch` and `crates/horch-core/src/agent.rs` have pre-existing diffs; do not reformat them). `cargo build` with 0 new warnings, `cargo test -p horch-core`, `horch teammates --check`.
7. Update `ai_docs/plans_to_improve.md` section 3.1: add a row after #10 for this switch (name, value, where: overlay, effect about 2,300 tokens of listing per Claude pane, quality none, RECOMMEND, applied), and in section 4 item 9 note that the overlay now accepts `skillOverrides` and `disabled_skills`.
8. Commit on the current branch (include this brief and the report). Message:
   ```
   Switch claude.ai-synced skills off in every fleet Claude pane

   The 11 anthropic-skills entries synced from claude.ai cost about 2,300
   tokens of listing in each pane and none serves a fleet worker or the
   orchestrator. horch now emits skillOverrides for them in its settings
   overlay whenever inherit_plugins is false, merged with the operator's
   own overrides. Teammates can opt back in with inherit_claudeai_skills
   or switch further skills off with disabled_skills.
   ```
   Adjust the message if step 2 took the global-switch branch. `git push`.
9. Report with `horch done` in Simplified Technical English: which branch of step 2 you took, files changed, test count, commit hash, and the live-check result.

## Out of scope
Do not edit `~/.claude/settings.json`. Do not change plugin handling, MCP handling, or non-Claude harnesses. Do not touch `.herdr-orchestrator/`.
