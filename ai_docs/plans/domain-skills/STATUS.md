# Domain-skills run status (Swift and Unreal Engine)

Integration branch: `design-skills` (worktree `/Users/mascott/projects/mh-wt/integ-ds`).
Plans: this directory. Reports: `ai_docs/reports/domain-skills/`.

| unit | plan | depends on | teammate | state |
|---|---|---|---|---|
| V0 vendoring-infra | v0-vendoring-infra.md | - | opus-38 | merged |
| V1 skill-fields | v1-skill-fields.md | - | opus-39 | merged |
| V2 roster-offer | v2-roster-offer.md | - | opus-40 | merged |
| W1 ue-vendor | w1-ue-vendor.md | V0 | sonnet-9 | merged |
| W2 ue-own | w2-ue-own.md | - | opus-41 | merged |
| S1 swift-core | s1-swift-core.md | V0 | opus-42 | merged |
| S2 swift-platform | s2-swift-platform.md | V0 | sonnet-10 | merged |
| S3 swift-review | s3-swift-review.md | V0 | opus-43 | merged (history rewritten: key-shaped example strings) |
| S4 swift-ship | s4-swift-ship.md | V0 | opus-44 | merged |
| V3 loose-ends | v3-loose-ends.md | V1 V2 | opus-46 | merged |
| P-UE ue-teammates | p1-ue-teammates.md | W1 W2 V1 V2 | opus-48 | merged; builders' Codex fallback removed by the orchestrator |
| P-Swift swift-teammates | p2-swift-teammates.md | S1-S4 V1 V2 | opus-49 | running |
| S5 skill-fixes | s5-skill-fixes.md | S3 S4 W2 | sonnet-11 | merged |

Merge order: V0, then any skill unit (rebase on V0), V1, V2; personas last.
Shared-file conflicts (`skills/provenance.json`, `skills/README.md`) are
resolved by `/tmp/dsmerge.sh` (union by name).

Remote: `origin/design-skills` is pushed after every merge that passes the
gate (operator request, 2026-10-03). Never push a red merge.

Design-skills follow-ups in this run: D16 dead-code merged; D17 polish running (opus-37); D18 flake-and-durable merged (cmp_05 was a real state-overwrite bug); D19 dataset-tidy running (opus-47).

The merge script scans each merge for key-shaped strings before the gate (GitHub push protection rejected an upstream example Stripe key on 2026-10-03; it never reached the remote).
