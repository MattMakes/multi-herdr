# 14: The Stop hooks of Claude Code, Codex and Antigravity: a push attempt cannot end its turn without result.json or a question file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib harness::claude harness::codex harness::antigravity fleetd::worker_profiles && cargo test -p horch-core --test harness_defaults && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/harness/antigravity.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/stop_gate.rs`, `crates/horch-core/tests/harness_defaults.rs`

Spec: `docs/specs/fleetd/design.md`, "Worker profiles and adapters" (the adapter table, row `result_gate`), "Supervision, liveness and reconciliation" (Finishing without reporting is closed at both ends), FM-6.

## Goal

Each launch of a push attempt in Claude Code, Codex or Antigravity has a `Stop` hook that runs `horch result --check`. While both `result.json` and a question file are missing, the hook exits 2 with "run `horch result` first", so the agent does not end its turn.

## Requirements

- FM-6 (the inside end): the stop gate refuses to end the turn without `result.json` or a question file.
- The hook is only on a push attempt. A claim attempt and a pane that is not a fleet worker get no hook.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Claude Code: the hook goes into the `--settings` overlay that `claude.rs` builds (`push_settings_arg`, `default_overlay`, `merged_inline_settings`), as `hooks.Stop` with 1 command hook. Merge it with the teammate's own settings; do not drop a hook the teammate has.
- Codex: "`Stop` hook (managed config, since user hooks need trust)". See `push_hook_trust` in `codex.rs` and the hook trust test near `codex.rs:1334`.
- Antigravity: the design says "`Stop` hook, with the same rule" and its seeded settings are "to confirm in conformance". If the synced docs of Antigravity name no Stop hook, send a QUESTION with the evidence and make Claude Code and Codex first.
- The rule (pure) is `domain/stop_gate.rs`: `fn may_end(outbox: &OutboxFacts) -> Decision`. The hook command is `horch result --check` (ticket 12) with the absolute path of the installed or test `horch`, as the launch knows it.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `claude_push_launch_has_a_stop_hook_that_runs_result_check`, `claude_hook_merges_with_teammate_hooks`, `codex_push_launch_has_a_trusted_stop_hook`, `antigravity_push_launch_has_a_stop_hook`, `claim_attempt_has_no_stop_hook`, and in `stop_gate.rs` `turn_may_end_with_a_result_or_a_question_only`.
2. Write the hook for each adapter.
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

The Check passes. The launch command of each of the 3 CLIs for a push attempt carries its Stop hook.
