# 30: A failed bundle is bisected: each half is a child bundle, the failing branch goes to gate_failed, and the others go on

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::integration::domain::bisector && cargo test -p horch-core --test bisect && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/bisector.rs`, `crates/horch-core/src/fleetd/integration/application/bundles.rs`, `crates/horch-core/tests/bisect.rs`

Spec: `docs/specs/fleetd/design.md`, "The bundle" (On a failure, the bundle moves to `failed`, and fleetd bisects it), "The bundle state machine" (`failed`).

## Goal

A bundle whose fast gate failed moves to `failed`. fleetd splits its members into 2 child bundles with `parent_id`, built and gated in the same way. A child of 1 member that fails sends that branch to `gate_failed`; a child that passes lands its members.

## Requirements

- Each member of a failed bundle ends in exactly 1 of: a child bundle, `gate_failed`, `ready`.
- The gate output of the failed run goes with the branch to `gate_failed` (ticket 33 gives it to the fix attempt).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The pure split (`fn split(members) -> (Vec, Vec)`, queue order kept) and the decision of each child result are in `domain/bisector.rs`. Add the observation of a failed fast gate and of each child to `application/bundles.rs` (ticket 29 wrote it first).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `failed_bundle_of_4_makes_2_children_of_2`, `child_of_1_that_fails_sends_its_branch_to_gate_failed`, `clean_child_lands_its_members`, `each_member_ends_in_1_place`.
2. Write the bisector.
3. Run the Check. Commit the exact files.

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

The Check passes. A failed bundle finds its failing branch, and the other branches go on.
