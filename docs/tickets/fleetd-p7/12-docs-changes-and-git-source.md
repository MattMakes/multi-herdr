# 12: CHANGES.md for each tool: the changed pages and the changed flags, env vars and settings keys, and the also_git source of Codex

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p agent-docs-sync --locked && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/agent-docs-sync/`, `Cargo.lock`

Spec: `docs/specs/fleetd/design.md` ("Docs sync and adapter conformance" (What one sync does, step 2; the `also_git` key)).

## Goal

After 1 sync, `<out>/<tool>/CHANGES.md` lists each changed page against the previous version and each changed flag, env var and settings key of the reference pages.

## Requirements

- FR-8 (part): step 2 of 1 sync.
- FM-19 (part): the diff that a drift idea carries (ticket 27).

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The previous version: choice (MAP.md Q4), `<out>/<tool>/versions.json` lists the versions in the order of their first sync. The previous version is the entry before the current one. The first sync of a tool writes a CHANGES.md that says "first sync".
- The design: "a simple regex pass over code spans". A flag is a code span that starts with `-`. An env var is a code span of capitals, digits and `_` with at least 1 `_`. A settings key is a code span with a `.` or a `:` and no space. Keep the 3 rules in 1 place.
- `also_git` (Codex: the `docs/` folder and the release notes of the repository): a shallow, sparse clone through the platform start function of ticket 03 (`git clone --depth 1 --filter=blob:none --sparse`, then `git sparse-checkout set docs`) into `<out>/<tool>/<version>/git/`. The test uses a local bare repository made in a temporary folder (a path, not the network). Add `horch-platform` as a dependency of the crate.

## Steps

1. Write the tests first in `crates/agent-docs-sync/tests/changes.rs`: `changes_list_added_updated_and_removed_pages`, `changes_list_changed_flags_env_vars_and_settings_keys`, `first_sync_writes_first_sync`, `also_git_copies_the_docs_folder_from_a_local_repository`, `also_git_starts_git_through_the_platform_start_function` (the forbidden name is set in the parent and is not in the child). They fail before the code.
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
