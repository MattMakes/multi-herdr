# Domain-skills run status (Swift and Unreal Engine)

Integration branch: `design-skills` (worktree `/Users/mascott/projects/mh-wt/integ-ds`).
Plans: this directory. Reports: `ai_docs/reports/domain-skills/`.

| unit | plan | depends on | teammate | state |
|---|---|---|---|---|
| V0 vendoring-infra | v0-vendoring-infra.md | - | opus-38 | running |
| V1 skill-fields | v1-skill-fields.md | - | opus-39 | running |
| V2 roster-offer | v2-roster-offer.md | - | opus-40 | running |
| W1 ue-vendor | w1-ue-vendor.md | V0 for the gate | sonnet-9 | running |
| W2 ue-own | w2-ue-own.md | - | opus-41 | running |
| S1 swift-core | s1-swift-core.md | V0 for the gate | opus-42 | running |
| S2 swift-platform | s2-swift-platform.md | V0 for the gate | sonnet-10 | running |
| S3 swift-review | s3-swift-review.md | V0 for the gate | opus-43 | running |
| S4 swift-ship | s4-swift-ship.md | V0 for the gate | opus-44 | running |
| P-UE ue-teammates | p1-ue-teammates.md | W1 W2 V1 V2 | | waiting |
| P-Swift swift-teammates | p2-swift-teammates.md | S1-S4 V1 V2 | | waiting |

Merge order: V0, then any skill unit (rebase on V0), V1, V2; personas last.
Shared-file conflicts (`skills/provenance.json`, `skills/README.md`) are
resolved by `/tmp/dsmerge.sh` (union by name).
