# U36 session-discovery-race

## Change

- `harness/launch.rs`: `discover_once` holds the single discovery attempt (herdr `agent_session_id`, then `discover_sessions` skipping claimed ids). The thread and `done` both call it.
- `harness/launch.rs`: new `pub fn discover_now`. It reads the brief, skips when the record has a session id, the harness does not discover sessions, the launch was a resume, or the launch marker is gone. It records the id and removes the marker.
- `harness/launch.rs`: `POLL_SCHEDULE` waits 0.5 s, 1 s, 2 s, then 3 s. There are 62 polls, about 3 minutes in total.
- `execution/lifecycle.rs`: `DoneSteps::record_session` (default: nothing). `done` calls it before `unregister`. An error is logged and `done` goes on.
- `crates/horch/src/cmd/messaging.rs` (orchestrator approved): `CliDone::record_session` calls `launch::discover_now`.

## Test

`tel_session_recorded_when_agent_ends_fast` in `crates/horch-e2e/tests/lifecycle.rs`. It fails with the fix removed (3 s first poll, no `record_session`) and passes with it. `fake-codex` needed no change.

## Gotchas

- `done` does not know `sessions_dir`, so Prime (which needs it) gets no final attempt. The thread still covers Prime, now from 0.5 s.
- `cargo test -p horch-e2e` does not rebuild the `horch` binary. Run `cargo build --workspace --bins` first.
- The new test sits under the ARC-16 section header in the test file.
