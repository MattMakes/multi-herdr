# E1-prep-4: bring the fix wave of the run branch into the trial merge of main, and prove it

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/merge-main-proof-3.md && grep -q '^## Result of each suite' docs/specs/fleetd/gates/merge-main-proof-3.md && grep -q '^## Tip of the trial branch' docs/specs/fleetd/gates/merge-main-proof-3.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/merge-main-proof-3.md`

## Goal

The local branch `keep/fleetd-finish-trial-merge-main` (tip 3fd534f7, task T-330; proof: `docs/specs/fleetd/gates/merge-main-proof-2.md` on the run branch; method: ticket `docs/tickets/fleetd-finish/215-*.md`, read both in full) holds the merge of `origin/main` 5863c698 (Release v0.5.2) with the run branch at 4e806d0b. Since then about 80 commits landed on `fleet/fleetd-finish`. `git merge-tree --write-tree fleet/fleetd-finish keep/fleetd-finish-trial-merge-main` now exits 1 with 2 conflicts: `crates/horch-core/tests/context_prime.rs` and `crates/horch-e2e/src/harness.rs`. The final merge task (E1) uses this trial, so it must merge with no conflict and be proven.

## Steps

1. `git fetch origin main`. If `origin/main` is not 5863c698, list the new commits, say so with `NOTE:` and bring them in too.
2. In your worktree make a branch `trial-proof-3` at `keep/fleetd-finish-trial-merge-main`. Merge the tip of `fleet/fleetd-finish` into it with a merge commit (no rebase, no squash: the history of the trial stays). Resolve each conflict in favor of both changes; read each side first (the harness has changes of tasks T-342, T-363, T-365, T-369 on the run side and of PR #363 on the main side). Write each conflict and its resolution in the report.
3. Semantic conflicts with no text conflict: main has a second pane host (`hc`) and schema steps 8 and 9; the run branch got new code since the trial. Check and fix on the trial, each as its own commit with a reason: the schema version numbers (the trial has fleetd steps 10 to 18 and `VERSION` 18: a test that pins a number, a new step of the fix wave if any exists: `git grep -n '_VERSION' -- crates/horch-core/src/taskledger/schema.rs`); each new place that talks to the pane host (the trial refuses `hc` for fleetd: `refuse_hc`); `arch_scan` (the trial skips the crate `ui-prop-tester` with a guard test); new `horch doctor` lines against the doctor of main; `cargo check --workspace --all-targets --locked` must be clean.
4. Prove, each through `scripts/dev-sandbox.sh`, with `--test-threads=4` when the 1-minute load is above 32: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo test -p horch-core --locked --no-fail-fast`; `cargo test -p horch --locked --no-fail-fast`; the e2e files that the conflicts and your fixes touch, and `fakes`, `hc_courier`, `hc_doctor`, `hc_fleet_start`, `pane_host_contract`, `taskledger`, `p0_liveness`, `fleet_start`. For each red test: is it red on the tip of the run branch without the merge too (task T-378 writes `docs/specs/fleetd/gates/full-gate-macos-early.md` now: read it if it landed)? A test that is red only in the trial is yours to fix on the trial. 1 `horch ledger progress` line each 10 minutes.
5. Move the keep branch: `git branch -f keep/fleetd-finish-trial-merge-main <tip of trial-proof-3>` in the main repository (a local branch of this run; never push it). Verify: `git merge-base --is-ancestor origin/main keep/fleetd-finish-trial-merge-main`, the same for the tip of `fleet/fleetd-finish` that you merged, and `git merge-tree --write-tree fleet/fleetd-finish keep/fleetd-finish-trial-merge-main` exits 0.
6. Write `docs/specs/fleetd/gates/merge-main-proof-3.md` in the form of proof 2: `## Tip of the trial branch` (commits and parents), `## Conflicts and resolutions`, `## Fixes on the trial`, `## Result of each suite` (first and final, passed and failed, load), `## Red before the merge`, `## For the operator`, `## Not verified`. Label each claim verified or not verified. The run branch moves on while you work: say which tip you merged; the last small refresh is the job of E1.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Never switch the branch of the main checkout. Never push `main`, never push the keep branch, never merge a pull request, never change a pull request.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`. Never read the real ledger. Never install the build. Never start the real Herdr desktop app or a real `hc`.
3. No flaky test, no `#[ignore]`, no weaker assert. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Never touch a process of another fleet.
4. Before DONE: no process of the unit is alive, `launchctl list | grep horch` shows nothing of yours, the sandbox folder and your `target/` are removed.

## Done when

The Check passes, the keep branch is moved and verified, and each red test has a row with its cause.
