# 10: The operator token: made 1 time with mode 0600, read by the horch command, compared in constant time

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test operator_token && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/token_file.rs`, `crates/horch-core/tests/operator_token.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

fleetd makes `<fleet root>/run/operator.token` at its first start, a `horch` command reads it, and fleetd accepts an `operator` request only with that token.

## Requirements

- SR-8 (the operator token in `/fleet/run/operator.token`, mode 0600)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The design: "At install, fleetd makes a random token in the file `/fleet/run/operator.token`, with mode 0600." `horch install` starts fleetd, so fleetd makes the token at start when the file is missing (ticket 41 calls it after the lock). The fleet root is `$FLEET_ROOT`, else `<ledger dir>/fleet` (Open question Q4 of `docs/tickets/fleetd-p1/MAP.md`).
- Make the file create-only (`O_EXCL`), with mode 0600 at the create, in the folder `run/` of mode 0700, with 32 random bytes as hex. Use the random source that the workspace already has (read the `Cargo.toml` files); add no crate if one exists.
- Read the file through the protected file reader of ticket 07 (regular, no link, owner, mode 0600, size at most 128 bytes). A token file that fails a check is an error with its reason, and fleetd opens 1 needs-you item (ticket 41). fleetd never makes a second token over a file that exists.
- Compare in constant time.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/operator_token.rs`: `first_start_makes_a_token_with_mode_0600`, `second_start_keeps_the_token`, `token_file_with_mode_0644_is_refused_with_its_reason`, `token_file_that_is_a_link_is_refused`, `wrong_token_does_not_match`, `token_of_other_length_does_not_match`.
2. Write `ensure(fleet_root) -> Result<Token>`, `read(fleet_root) -> Result<Token>` and `Token::matches(&self, text) -> bool`.

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
- The 6 tests pass.
- No test reads the token of the live fleet.
