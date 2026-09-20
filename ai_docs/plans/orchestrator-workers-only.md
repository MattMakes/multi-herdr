# Brief: tell the orchestrator to use fleet workers only, never subagents

Repo: /Users/mascott/projects/multi-herdr, branch `fleet-efficiency-plan` (confirm with `git branch --show-current`). Another worker is committing to this branch at the same time, on different files. Before you commit, run `git pull --rebase`; if `git push` is rejected, run `git pull --rebase` again and push again.

## Goal
Add this rule to the fleet orchestrator's base prompt so every orchestrator (Claude and Codex flavour) sees it:

> Use your fleet workers only. Do not use subagents, the Agent tool, background tasks, or any in-session delegation. Every piece of delegated work goes through `horch spawn` or `horch assign`, so it is visible in the ledger and the grid.

## Files
- `/Users/mascott/projects/multi-herdr/teammates/_base/fleet-orchestrator.md`: add the rule as its own two-line bullet in the `== Protect your context ==` section (the last section), directly after the sentence that says to spawn workers to do the work. Keep the wording above; adjust only the surrounding punctuation to fit the section's style. Do not add a new heading.
- Check `/Users/mascott/projects/multi-herdr/teammates/orchestrator-codex.md` and `orchestrator.md`: if either carries its own copy of the "Protect your context" text instead of inheriting the base, add the same bullet there too. Otherwise leave them.
- `/Users/mascott/projects/multi-herdr/crates/horch-core/tests/golden_prompts.rs`: the golden tests compare rendered prompts to frozen files and pin each sanctioned change with a `.replace(...)` call. Read how the `ste` section was pinned (function `ste_section` and its use in the orchestrator test around lines 225-235) and pin this bullet the same way: find the exact text that precedes the insertion point in the golden and replace it with itself plus the new bullet. Do not edit the files under `crates/horch-core/tests/golden/`.

## Steps
1. Read the base file fully. Make the edit.
2. `cargo test -p horch-core` (all tests, not a filter; the golden tests are skipped by a `teammates` filter). All must pass.
3. `cargo build -p horch` then `horch teammates --check`, exit 0.
4. `git pull --rebase`, then commit (include this brief file) with message:
   ```
   Tell the orchestrator to delegate through fleet workers only

   No subagents, Agent tool, or background tasks: every delegated piece
   of work goes through horch spawn or horch assign so the ledger and
   the grid show it.
   ```
5. `git push` (retry after `git pull --rebase` if rejected).
6. Report with `horch done` in Simplified Technical English: files changed, added lines per file, test count, commit hash.

## Out of scope
No other prompt changes. Do not touch `.herdr-orchestrator/` or any file under `ai_docs/` except adding this brief to the commit.
