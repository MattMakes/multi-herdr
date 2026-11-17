# 15: New features: a feature row for each idea with no --to, its internal run, a checked kebab-case branch name, and the branch created after its row

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test new_features && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/application/new_features.rs`, `crates/horch-core/tests/new_features.rs`

Spec: `docs/specs/fleetd/design.md` (Features (the first 5 bullets; A merge with ideas in flight); Ledger (`features`); Between features (Parallel is the default)).

## Goal

Several features exist at the same time. Each idea with no `--to` gets its own feature, and each feature gets its branch only after its row.

## Requirements

- Several features at the same time (in place of 1 non-solo run for each workspace)
- I11 for a new feature branch (the row exists before the branch)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Use cases of integration: `OpenFeature` (row in `open`, no branch yet), `NameFeatureBranch` (the name from the sub-orchestrator in `assumptions.md`, or from the ticket title of a `--direct` idea), `JoinFeature` (`--to`).
- The internal run (MAP.md, Q5): fleetd makes 1 `runs` row for each feature, through `transition()`, with the feature branch as its run branch. The id of the feature is `F-<run seq>` as in Phase 4 (Q5 of fleetd-p4). The user never runs `horch ledger run new`.
- A name is kebab-case and unique among features. For `--direct`, a name that exists gets a number suffix (`retry-sync-2`). A name from the sub-orchestrator that exists, or that is not kebab-case, is refused: the idea stays in `clarifying`, and 1 needs-you item and 1 doorbell to the sub-orchestrator give the reason (MAP.md, Q7).
- The branch: fleetd writes the `branches` row and the feature `branch` first, then creates the ref from the fetched primary tip as an effect after the commit, keyed by the row, with `git update-ref <ref> <tip> ""` (create only). The git gateway is `crates/horch-core/src/fleetd/integration/adapters/git.rs` (ticket 23 of fleetd-p4).
- `--to <feature>`: the feature must be `open` or `ready`. A `ready` feature moves to `open` (the door exit of ticket 04). A `merged` or `abandoned` feature is refused with its state.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/new_features.rs`: `idea_without_to_opens_a_new_feature`, `2_ideas_open_2_features_in_1_workspace`, `feature_row_and_branch_row_exist_before_the_ref`, `crash_after_the_commit_creates_the_ref_once`, `direct_name_gets_a_number_suffix_when_it_exists`, `non_kebab_name_is_refused`, `to_a_merged_feature_is_refused`, `to_a_ready_feature_moves_it_to_open`. They fail because the use cases are empty.
2. Write the 3 use cases.
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
