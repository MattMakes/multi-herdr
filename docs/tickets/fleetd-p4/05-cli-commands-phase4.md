# 05: The 4 new commands in the CLI: horch result, horch feature abandon, horch branch abandon and horch rescue drop, each in its own file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch --bin horch -- cli && cargo build -p horch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/main.rs`, `crates/horch/src/cmd/mod.rs`, `crates/horch/src/cmd/resultcmd.rs`, `crates/horch/src/cmd/featurecmd.rs`, `crates/horch/src/cmd/branchcmd.rs`, `crates/horch/src/cmd/rescuecmd.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 6; "Features, landing and integration" (`horch feature abandon`, `horch branch abandon`, `horch rescue drop`); "Workspace layout and file protocol" (`horch result`).

## Goal

The 4 commands exist in the CLI with their arguments and help text, so that tickets 12, 35, 40 and 43 each own only the file of their command. Each command prints "not built yet" and exits 2 until its ticket lands.

## Requirements

- `horch result [--summary <text>] [--status completed|blocked|failed] [--commit <sha>]... [--file <path>]... [--notes <text>] [--check]` (ticket 12 fills it; `--check` is the test of the Stop hooks).
- `horch feature abandon <feature>`, `horch branch abandon <branch>`, `horch rescue drop <ref>`.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Follow the registration of `horch job` (`crates/horch/src/cmd/jobcmd.rs`, Phase 1 ticket 23) and `horch task` (Phase 2 ticket 13) in `main.rs` and `cmd/mod.rs`.
- The 3 abandon and drop commands are operator commands; ticket 35, 40 and 43 add the role check.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the test first: `cli_has_result_feature_branch_and_rescue_commands` parses each command line with its arguments.
2. Register the commands. Each `run` function in its file bails with "not built yet".
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

The Check passes. `horch result --help`, `horch feature abandon --help`, `horch branch abandon --help` and `horch rescue drop --help` print their arguments.
