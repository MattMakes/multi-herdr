# 19: E1 merge origin/main into fleet/fleetd-build and pass the full workspace check

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check`
**Own:** `Cargo.toml`, `Cargo.lock`, `crates/`, `docs/`, `scripts/`, `teammates/`
**Do not touch:** `CHANGELOG.md`

Spec: `ai_docs/plans/fleetd-finish/plan.md` (section 1 item L15; section 3 unit E1).

## Goal

Merge `origin/main` into `fleet/fleetd-build` after waves A, C, and D have landed, resolve all conflicts, and pass the full workspace clippy, test, and format check inside `scripts/dev-sandbox.sh`.

## Requirements

- **L15**: Merge `origin/main` into the worktree branch with `git merge --no-ff origin/main`, keeping every commit from both `fleet/fleetd-build` and `origin/main`.
- The full check `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check` passes.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
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
1. Copy the folder `docs/tickets/fleetd-finish/` to `docs/tickets/fleetd-finish/` in your worktree: `mkdir -p docs/tickets/fleetd-finish && cp docs/tickets/fleetd-finish/*.md docs/tickets/fleetd-finish/`.
2. Commit it: `git add docs/tickets/fleetd-finish && git commit -m "fleetd-finish: the tickets and the map of the run"`.
   - Check: `git show --stat HEAD` lists only files under `docs/tickets/fleetd-finish/`.


## Out of scope

- Writing the Phase 4, 5, 6, or 7 gate files (units E2, E3, E4, F6).
- Pushing to `origin` or opening a pull request (unit E5).

## Done when

`origin/main` is merged into the run branch with `--no-ff`, and `cargo clippy --workspace --all-targets --locked && scripts/dev-sandbox.sh cargo test --workspace --locked && cargo fmt --all --check` passes.

## Added 2026-11-21: use the proven trial merge

Tasks T-312 and T-314 made and proved the merge: read `docs/specs/fleetd/gates/merge-main-plan.md` and `docs/specs/fleetd/gates/merge-main-proof.md` first. Do not merge `origin/main` by hand. In your worktree: (1) `git merge-tree --write-tree HEAD keep/fleetd-finish-trial-merge-main` must exit 0; (2) `git merge --no-ff keep/fleetd-finish-trial-merge-main`; (3) `git merge-base --is-ancestor origin/main HEAD` must exit 0; if `origin/main` moved after the trial (`git fetch origin main`), merge the new commits too and list them. If step 1 fails, follow "How E1 uses the trial" of the plan. This task runs alone: the land step is a plain rebase and must find nothing to do. If `fleet/fleetd-finish` moved before your land, stop and send `QUESTION:`; never let a rebase flatten the merge. After the land: `git merge-base --is-ancestor origin/main fleet/fleetd-finish` exits 0.
