# E1-prep-2: prove the trial merge of main: run the suites that the plan marks "not verified", and fix the trial

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/merge-main-proof.md && grep -q '^## Result of each suite' docs/specs/fleetd/gates/merge-main-proof.md && grep -q '^## Tip of the trial branch' docs/specs/fleetd/gates/merge-main-proof.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/merge-main-proof.md`

## Goal

Task T-312 made the trial merge of `origin/main` into the run branch: the local branch `keep/fleetd-finish-trial-merge-main` (commit 52bca240), with the plan `docs/specs/fleetd/gates/merge-main-plan.md`. The plan marks rows as "not verified": S4, S5, S6, S7, S11, conflict rows 7, 9 and 10, clippy and the test suites. The end task E1 (T-221) is exclusive, so each fault that we find now saves time of the whole fleet. Prove the trial, fix what is red because of the merge, and keep the fixes on the trial branch. This task lands only the proof file.

## Steps

1. Read the plan file. In your worktree: `git switch -c trial-proof keep/fleetd-finish-trial-merge-main`, then `git merge --no-ff fleet/fleetd-finish` (the run branch moved since the trial; the plan says this merge has no conflict; if it has one, resolve it by the rules of the plan).
2. Build: `scripts/dev-sandbox.sh cargo check --workspace --all-targets --locked`, then `scripts/dev-sandbox.sh cargo clippy --workspace --all-targets --locked` and `cargo fmt --all --check`.
3. Run these suites, 1 at a time, at most 4 test threads when the 1-minute load is above 32: (a) `cargo test -p horch-core --lib --locked`; (b) the count and oracle tests of S6 and rows 7, 9, 10: `skills_catalog`, `golden_prompts`, `baseline_oracles`, `godot_wave`, `tests/harness.rs`, `roster` (find each target with `cargo test -p <crate> --test <name>`); (c) `cms_comment_sweep` and each caller of `finish/comments.rs` (S7: search with `git grep`); (d) the tests of `routing::quota` and of `fleet_runtime/application/holds.rs` (S5); (e) `cargo test -p ui-prop-tester --locked` (S11); (f) `cargo test -p horch-core --locked --test taskledger_schema_p7 --test trf_48_ledger_guard --test taskledger_queue` and `cargo test -p horch-e2e --locked --test p7_release` (the schema renumber of row 8: each test that names a schema version); (g) `cargo test -p horch --locked` (the CLI crate).
4. For each red test: decide if the merge caused it (it is green on `fleet/fleetd-finish`: check with 1 run there only when you cannot tell from the diff). A test that is red on the run branch too is not yours: list it with the words "red before the merge". Fix each fault of the merge on the branch `trial-proof` with small commits; a count in a test changes only to the true new count (count the files; do not guess).
5. S4: read `harness/claude.rs` and say if a sandboxed fleetd worker (`HorchAccess::Report`) can commit in its worktree after the merge: name the settings lines; prove with 1 existing test or 1 new test.
6. Schema renumber: search the repository for each literal schema version (`user_version`, `PHASE_`, `VERSION`, "version 8" to "version 17" in tests, docs and `docs/specs/fleetd/design.md`). List each place that still has the old number. Fix code and tests. Do NOT edit `design.md`: list its lines for the operator.
7. Save: move the keep branch to your result: `git branch -f keep/fleetd-finish-trial-merge-main trial-proof` in the main repository (a local branch; never push it). Then `git switch` back to your task branch.
8. Write `docs/specs/fleetd/gates/merge-main-proof.md`: `## Tip of the trial branch` (commit id, and its 2 parents chain: it has `origin/main` and the run branch tip that you merged); `## Result of each suite` (1 row for each command of steps 2 and 3: passed and failed counts, the load); `## Fixes on the trial branch` (commit, file, why); `## Red before the merge`; `## For the operator` (the design lines of step 6, and what stays not verified). Label each claim verified or not verified.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write and never read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never push `main`, the trial branch or the keep branch. Never merge a pull request. Do not add a CHANGELOG line.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry.
6. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. Never run `cargo test -p horch --bin horch` with a filter that matches `watch_once` unless commit 38736b49 is in your tree (it is in the run branch now: check with `git merge-base --is-ancestor 38736b49 HEAD`).
7. Write 1 `horch ledger progress` line each 10 minutes.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes; each suite of step 3 has its row; each red test is fixed on the trial branch or listed as red before the merge.
