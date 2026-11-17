# 12: The requests rows: pending, then applied or refused in 1 transaction, and the stored result for a seen request_id

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_requests && cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/requests_store.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/requests.rs`, `crates/horch-core/tests/fleetd_requests.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

fleetd records each request in its `requests` row and applies it at most 1 time, so a retry or a second copy never has a second effect.

## Requirements

- RR-8 (a repeated `request_id` gets the stored result with no second effect)
- FM-15
- SR-8 (the `requests` row records the class and the decision)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The design, "1 row for each request": fleetd writes the row with the state `pending` when it reads a request; it applies the request and writes its result in 1 transaction, and the row goes to `applied` with its result or to `refused` with its reason; a request with an id that is already applied or refused gets the stored result and has no second effect; a known id with a different payload hash is refused.
- A `pending` row that a crash left is applied when the same `request_id` comes again (from the client retry or from the request file, which stays until its row is committed).
- Each write transaction checks the generation of the `fleetd` row (Phase 1, `crates/horch-core/src/fleetd/fleet_runtime/domain/generation.rs` and `adapters/fleetd_row.rs`) and goes through `transition()` where it moves a state.
- The handlers: a map from `Kind` to 1 function `fn(&Transaction, &Value) -> Result<Value>`. A handler writes rows only. An effect (a stop, a launch, a git command) is an intended row that the pass carries out after the commit (A24). The handlers of tickets 17 to 20 fill the map. A kind with no handler is refused with the reason "not handled in this version".
- The `caller` column: the peer user, or the owner of the request file. The `class` column: from the kind. The `auth` column: the text of the decision of ticket 09, null for a refusal.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_requests.rs`, with a test handler that counts its calls: `request_with_a_seen_request_id_returns_the_stored_result` (2 sends, 1 call), `known_request_id_with_another_payload_hash_is_refused`, `refused_request_returns_its_reason` (the second send gets the same reason), `pending_row_of_a_crash_is_applied_once_when_the_id_comes_again`, `handler_error_refuses_the_request_and_rolls_back_its_rows`, `wrong_generation_writes_nothing`, `operator_request_without_a_valid_token_is_refused_and_recorded` (the row has the class, the reason and a null `auth`).
2. Write the store functions and `fn handle(store, request, decision, handlers) -> Reply`.

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

- The Check passes, with the guard tests.
- The 7 tests pass.
- No path applies a request outside 1 transaction with its row.
