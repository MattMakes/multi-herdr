# 23: The git gateway of integration: compare-and-set ref moves, the merge-tree candidate, fleet branch and stash reads, and the by-content check of I8

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration::adapters::git && cargo test -p horch-core --test integration_git && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/adapters/git.rs`, `crates/horch-core/src/fleetd/integration/application/ports.rs`, `crates/horch-core/tests/integration_git.rs`

Spec: `docs/specs/fleetd/design.md`, "No work is lost", I8, I9, I11, I12, "fleetd sets the git settings that it needs", "The bundle", `docs/specs/fleetd/preservation-04-behavior.md` rows F11, W6.

## Goal

Integration has 1 git port and 1 adapter (`CliGitGateway`) for every git call it makes. Each local ref move is `git update-ref <ref> <new> <expected old>`. The by-content check says whether the changes of a branch are in a target branch, also after a squash merge.

## Requirements

- I9: each ref move is a compare-and-set on the tip that fleetd last saw; a mismatch moves nothing and returns the tip that git has.
- I8: `changes_in(repo, branch_tip, base, target_tip) -> bool`: true when each file that the branch changed from its merge base has the same content in the target (a squash merge with a conflict resolution passes when the result has the branch's content; Q14 gives the rule for a resolution that changed a file).
- The candidate (Q15): `build_candidate(base_tip, members) -> Candidate | Conflict(member, files)` with `git merge-tree --write-tree` and `git commit-tree`, 1 merge commit for each member in queue order, with no checkout and no worktree. Each task's commits are kept.
- Reads: the fleet branches (`git for-each-ref refs/heads/fleet/`), the branches named by `git stash list` (W6), the tip of a ref, and `git show <tip>:fleet.toml` (ticket 04).
- Settings: `rerere.enabled=true` and `merge.conflictStyle=zdiff3` in each repository and worktree that fleetd manages, set by fleetd, not assumed.
- F11: no git network call here; `fetch` and `push` are jobs (tickets 37, 38) through `crates/horch-core/src/vcs/network.rs`.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Wrap `crates/horch-core/src/vcs/git.rs` (the `Git` runner) as `CliGitGateway`. `vcs/landing.rs` is the old land path; do not change it.
- Each call is short and local (no working tree, no network), so the pass can make the compare-and-set after its commit (a short effect, as the deletion of a candidate ref in the design). The candidate build is a job (ticket 27).
- Tests make temporary repositories; no test touches this repository.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/integration_git.rs`: `update_ref_moves_only_from_the_expected_old_tip`, `update_ref_with_a_moved_tip_moves_nothing_and_returns_the_tip`, `i8_squash_merge_with_a_conflict_resolution_is_seen_as_landed`, `i8_branch_with_a_change_missing_in_the_target_is_not_landed`, `candidate_of_3_members_keeps_each_commit`, `candidate_with_a_conflict_names_the_member_and_its_files`, `stash_list_names_its_branch`, `fleet_settings_are_set_in_the_repository`.
2. Write the port in `application/ports.rs` and the adapter.
3. This ticket moves a ref, lands, pushes, deletes a branch or a ref, or changes who runs the finish steps. It has a crash test: a fault point before the commit of the intended row, and after the commit and before the effect. In each case the next pass completes or fails the effect 1 time, and no tip and no work is lost.
4. Run the Check. Commit the exact files.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. Every ref move of integration is a compare-and-set, and the by-content check sees a squash merge as landed.
