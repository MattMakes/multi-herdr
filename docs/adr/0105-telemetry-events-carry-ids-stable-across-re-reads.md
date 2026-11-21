# Telemetry events carry ids stable across re-reads

`crates/horch-core/src/telemetry/readers.rs` holds 1 incremental reader per
harness (design section 7.1). A reader turns new input into `Observation`s:
usage events, and limit signals for the quota module. It copies usage
fields only. No message text, tool input or tool output ever leaves it
(TEL-11). Every event carries an id that is stable across re-reads, so
reading a file twice (a restart, a rotation, `horch cost` from offset 0)
yields the same events, and the store drops the repeats. Because re-reading
is always safe, `crates/horch-core/src/telemetry/cursor.rs` can start a
cursor over whenever it is unsure where it stands.

| agent | input | event id |
|---|---|---|
| claude | `<projects>/*/<sid>.jsonl` + `<sid>/subagents/**/*.jsonl` | `message.id`; a later repeat with more tokens adds a correction `<id>+<n>` |
| codex | `rollout-*-<sid>.jsonl` | `token_usage_record.response_id`; before 0.157, `tc:<total_tokens>` |
| pi, prime | the session `.jsonl` | the line's `id`, else `@<byte offset>` |
| opencode | `opencode.db` via `sqlite3 -readonly -json` | `message.id`; a changed message adds `<id>+<n>` |

## Consequences

- A cursor belongs to 1 input: a transcript file, or 1 OpenCode session in
  the database. Only complete lines (ending in `\n`) are consumed. A
  partial last line waits for the next tick.
- When the file's identity changes (a new inode: a rewrite or rotation) or
  it shrinks below the offset, the cursor starts over at 0 with fresh
  reader state.
- `cursor::file_identity` is `(dev, ino)` on Unix. Stable Rust has no file
  index on Windows, so there the creation time stands in for the inode.
- `cursor::RECENT_IDS` is 8: a cursor remembers only 8 recent Claude
  message ids. Claude writes 1 line per content block, so repeats of 1 id
  are adjacent and a handful is plenty.
- `Collector::tidy_cursors` keeps `cursors.json` small (NFR-02), since it is
  rewritten whole whenever an input moves. A cursor whose input file is
  gone is dropped. A cursor whose session has no live record and no record
  inside the retention window, and whose input has not changed for
  `QUIET_H` (24) hours, is dropped. A cursor whose records are all done,
  and whose input has not changed for `QUIET_H` hours, keeps only its
  newest `QUIET_RECENT_IDS` (4) Claude message ids: the recent ids were
  most of the file. Claude repeats a line only for the message in flight,
  so a later line can correct only the newest ids. An input that grows
  again is read again: a dropped cursor from the start, a tidied one from
  its offset. The store drops the events it already holds (section 8.2).
- For OpenCode the cursor keeps the newest `time_updated` seen and what each
  message has emitted so far, because a message row can be updated after
  it completes.
- `readers::claude_subagent_files` returns every `*.jsonl` under
  `<sid>/subagents/`, nested directories included, because Claude Code
  nests subagent transcripts and a flat glob misses them ([S] C1). The list
  is sorted, for a stable order.
- `readers::CODEX_RECORDS_SINCE` is 0.157.0, the first Codex version known
  to write `token_usage_record` lines ([S] X1: the type exists in 0.158.0
  source, and the operator saw it written by 0.157.1).
- Before `token_usage_record`, `readers::codex_fallback` takes the
  difference of consecutive `total_token_usage` running totals. Codex
  writes each total twice. The repeat differs by nothing and emits no
  event.
- `Store::append` drops an event older than the retention window instead
  of storing it. Its dedupe key may be gone with its old event file, so a
  transcript re-read from the start would add it again as new. The store
  never takes such an event back.
- `"antigravity"` has no reader in `readers::locate`: agy 1.2.17 keeps no
  local usage horch can read (U-60). Its state files are binary protobuf,
  and the `conversation_summaries.db` table has no token columns. Only
  `-p --output-format json` prints per-run tokens, and horch runs the TUI.
  Seen live: `docs/live-checks/harnesses.md`.
