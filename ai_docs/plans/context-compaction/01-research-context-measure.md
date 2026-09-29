# Brief 01: measure the CURRENT context size of a session, per harness

## GOAL
Write `ai_docs/reports/context-compaction/01-context-measure.md`. It tells horch
exactly how to read the current context occupancy (tokens in the live window) of
a running session on each of the 5 harnesses, and how to detect that a
compaction happened.

## CONTEXT
Read `ai_docs/plans/context-compaction/00-shared-context.md` first.
horch will poll each session and act when its context crosses 300,000 tokens.
It needs the number that the harness itself would compare against its
auto-compact threshold, not the cumulative spend that `usage.rs` reads.
Brief 02 (compaction triggers) and brief 03 (horch/herdr integration) run in
parallel. Do not research their topics.

## FILES
- own: `ai_docs/reports/context-compaction/01-context-measure.md` (create it).
- do not touch: every other file. No source edits.

## PRIOR WORK
- `crates/horch-core/src/usage.rs` (read it, especially `read_claude`,
  `read_codex`, the pi reader, and the transcript finders at lines 197-260).
- `ai_docs/reports/telemetry-sources.md` sections C1, C3, X1, X2, O1, pi, Prime.
- Test fixtures: `crates/horch-core/tests/fixtures/usage/*.jsonl`.

## STEPS
For each harness (Claude Code, Codex, OpenCode, pi, Prime Agent):
1. Name the file or store that holds the live signal and how horch finds it
   from the ledger `session_id`. Check: cite the finder in `usage.rs` or the
   path pattern.
2. Give the exact formula for current context tokens. Examples to confirm or
   correct: Claude = last assistant message `usage.input_tokens +
   cache_read_input_tokens + cache_creation_input_tokens` (+ output_tokens?);
   Codex = `last_token_usage` in the newest `token_count` event, and the
   `model_context_window` field. Check: apply the formula to a REAL recent
   transcript on this machine and print the number. Show the command.
3. Say how a compaction appears in the transcript (Claude `compact_boundary`
   / `isCompactSummary` lines; Codex `compacted` items or events; pi
   compaction entries; OpenCode summary messages) and how the number reads
   right after it. Check: find one real compaction in a local transcript, or
   mark UNVERIFIED and cite source code.
4. Say how the harness exposes its context window size and auto-compact
   threshold, so horch can report "N of M".
5. Note stale-read hazards: streaming duplicates per `message.id`, sidechain
   or subagent lines, file rotation, a session that resumed into a new file.
6. Measure read cost: the time to read the tail of a 50 MB transcript. Recommend
   tail-reading (seek from end) if a full read is slow.

## CONSTRAINTS
- Read-only. You may run `jq`, `rg`, `sqlite3 -readonly`, `tail`.
- For OpenCode, open the SQLite database read-only. Do not write to it.
- Do not start any agent CLI session.

## DONE WHEN
- The report has one section per harness with: source, finder, formula,
  compaction marker, window/threshold source, hazards, and a verified example
  number (or UNVERIFIED with reason).
- The report ends with a summary table and a "Recommendation input" list.

## REPORT
- `horch note` at each harness finished.
- `horch tell orchestrator "QUESTION: ..."` if blocked. Continue other harnesses meanwhile.
- `horch done` summary: report path, per-harness formula in one line each,
  verified vs UNVERIFIED counts.
