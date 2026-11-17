# 28: The fast_gate and full_gate jobs: the declared gate of the project runs on a tip in the gate tree of the feature

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration::adapters::exec_gate fleetd::worktree_custody::application::gate_tree && cargo test -p horch-core --test gate_jobs && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/adapters/exec_gate.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/gate_tree.rs`, `crates/horch-core/tests/gate_jobs.rs`

Spec: `docs/specs/fleetd/design.md`, "Between features" (Two declared gates; The full gate), "The kinds of job" (`fast_gate`, `full_gate`: pure), FR-11, PL-5 (a pass never waits for a gate), SR-4 and SR-8 (code that a worker wrote never runs with the rights of fleetd).

## Goal

A `fast_gate` job runs the project's fast gate on the candidate tip of a bundle; a `full_gate` job runs the full gate on the feature tip over the fetched primary tip. Each runs in the gate tree of its feature, through the launcher, inside the gate sandbox (never with the rights of fleetd), with its time limit, and its exit code is the verdict, written in `gate_runs`.

## Requirements

- The gate tree (Q18): 1 worktree for each feature at `<project>/.worktrees/gate-<feature id>`, a `worktrees` row with no task and the owner `fleetd`, made by the `worktree_add` job of Phase 3 (row first, FR-11) and removed when its feature is `merged` or `abandoned`. The gate job runs `git checkout --detach <tip>` in it (not `git worktree add`), then the gate command, so the build cache stays.
- The gate sandbox (Q19). A gate runs code of the project, and workers write that code. So a gate command never runs with the rights of fleetd. It runs through the launcher, in the gate tree, inside the gate sandbox. The gate sandbox is this ticket's policy on top of the platform sandbox primitive.
- The platform sandbox primitive is `sandboxed(cmd, writable)` in `crates/horch-platform/src/sandbox.rs` (ticket 17). The gate sandbox calls it. Do not write a second primitive. The check sandbox of ticket 18 is a different policy for a worker persona, and the gate does not use it.
- The writable paths of a gate are: the gate tree of the feature, the temporary folder of the job, and the gate cache of the feature (Q24 of MAP.md). Nothing in the project adds a path: the list comes only from fleetd. The gate cannot write the rest of the fleet root, the ledger, another worktree, `~/.cargo`, or the repository of the user outside the gate tree.
- NotSupported: the gate does not run, and no `gate_runs` row gets a result. The job ends with the cause `no gate sandbox: <reason>`. The feature gets a hold: fleetd queues no gate of the feature, and no bundle of the feature gates or lands. fleetd opens 1 needs-you item with the key `no_gate_sandbox:<feature id>`. A later pass that finds a sandbox ends the hold and closes the item. Never a run with the rights of fleetd.
- The gate cache (Q24): 1 folder for each feature at `<fleet root>/gate-cache/<feature id>/`, made by fleetd before the first gate and removed with the gate tree. fleetd sets `CARGO_HOME=<gate cache>/cargo` and `XDG_CACHE_HOME=<gate cache>/xdg` in the environment of the gate command. The gate cache starts empty, so it holds no configuration and no credential of the user.
- The `git checkout --detach <tip>` of the gate runs outside the sandbox, so it follows the fleetd git rule of ticket 41 (hooks off, no `core.fsmonitor`, and a program key of the repository config stops the job), and no hook or filter of the project runs.
- Pure jobs: a `passed` verdict of 1 input key is used again; a fail is used again only when the gate ran to its end.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The commands come from the project `fleet.toml` of ticket 04, read at the tip that the gate judges.
- The full gate on "the feature tip over the fetched primary tip": when the feature tip contains the fetched primary tip, the input is the feature tip; else the gate waits for the rebase of ticket 41 (no merge is made here).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `fast_gate_runs_the_declared_command_on_the_candidate_tip`, `full_gate_runs_on_the_feature_tip`, `passed_gate_of_1_key_is_used_again`, `gate_with_no_declared_command_fails_with_the_cause`, `gate_tree_is_a_custody_row_with_no_task`, `gate_at_its_limit_is_abandoned_and_its_group_killed`, `gate_command_that_writes_outside_its_paths_is_refused` (the gate writes 1 file in the fleet root, 1 file in another worktree and 1 file in `$HOME/.cargo` of the test; the gate fails, and the 3 files do not exist), `gate_writes_its_cache_only_in_the_gate_cache` (the gate command sees `CARGO_HOME` under `<fleet root>/gate-cache/<feature id>/` and writes there), `gate_with_no_sandbox_does_not_run_and_holds_the_feature_with_1_item` (the sandbox gives NotSupported through a test seam; the gate command does not run, the feature holds, and 1 needs-you item is open).
2. Write the executor and the gate tree.
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

The Check passes. Each gate runs as a job in the gate tree of its feature, inside the gate sandbox, and records its result in `gate_runs`. No gate command runs with the rights of fleetd.
