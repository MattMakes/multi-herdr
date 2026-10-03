# design-skills run status

Integration: /Users/mascott/projects/mh-wt/integ-ds on branch design-skills (from arch-refactor-dataset, PR #15).
Sources: /Users/mascott/projects/mh-wt/_sources/design-skills (read-only, PINS.txt).
Gate: /tmp/igate-ds.sh

| Unit | Slug | Worker | State |
|---|---|---|---|
| D00 | skills-infra | sonnet-6 | running |
| D01 | skill-taste | opus-23 | running |
| D02 | skill-art-direction | opus-24 | running |
| D03 | skill-landing-page | opus-25 | running |
| D04 | skill-design-system | opus-26 | running |
| D05 | skill-motion | opus-27 | running |
| D06 | skill-imagery | opus-28 | running |
| D07 | design-personas | opus-29 | running (adds skill ids as skills merge) |
| D08 | antigravity-harness | opus-30 | running (research first) |
| D09 | agent-list | sonnet-7 | running |

Merge order: D00 first; then D01-D06 (tell each to rebase and add provenance), D09, D08; D07 last.
Notes:
- Oracle rule: new teammates go in SKIP_NEW_TEAMMATES (no new oracle files; arc_01 counts fixed). designer/frontend-developer skills oracles re-blessed on purpose.
- Installed horch (~/.local/bin) is the old build; roster overlay from HORCH_TEAMMATES_DIR=v1 path in the orchestrator shell. No sonnet-feature there.
