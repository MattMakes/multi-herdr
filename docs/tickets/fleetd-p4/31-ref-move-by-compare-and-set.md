# 31: A passed bundle moves the feature ref by compare-and-set; the move is observed before landed; a failed compare-and-set drops the bundle

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration::application::ref_move && cargo test -p horch-core --test ref_move && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/application/ref_move.rs`, `crates/horch-core/tests/ref_move.rs`

Spec: `docs/specs/fleetd/design.md`, I8, I9, FM-37, "The bundle state machine" (`passed`, `landed`), "Landing a task branch" (F9), "A moved tip drops a bundle".

## Goal

A bundle whose fast gate passed moves to `passed`. After the commit, fleetd checks once more that each member's task is not cancelled (F9) and moves the feature branch from the base tip to the candidate tip with 1 compare-and-set. The next pass reads the ref: at the candidate tip the bundle is `landed` and each member is `landed` (after the by-content check, I8). A compare-and-set that fails drops the bundle; its members go back to `ready`, and fleetd records the new tip.

## Requirements

- I9: a gate result on a stale tip moves no ref.
- FM-37: a moved tip makes the gate result stale; a `passed` bundle whose compare-and-set fails goes to `dropped`.
- I8: a member is `landed` only when its changes are in the feature branch by content. `landed` is terminal and not checked again.
- `bundle_passed_limit_s` (10 min) for `passed`; past it, 1 needs-you item.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- `seen_tip` of the feature branch row is the expected old tip. A crash between the compare-and-set and the observation: the next pass reads the ref, and the result is the same (the move is safe to repeat).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `passed_bundle_moves_the_feature_ref_from_the_base_tip`, `move_is_observed_before_landed`, `passed_bundle_whose_compare_and_set_fails_is_dropped_and_its_members_are_ready`, `cancelled_member_task_moves_no_ref`, `crash_after_the_move_before_the_observation_lands_once`.
2. Write the move and the observation.
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

The Check passes. The feature branch moves only by a compare-and-set to a gated candidate, and a lost race moves nothing.
