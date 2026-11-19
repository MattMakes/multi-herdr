# Bring the current main branch into the build branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** full
**Check:** `git merge-base --is-ancestor e51707520 HEAD 2>/dev/null || git merge-base --is-ancestor origin/main HEAD; cargo test --workspace --locked`
**Own:** `crates/`, `Cargo.toml`, `Cargo.lock`, `.github/workflows/`, `scripts/`, `docs/`, `teammates/`, `skills/`

## Goal

The branch of your task contains commit `e5170752` of `origin/main` (Release v0.5.1), each conflict
is resolved with both sides kept, and the check of the run passes.

## Context

- The run `fleetd-build` builds fleetd on the branch `fleet/fleetd-build`
  (`docs/specs/fleetd/design.md`). The branch started from an older `main`. Since then `main` got
  10 commits (v0.4.9 to v0.5.1): game design teammates and skills, the new workspace crate
  `crates/ui-prop-tester`, a sandbox change, a comment sweep, a root-cause auditor, and more.
  `git log --oneline $(git merge-base origin/main HEAD)..origin/main` lists them.
- The build branch has: Phase 0 (red tests in `crates/horch-e2e/tests/p0_*.rs` and the list of
  expected failures in `crates/horch-e2e/expected-failures/`), Phase P (the crate
  `crates/horch-platform`; each OS-specific site of the 4 old crates moved behind it; the
  `arch_scan` rule of PL-2 in `crates/horch-core/tests/arch_scan.rs`), and the first tickets of
  Phase 1 (`crates/horch-core/src/fleetd/`, and a new ledger schema version).
- Known conflict 1, the schema. `origin/main` has `pub const VERSION: i64 = 8` in
  `crates/horch-core/src/taskledger/schema.rs` with its own migration to v8. The build branch
  also made a version 8 (the tables `fleetd`, `attempts`, `jobs`, `pass_cursors`, `needs_you`
  and new `events` columns). The live ledgers of users are at the v8 of `main`. So: keep the v8
  of `main` exactly as it is, and make the fleetd migration version 9, on top of it. Correct each
  test, each backup name, each downgrade helper in a test and each comment that names the
  version. The numbers of the design text are then 2 lower than the code from Phase 2; correct
  the sentence in the code that states the offset.
- Known conflict 2, PL-2. `main` added code with OS `cfg` lines after Phase P moved the old ones.
  Run `cargo test -p horch-core --test arch_scan`. For each new OS `cfg` site in the 4 old crates
  (`horch-core`, `horch`, `herdr-install`, `horch-marketplace`): move it behind
  `crates/horch-platform`, by the rules in the module docs of that crate (no change of behavior).
  For the new crate `crates/ui-prop-tester`: it is a separate test tool with its own PTY code. Do
  not move its sites. Exempt the crate in the PL-2 rule by its path, with the reason in the rule
  ("a separate tool, added after decision D16; the operator decides if it moves"), and name it in
  your done summary.
- Known conflict 3, the dependency tests `nfr_05` and `nfr_06` in `crates/horch-core/tests/nfr.rs`
  and each other test that lists crates, teammates, skills or files: keep the entries of both sides.
- `scripts/pl6/` holds the test record of Phase P. Its gate has passed. Do not record it again.
  If a check script of `scripts/pl6/` fails only because `main` added tests, say so in your done
  summary and change nothing there.

## Steps

1. `git fetch origin main`. `git merge --no-ff origin/main`. Do not rebase.
2. Resolve each conflict. Keep the behavior of both sides. Where both sides changed the same
   function, read both changes and combine them; do not take 1 side blindly. List each conflicted
   file and how you resolved it, for your done summary.
3. Do the 3 known conflicts above, also where git reports no conflict.
4. `cargo build --workspace --locked` (correct `Cargo.lock` with `cargo update -w` only if the
   build asks for it; add no new version of a crate that neither side has).
5. Run `cargo test --workspace --locked --no-fail-fast`. Correct each failure that the merge
   caused. A red test of Phase 0 stays on the list of expected failures. A test of `main` that
   fails without the merge also (check on `origin/main` in a scratch worktree only if you must)
   is reported, not hidden.
6. Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`.
   Correct what the merge caused.
7. Commit the merge. Run the Check.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. Each test and each process that you start uses
   temporary folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
   development build at the live ledger. Never install a development build. Never run
   `horch self-update` or `cargo install`. Never register a service.
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it.
3. Do not use a shared `CARGO_TARGET_DIR`.
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
5. No co-author line. Never write the em dash character in a line that you write. Never edit
   `CHANGELOG.md` by hand: take the version of `main`.
6. Add no feature. Change nothing that the merge does not need.
7. While you wait for a long run, run `horch ledger progress` each 5 minutes.
8. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.
9. If a conflict needs a design decision, send QUESTION with the 2 options and your recommendation.

## Done when

The Check passes; the schema of `main` is version 8 and the fleetd schema is version 9; the PL-2
test passes; your done summary lists each conflicted file, each new OS site that you moved, and
each test of `main` that fails for a cause outside this merge.
