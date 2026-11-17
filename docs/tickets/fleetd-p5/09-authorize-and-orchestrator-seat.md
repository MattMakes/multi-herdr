# 09: The authorization decision of fleetd, and the 1 function that knows an orchestrator seat

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime::domain && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/operator.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

1 pure function decides each request from its facts: authorized with the text of `auth`, or refused with a reason. An orchestrator seat is known only from the ledger and `fleet.toml`.

## Requirements

- SR-5 (the peer user and the owner of a request file)
- SR-8 (the class, the token, the peer process check)
- FM-32
- REQUIRED item 3 of the ticket of this map (server-side operator check)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The facts of a request: its source (the channel with the peer user and the peer process, or a request file with its owner); the user that owns the ledger; the class of its kind; whether its token is valid; for the channel, whether the peer process is in a live attempt (ticket 11 gives this fact: yes, no or unknown); and whether that attempt is an orchestrator seat.
- The rules, in this order: a kind that fleetd does not know is refused; a `worker` kind is refused ("a worker kind is a file of its attempt"); a caller that is not the user of the ledger, or an unknown caller, is refused (SR-5); an `operator` kind with no valid token is refused (SR-8); on the channel, a peer in a live attempt, or a peer whose fact is unknown, is refused, unless its attempt is an orchestrator seat (Q2). Else the request is authorized. `auth` names how: for example `token; peer outside each live attempt` or `token; orchestrator seat T-<attempt>` or `token; request file`.
- A guard whose fact is unknown does not hold. An unknown fact never authorizes.
- The orchestrator seat (Open question Q2, answer of the orchestrator): `fn is_orchestrator_seat(attempt: &AttemptFacts, seats: &[String]) -> bool` in `domain/operator.rs`. An attempt is a seat when its task id is null and its persona, as the ledger records it, is in `orchestrator_seats` of `fleet.toml` (ticket 03). It never reads an environment variable. Its doc says: "Phase 6 removes this exemption." This is the only place of the exemption.
- Until Phase 6 the orchestrator agent has operator rights. MAP.md lists this as a security choice for the operator.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `worker_kind_is_refused`, `unknown_kind_is_refused`, `other_user_on_the_channel_is_refused`, `unknown_peer_user_is_refused`, `request_file_of_another_owner_is_refused`, `operator_kind_without_token_is_refused_and_names_the_token`, `operator_kind_from_a_live_attempt_is_refused`, `operator_kind_with_unknown_peer_process_is_refused`, `operator_kind_from_an_orchestrator_seat_is_authorized`, `request_file_with_valid_token_is_authorized`, and for the seat: `attempt_with_a_task_is_never_a_seat`, `persona_not_in_the_seats_of_fleet_toml_is_not_a_seat`, `worker_that_sets_horch_role_or_the_persona_name_in_its_environment_is_not_a_seat` (the function has no input from the environment, and the test sets `HORCH_ROLE=orchestrator` around the call to prove it).
2. Write `fn authorize(facts: &RequestFacts) -> Decision` and `is_orchestrator_seat`.

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
- The 13 tests pass.
- No other file holds a rule of the orchestrator exemption.
