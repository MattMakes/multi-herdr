# 07: The idea folder: idea.md, amendments, interview.md, assumptions.md, spec.md and requirements.yaml, written atomically and read with the checks of SR-6

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test idea_folder && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/domain/idea_folder.rs`, `crates/horch-core/src/fleetd/ideas/adapters/idea_folder_files.rs`, `crates/horch-core/tests/idea_folder.rs`

Spec: `docs/specs/fleetd/design.md` (Workspace layout and file protocol (Directory layout `ideas/I-0007/`; Protocol rules 1, 4 and 7; Example: an idea; Example: requirements.yaml); Ideas (The interview; `assumptions.md`)).

## Goal

1 module writes and reads each file of `<fleet root>/ideas/I-NNNN/`. A file that a sub-orchestrator writes is not trusted.

## Requirements

- FR-1 (the immutable idea file)
- FM-30 (an `[open]` item refuses `assumptions.md`; each requirement cites an approved assumption or an answer)
- SR-6 for the files of an idea folder

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `idea.md` has the frontmatter of the design example (`id`, `created`, `source`, `priority`, `depends_on`) and the text. It is written 1 time, create-only, and never changed.
- `assumptions.md`: numbered items marked `[confirmed]`, `[assumed]` or `[open]`, with an optional `(Q<n>)`. A file with an `[open]` item does not validate. Its first line after the heading may be `feature: <kebab-case name>`, the name of the feature branch (MAP.md, Q7).
- `requirements.yaml`: `idea` and a list of `id`, `text`, `cites`. Each `cites` names an item of `assumptions.md` (`A<n>`) or an answer of `interview.md` (`Q<n>`). A requirement with no citation does not validate.
- `spec.md` validates when it exists and has at least 1 `## ` heading.
- Reads use the protected file reader of Phase 5 (`crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`, ticket 07 of fleetd-p5): a regular file, no link, real path inside its own idea folder, under the size limit of 64 KiB.
- Writes are atomic: `name.tmp`, fsync, rename (Protocol rule 1).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/idea_folder.rs`: `idea_md_is_written_once_and_never_changed`, `assumptions_with_an_open_item_do_not_validate`, `assumptions_give_the_feature_name`, `requirement_without_a_citation_does_not_validate`, `requirement_citing_a_missing_assumption_does_not_validate`, `assumptions_that_are_a_symlink_are_rejected`, `a_file_over_its_limit_is_rejected`. They fail because the module is empty.
2. Write the domain parse in `idea_folder.rs` (pure) and the file adapter.
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
