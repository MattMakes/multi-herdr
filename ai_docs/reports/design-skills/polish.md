# D17 polish: report

Unit: `polish`. Branch: `ds/polish`. Author: opus-37.

## Summary

- 6 gaps, 6 commits. Each commit has its test and passed the full gate.
- Gaps 1 to 5 each have a test that fails on the old code. I ran each test against the old code to check this. Gap 6 is documentation only. The gate's skill checks cover it.
- No oracle or golden changed. No event or receipt format changed.
- 1 file outside the plan: `crates/horch-core/src/routing/eligible.rs` (gap 1). The orchestrator approved it.

## Gap 1: planner determinism and unpriced picks

- Determinism: `plan_round` already gives the same plan for the same input. `cmp_06_planner_deterministic` checks this. D13 saw a different third pick because its test mints the round id with `RoundId::mint(now)`. That id has random bits, and the exploration slot is seeded by `sha256(round_id)`. So the input was not the same between runs. This is the design: a different round id moves only the exploration slot.
- Unpriced picks, cause: the planner did not look at prices. An eligible teammate whose model is not in `builtin_prices` could be planned, and the budget then counted it as 0.
- Which pick D13 saw: in the repo roster, the only unpriced model is `gemini-3-1-pro` (teammate `antigravity`). That teammate is already excluded as `trains_on_input`. The pick that D13 saw "with no price" was most likely `pi` on `ollama/qwen3.8`. That model is free, so `price_for` gives $0 and `UsageMeter::projected` gives 0. A $0 price is a real price, so I did not change it.
- Fix (`planner.rs`, `eligible.rs`): new `ExclusionReason::Unpriced` (`"unpriced"`). After `roster_eligibility`, the planner marks every eligible entry whose model has no price as `Excluded(Unpriced)`. The `round.created` eligible set records this reason. An unpriced baseline becomes `Baseline::Unusable` with the reason `model <m> has no price`. "Priced" means that `estimate_cost` (gap 2) returns `Some`, so the planner and both budget callers use the same rule.
- Decision: exclude, not price. The orchestrator said: do not add an unverified price.
- Test: `cmp_06_plan_is_deterministic_and_priced` (`tests/competition_planner.rs`). It adds a teammate `codex-nova` on the unpriced `gpt-9-nova` to the fixture roster. It runs 200 round ids and checks:
  - each plan is the same when it runs again;
  - every slot has a price;
  - `codex-nova` is `unpriced` in the eligible set and in the JSON payload;
  - as a baseline it is not planned, and the plan gives the reason.
  On the old planner, the entry is `Eligible`, so the test fails.

## Gap 2: one cost model

- Cause: `competition/preflight.rs` `cost_of` priced input, cache read and output only. `budget.rs` `nano_cost` (behind `UsageMeter::projected`) also requires the derived cache-write rates to be a whole number of n$ per token. The 2 functions agree for today's table, but they could drift. Note: the plan names `crates/horch/src/dataset/preflight.rs`, but `cost_of` is in `crates/horch-core/src/competition/preflight.rs`. The orchestrator approved that file.
- Fix: new `budget::estimate_cost(prices, model, TokenEstimate) -> Option<NanoUsd>` is the single source. `UsageMeter::projected` calls it. Preflight's `project_cost` calls it too, with the price table as a parameter. `cost_of` is deleted.
- Test: `pre_09_projection_matches_the_live_budget` (unit test in `preflight.rs`). It covers every table model, `gemini-3-1-pro` (no price), and an `edge` price of $0.001/MTok. For the `edge` price, the 5-minute write rate is 1.25 n$, which is not a whole number. Preflight's per-candidate cost must equal `UsageMeter::projected`. On the old `cost_of`, `edge` gives 3260 µ$ in preflight and 0 µ$ in the meter, so the test fails.

## Gap 3: receipt publish label

- Cause: `PublishMode::MergeFfOnly` gave the receipt value `merge_ff_only`. Since D13, that mode is a CAS `update-ref` followed by `read-tree -m -u`.
- Fix (`promotion.rs`): the variant is now `PublishMode::CasReadTree`, and its value is `update_ref_cas_read_tree`. `settle_checkout` (the resume path) accepts the new value and the legacy `merge_ff_only`. So a `promotion.started` from an older binary still gets its checkout files moved.
- Decision: `publish` is a `String` in `PromotionReceipt` and in `PromotionStarted` (`measure/event.rs`). Old receipts and events parse with no change, so no serde enum or serde default is needed. Changing the field type would change the event schema in `event.rs`, which is outside my scope. The "new enum value" is the new `PublishMode` variant and its new string.
- Tests:
  - `pro_03_ff` and `pro_03_checked_out_publish_is_cas` now expect `update_ref_cas_read_tree`. They are the gap's own tests.
  - New `pro_03_checked_out_publish_label` checks that the new label is in `promotion.started`. Then it does a crash between the swap and the tree update, and resumes with a legacy `merge_ff_only` event. The files move, the status is clean, and the receipt on disk parses with the old label.
  - The old code writes `merge_ff_only`, so the test fails.

## Gap 4: gone pane or unreachable herdr in `horch done`

- Cause: `done` treated every failure of `pane get` (and of the final close check) as "the pane is gone" and exited 0. A herdr server that does not answer gave the same result, but the pane may still be open.
- Fix:
  - `WorkspaceClient::server_reachable()` is new. `Herdr` uses its existing `server_reachable` (`herdr workspace list`). `FakeWorkspace` gets `set_reachable(bool)`. While the fake is unreachable, every call fails, and `server_reachable` is false.
  - `done` runs the probe only after a pane call fails, so the success path makes no extra herdr call. On an unreachable server, `done` writes 1 line to stderr. It still runs `record_session`, `unregister` and `settle` when it knows the workspace. Then it returns an error: "herdr is unreachable: the summary is recorded, but this pane may still be open". The CLI exits non-zero.
  - The same rule applies when the final close fails.
  - A gone pane on a reachable server is still exit 0.
- D12 had a concern: a closed dataset workspace could make the probe fail. The probe is `workspace list`, a server call that does not depend on any one workspace, so this does not occur.
- Tests:
  - `done_fails_when_herdr_is_unreachable_but_not_when_the_pane_is_gone` (`execution/lifecycle.rs`). It has 8 cases: herdr down or pane gone, at `pane get` or at the close, with or without a request workspace. In each case it checks the result, the steps that ran, and that the pane is kept or closed.
  - `an_unreachable_server_fails_every_call_until_it_answers` (`workspace/testing.rs`).
  - The old `done` returns `Ok` when herdr is down, so the test fails.

## Gap 5: fsx breaker residual

- Cause: a stale `<name>.lock.break/` was removed by name, with no exclusion. Here is the bad sequence:
  1. Processes A and B both see the stale breaker.
  2. A removes it.
  3. Process C makes a fresh breaker.
  4. B removes C's breaker by name.
  5. Process D makes another breaker.
  6. C and D now break the lock at the same time.
- Why not the rename idea from the plan: a rename that B does at step 4 moves C's fresh breaker in the same way. The check and the rename are not 1 atomic step.
- Fix (`fsx.rs`): a new `BreakerGuard` takes a non-blocking exclusive `flock` on the lock's parent directory. `break_lock` takes the guard first. A process that cannot take it returns "not broken", backs off and retries. The kernel releases a `flock` when its process dies, so the guard is never stale, and a killed breaker cannot block anyone. Under the guard, the old steps run unchanged:
  1. Take `.break/`, or remove it when it is older than 5 s.
  2. Check the lock again.
  3. Rename the lock away.
  So at most 1 process at a time removes a left breaker, and at most 1 process breaks the lock.
- Compatibility: the `.break/` directory stays, so binaries from before this change are still excluded by it. When `flock` is not available (not unix, or a filesystem that returns an error other than `EWOULDBLOCK`), the guard holds nothing, and the old behavior applies. `libc` is already a unix dependency, so there is no new crate. I checked that macOS gives `EWOULDBLOCK` (35) for a second `flock` on a directory.
- Tests:
  - `dirlock_concurrent_breakers_keep_one_holder` is extended: every odd round of 10 starts with a stale breaker directory. It passed on the old code too, because the race needs an exact interleaving of 3 processes.
  - New `dirlock_paused_breaker_blocks_other_breakers` is a deterministic test. A breaker that holds the guard has a breaker directory that looks stale, because it was paused for more than 5 s. Other calls must not remove that directory or break the lock. After the guard is released, the left directory is removed, and then the lock is broken. With the guard call removed from `break_lock`, the second call broke the lock while the first breaker was still in its window, and the test failed.

## Gap 6: GSAP `easeReverse`

- Cause: GSAP 3.15.0 added `easeReverse` and deprecated `yoyoEase`. The D14 fact-check (`gsap-factcheck.md:248`) found that the skill did not mention it.
- Fix (`skills/motion-gsap/references/core.md`):
  - A row in "Common vars": what it does, the default `false`, the values `true` or an ease name, and that it replaces `yoyoEase`.
  - A short paragraph and an example in "Eases": a hover or toggle that calls `reverse()` sets `easeReverse: true`.
  - The skill never mentioned `yoyoEase`, so I removed nothing.
- Sources:
  - https://gsap.com/docs/v3/GSAP/Tween/vars/ ("Controls the ease used when the tween's playhead reverses direction", default `false`, added in 3.15.0; `yoyoEase` deprecated in 3.15.0).
  - https://gsap.com/blog/3-15/ (the ease runs on the remaining distance from the point where the playhead changed direction, and works in reversed timelines).
- Size: `SKILL.md` is unchanged at 10,206 bytes (budget 12 KB). The skill text is 66,809 bytes (budget 160 KB).
- Test: none of its own. This is a documentation change, and the gate's skill and catalog checks pass.

## Gotchas

- I saw 2 load flakes in the gate. The machine load average was about 40 because other fleet workers were running gates. Each flake passed on reruns, and the next full gate was green:
  - `tel_session_recorded_when_agent_ends_fast`: fake-herdr `workspace create` was killed by SIGKILL before horch ran. It passed 5 of 5 reruns.
  - `cmp_05_e2e_candidates_in_dataset_workspace`: 1 of 2 `candidate.completed`. It passed 4 of 4 reruns.
- `FakeWorkspace` now records a `server_reachable` call, but only after a pane call fails. Tests that compare the full call list on a failure path see 1 more entry. No current test did this.

## Outside my scope (not fixed)

- `measure/event.rs:372`: the doc comment on `PromotionStarted::publish` still says "`update_ref_cas` or `merge_ff_only`". It should name `update_ref_cas_read_tree` and mark `merge_ff_only` as legacy.
- `tests/coordinator.rs` mints random round ids, so its plans change between runs. The tests that need a stable plan could use a fixed `RoundId::new(...)`.
- Clippy reports `clone_on_copy` in `competition/coordinator.rs:975`, `:1272` and `competition/judging.rs:246`. These warnings existed before this unit. The gate does not fail on them.
