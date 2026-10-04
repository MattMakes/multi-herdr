# R2 review-roster: cross-vendor review of the roster and skill-field work

Read-only review. No branch, no commits, no file writes outside /tmp.

## GOAL

Review the new teammate fields and skill exposure for correctness and
security, ranked by consequence, each with file:line and a failure scenario.

## SCOPE

On branch `design-skills` (read in `/Users/mascott/projects/mh-wt/integ-ds`):
- `available_skills`, `operator_skills` (catalog, activation, briefing,
  materialize, routing merge, spawn and coordinator paths);
- `offer_when`, `requires`, `project_facts`, `horch doctor` xcode check;
- `teammate_env` `~` expansion (`harness/launch.rs`);
- the `app-release-preparer` deny list and credential isolation
  (`teammates/app-release-preparer.md`): can the teammate reach a live App
  Store write or another key?
Reports: `ai_docs/reports/domain-skills/{skill-fields,roster-offer,loose-ends,swift-teammates}.md`.

## REPORT

As R1: findings by `horch tell orchestrator`, full ranked list in `horch done`.
