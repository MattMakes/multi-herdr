# 10: The profile and gate rules of the plan validator: each profile exists, has a fallback and a sandbox on this OS, and the project declares a fast gate and a full gate

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test plan_rules_profiles && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/domain/plan_rules_profiles.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_gates.rs`, `crates/horch-core/tests/plan_rules_profiles.rs`

Spec: `docs/specs/fleetd/design.md` (Worker profiles and adapters (The sandbox of a profile denies the operator path; Every worker persona names a fallback); Between features (Two declared gates); Failure mode FM-10).

## Goal

The validator refuses a plan that names a profile that does not exist, that has no fallback, or that profile validation refuses on this OS, and a plan of a project with no fast gate or no full gate.

## Requirements

- FM-10 (a missing profile)
- FR-3

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Profile facts come from the roster and from profile validation of Phase 5 (`crates/horch-core/src/roster/validation.rs`, ticket 38 of fleetd-p5): "not available on this OS: no sandbox". When the fallback has a sandbox on this OS, the plan is valid and the problem is a note, not a refusal (design: "the fallback rule of the roster applies").
- The declared gates come from the project `fleet.toml` (`[gates]`, ticket 04 of fleetd-p4). The rule reads them from `PlanFacts`.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `plan_with_an_unknown_profile_is_refused`, `plan_with_a_profile_without_sandbox_on_this_os_is_refused`, `plan_whose_profile_has_a_sandboxed_fallback_is_valid_with_a_note`, `plan_without_declared_fast_gate_is_refused`, `plan_without_declared_full_gate_is_refused`. They fail because the rules are empty.
2. Write the 2 rules and add them to `PlanValidator`.
3. Run the Check and the guard tests. Commit the exact files.

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
