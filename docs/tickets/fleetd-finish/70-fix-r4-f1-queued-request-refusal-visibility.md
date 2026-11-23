# 70: Fix R4-F1: reconcile refused or rejected queued requests and do not report queued session_records as committed (RR-8, FM-15)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib execution::store && scripts/dev-sandbox.sh cargo test -p horch-core --test execution_store --test fleetd_request_files && cargo fmt --all --check`
**Own:** `crates/horch-core/src/execution/store.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`, `crates/horch-core/tests/execution_store.rs`, `crates/horch-core/tests/fleetd_request_files.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p5.md` (finding R4-F1 and section "Tests that do not prove their rule" row 1) and `docs/specs/fleetd/design.md` (rules RR-8 and FM-15).

## Goal

A queued request file in `<fleet root>/run/requests/` whose compare-and-swap or handler refuses, or whose file fails protected-file or JSON parsing checks, is never silently dropped: the spool opens 1 visible `needs_you` item that identifies the request and refusal reason, keeps a compare-and-swap refused `session_records` request recoverable or reconcileable, and `ExecutionStore::update` does not treat a queued `session_records` file as committed success when the row has not been applied to the ledger.

## Requirements

- **R4-F1 (Critical — RR-8, FM-15)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/execution/store.rs:995`; `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs:180`. `ExecutionStore::update` reports success as soon as `session_records` is durably queued. If another process changes a session row before fleetd starts, the queued compare-and-swap is refused. The spool then archives the request, and the original caller has already returned success. A session update is lost without a retry, an error to its caller, or a needs-you item. A malformed or permission-rejected queued file has the same invisible terminal state because its path is recorded as ingested and never read again. The smallest fix is to make every rejected or refused queued state change visible and recoverable. Keep the request for reconciliation or retry, and open one needs-you item that identifies the request and refusal. Do not describe a queued session update as committed success.
  - **Design rules**:
    - **RR-8** (`docs/specs/fleetd/design.md:161`): "Each write of a message or a snapshot in the protocol shall be atomic (write to a temp name, fsync, rename), a log shall be appended by whole lines, and ingestion shall be idempotent. Each effect outside the ledger (a process start or stop, a file write, a git ref move) shall be a committed row in an intended state before it occurs, shall be keyed by the id of that row, and shall be safe to repeat. Each command request shall carry a `request_id`, and a repeated `request_id` shall get the stored result with no second effect."
    - **FM-15** (`docs/specs/fleetd/design.md:1560`): "Typed prompt or command request lost or delivered twice ... Each command request has a `request_id`: fleetd applies it and writes its `requests` row in 1 transaction, a repeated id gets the stored result with no second effect, and a request file is archived only after its row is committed."

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Write the failing tests first:
   - In `crates/horch-core/tests/fleetd_request_files.rs`:
     - Add test function `refused_and_rejected_queued_request_files_open_needs_you_items`:
       - Queue 1 valid request file whose handler refuses (for example `session_records` with a stale `expected` row hash that fails with `SESSION_RECORD_CHANGED`, or a handler that returns `Err`), and 1 malformed or permission-rejected request file (`0644` mode or invalid JSON).
       - Run `w.scan(&mut cursor)`.
       - Assert that for the rejected file (`Handled::Rejected(reason)`), `request_spool::scan` records the `ingested` row AND opens 1 `needs_you` item of kind `request_file_rejected` (key `request_file_rejected:<path>`) whose text names the file path and the rejection reason.
       - Assert that for the refused queued request (`Outcome::Refused(reason)`), `request_spool::scan` opens 1 `needs_you` item of kind `request_file_refused` (key `request_file_refused:<request_id>`) whose text names the `request_id`, the request kind, the archive or request path, and the refusal reason.
   - In `crates/horch-core/tests/execution_store.rs` (or `crates/horch-core/src/execution/store.rs` module `session_records_tests`):
     - Add test function `queued_session_records_is_not_reported_as_committed_and_is_recoverable_on_conflict`:
       - Set up a ledger with an existing `session_records` row `r1` and no running fleetd channel server so `channel_client::send` falls back to `<fleet root>/run/requests/<request_id>.json` (`Sent::Queued`).
       - Call `ExecutionStore::update` to mutate `r1` while fleetd is down, and verify that `ExecutionStore::update` does not silently report committed success when the row is only queued (or merges queued pending `session_records` changes from `<fleet root>/run/requests/` on read/update so a second update or fleetd startup reconciles the field-level record mutation instead of losing the update on `SESSION_RECORD_CHANGED`).
       - Simulate another writer updating `r1` before fleetd ingests the queued request file, then run `request_spool::scan` with the `session_records` handler: assert that either the queued `session_records` change is reconciled/retried or the refusal opens the `request_file_refused:<request_id>` needs-you item and preserves the request payload for recovery.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_request_files -- refused_and_rejected_queued_request_files_open_needs_you_items` and see it fail.
2. In `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`:
   - Add needs-you item kinds `pub const FILE_REJECTED: &str = "request_file_rejected";` and `pub const REQUEST_REFUSED: &str = "request_file_refused";`.
   - In `read_file` (lines 201-215): when `parsed` returns `Err(reason)`, inside the same `write(conn, spool.guard, |tx| ...)` transaction that calls `ingested_store::record_message`, also call `needs_you::open_item` with key `format!("{FILE_REJECTED}:{path_text}")`, kind `FILE_REJECTED`, entity `"request_file"`, `entity_id: &path_text`, and text naming `&path_text` and `&reason`.
   - In `read_file` (lines 225-236): when `reply.outcome` is `Outcome::Refused(reason)`, open 1 `needs_you` item in a guarded transaction with key `format!("{REQUEST_REFUSED}:{}", request.request_id)`, kind `REQUEST_REFUSED`, entity `"request"`, `entity_id: request.request_id.as_str()`, and text naming `request.request_id`, `request.kind.as_str()`, the archived request file path, and `&reason`.
3. In `crates/horch-core/src/execution/store.rs`:
   - In `ExecutionStore::update` (line 995): do not silently discard `Sent::Queued { request_id, path }` as if the session row were already committed. When `self.send(changed)?` returns `Sent::Queued { request_id, path }`, or in `apply_session_records` when reconciling a queued `session_records` request:
     - Ensure that `ExecutionStore::read_rows` / `update` accounts for or reconciles queued `session_records` updates, or returns an explicit error / queued status when the caller requires committed session state, and that `apply_session_records` can reconcile non-conflicting record updates or surface the conflict through the spool's `request_file_refused` needs-you item without silent data loss.
   - Note: Ticket 05 (`05-c2-t899-wrapper-operator.md`, task T-5) also edits `crates/horch-core/src/execution/store.rs` so the worker wrapper stops sending `session_records`. Keep `ExecutionStore::update` and `apply_session_records` compatible with T-5 (`Blocked by: T-5`).
4. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Removing operator token access from `horch worker` (covered by ticket `05-c2-t899-wrapper-operator.md`).

## Done when

`refused_and_rejected_queued_request_files_open_needs_you_items` and `queued_session_records_is_not_reported_as_committed_and_is_recoverable_on_conflict` pass, every rejected or refused queued request file opens a visible `needs_you` item, and the ticket Check passes.
