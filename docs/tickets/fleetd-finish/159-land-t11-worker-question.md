# Land the work of task T-11 (unit C8, T-897: worker question in a push attempt) with a Check that has no suite that is red on the base

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test fleetd_handlers_cli --test fleetd_pass_p2 --test idea_release --test outbox_ingest --test outbox_writer --test question_answers --test question_flow --test result_ingest && scripts/dev-sandbox.sh cargo test -p horch --locked --test p5_cli_writers --test p6_fleet_start --test p4_question_file && scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p6_worker_question --test p6_sub_orchestrators && cargo fmt --all --check`
**Own:**, `crates/horch/tests/p4_question_file.rs`, `crates/horch-e2e/tests/p6_sub_orchestrators.rs`, `crates/horch-e2e/expected-failures/p6_sub_orchestrators.txt`

## Goal

Task T-11 (ticket `11-c8-land-t897.md`) merged the branch of T-897 and fixed 2 test files. Its Check held `fleet18_messaging` and `p2_leases`, which are red on the base for other causes (tasks T-86 and T-89 own them and come after this unit), so its finish could not pass. The local branch `keep/fleetd-finish-T11` (tip d7daacc4) holds the work: the `--no-ff` merge 336acc0d with the 3 commits of T-897, 3b4ce3aa (`p4_question_file.rs`) and d7daacc4 (`p5_cli_writers.rs`). This task lands it.

## Steps

1. In your worktree: `git merge --ff-only keep/fleetd-finish-T11`. If the run branch moved and that fails: `git reset --hard keep/fleetd-finish-T11`, then `git rebase --rebase-merges fleet/fleetd-finish`. The merge commit and the 3 commits of T-897 must stay (`git log --merges`). Keep each commit message.
2. 1 thing is left of ticket 11: `crates/horch-e2e/expected-failures/p6_sub_orchestrators.txt` has the entry `fm16_no_orchestrator_and_questions_stay_in_their_feature` ("needs the questions row of T-897"). Remove the entry and its comment lines (keep the first line of the file). Run `scripts/dev-sandbox.sh cargo test -p horch-e2e --test p6_sub_orchestrators --locked` 3 times. If the FM-16 test is red, find the cause: the test must pass by the questions row that the ingest of T-897 writes. Do not weaken an assert; if the cause is outside your Own, send `QUESTION:`.
3. Do not fix `fleet18_messaging` or `p2_leases`.
4. Run `scripts/dev-sandbox.sh cargo test --workspace --locked --no-run` 1 time.
5. Landing: when the Check passes, do NOT run `horch ledger finish`. Send `NOTE: READY-TO-LAND` with your task number to the orchestrator and wait for GO. Then `git fetch`, `git rebase --rebase-merges` on `fleet/fleetd-finish`, and run `horch ledger finish` at once.

## Out of scope

New product changes other than a cause that step 2 finds.

## Done when

The work of T-11 is on the run branch with its merge commit, the FM-16 entry is removed and the test passes 3 of 3, and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

