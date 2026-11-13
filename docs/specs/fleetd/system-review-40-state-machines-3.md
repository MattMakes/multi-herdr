# fleetd systems review 40, round 3: the state machines

Scope: `docs/specs/fleetd/design.md` at `HEAD` (commit `5327458b`, 1,748 lines), and the change since `c8bf35ac`. Lens: the state machines. Line numbers are lines of `design.md` at `HEAD`. The second report is `docs/specs/fleetd/system-review-40-state-machines-2.md`. The amendments are A33 and A34 in `docs/specs/fleetd/amendments.md`.

## Verdict

PASS
No HIGH finding is open.

## Answers

- S40-8 (HIGH) closed: 1092, 835, 1099, 1181, 1272. The `open` row now has the exit "git shows the changes of the feature tip in the primary branch by content → `merged`", and each idea in flight moves to a new feature (1092). Rule 1 names feature `open` → `merged` as an allowed transition (835), so `transition()` accepts it. The rule "A merge with ideas in flight" agrees (1099), and `integrate_features` moves the ideas in the same pass (1272). So I-2 gets a new feature, and no branch lands on the branch of a merged pull request.
- T40-1 (HIGH) closed: 1114, 1115, 1118, 1236, 1336, 1116, 836, 838, 689, 1233, 1274. A failed resolver or fix attempt does not move its `done` task: rung 6 adds 1 to `repair_attempts` of the branch (1236, 1118, 689), and fleetd starts the next attempt after the backoff (1114, 1115). At `max_attempts`, 1 needs-you item names 2 exits: `horch task requeue`, which `done` now accepts (1336), and `horch branch abandon` → `abandoned` (1116), which the idea roll-up skips (1274). The attempt has the lease of its task (836, I1 at 838) and the stall ladder: a hung resolver reaches `hung` at `stall_after`, and rung 3 stops it (1233) before its lease expires, 120 s later (1217). Note: rule 2 (836) still says that a lease expiry moves "the task" to `stalled`, and `done` has no such exit (1336). The scenario does not reach that sentence, because rung 3 ends a hung attempt first.
- T40-2 (HIGH) closed: 1128, 1137, 846, 1112, 1176. The `passed` row has the exit "the compare-and-set fails because the feature tip is not the base tip → `dropped`, and the members go back to `ready`" (1128). I9 and "A moved tip drops a bundle" state the same rule (846, 1137). The member row `in_bundle` leaves to `ready` on a `dropped` bundle (1112). fleetd does not queue a rebase of the feature branch while a bundle is in `gating` or `passed` (1137, 1176), so the second cause of the scenario does not occur.
- T40-3 (HIGH) closed: 1328-1338, 1341, 973, 1095, 1334. Each task state that is not terminal has the exit "A cancel → `cancelled`" (1328-1338). A replan, `horch task cancel`, and the cancel or abandon of the idea or feature are each a cancel, and fleetd stops a live attempt before the move (1341). `horch idea cancel` (973) and `horch feature abandon` (1095) send each task that is not terminal through A cancel. A cancel from `blocked_on_user` closes its question and its needs-you item (1334). The cancel of a judged task cancels its judge, and the cancel of a judge alone is refused (1341), so no judged task waits for a judge that never runs.
- T40-4 (HIGH) closed: 751, 753, 1295, 1297-1309, 1101, 1183. The uniqueness rule is kind, subject, input key and run (751). An effect job never uses an old result, and each retry is a new run (1295). `push` runs again after `push_not_before` when the last run failed or ended `abandoned` (1307, 1101). `fetch` has the input key "the repository and its remote" and runs at each period, also after a failed run (1306, 1183). A pure job that ended `abandoned` has no verdict and runs again, so a gate stopped at its time limit runs again (1295, 1300, 1301), with a bound of 3 runs and the exit `horch job requeue` (1295).

## New CRITICAL

None.
