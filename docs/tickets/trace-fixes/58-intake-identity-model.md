# 58: The spawn_only controller is identified by its recorded process, intake roles run on Opus

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** no
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p horch -- trf_43 spawn_only report && cargo build --workspace --bins && cargo test -p horch-e2e --test skill_intake_spawn`
**Own:** `crates/horch/src/cmd/spawn.rs`, `teammates/intake-sanitizer.md`, `crates/horch-e2e/tests/skill_intake_spawn.rs`

Spec: `docs/specs/trace-fixes.md` (conventions in section 8). Added by the orchestrator after report 56.
Prior work: ticket 56, report `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/56.md` (Decision and "Follow-ups outside scope"); decisions R67.
Waits for: 56.

## Goal

The TRF-43 exception (R67) lets only a `spawn_only` controller send a brief. The orchestrator's condition was: horch decides who the caller is, never from a flag or an env value the caller can set. Today `spawn_only_check` in `spawn.rs` takes the caller from `HERDR_PANE_ID`, which any unsandboxed worker can set to the controller's pane id.

## Requirements

- **TRF-43** (continued): a task-text spawn passes the exception only when the calling process descends from the controller's recorded agent process (the pid that horch recorded at launch, ADR 0002). A matching `HERDR_PANE_ID` alone is not enough. Test names start with `trf_43_`.

## Steps

1. In `spawn_only_check`, after the pane lookup, read the controller's recorded agent pid (worker row or session record) and check that the caller's process is that pid or a descendant of it (walk parent pids with `libc`/`/proc`-free code that works on macOS and Linux; reuse an existing process-tree helper in horch-core if one exists). Refuse with the TRF-43 refusal text otherwise.
2. Test `trf_43_a_spoofed_pane_id_cannot_send_a_brief`: a caller with the controller's `HERDR_PANE_ID` but not in its process tree is refused. Keep `trf_43_spawn_only_controller_may_send_a_brief` passing (inject the pid source so the test needs no real tree).
3. `teammates/intake-sanitizer.md`: `model: sonnet` -> `model: opus` (operator rule: workers on Opus, no Sonnet personas). Keep `effort: medium`. Update any roster or golden test that pins it.
4. Report socket `done` for an intake role with a ledger row and no claimed task (report 56): add 1 e2e test in `skill_intake_spawn.rs` that runs a role to `done` and checks the row ends and nothing errors. Fix the code if it fails (name the file in the report; outside Own needs a QUESTION).
5. Run the Check.

Warning: never run a branch-built horch against the shared ledger. Set `HORCH_LEDGER_DB` to a scratch file for any manual run.

## Done when

The Check passes.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/58.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
