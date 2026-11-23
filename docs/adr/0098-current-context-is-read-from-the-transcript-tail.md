# Current context is read from the transcript tail

`crates/horch-core/src/telemetry/context.rs` reads the current context of 1
session (design section 6.6, CTX-02 to CTX-04). Current context is the
harness's own comparison number from the newest completed response, never a
cumulative total (CTX-02). It is read from the tail of the main transcript
only, so a 160 MB transcript costs 1 window of 256 KiB (CTX-26).
Compaction markers can be anywhere in the file, so a small cache per
transcript, `<cache_dir>/markers/<16 hex>.json`, keeps the markers found and
the byte where the search stopped, and each read searches only the new
bytes.

| agent | usable line (the newest wins) | marker |
|---|---|---|
| claude | a main-chain `assistant` line with usage | `system` / `compact_boundary` |
| codex | an `event_msg` `token_count` with `info` | `compacted` |
| pi, prime | an assistant `message` that did not fail, with usage | `compaction` |
| opencode | the newest finished assistant row that is not a summary | a summary row |

## Consequences

- The marker cache is best effort. A read never fails, and never loses its
  reading, because the cache cannot be written (a sandboxed Codex worker).
  `context::save_cache` ignores write failures: a cache that cannot be
  written costs 1 re-scan.
- OpenCode keeps its sessions in a database, not a file. Each read queries
  it through `sqlite3`, and the summary rows are the markers, so it uses no
  marker cache.
- The OpenCode session id goes into the SQL text passed to `sqlite3`, so
  `context::is_opencode_id` accepts only `ses_` followed by ASCII letters
  and digits.
- `context::search` finds markers by plain byte search, and
  `context::confirm` accepts a hit only when the line's top-level fields
  say so. A needle inside a string is escaped, and one in a nested object
  fails this check.
- For Claude (`context::claude_usable`) the context is the input side plus
  the output of the last `message` iteration when the line has iterations,
  because an advisor call is not the context. Without iterations it is the
  line's usage.
- Codex names its model in the `turn_context` line before the usage. Claude,
  pi and Prime name it on the usable line, so `context::reads_turn_context`
  searches backwards only for Codex.
- `context::current_tokens` returns the figure and whether it is
  provisional. While a compaction is pending, only Claude has a figure: the
  post-compaction tokens it writes into its `compact_boundary` marker.
  Codex writes a new `token_count` at once, and pi and Prime a new
  response, so they have no marker figure.
