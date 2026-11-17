# 08: The request contract: the wire line, the request_id, the payload hash, and the kind catalog with the class of each kind

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime::domain::requests && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

1 pure module defines what a request is, which kinds exist, the class of each kind, and the bytes of a request and of a reply.

## Requirements

- RR-8 (each command request carries a `request_id`)
- SR-8 (each request kind has a class)
- FM-15

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The design, Architecture and roles, "The command channel": the client makes a `request_id` for each request; a request has a kind and a payload that never changes; each kind of request on the channel or in a request file is of class `operator`; the worker kinds `progress`, `result` and `ask` (and `idea note` from Phase 6) are files and never use the channel or the request folder; fleetd refuses a kind that it does not know and a `worker` kind.
- The list of kinds of this phase is the table "Request kinds" of `docs/tickets/fleetd-p5/MAP.md` (Open question Q6). Copy it into 1 `enum Kind` with `fn class(self) -> Class` and `fn as_str(self)`. The kinds of Phase 6 (`idea_add` and the others) are not in this phase.
- The wire form: 1 JSON line `{"request_id": <text>, "kind": <text>, "payload": <object>, "token": <text or absent>}`; the reply is 1 JSON line `{"request_id", "state": "applied"|"refused", "result" or "reason"}`. A request file holds the same JSON object.
- `payload_sha256` is the sha256 of the payload with its object keys sorted, so that 2 equal payloads always have 1 hash.
- `request_id`: 1 UUID v4 text that the client makes. The parser refuses an empty id, an id over 64 bytes, and an id with a character outside `[A-Za-z0-9-]`.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first, in the test module of `domain/requests.rs`: `each_kind_of_the_map_table_parses_and_has_its_class`, `unknown_kind_is_refused_with_its_name`, `worker_kind_is_class_worker`, `equal_payloads_with_other_key_order_have_1_hash`, `request_id_with_a_slash_is_refused`, `line_over_the_limit_is_refused_before_parse` (the limit is an input: `request_max_bytes` of ticket 03), `reply_round_trips`. They fail because the module is empty.
2. Write the types `Request`, `Kind`, `Class`, `Reply` and the parse functions. No I/O.

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
- The enum `Kind` has exactly the kinds of the MAP table.
