# Map: fleetd-finish

Spec: `ai_docs/plans/fleetd-finish/plan.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [A1 isolation: dev-sandbox script, shared-ledger write guard, service label, e2e seal, and old-watchdog stop proof](01-a1-isolation.md) |  | 1 |
| 02 | [A2 triage bug: fix detached-HEAD worktree handling in horch worktree triage apply](02-a2-triage-detached.md) | 01 | 2 |
| 03 | [B1 baseline: execute fmt, clippy, and full workspace tests in the sandbox and record baseline-2026-11.md](03-b1-baseline.md) | 01 | 2 |
| 04 | [C1 land T-898: FR-10 fleet-process security boundary](04-c1-land-t898.md) | 01 | 2 |
| 05 | [C2 T-899: remove operator-class requests and operator token access from the worker wrapper](05-c2-t899-wrapper-operator.md) | 04 | 3 |
| 06 | [C3 land T-558: intake and approval e2e tests (FR-1, FR-10, FR-12, FM-30, FM-32)](06-c3-land-t558.md) | 04 | 3 |
| 07 | [C4 land T-559: dispatcher and sub-orchestrator e2e tests (FR-2, FM-16, FM-29, FM-31)](07-c4-land-t559.md) | 06 | 4 |
| 08 | [C5 land T-561: judge-pair e2e tests and product fixes (RR-3, FM-9, FM-33)](08-c5-land-t561.md) | 07 | 5 |
| 09 | [C6 land T-603: exec-mode e2e tests](09-c6-land-t603.md) | 08 | 6 |
| 10 | [C7 land T-895: restart on fallback after a usage limit through push dispatch](10-c7-land-t895.md) | 09 | 7 |
| 11 | [C8 land T-897: worker question in a push attempt](11-c8-land-t897.md) | 10 | 8 |
| 12 | [C9 land T-562: questions, replans, and needs-you e2e tests plus the owner-note test from T-897](12-c9-land-t562.md) | 11, 04 | 9 |
| 13 | [C10 land T-893: stop and stall of a push attempt](13-c10-land-t893.md) | 12 | 10 |
| 14 | [C11 T-900: complete code-sweep review tasks of a push attempt and make taskledger.txt empty](14-c11-t900-review-done.md) | 13 | 11 |
| 15 | [C12 T-882: resume under push dispatch and migrate 4 retired horch ledger run new tests to ledger_seed](15-c12-t882-observability.md) | 14 | 12 |
| 16 | [D1 Antigravity Stop hook: add the Stop hook in antigravity.rs and its test in harness_defaults.rs](16-d1-antigravity-stop-hook.md) | 01 | 2 |
| 17 | [D2 SR-3 external-message boundary: test and enforce external messaging denial from a verifier and from a worker](17-d2-sr3-external-message.md) | 01 | 2 |
| 19 | [E1 merge origin/main into fleet/fleetd-build and pass the full workspace check](19-e1-merge-main.md) | 15, 05, 16, 17, 02 | 13 |
| 20 | [E2 Phase 4 gate: verify all Phase 4 gate rows and write docs/specs/fleetd/gates/fleetd-p4.md](20-e2-gate-p4.md) | 19 | 14 |
| 21 | [E3 Phase 5 gate: verify all Phase 5 gate rows and write docs/specs/fleetd/gates/fleetd-p5.md](21-e3-gate-p5.md) | 19 | 14 |
| 22 | [E4 Phase 6 gate: verify all Phase 6 gate rows and write docs/specs/fleetd/gates/fleetd-p6.md](22-e4-gate-p6.md) | 19 | 14 |

## Waves

- Wave 1: 01
- Wave 1b (after 01): 02, 16, 17
- Wave 2: 03, 04
- Wave 3: 05, 06
- Wave 4: 07
- Wave 5: 08
- Wave 6: 09
- Wave 7: 10
- Wave 8: 11
- Wave 9: 12
- Wave 10: 13
- Wave 11: 14
- Wave 12: 15
- Wave 13: 19
- Wave 14: 20, 21, 22

## Critical path

01 -> 04 -> 06 -> 07 -> 08 -> 09 -> 10 -> 11 -> 12 -> 13 -> 14 -> 15 -> 19 -> 20

## Open questions

1. D1 and D3. Decision: ticket 18 (D3) is removed. The only part of Phase 4 ticket 14 that is left is the Antigravity Stop hook, which ticket 16 (D1) owns. (orchestrator)
2. Wave 1 checks and `scripts/dev-sandbox.sh`. Decision: ticket 01 blocks tickets 02, 16 and 17. (orchestrator)
3. C2 overlap with C8 and C12. Decision: C2 stays blocked by C1 only, because the security fix lands early. C2 keeps its changes out of `task_files.rs` and `ingest.rs` where possible; the ledger serializes tickets whose Own paths overlap. (orchestrator)
4. Design text questions Q1 to Q4 of `plan.md`. Open: the operator decides. The code stays as it is, and each gate file records the open question.
