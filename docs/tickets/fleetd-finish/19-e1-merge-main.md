# 19: E1 merge origin/main into fleet/fleetd-build and pass the full workspace check

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check`
**Own:** `Cargo.toml`, `Cargo.lock`, `crates/`, `docs/`, `scripts/`, `teammates/`
**Do not touch:** `CHANGELOG.md`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 1 item L15; section 3 unit E1).

## Goal

Merge `origin/main` into `fleet/fleetd-build` after waves A, C, and D have landed, resolve all conflicts, and pass the full workspace clippy, test, and format check inside `scripts/dev-sandbox.sh`.

## Requirements

- **L15**: Merge `origin/main` into the worktree branch with `git merge --no-ff origin/main`, keeping every commit from both `fleet/fleetd-build` and `origin/main`.
- The full check `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check` passes.

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

## Steps

1. Fetch the latest `origin/main` and inspect the commits on `origin/main` not yet in `HEAD`:
   `git fetch origin main && git log --oneline HEAD..origin/main`
   - Check: `origin/main` is fetched.
2. Merge `origin/main` with `--no-ff`:
   `git merge --no-ff origin/main`
   Resolve any merge conflicts while preserving both `origin/main` fixes and the `fleet/fleetd-build` architecture and guard lists (`nfr`, `arch_scan`, `architecture_doc`).
   - Check: `git status` shows a clean working tree and `git merge-base --is-ancestor origin/main HEAD` exits 0.
3. Build all workspace binaries and run clippy:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && cargo clippy --workspace --all-targets --locked`
   - Check: clippy exits 0 with no errors.
4. Run the full workspace test suite through `scripts/dev-sandbox.sh`:
   `scripts/dev-sandbox.sh cargo test --workspace --locked`
   - Check: every workspace unit, integration, guard, and e2e test binary passes.
5. Run `cargo fmt --all --check`.

## Extra step: commit the ticket files of this run

Do this step before the merge of `origin/main`.
1. Copy the folder `/Users/matthewscott/repo/multi-herdr/docs/tickets/fleetd-finish/` to `docs/tickets/fleetd-finish/` in your worktree: `mkdir -p docs/tickets/fleetd-finish && cp /Users/matthewscott/repo/multi-herdr/docs/tickets/fleetd-finish/*.md docs/tickets/fleetd-finish/`.
2. Commit it: `git add docs/tickets/fleetd-finish && git commit -m "fleetd-finish: the tickets and the map of the run"`.
   - Check: `git show --stat HEAD` lists only files under `docs/tickets/fleetd-finish/`.


## Out of scope

- Writing the Phase 4, 5, 6, or 7 gate files (units E2, E3, E4, F6).
- Pushing to `origin` or opening a pull request (unit E5).

## Done when

`origin/main` is merged into the run branch with `--no-ff`, and `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check` passes.

## Added 2026-11-21: use the proven trial merge

Tasks T-312 and T-314 made and proved the merge: read `docs/specs/fleetd/gates/merge-main-plan.md` and `docs/specs/fleetd/gates/merge-main-proof.md` first. Do not merge `origin/main` by hand. In your worktree: (1) `git merge-tree --write-tree HEAD keep/fleetd-finish-trial-merge-main` must exit 0; (2) `git merge --no-ff keep/fleetd-finish-trial-merge-main`; (3) `git merge-base --is-ancestor origin/main HEAD` must exit 0; if `origin/main` moved after the trial (`git fetch origin main`), merge the new commits too and list them. If step 1 fails, follow "How E1 uses the trial" of the plan. This task runs alone: the land step is a plain rebase and must find nothing to do. If `fleet/fleetd-finish` moved before your land, stop and send `QUESTION:`; never let a rebase flatten the merge. After the land: `git merge-base --is-ancestor origin/main fleet/fleetd-finish` exits 0.

## Added 2026-11-21 19:55Z: `origin/main` moved to 1f03526b (the comment ban) - READ THIS FIRST, it replaces the method above

`origin/main` got commit 1f03526b (#464): "Ban comments in code". It deletes 23,704 comment lines in 835 files, adds the tool `scripts/comments/comment_lint.py` (`check`, `list`, `strip`, `verify`, `estimate`), the file `scripts/comments/baseline.json` (only `scripts/live/context.sh: 15`), ADR `docs/adr/0004-no-comments-in-code.md`, the skill `.claude/skills/delete-comment/SKILL.md`, a gate step `comment_ban` and a CI job `comments`. A file that the baseline does not name may hold 0 counted comments. Read the ADR, the skill and the tool help first.

Measured by the orchestrator: `git merge-tree origin/fleet/fleetd-finish origin/main` gives 160 conflicts; the trial branch `keep/fleetd-finish-trial-merge-main` (e3574c64, holds main to v0.5.2 5863c698) against the run tip gives 1 conflict (`crates/horch-core/src/execution/service.rs`: the role fix of T-400 against the `hc` change of main).

Method (in your worktree, on the run tip):

1. Merge the trial first: `git merge --no-ff keep/fleetd-finish-trial-merge-main`; resolve the 1 conflict in `service.rs` (keep both: the role retry of T-400 and the change of main); `cargo check --workspace --all-targets --locked`; commit.
2. Bring only the tool to your tree without the merge: `git checkout origin/main -- scripts/comments` (and the files it needs, see its imports and `skills/copied.json`). Save the comments before they go: `python3 scripts/comments/comment_lint.py list > docs/specs/fleetd/gates/comments-before-the-ban.jsonl` (the operator can take decisions and invariants from it into ADRs later; the skill asks for that; do not write the ADRs in this task).
3. `python3 scripts/comments/comment_lint.py strip <each tracked code file that holds counted comments>`; then `python3 scripts/comments/comment_lint.py verify` must say that only comments changed. `cargo fmt --all`; `cargo check --workspace --all-targets --locked`. Commit as its own commit ("strip the comments before the merge of 1f03526b; verify proves only comments changed").
4. Now `git merge --no-ff origin/main`. Most conflicts are gone, because both sides hold the same code without comments. For each conflict that is left: take the code of the run branch, and port each change of main that is not a comment (read `git show 1f03526b -- <file>`; 8526 inserted lines are not all comments). List each conflict file with its resolution in the gate file `docs/specs/fleetd/gates/merge-main-proof-4.md`.
5. `git merge-base --is-ancestor origin/main HEAD` exits 0. `python3 scripts/comments/comment_lint.py check` exits 0 (strip what the merge brought back; never raise the baseline). A `SAFETY:` note and each comment that a program reads stays: the tool knows them.
6. Tests that read comments: `arch_scan` and other source scans can name a comment text; a doc test can go with its doc comment. Fix each such test by its intent, and list them. A test must not get weaker.
7. Then the Check of this ticket. The host is loaded: run the workspace test 1 time, not in parallel with another test run; write 1 `horch ledger progress` line each 5 minutes; `--check-timeout` is long. If only tests fail that are flaky by host load, send the list with the failure text; do not rerun in a loop.
8. After the comment sweep step of horch: run finish again at once; restore no comment (none is allowed now).

Do not push `main`, do not touch `CHANGELOG.md`, do not change `scripts/comments/baseline.json` except to lower it.
