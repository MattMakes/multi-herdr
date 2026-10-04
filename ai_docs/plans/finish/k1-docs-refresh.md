# K1 docs-refresh: the docs describe what the branch now does

Unit slug: `docs-refresh`. Branch: `ds/docs-refresh`.

## GOAL

A junior engineer who reads `README.md`, `docs/` and `teammates/README.md`
can use every feature added in this run without reading code: vendored
skills and re-vendoring, `available_skills`, `operator_skills`,
`offer_when`, `requires`, `horch doctor` checks, the Unreal team (Git LFS,
UE 5.8), the Swift/Apple team (MobileBuildMCP, app-release-preparer), the
Blender teammate (when merged), the design team, `horch agent-list`, the
Antigravity harness, `multi-herdr-dataset`, and the gate (clippy, slots,
the git-env scrub, "never gate under git rebase -x").

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md`, the reports in
  `ai_docs/reports/design-skills/`, `ai_docs/reports/domain-skills/`,
  `ai_docs/reports/finish/`, and the current docs (`docs/handbook*`,
  `docs/command-flow.md`, `docs/recipes/`).
- Other units edit `README.md` (opus-56: the Apple section) and
  `teammates/README.md` (UE and Blender rows). Do not edit those sections;
  link to them. Prefer `docs/` pages for new material.
- Check every command you document by running it with the installed binary
  (`--help` at least). Write in the docs' existing voice.

## FILES

own: `docs/**` (except files another active unit names), README sections
other than the Apple section, `ai_docs/reports/finish/docs-refresh.md`.

## STEPS

1. Inventory: feature → where documented today → gap. 2. Write. 3. Verify
commands. 4. Targeted checks (markdown only: the doc tests if any, and
`horch teammates --check`). READY-TO-MERGE (merge train).
