# 27: A drift becomes an idea: a failed conformance run adds 1 idea "Update the <tool> adapter for <version>" with the diff and the failed checks

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test p7_drift_idea && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/application/drift_idea.rs`, `crates/horch-core/tests/p7_drift_idea.rs`

Spec: `docs/specs/fleetd/design.md` ("Docs sync and adapter conformance" (step 4; "a drift that docs sync finds becomes an idea, like any other work")).

## Goal

Each failed conformance run of a CLI version gives exactly 1 idea, with `CHANGES.md` and the failed checks in its text, and you see it in `needs-you/`.

## Requirements

- FM-19 (part): the drift idea.
- FR-8 (part): the result of a failed run reaches you.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Add the idea through the intake of Phase 6 (`crates/horch-core/src/fleetd/ideas/application/intake.rs`, fleetd-p6 ticket 16). Do not write an idea row or file by another path.
- The text: the title "Update the <tool> adapter for <version>", then `<fleet root>/docs/<tool>/CHANGES.md`, then the failed checks of `conformance.json`.
- Idempotent (Slice 7, `idempotent: adapter-conformance-failed`): a second failed run of the same CLI and version adds no second idea. Use the request key of the intake, built from the CLI, the version and the adapter hash.
- You see the idea in `needs-you/` through the needs-you mirror of Phase 6 (fleetd-p6 ticket 21). Add 1 needs-you item for the idea if the intake does not give one.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_drift_idea.rs`: `failed_conformance_adds_1_idea_with_the_diff_and_the_checks`, `idempotent_adapter_conformance_failed_adds_no_second_idea`, `drift_idea_is_in_needs_you`, `passed_conformance_adds_no_idea`. They fail before the code.
2. Write the code. Run the Check and the guard tests. Commit the exact files.

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

## Done when

- The Check passes.
