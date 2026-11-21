# E1-prep-3: origin/main moved (3 new commits, PR #363 and #364, Release v0.5.2): bring them into the trial merge and prove it

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/merge-main-proof-2.md && grep -q '^## Result of each suite' docs/specs/fleetd/gates/merge-main-proof-2.md && grep -q '^## Tip of the trial branch' docs/specs/fleetd/gates/merge-main-proof-2.md && grep -q '^## Changes of main that fleetd code must follow' docs/specs/fleetd/gates/merge-main-proof-2.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/merge-main-proof-2.md`

## Goal

Tasks T-312 and T-314 made and proved a trial merge of `origin/main` at `e5170752` (Release v0.5.1): the local branch `keep/fleetd-finish-trial-merge-main` (tip 1e782060), the plan `docs/specs/fleetd/gates/merge-main-plan.md` and the proof `docs/specs/fleetd/gates/merge-main-proof.md`. Since then `origin/main` got 3 commits: `c28e6dd6` (#364, headless Chrome code-sign clones on macOS), `50e63b2a` (#363, herdr-core `hc` and the Herdr desktop app as a second pane host, with the new fakes `fake-hc` and `fake-herdr-app` in `crates/horch-e2e`) and `5863c698` (Release v0.5.2). Bring them into the trial, resolve, prove, and keep the result on the keep branch. This task lands only the proof file. It does not land the merge.

PR #363 is the important one: it changes how horch talks to a pane host. fleetd has its own herdr adapter and launch path (`crates/horch-core/src/workspace/herdr.rs`, the launcher, `Herdr::with_timeout` of task T-281, the fake herdr of `crates/horch-e2e`). Read both sides before you resolve.

## Steps

1. Read the plan and the proof of the first trial. In your worktree: `git fetch origin main`; `git switch -c trial-proof-2 keep/fleetd-finish-trial-merge-main`; `git merge --no-ff fleet/fleetd-finish` (the run branch moved; the first proof says this has no conflict); then `git merge --no-ff --no-commit origin/main`. Record each conflicted path.
2. For each of the 3 commits of main: read its diff and its PR text (`gh pr view 363`, `gh pr view 364`). Write what it changes and why.
3. Resolve each conflict by reading both sides. A change of main to code that the run branch deleted or replaced needs a decision: (a) not needed in the new code, with the reason; (b) the same change must be made in the new code: name the file and the function, and make it. For #363: decide and write down if fleetd can run its workers on the second pane host (`hc`, the desktop app) after the merge, or only on herdr. Do not build new support in this task: if fleetd cannot use the new host, say exactly what is missing (which adapter calls, which launch step), make sure that the old claim path of main still compiles and its tests pass, and that fleetd refuses the new host with a clear message and does not fail in a silent way. Put the gap in `## For the operator`.
4. Silent conflicts: for each function, flag, file, fake, test helper, count and oracle that the 3 commits touch, search the run branch. Check: the fake binaries of `horch-e2e` (the harness finds its fakes by name), the roster and oracle counts, the schema version (does main add a migration? the trial has the fleetd steps at 9 to 17), `Cargo.toml` members and `Cargo.lock`, `.github/workflows/`, the architecture fitness tests `arch_scan` (new OS `cfg` sites and `Command::new` outside `horch-platform` in the new code of main: PL-2 and SR-7; the first trial skipped the crate `ui-prop-tester` by name with a guard test; do not add a skip for code that horch links: port it to `horch-platform` or send `QUESTION:`), and each CHANGELOG change (list them; add no line).
5. Build and prove, 1 command at a time through `scripts/dev-sandbox.sh`, `--test-threads=4` when the 1-minute load is above 32: `cargo check --workspace --all-targets --locked`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo fmt --all --check`; `cargo test -p horch-core --locked --no-fail-fast`; `cargo test -p horch --locked --no-fail-fast`; each new test file and each new fake of the 3 commits; in `horch-e2e`: the test files that the 3 commits add or change, plus `p4_release`, `p7_release`, `taskledger` and `fleet_start`.
6. For each red test: decide if the merge caused it. Fix each fault of the merge on `trial-proof-2` with small commits. A test that is red on `fleet/fleetd-finish` too is not yours: list it under "Red before the merge" (known now: `arch_scan` `fitness_no_context_touches_foreign_tables`, owner T-327).
7. Save: `git branch -f keep/fleetd-finish-trial-merge-main trial-proof-2` in the main repository (a local branch; never push it). Then `git switch` back to your task branch.
8. Write `docs/specs/fleetd/gates/merge-main-proof-2.md` with the sections: `## Tip of the trial branch` (the commit and the proof that `origin/main` at 5863c698 and the run branch tip that you merged are its ancestors); `## Commits of main`; `## Conflicts` (1 row each: the rule and the state); `## Changes of main that fleetd code must follow`; `## Result of each suite`; `## Fixes on the trial branch`; `## Red before the merge`; `## For the operator`. Label each claim verified or not verified.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`. A test of main that starts the real Herdr desktop app or the real `hc` is not run: use the fakes only, and list each test that you did not run.
3. Never push `main`, the trial branch or the keep branch. Never merge a pull request. Add no CHANGELOG line. Do not edit `docs/specs/fleetd/design.md`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry.
6. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet or of the operator's herdr.
7. Write 1 `horch ledger progress` line each 10 minutes.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes; `git merge-base --is-ancestor origin/main keep/fleetd-finish-trial-merge-main` exits 0; each suite of step 5 has its row.
