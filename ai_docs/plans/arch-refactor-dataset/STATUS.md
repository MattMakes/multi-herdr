# Orchestrator status (arch-refactor-dataset)

Integration worktree: /Users/mascott/projects/mh-wt/integration (branch arch-refactor-dataset, base 575c2c2).
Merge protocol: worker sends READY-TO-MERGE ard/<unit> <sha>; orchestrator runs
`git merge --ff-only ard/<unit>` (or asks REBASE), runs `just gate`, replies MERGED.
CURRENT_PHASE: orchestrator appends a phase when its last unit merges.

## Wave 1 (spawned 2026-10-02)
| Unit | Slug | Worker | Status |
|---|---|---|---|
| U01 | a0-gate | opus-3 (codex-sol-1 retired: sandbox) | running |
| U02 | a0-oracles | opus-1 | MERGED |
| U03 | a0-designs | staff-engineer-1 | running |
| U04 | a8-marketplace | backend-developer-2 | running (needs U01 allowlist) |
| U05 | a1-vocabulary | opus-2 | running |
| U06 | b1-primitives | backend-developer-1 | running (needs U01 allowlist) |
| U07 | e2e-fakes | qa-engineer-1 | running |
| U08 | machine-teacher | sonnet-1 | MERGED 658b57d |

## Wave 2 plan (after A1 merges)
- A2-core (runtime/*, bootstrap, ledger/mailbox/clock/usage/telemetry env, Brief v2), arc_05 scan with temp allowlist for A3/A4/A5 files.
- A3 roster split (owns teammates env removal, fallback_problems move).
- A4 harness ownership (owns launch/codex/opencode/prime/plugins/skills env removal). Needs U02 oracles.
- A5 routing (owns policy/quota env removal). Needs U02 oracles.
- A7a workspace move (herdr/layout/tile/balance/paneshell/tilecmd).
Later: A6 (split store early after U06+A1), A7b messaging, A9 after A8+A3, A10, A11, A12; B1 rest after U03+U06; B2 vcs after A8.

## Open items
- Spec A / Spec B verbatim text: requested from operator. Needed for design appendices, judge.md (§10), Judgment schema (§11), event list, CHECKLIST (§16), final audit.
