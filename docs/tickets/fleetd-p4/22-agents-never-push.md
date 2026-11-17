# 22: SR-3: no agent pushes, deploys or messages outside the fleet: the deny rules of the profiles and a pre-push hook in the environment of each agent

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib harness fleetd::worker_profiles::domain::launch_env && cargo test -p horch-core --test sr3_no_agent_push && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/launch_env.rs`, `crates/horch-core/src/harness/launch.rs`, `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/tests/sr3_no_agent_push.rs`

Spec: `docs/specs/fleetd/design.md`, SR-3 ("Git hooks and profile defaults"), "Goals, non-goals and defaults" (Non-goals: agents merging to protected branches or deploying), "Architecture and roles" (Landing: pushing the feature branch is fleetd's).

## Goal

An agent of the fleet cannot push. Each agent process gets, through its launch environment, a git hooks path whose `pre-push` hook refuses with "fleet agents do not push; fleetd pushes the feature branch". The profiles of Claude Code and Codex deny `git push`, `gh pr merge` and `gh release` by default. fleetd's own push job is not blocked.

## Requirements

- SR-3: "No agent shall push to a protected branch, deploy, or message anyone outside the fleet." Choice Q13: in Phase 4 no agent pushes at all; each push is fleetd's push job (ticket 38).
- The old path of a claim attempt still pushes through horch's own git calls, which set `core.hooksPath=/dev/null` with `-c` (`crates/horch-core/src/vcs/network.rs`, F11). A `-c` value wins over the environment values `GIT_CONFIG_COUNT`/`GIT_CONFIG_KEY_n`/`GIT_CONFIG_VALUE_n` (git documentation), so the old path keeps working. Prove it with a test.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The hook: the launch environment of an agent (`launch_env.rs`, called from `crates/horch-core/src/harness/launch.rs`) adds `GIT_CONFIG_COUNT`, `GIT_CONFIG_KEY_0=core.hooksPath` and `GIT_CONFIG_VALUE_0=<fleet root>/hooks/agent`. fleetd writes `<fleet root>/hooks/agent/pre-push` (mode 0755, a POSIX shell script; on Windows git runs hooks with its own sh). A worktree with a hooks path of its own is not affected by this rule; name that gap in the done summary.
- Profile defaults: Claude Code `permissions.deny` gets `Bash(git push:*)`, `Bash(gh pr merge:*)`, `Bash(gh release:*)`; Codex gets `forbidden` `prefix_rule`s for the same (follow the deny rules of Phase 3 ticket 17 for `git worktree`).
- Ticket 14 owns `claude.rs` and `codex.rs` before this ticket; ticket 11 owns `launch.rs` before this ticket.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/sr3_no_agent_push.rs`: `agent_push_is_refused_by_the_pre_push_hook` (a temp repo with a bare remote), `horch_old_path_push_with_dev_null_hooks_still_works`, `claude_profile_denies_git_push_by_default`, `codex_profile_forbids_git_push_by_default`.
2. Write the launch environment, the hook file and the deny rules.
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

The Check passes. An agent's `git push` is refused, and fleetd's push and the old finish path push as before.
