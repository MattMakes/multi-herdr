# D20 pid-identity: never signal or trust a pid that may be reused

Unit slug: `pid-identity`. Branch: `ds/pid-identity`.

## GOAL

No product code sends a signal to, or decides liveness from, a bare pid that
it did not just spawn and still holds unreaped. Every stored pid is stored
with its process start time, and every signal and liveness check confirms
that the pid still names the same process (same start time) first.

## CONTEXT

- Read first: `ai_docs/plans/design-skills/00-conventions.md`.
- Found by opus-47 (D19). The test fake fake-herdr killed a reused process
  group (`kill -TERM -<pid>` on a pid recorded long before) and stopped
  another worktree's `just gate` with SIGTERM. D19 fixed the fake in commit
  7b6a152 on `ds/dataset-tidy` by recording the leader's start time. Reuse
  that helper's approach (read it). Do not copy it: if it lives in test code,
  move the shared part to `horch-core` and make the fake use it.
- Product sites, each with the reason it is a risk:
  1. `competition/judging.rs` ~199: for `JobState::Lost` the coordinator
     calls `evaluation::scheduler::kill_job(hb.pid)`. `kill_job`
     (`scheduler.rs` ~270) sends SIGKILL to group `-pid` and to `pid`.
     `Lost` includes "pid not alive" (`scheduler.rs` `decide` ~141), so it
     kills a pid it knows is dead. A stale heartbeat whose pid was reused
     and looks alive has the same result. Fix: the heartbeat records the
     job's start time; kill only when the pid and its start time match;
     never signal a pid that `decide` found dead.
  2. `harness/prime.rs` ~119: SIGTERM to a daemon pid read from a file.
     Same check.
  3. `fsx.rs` ~229 and `telemetry/lock.rs`: lock liveness is `kill(pid, 0)`.
     A reused pid makes a stale lock look alive forever (a hang, not a
     wrong kill). Store the start time with the holder pid and treat a
     mismatch as dead. Keep old lock files readable (no start time = the
     current behaviour).
  4. Search for any other site: `rg -n 'kill\(|killpg|SIGKILL|SIGTERM|tasklist' crates/*/src`.
     A `Child::kill()` or `killpg(child.id())` on a child the code still
     holds unreaped is safe (the zombie keeps the pid). Say so for each.
- Start time: on macOS `proc_pidinfo`/`sysctl KERN_PROC` (`kp_proc.p_starttime`),
  on Linux `/proc/<pid>/stat` field 22. Windows: the code uses `tasklist`;
  use process creation time if cheap, or keep the current behaviour there
  and say so. Use `libc` (already a dependency); no new crates without a
  reason in the report.
- Tests: a unit test per site that a pid with a different start time is not
  signalled / is dead; an e2e or integration test for the judge-job Lost
  path. Never send a signal to a real foreign process in a test: spawn your
  own child, let it exit, and check that the stale record is not signalled
  (inject the signaller through a seam if needed).

## FILES

own: `crates/horch-core/src/evaluation/scheduler.rs`, `competition/judging.rs`
(the Lost path only), `harness/prime.rs` (the signal only), `fsx.rs` (the
liveness only), `telemetry/lock.rs`, a new `crates/horch-core/src/procid.rs`
(or a similar name) for the shared helper, the fake that D19 changed
(only to call the shared helper), tests, `ai_docs/reports/design-skills/pid-identity.md`.

do not touch: skills, teammates, roster. D19 (opus-47) may still change the
e2e harness for its items 7 and 8: rebase after its second merge and tell
the orchestrator if you overlap.

## STEPS

0. Create the worktree from design-skills after D19 merge 1 lands (the
   orchestrator spawns you then).
1. The shared start-time helper with tests. Commit.
2. Sites 1–4, a commit each, gate before each.
3. Report: every site, its decision and its test. Follow the merge protocol.

## ITEM 5 (added 2026-10-03): git environment isolation

Incident: `git rebase -x '... just gate' design-skills` in this worktree
exported `GIT_DIR` to the gate. Tests that build temp repos
(`crates/horch/tests/skills_cli.rs:175` and `crates/horch-e2e/tests/skills_exposure.rs:265`
`git init --bare` + commit "demo"; `crates/horch-core/tests/vcs.rs:94` and
`crates/horch-core/tests/coordinator.rs:113` `git init -b main` + commit
"base") then wrote to the real repository: `core.bare=true`, HEAD=main and 2
commits on main. The operator repaired it.

Fix, so no environment can point a git call at the wrong repository:
1. Product: every place horch runs `git` (`vcs::git::SystemGit` and any other
   `Command::new("git")` / `HORCH_GIT_BIN` site; search for them) removes
   `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, `GIT_COMMON_DIR`,
   `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`,
   `GIT_NAMESPACE`, `GIT_CEILING_DIRECTORIES` and `GIT_PREFIX` from the
   child, and always passes the repository with `-C <dir>`. One shared
   helper, not N copies.
2. Tests: every test helper that runs git uses the same scrub (a test-side
   helper in each test crate's support module is fine if it calls a shared
   list of variable names exported from horch-core).
3. `scripts/phase-gate.sh` unsets the same variables at the top, with a
   comment that names this incident.
4. Regression test: create a decoy repository, export `GIT_DIR` and
   `GIT_WORK_TREE` to it, run the 4 named tests' setup helpers (or the
   product git runner), and assert the decoy's config, HEAD and refs are
   unchanged.
5. Never run the gate under `git rebase -x` again in this run; rebase, then
   run the gate.
