# 02: Fix the refused brief spawn of skill-importer to its intake roles

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** light
**Check:** `cargo test -p horch -- spawn && cargo test -p horch-e2e --test skill_intake_spawn`
**Own:** `crates/horch/src/cmd/spawn.rs`, `crates/horch-core/src/procid.rs`, `crates/horch-e2e/tests/skill_intake_spawn.rs`, `skills/skill-intake/scripts/intakelib/cmd_spawn.py`
**Do not touch:** `crates/horch-core/src/taskledger/`, `skills/skill-intake/scripts/intakelib/roles.py`, `skills/skill-intake/scripts/intakelib/cmd_subagent_prompt.py`, `skills/skill-intake/roles/`, `teammates/`

## Goal

`intake spawn auditor`, run by a live skill-importer worker, starts the
intake-auditor pane. Today horch refuses it as task text.

## Context

- Run `solo-w60`. Task T-2 imports a skill through skill-importer-1.
- skill-importer-1 ran `intake spawn auditor`. It failed with exit 11.
- `skills/skill-intake/scripts/intakelib/roles.py` runs
  `horch spawn intake-auditor --exact "Read and follow <brief> exactly."`.
- horch refused it with the plain TRF-43 refusal ("horch spawn takes no task text").
- That argv is correct. Do not change it to a ledger-task flow.
- `crates/horch/src/cmd/spawn.rs` has 1 exception to TRF-43:
  `check_task_text` allows a brief from a `spawn_only` controller to a
  `horch_access: report` teammate.
- `spawn_only_check` also requires `runs_in(c.agent, procs)`: the caller is
  the recorded agent process of the controller's ledger row, or its descendant.
- The refusal came from `runs_in` returning false. The message had no
  "is in no ledger run" line, so the controller row has a worker id.
- The same bug blocks fleet w71 (run `solo-w71`, importing bombadil).
- The skill-importer Bash runs inside the Claude Code OS sandbox
  (`sandbox:` block of `teammates/skill-importer.md`).
- skill-importer-1 was spawned with `--force` at about 20:38Z.
- horch restarted the run watchdog at about 20:40Z on a new horch build.
- Other people have uncommitted edits in the main checkout. Work only in your worktree.

## Hypotheses to test, in this order

1. `libc::proc_pidinfo` in `parent_pid` (macOS) fails inside the Claude Code
   sandbox for a process outside the sandbox. The walk then stops early.
2. `horch_core::procid::is_same` fails inside the sandbox for the same reason.
3. The ledger row of skill-importer-1 has no `agent_pid` or no
   `agent_started`, or the recorded pid is not the claude process.

## Steps

1. Read the worker row of skill-importer-1. Use `horch ledger show T-2`
   and `horch sessions`. Record its agent pid and start time.
   Check: you can name the recorded pid and the real claude pid of pane w60:p2.
2. Reproduce `runs_in` under the sandbox. Write a small test binary or a
   debug flag that prints each step of the parent walk. Run it with
   `sandbox-exec` and a profile like the Claude Code sandbox profile.
   Do not message skill-importer-1. Do not type into its pane.
   Check: you can show which call returns `None` or `false`.
3. Fix the root cause. Keep the security property: only the recorded
   controller agent process, or a descendant, passes. `HERDR_PANE_ID` alone
   must not pass. Do not add an env var, a flag or a file that bypasses the check.
   Check: a unit test in `spawn.rs` covers the failing case with fake `Procs`.
4. If the sandbox blocks a syscall, choose a method that works in the
   sandbox. Examples: `sysctl` `KERN_PROC_PID`, or `proc_pidinfo` with
   `PROC_PIDT_SHORTBSDINFO`. Prove it under `sandbox-exec`.
5. If the cause is hypothesis 3, fix where horch records the agent pid. That
   file is outside your Own list. Send QUESTION: to the orchestrator with
   the file path first, then end your turn.
6. Run `cargo test -p horch -- spawn` and
   `cargo test -p horch-e2e --test skill_intake_spawn`.
   Check: both exit 0.
7. Commit the exact files you changed. Use explicit file paths.
8. Run `cargo build --release -p horch`, then
   `./target/release/horch install` from your worktree.
   Check: `horch --version` runs, and `which horch` is `~/.local/bin/horch`.
9. Run `horch ledger finish`.

## Constraints

- Do not weaken the TRF-43 refusal for any other caller.
- Do not edit `roles.py`. It has uncommitted edits by another person.
- A new executable takes 14-35 s at first exec on this Mac. Do not raise test
  timeouts for it.
- Do not use `ANTHROPIC_API_KEY`.

## Done when

- A unit test reproduces the old refusal with fake `Procs`, and passes with the fix.
- A sandboxed caller that descends from the recorded agent passes `runs_in`.
- A caller that does not descend from it still fails.
- The new horch is installed in `~/.local/bin/horch`.
- `horch ledger finish` passed every step.

## Report

Send with `horch ledger done`: the root cause in 1 sentence, the files
changed, the tests added, and the installed horch version.
