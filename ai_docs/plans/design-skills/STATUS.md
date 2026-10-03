# design-skills run status

Integration: /Users/mascott/projects/mh-wt/integ-ds on branch design-skills (from arch-refactor-dataset, PR #15).
Sources: /Users/mascott/projects/mh-wt/_sources/design-skills (read-only, PINS.txt).
Gate: /tmp/igate-ds.sh

| Unit | Slug | Worker | State |
|---|---|---|---|
| D00 | skills-infra | sonnet-6 | MERGED |
| D01 | skill-taste | opus-23 | MERGED |
| D02 | skill-art-direction | opus-24 | MERGED |
| D03 | skill-landing-page | opus-25 | MERGED |
| D04 | skill-design-system | opus-26 | MERGED |
| D05 | skill-motion | opus-27 | MERGED |
| D06 | skill-imagery | opus-28 | MERGED |
| D10 | followups | opus-31 | MERGED |
| D07 | design-personas | opus-29 | MERGED |
| D08 | antigravity-harness | opus-30 | MERGED |
| D09 | agent-list | sonnet-7 | MERGED |

Merge order: D00 first; then D01-D06 (tell each to rebase and add provenance), D09, D08; D07 last.
Notes:
- Oracle rule: new teammates go in SKIP_NEW_TEAMMATES (no new oracle files; arc_01 counts fixed). designer/frontend-developer skills oracles re-blessed on purpose.
- Installed horch (~/.local/bin) is the old build; roster overlay from HORCH_TEAMMATES_DIR=v1 path in the orchestrator shell. No sonnet-feature there.
- Orchestrator fix merged: skill tests independent of new skills (A0 view for full listing; DESIGN_SOURCE_PINS).
- Follow-ups: harness_version should read stderr (prime prints --version there; pi --version crashes here); SkillExposure::as_str; skills/README intro still says one upstream.
- Orchestrator: harness_version 15 s + stderr. Merge helper /tmp/dsmerge.sh resolves README rows and provenance (union by name).
