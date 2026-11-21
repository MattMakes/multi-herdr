# Worker session ids are minted by horch or recovered after launch

A worker's resume handle is its harness session id. horch mints the id at
launch when the harness accepts one, and otherwise recovers it after launch
from the harness's own records for the workdir. pi's `--session-id` creates
the session if it does not exist, so horch mints the id and nothing is
discovered after launch: a fresh launch and a resume are the same flag with
a different id (`crates/horch-core/src/harness/pi.rs`). Prime has no
`--session-id` (verified against 0.9.4), so a fresh Prime launch must not
invent one. `horch worker` gives Prime a `--session-dir` that only this
launch writes, and the session file that appears there is this worker's,
with no timestamps or project paths to disambiguate. Codex, OpenCode and
Antigravity mint their own ids, and horch recovers them.

Because an id can appear only some time after launch, a ledger record is
created before the id exists. `LedgerRecordV1::session_id`
(`crates/horch-core/src/execution/legacy.rs`) is `null` until known and is
filled in later.

## Consequences

- `Ports::reading` in `crates/horch/src/cmd/compact.rs` reads the session
  id from the ledger again on every reading instead of trusting the value
  captured at launch.
- `discover_once` in `crates/horch-core/src/harness/launch.rs` uses herdr's
  native `agent_session_id` for the pane first, else the harness's records
  for the workdir, newest first. It skips ids already claimed by another
  ledger record: concurrently spawned workers in the same directory would
  otherwise record the same session.
- `discover_now` is the last discovery attempt. `horch done` runs it before
  it closes the pane, because the close kills the background discovery
  thread. It does nothing when the record already has a session id, the
  harness mints its id at launch, the launch was a resume, or the launch
  marker is gone. The marker holds the launch's `sessions_dir` (Prime's),
  so this attempt searches where the thread does.
- `Workdir` in `crates/horch-core/src/harness/mod.rs` compares session
  directories as written or after resolving symlinks. A herdr pane's cwd
  and the path an agent records can differ by `/var` versus `/private/var`
  (or `/tmp` versus `/private/tmp`) on macOS, and a string compare would
  silently find nothing.
- Codex ids are harvested from the rollout files codex writes
  (`crates/horch-core/src/harness/codex.rs`). `session_id_from_rollout`
  relies on codex naming them `rollout-<timestamp>-<uuid>.jsonl`. It
  returns `None` for any name that does not end in a well-formed
  36-character uuid after a dash, which stops a partially written or
  unrelated file from being recorded as a session.
- `find_rollouts` returns rollout files modified at or after `since` whose
  head names the project dir as its cwd, newest first. The cwd check keeps
  2 projects' concurrent codex sessions apart. Each `"cwd"` value in the
  first 16KB is compared canonically with `Workdir`, so `/var/...` finds a
  session recorded as `/private/var/...`.
- `find_launch_rollouts` matches the launch token only in an `event_msg`
  with payload type `user_message`. That event records the actual
  submitted prompt. Response items can include preceding AGENTS or
  environment context represented as user text.
- `rollout_candidate` accepts a rollout whose mtime equals the launch
  marker's (`>=`). Mtimes come from a coarse clock, so a rollout written
  in the marker's tick has the marker's mtime.
- OpenCode mints its own `ses_...` ids and offers no way to supply one.
  Unlike codex there are no rollout files to scan. `opencode session list`
  is the supported interface, it takes `--format json`, and every record
  carries the `directory` it belongs to and when it was `created`. That
  tells one worker's session from another's without reading OpenCode's
  database (`crates/horch-core/src/harness/opencode.rs`).
- `parse_sessions` moves its cut 1 s before the launch marker's mtime. The
  marker's timestamp has whole-second resolution on some filesystems while
  OpenCode records milliseconds, so a session created in the marker's
  second would otherwise be missed.
- The Antigravity CLI `agy` mints its own conversation ids, and
  `--conversation <id>` with an id it does not know silently starts a
  different conversation, so horch cannot choose one. The id is recovered
  after launch from `~/.gemini/antigravity-cli/cache/last_conversations.json`,
  which maps each workspace directory to its newest conversation id. A
  resume passes `--conversation <id>`
  (`crates/horch-core/src/harness/antigravity.rs`).
- That file has no per-entry time, so `find_session` uses its mtime: an
  entry written before this launch is another worker's, or an old one. It
  allows 1 s of slack, because the launch marker's mtime can be coarser
  than the cache's.
