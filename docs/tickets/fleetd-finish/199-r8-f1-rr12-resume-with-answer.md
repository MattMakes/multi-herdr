# R8-F1 (Important): RR-12: an answered question does not resume the session of the worker

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-e2e --locked --test p6_worker_question --test p6_questions && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test push_dispatch && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p6_worker_question.rs`, `crates/horch-core/src/fleetd/task_execution/application/launch.rs`

Source: the end review `docs/specs/fleetd/reviews/end-review-a.md` (task T-297, reviewed tip 08c6d4ff). Read its finding first; it has the file and line of each fact.

## Fault

RR-12 (`docs/specs/fleetd/design.md:165`) and the flow text at line 1015: "fleetd resumes the worker's session with the answer. If the resume fails, a fresh worker starts from the question, the answer and the committed work." Only the fresh arm exists (`dispatch.rs:637`, `task_files.rs:124`, test `answer_by_tell_answers_reaches_the_next_attempt`). No production path sets `workers.resumed_from_session`. Task T-15 deleted `g52_resume_uses_own_session` (the manual `horch spawn --resume`, retired by design) and no test proves the resume with an answer.

## Goal

When a question of a task attempt is answered and the ended attempt has a session id (fleetd reads it from `session.json` of the attempt, T-899), the next attempt of the task starts the CLI with the resume form of its adapter (the design table of the CLIs, row "Resume": claude `--resume <id>`, codex `exec resume <id>`, antigravity `--conversation <id>`) through the launcher (SR-7: never the native agent resume of herdr), in the same worktree, with the answer as its input, and the worker row records `resumed_from_session`. If the adapter has no resume form, the session id is missing, or the resumed CLI ends at once with a failure of the resume, the fresh arm of today runs. The resume burns no attempt of the task by itself; say in the commit message how a failed resume is counted, with the design line.

## Steps

1. Read how an attempt is launched (`launch.rs`, the recipes of each adapter, the wrapper) and where the session id of an attempt is stored. List the files that the change needs and send `NOTE:` with them before you edit a file outside Own; they are then in your scope if no running task holds them.
2. Write the failing tests first in `p6_worker_question.rs` with the fake CLIs: (a) the next attempt after an answer runs the fake CLI with the resume form and the session id of the first attempt, and `resumed_from_session` is set; (b) a fake CLI that refuses the resume leads to a fresh attempt with the question, the answer and the committed work, and no work is lost.
3. Implement. Security: the session id that fleetd passes must come from the ledger or the attempt folder that fleetd owns, checked for form (it becomes a CLI argument: no option injection; it must not start with `-`), never from a file that the worker can write after its attempt ended without a check.
4. Run the 2 e2e files 3 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build or a tool. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`. Each `claude` command of a test or of the product removes it from the environment.
5. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
6. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never run `cargo test -p horch --bin horch` with a filter that matches `watch_once` before task T-287 has landed (it starts a process chain).
7. The host has a high load from another fleet of the operator: never touch its processes. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Other tasks edit the same large files (`pass.rs`, `handlers_worker.rs`, `cmd/fleetd.rs`): change only the functions of your ticket there; `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish` and again when the reviews are in; resolve a conflict in favor of both changes.
9. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes, tests (a) and (b) pass 3 times, and the commit message says how each adapter resumes.
