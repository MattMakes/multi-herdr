# G5 fleet-messaging: report

Plan: `ai_docs/plans/finish/g5-fleet-messaging.md`. Worker: opus-73.

## 1. `horch inbox` and closed panes

Decision: `horch inbox` keeps a role whose pane is closed and marks it
`(pane closed)`. It does not hide it.

Reason: the role's id file is still in the mailbox. If `inbox` hid the role,
a failed `horch tell <role>` would list the role as known, and the reader
would have to guess where it went. The mark explains the failure.

- `Mailbox::role_states(&dyn WorkspaceClient)` (`messaging/mailbox.rs`)
  makes 1 `pane list` call for the workspace. It returns each role with
  `PaneState::Open`, `Closed` or `Unknown`.
- When herdr does not answer, every role is `Unknown`. `inbox` then lists
  every role without a mark and prints 1 line on stderr.

## 2. `horch done` with no registered orchestrator

Finding: `horch done` did not fail. It already exited 0. It printed
`horch done: could not reach orchestrator (role 'orchestrator' is not
registered ...); ledger is updated, shutting down anyway`.

The cause is the worker prompt and the `tell` error:

- `teammates/_base/fleet-worker.md` says "TRULY complete - results
  reported". So the worker first sends `horch tell orchestrator`.
- That `tell` exits 1 with "role 'orchestrator' is not registered".
- The prompt also says "If you are blocked ... WAIT". Codex read the failed
  `tell` as a block, reported "Blocked from notifying the orchestrator", and
  did not run `done`.

Fixes:

- `fleet-worker.md`: 2 new lines in the `done` item. When no orchestrator is
  registered, `horch done` still records the summary and exits 0. A failed
  `horch tell` never blocks `done`. The 16 `worker-*.txt` golden prompts
  carry the same 2 lines.
- `horch tell orchestrator` from a worker, with no orchestrator registered,
  now ends its error with 2 lines: nobody reads reports here, and
  `horch done "<summary>"` still records the summary and closes the pane.
- `horch done` (`CliDone::report`) checks the mailbox first. With no
  registered orchestrator it prints `horch done: no orchestrator is
  registered in this workspace, so nobody is told; the summary is recorded`
  and does not try the `tell`. If the mailbox cannot be resolved, the old
  path runs (`tell`, then the "could not reach" line).

`execution/lifecycle.rs` is unchanged (do-not-touch).

## 3. `horch spawn --resume` after a closed pane

- `ExecutionStore::end_if_pane_closed(key, &dyn WorkspaceClient)`
  (`execution/store.rs`) ends the newest record for `key` when all of these
  are true: the status is `working`, the record is not an orchestrator, it
  names a pane, herdr answers, and herdr does not have that pane. The record
  becomes `Failed(PaneVanished)`, legacy status `done`, with `finished_at`
  and the history entry `ended: pane closed without horch done`
  (`store::PANE_CLOSED`). The status is checked again under the ledger lock.
- `crates/horch/src/cmd/spawn.rs` calls it before it reads the record to
  resume (4 lines, commit d5520d7, approved by the orchestrator and told to
  opus-71).
- An open pane, a herdr that does not answer, and a record with no pane keep
  the record `working`. Planning then refuses. The refusal
  (`execution/plan.rs`) now names the pane and the exact command:
  `horch ledger done <id> "pane closed without horch done"`.

## Tests

- `crates/horch-core/tests/messaging.rs`:
  `role_states_mark_a_closed_pane`, `end_if_pane_closed_ends_only_a_vanished_pane`.
- `crates/horch-core/src/execution/plan.rs`: `working_refusal_names_the_fix`.
- `crates/horch-e2e/tests/messaging.rs` (new file):
  `g5_done_without_an_orchestrator`, `g5_resume_after_a_closed_pane`
  (refuses while the pane is open; `inbox` marks the closed role; the resume
  ends the record and resumes).

The e2e tests are in a new file because opus-71 (G3) has uncommitted edits
in `crates/horch-e2e/tests/lifecycle.rs`.

## Checks

- `cargo build --workspace --bins`: pass.
- `horch-core --test messaging`: 10 of 10. `--test golden_prompts`: 6 of 6.
- `horch-core --lib execution::`: 23 of 23.
- `horch --bin horch cmd::messaging`: 1 of 1.
- `horch-e2e --test messaging`: 2 of 2.
- `cargo clippy -p horch-core -p horch --all-targets -- -D warnings`: 0
  warnings. `cargo clippy -p horch-e2e --test messaging`: 0 warnings.
- `rustfmt --check` on my files: pass.

`horch-e2e --test lifecycle` did not compile at the time of the checks:
opus-71's uncommitted test at `lifecycle.rs:291` binds `let text`, which
shadows the `text()` function used at line 301.
