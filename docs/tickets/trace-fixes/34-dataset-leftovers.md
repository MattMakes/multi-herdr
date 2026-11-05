# 34: Remove the dataset leftovers that ticket 02 found

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `backend-developer` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch --lib -- trf_47 && cargo test -p horch-core --lib`
**Own:** `crates/horch-core/src/vcs/git.rs`, `crates/horch-core/src/harness/capabilities.rs`, `crates/horch-core/src/usage/money.rs`, `crates/horch-core/src/heartbeat.rs`, `crates/horch-core/src/compaction/jobfile.rs`, `crates/horch/src/cmd/install.rs`, `crates/horch/src/cmd/selfupdate.rs`, `crates/horch/src/lib.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Prior work: ticket 02, report
`/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/02.md`, section "Follow-ups outside scope".
Waits for: 02. Added by the orchestrator after ticket 02.

## Goal

Finish D17: no dead code from the dataset feature stays, and an old installed `multi-herdr-dataset` binary is removed.

## Requirements

- **TRF-47**: `multi-herdr-dataset` and its modules are gone. Test name starts with `trf_47_`.

## Steps

1. Read report 02, section "Follow-ups outside scope".
2. `GitClient` in `vcs/git.rs`: for each pub method, run `git grep -n <method>` over `crates/`. Delete each method that has no caller outside its own tests. Examples from report 02: `diff_patch`, `diff_digest`, `commit_all`, worktree helpers. Keep every method with a live caller.
3. Delete `Capabilities::footprint_bytes` when it has no reader.
4. Delete `MicroUsd` and `CostSource` in `usage/money.rs` when nothing outside that file uses them.
5. Remove the judge job from the doc comments in `heartbeat.rs` and `compaction/jobfile.rs`. Change only comments.
6. `horch install` and self-update: when `<bin dir>/multi-herdr-dataset` exists, delete it and print 1 line that names the path. Use the same bin dir that `RELEASE_BINARIES` installs into.
7. Write 1 test `trf_47_install_removes_old_dataset_binary`: a temp bin dir with a fake `multi-herdr-dataset`; after install, the file is gone and `horch` stays.

Out of scope: README, CHANGELOG and other prose (ticket 30). `ExecutionKind::Candidate` and `Judge` stay, because old ledgers hold such records.

## Done when

The check passes. Report 34 lists every deleted item and every item kept with its caller.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/34.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
