# Brief: branch, commit, push, and open the PR

Repo: /Users/mascott/projects/multi-herdr. Current branch: `main`. Remote `origin` is GitHub (`MattMakes/multi-herdr`), `gh` is logged in.

## Steps, in order
1. `git status --short`. Expected uncommitted work: modified `crates/horch-core/tests/golden_prompts.rs`, `teammates/README.md`, `teammates/_base/fleet-orchestrator.md`, `teammates/_base/fleet-worker.md`; untracked `ai_docs/plans/env-research/`, `ai_docs/plans/tiling-manager-spec.md`, `ai_docs/plans_to_improve.md`, `ai_docs/reports/env-research/`, `ai_docs/reports/layout-survey.md`, and `.herdr-orchestrator/`. If anything else is modified, stop and report it with `horch tell orchestrator` before committing.
2. Do NOT commit `.herdr-orchestrator/`. It is a stale scratch dir from an old run. Leave it untracked; do not delete it.
3. `git checkout -b fleet-efficiency-plan`.
4. Commit 1, teammate prompts only:
   `git add teammates/README.md teammates/_base/fleet-orchestrator.md teammates/_base/fleet-worker.md crates/horch-core/tests/golden_prompts.rs`
   Message (subject line, blank line, body):
   ```
   Require Simplified Technical English in agent-to-agent messages

   Both base briefings gain a "Message style" section: one fact per
   sentence, 20-word cap, active voice, one word per thing, no idioms,
   verbatim identifiers, flat lists, a fixed opening keyword, digits
   with units, warnings before actions. Each has a role-tagged example.
   The golden prompt test pins the new section the same way it pins the
   earlier sanctioned drifts.
   ```
5. Before commit 2, run `grep -rEn 'sk-ant-|api03|MESSAGING_TOKEN=' ai_docs/` and confirm zero hits. If there is a hit, stop and report it; do not commit.
6. Commit 2, research and plans:
   `git add ai_docs/plans_to_improve.md ai_docs/plans/tiling-manager-spec.md ai_docs/plans/env-research ai_docs/reports/env-research ai_docs/reports/layout-survey.md`
   Message:
   ```
   Plan fleet efficiency settings per harness and spec a tiling manager

   ai_docs/plans_to_improve.md ranks the top 10 env vars and settings for
   Claude Code, Codex, OpenCode, pi with Ollama, and Prime Agent, sets
   per-tier compaction thresholds from MRCR v2 and GraphWalks evidence,
   inventories 39 background model calls with their off switches, lists
   the options skipped and why, and gives a measured rollout order.
   Supporting research reports and briefs sit under ai_docs/reports and
   ai_docs/plans/env-research. ai_docs/plans/tiling-manager-spec.md
   specs a tree-driven pane placer for horch spawn.
   ```
7. `cargo test -p horch-core` must pass and `horch teammates --check` must exit 0 before pushing. If either fails, stop and report.
8. `git push -u origin fleet-efficiency-plan`.
9. Open the PR with the body file:
   `gh pr create --base main --head fleet-efficiency-plan --title "Fleet efficiency plan, side-call inventory, STE messaging, tiling spec" --body-file /Users/mascott/projects/multi-herdr/ai_docs/plans/env-research/07-pr-body.md`
10. Report the PR URL with `horch done`. Write the message in Simplified Technical English.

## Out of scope
Do not edit any file. Do not merge. Do not touch `.herdr-orchestrator/`. Do not rebase or squash.
