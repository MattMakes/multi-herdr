# 39: The pull request follows the feature state: gh opens it when ready and marks it draft when open again; manual shows the compare URL

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib taskledger::finish::pr && cargo test -p horch-core --test pr_follows_state && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/finish/pr.rs`, `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs`, `crates/horch-core/tests/pr_follows_state.rs`

Spec: `docs/specs/fleetd/design.md`, "The pull request follows the state", "Between features" (The pull request), `docs/specs/fleetd/preservation-04-behavior.md` row F17.

## Goal

fleetd opens the pull request of a feature (`pr = gh`) or puts the compare URL on the needs-you list (`pr = manual`) only when the feature is `ready`. When the feature moves back to `open`, fleetd marks the pull request as a draft (`gh`), or changes the item to "do not merge yet" (`manual`). The body keeps the section of each task between its markers (F17).

## Requirements

- F17: a section per task between `<!-- horch:task T-<n> -->` markers.
- A claim attempt's `pr` step keeps working until its run has no claim attempt.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Today: `crates/horch-core/src/taskledger/finish/pr.rs` (`step`, `with_section`, `pr_check`). Keep the body format. The `gh` calls run through the launcher as short effects after the commit, or inside the push job; choose the one that keeps the pass free of network calls (PL-5): a `gh` call is a network call, so it runs in a job (Q22: the push job opens or updates the pull request after its push).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first (a fake `gh` on PATH): `ready_feature_opens_its_pull_request`, `feature_open_again_marks_it_draft`, `manual_pr_puts_the_compare_url_on_the_needs_you_list`, `task_sections_keep_their_markers`.
2. Change `pr.rs`.
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

The Check passes. A pull request exists only for a ready feature.
