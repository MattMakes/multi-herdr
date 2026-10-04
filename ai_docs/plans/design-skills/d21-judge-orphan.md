# D21 judge-orphan: a dead judge job never leaves a paid judge CLI running

Unit slug: `judge-orphan`. Branch: `ds/judge-orphan`.

## GOAL

When a judge job is Lost because its leader process died, the judge CLI it
started (`claude -p`, which spends money) is stopped too, and the check
cannot signal an unrelated process.

## CONTEXT

- Read first: `ai_docs/plans/finish/00-conventions.md` (never run the gate
  under `git rebase -x`) and `ai_docs/reports/design-skills/pid-identity.md`
  (D20: `procid`, `Heartbeat.started`, `kill_job`, and the validator rule).
- D20 left this open: on Lost with a dead leader, `kill_job` signals nothing,
  so an orphaned judge CLI keeps running.
- Rule to apply (the same one D20 used in `evaluation/validator.rs`): POSIX
  never gives a new process a pid equal to the id of a process group that
  still has members. So if the judge job runs the CLI in its own process
  group (pgid = the job leader's pid) and the leader is dead:
  (a) no process has that pid → any remaining members are ours → `killpg`
      is safe and required;
  (b) a process has that pid with a different start time → reused → no
      signal.
  Check that the job really starts the CLI in that group (read
  `crates/horch/src/dataset/judge_job.rs` and the scheduler); if it does
  not, make it so.
- Windows: keep the current behaviour and say so.
- Test without signalling a foreign process: a fake judge CLI that sleeps and
  writes its pid; kill the job leader; run the Lost path; assert the fake
  CLI is gone. Put group tests where the CMP-16 audit allows them
  (`crates/horch-core/tests/procid.rs` or a new test file; see the D20
  gotcha).

## FILES

own: `crates/horch-core/src/evaluation/scheduler.rs`, `competition/judging.rs`
(Lost path), `crates/horch/src/dataset/judge_job.rs`, tests,
`ai_docs/reports/design-skills/judge-orphan.md`.

do not touch: the spec-closure units' files (`ai_docs/designs/`, marker
lines): T4 (opus-54) closes markers in `evaluation/`; rebase on its merge if
you overlap and tell the orchestrator.

## STEPS

0. Worktree. 1. Group launch check/fix. 2. Lost-path group kill. 3. Test
(prove it fails before the fix). 4. Gate. Report. Merge protocol.
