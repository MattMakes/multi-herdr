# Report 01: measure the current context size of a session, per harness

Brief: `ai_docs/plans/context-compaction/01-research-context-measure.md`.
Author: researcher-4. Date: 2026-09-28. Read-only research; no source edits.

Installed versions (from `00-shared-context.md`, round 1): Claude Code 2.1.284,
Codex 0.157.1, OpenCode 1.18.2, pi 0.85.1, Prime Agent 0.9.4.

"Current context" in this report means the number that the harness itself
compares against its auto-compact threshold. It is NOT the cumulative spend that
`crates/horch-core/src/usage.rs` reads.

Common rule for all 5 harnesses: each harness computes current context as
**(usage of the last completed model response) + (an estimate of the messages
added after that response)**. A transcript reader sees only the first part.
So horch reads a number that lags the harness by at most 1 tool result or 1 user
message. For a 300,000 threshold with a 1,000,000 window this lag does not
matter. For Codex (window 272,000) it matters; see the Codex section.

---

## 1. Claude Code 2.1.284

### Source and finder
- File: `~/.claude/projects/<cwd-slug>/<session_id>.jsonl`.
- Finder: `find_claude_transcript` (`crates/horch-core/src/usage.rs:197-207`). It
  scans every project directory for `<session_id>.jsonl`.
- Subagents write separate files under
  `~/.claude/projects/<slug>/<session_id>/subagents/agent-<id>.jsonl`. The finder
  does not read them. This is correct: a subagent context is not the parent context.

### Formula
Take the last line with `type == "assistant"`, `isSidechain != true`,
`message.model != "<synthetic>"`, and a `message.usage` object. Then:

```
context = usage.input_tokens
        + usage.cache_creation_input_tokens
        + usage.cache_read_input_tokens
        + usage.output_tokens
```

CORRECTION to round 1: round 1 left out `output_tokens`. The auto-compact count
includes it. Evidence, binary strings of 2.1.284
(`/tmp/horch-compact-research/claude-2.1.284.txt`):
- `function Ox(e){let n=kTe(e);return hs(n)+n.output_tokens}`
- `function hs(e){return e.input_tokens+e.cache_creation_input_tokens+e.cache_read_input_tokens}`
- `function yy(e,n){let r=Xlt(e);if(!r)return Pm(e,n);return Ox(r.usage)+Pm(e.slice(r.anchorIndex+1),n)}`
  = usage of the last response (with output) + estimate (`Pm`) of later messages.
- `function Zie(e){... return s?Ox(s)>200000:!1}` also uses `Ox`.
- The statusLine and `/context` percentage use `hs` (no output):
  `function bmn(e,n){... let r=e.input_tokens+e.cache_creation_input_tokens+e.cache_read_input_tokens ...}`
  and `qEe` builds `context_window.total_input_tokens` from the same 3 fields.

Numeric check against the real auto-compaction in session `56c891bc` (see
below): last assistant usage = 4 + 2,929 + 988,927 = 991,860; plus
`output_tokens` 292 = 992,152. `compactMetadata.preTokens` = 993,784. The rest
(1,632) is the estimate for the tool result after that response. This matches `yy`.

`kTe(e)`: when `usage.iterations` exists (server-side context management), Claude
Code uses the last iteration that is not of type `compaction`
(`e.iterations.findLast((r)=>!pa(r))`). horch sees plain usage in all local
transcripts; treat `iterations` as an edge case (see hazards).

### Verified example
My own live session, command:

```
F=~/.claude/projects/-Users-mascott-projects-multi-herdr/0932f5dd-fdb4-43ca-862f-545464070ef7.jsonl
tail -c 1048576 "$F" | tail -n +2 | jq -c 'select(.type=="assistant" and .isSidechain!=true
  and .message.model!="<synthetic>" and .message.usage!=null) | .message.usage
  | {ctx:(.input_tokens+.cache_creation_input_tokens+.cache_read_input_tokens),
     with_out:(.input_tokens+.cache_creation_input_tokens+.cache_read_input_tokens+.output_tokens)}' | tail -1
```
Output: `{"ctx":77166,"with_out":77788}`. Round 1 also read session 7232efa9 = 43,395 (without output).

### Compaction marker (VERIFIED, round 1, re-checked)
File `~/.claude/projects/-Users-mascott-projects-multi-herdr/56c891bc-8693-44fc-896d-ce87e0b6f6fa.jsonl`:
- Line 388: `{"type":"system","subtype":"compact_boundary","compactMetadata":{"trigger":"auto","preTokens":993784,"postTokens":10486,"cumulativeDroppedTokens":983298,"durationMs":103587,"preservedSegment":{...},"preservedMessages":{...}}}`.
- Line 389: `type: "user"` with `isCompactSummary: true`.
- Reading right after: the last assistant line before line 388 reads 991,860 (992,152 with output). The first assistant line after line 389 reads 33,352.
- Between the boundary and the first new assistant response, the "last assistant" is still the pre-compaction one. horch must treat a `compact_boundary` line newer than the last assistant line as "compacted, number pending". It can use `compactMetadata.postTokens` (10,486 here) as the provisional value.
- `compactMetadata.trigger` is `auto` or `manual` (`/compact`). Only `auto` was seen locally; `manual` is UNVERIFIED.

### Window and threshold
- The transcript does not store the window size.
- statusLine stdin carries `context_window.context_window_size`, `used_percentage`, `remaining_percentage` (binary `qEe`; docs https://code.claude.com/docs/en/statusline). horch does not own the statusLine (the operator has one), so horch cannot read it without a wrapper.
- Default window for opus, sonnet and fable on this account: 1,000,000. Default trigger: 967,000. With `CLAUDE_CODE_AUTO_COMPACT_WINDOW=N` the trigger is N − 33,000 (source: `ai_docs/reports/env-research/compaction-benchmarks.md` section 2.2). horch sets the teammate env, so horch knows N when it sets it.
- Recommendation: horch derives M from the teammate env: `CLAUDE_CODE_AUTO_COMPACT_WINDOW` if set, else 1,000,000 (200,000 when `CLAUDE_CODE_DISABLE_1M_CONTEXT=1`).

### Hazards
- Streaming duplicates: 1 API response writes 1 line per content block, all with the same `message.id`. On 2.1.283 the repeated usage values are identical (`telemetry-sources.md` C1). The last line wins, so no deduplication is needed for a "last value" read.
- `<synthetic>` lines (Claude Code's own messages) carry zero usage. Skip them.
- Sidechain: in the 60 newest transcripts of this project, 0 lines have `"isSidechain":true` and 0 lines carry a foreign `sessionId`. Keep the `isSidechain != true` filter; it costs nothing.
- `usage.iterations` (server-side compaction): use the last non-`compaction` iteration. Not seen locally.
- Resume: in the 60 newest files, every `sessionId` field equals the file name. A resume into a new file was not observed. `/clear` and `--fork-session` start a new session id (UNVERIFIED on 2.1.284); after that the ledger id points at a file that stops growing. Guard: if the file mtime is older than the pane's last activity, report "stale".
- Lines up to 1,365,985 bytes exist (`~/.claude/projects/-Users-mascott-gateaccess-v1-gateaccess-ai-marketplace/b44bddb8-....jsonl`). A tail window must grow until it contains 1 complete assistant line.

---

## 2. Codex 0.157.1

### Source and finder
- File: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<ts>-<session_id>.jsonl`. horch gives each pane a private `CODEX_HOME` whose `sessions` symlinks to `~/.codex/sessions` (`crates/horch-core/src/codex.rs:10-16`).
- Finder: `find_codex_rollout` (`usage.rs:210-215`) via `Locations::from_env` (`usage.rs:467-477`).
- Codex subagents write their own rollout files. 25 of 353 September rollouts hold 2 `session_meta` lines: line 1 is the subagent (`source.subagent.thread_spawn.parent_thread_id`), line 2 is the copied parent meta. The finder matches the file name suffix, so it does not pick these files for the parent id.

### Formula
Take the last line with `type == "event_msg"`, `payload.type == "token_count"` and `payload.info != null`. Then:

```
context = payload.info.last_token_usage.total_tokens      (input incl. cached + output)
window  = payload.info.model_context_window               (258,400 for gpt-5.6-sol/terra)
```

Use `total_tokens`, not `input_tokens`. Right after a compaction Codex writes
`input_tokens: 0` with `total_tokens: 12847` (see below), so `input_tokens` reads 0.

`cached_input_tokens` is inside `input_tokens`; do not add it again (`usage.rs:361-362`).

Alternative line: `type == "token_usage_record"`, `payload.usage.total_tokens`. It is
written for every response, including the compaction request itself, and it is
sometimes the last line (`telemetry-sources.md` X1). Do not use it for context: the
compaction request's record reads the pre-compaction size (229,420 below).

### Verified example
```
F=~/.codex/sessions/2026/09/27/rollout-2026-09-27T17-37-46-01a0e572-451c-7b30-b9cc-53581b7f8772.jsonl
jq -c 'select(.type=="event_msg" and .payload.type=="token_count" and .payload.info!=null)
  | [input_line_number, .payload.info.last_token_usage.total_tokens,
     .payload.info.last_token_usage.input_tokens, .payload.info.model_context_window]' "$F" | tail -1
```
Output: `[505,78834,78699,258400]`. So 78,834 of 258,400. Second file
`rollout-2026-09-27T12-01-50-01a0e43e-...jsonl` (newest, 2,246,277 bytes): 129,933 of 258,400.

### Compaction marker (VERIFIED)
- 0.157.1 and 0.155.1: a top-level line `{"type":"compacted","payload":{"message":"","window_number":N,"window_id":...,"previous_window_id":...,"first_window_id":...,"compaction_response_id":...,"latest_token_usage_record":{...},"replacement_history":[...],"retained_context":...,"guardian_history":...}}`.
- 0.154.0 also wrote `{"type":"event_msg","payload":{"type":"context_compacted"}}` right after the `compacted` line in some files (e.g. `2026/09/16/rollout-2026-09-16T20-28-28-01a0ad68-9aea-....jsonl` lines 269 and 272). 0 of 23 compacted 0.155.1/0.157.1 files have it. Use `type == "compacted"` as the only marker.
- `replacement_history` holds the new context; its last item has `type: "compaction"`. The line can be large: up to 4,617,383 bytes in `2026/09/16/rollout-2026-09-16T20-23-48-01a0ad64-5312-....jsonl`.
- Example, `~/.codex/sessions/2026/09/26/rollout-2026-09-26T08-55-51-01a0de6e-14ae-7cf1-a456-a464abc5dfdc.jsonl`:
  - last `token_count` before: `last_token_usage.total_tokens` 227,306, window 258,400.
  - line 387 `compacted`, `window_number` 1, `latest_token_usage_record.usage.total_tokens` 229,420 (the compaction request).
  - first `token_count` after: `{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0,"total_tokens":12847}`, window 258,400.
  - So Codex writes a fresh estimate right after compaction. horch reads the correct small number at once.
- The machine has 159 `compacted` lines in 57 rollout files (0.154.0: 34 files, 0.155.1: 9, 0.157.1: 14). In a sample of 29 of them: the last `token_count` before each reads 207,876 to 248,144 (window 258,400); the first after reads 12,194 to 60,708. 2 compactions in `rollout-2026-09-10T23-23-55-...` ran with window 121,600 (a different model).

### Window and threshold
- `model_context_window` in every `token_count` = the effective window, 95% of the catalog window (272,000 × 0.95 = 258,400).
- Default auto-compact trigger: 90% of the catalog window = 244,800 (`compaction-benchmarks.md` 2.3, source `codex-rs/protocol/src/openai_models.rs` at `rust-v0.154.0`). The rollout does not record the trigger. `turn_context.payload` has no `model_context_window` or `auto_compact_token_limit` key (checked, line 389 of the example).
- horch knows the trigger only if it sets `model_auto_compact_token_limit`; else use 0.9 × 272,000 = 244,800, or `model_context_window / 0.95 × 0.9`.
- Observed pre-compaction readings below 244,800 (e.g. 207,876) show that Codex adds an estimate of items after the last response, or that some compactions were manual. Which one applies to each case is UNVERIFIED.
- Note for the 300,000 goal: Codex's default window (272,000) is below 300,000. A Codex worker auto-compacts before it reaches 300,000. A horch threshold for Codex must be lower than 244,800 or be a percentage.

### Hazards
- Tail reads MUST drop the first partial line. `jq` stops at the first parse error. Measured: `tail -c 2000000 <file> | jq ...` printed nothing; `tail -c 2000000 <file> | tail -n +2 | jq ...` printed `[129933,258400]`.
- A `compacted` line can be 4.6 MB, so a fixed 1 MiB tail can contain no `token_count` line. The reader must double the window until it finds 1 `token_count` line, or fall back to a full read.
- `token_count` with `info: null` exists (start of a session). Skip it.
- `state_5.sqlite` `threads.tokens_used` is cumulative, not current (`telemetry-sources.md` X2). Do not use it.
- `sqlite3 -readonly ~/.codex/state_5.sqlite` failed in this session with "unable to open database file" (sandbox). Not needed for this signal.
- Resume: UNVERIFIED whether `codex resume` appends to the same rollout file on 0.157.1. 0 of 353 September files have a file name id that differs from `session_meta.payload.id` on line 1.

---

## 3. OpenCode 1.18.2

### Source and finder
- Store: SQLite `~/.local/share/opencode/opencode.db`, WAL mode (`opencode.db-wal` present). `sqlite3 -readonly` reads while OpenCode writes.
- The ledger `session_id` is the `ses_...` id. horch recovers it with `opencode session list --format json` (`crates/horch-core/src/opencode.rs:26-40`). `usage.rs` does not read OpenCode (`usage.rs:13`, `Missing::NotRead`).
- Table `message`, column `data` (JSON). Index `message_session_time_created_id_idx` covers the lookup by session.
- Child sessions (the `task` tool) have their own `session.id` with `parent_id` set. Filtering by `session_id` excludes them.

### Formula
OpenCode's own check (bundle strings, `/tmp/r4-opencode.txt` extracted from `~/.opencode/bin/opencode`):
- `function YQ(Q){... if(Z.role==="assistant"&&Z.finish&&(!Y||Z.id>Y.id))Y=Z} ... return{user:$,assistant:K,finished:Y,tasks:W}}` - `finished` = the assistant message with the highest `id` that has `finish` set.
- `if(ie&&ie.summary!==!0&&(yield*r.isOverflow({tokens:ie.tokens,model:z}))){yield*r.create({sessionID:t,...,auto:!0});continue}` - the overflow check uses `finished.tokens`, and skips it when it is a summary message.
- `isOverflow`: `(e.tokens.total || e.tokens.input+e.tokens.output+e.tokens.cache.read+e.tokens.cache.write) >= Is(e)` (`compaction-benchmarks.md` 2.4).

So:
```
row     = last assistant message (by id) of the session with data.finish set
context = data.tokens.total, or else input + output + cache.read + cache.write
if row.data.summary == true -> context is "compacted, pending"
```

### Verified example
```
sqlite3 -readonly ~/.local/share/opencode/opencode.db "select m.id,
  json_extract(m.data,'$.finish'), json_extract(m.data,'$.summary'),
  json_extract(m.data,'$.tokens.total'),
  json_extract(m.data,'$.tokens.input')+json_extract(m.data,'$.tokens.output')
   +json_extract(m.data,'$.tokens.cache.read')+json_extract(m.data,'$.tokens.cache.write'),
  json_extract(m.data,'$.modelID')
  from message m where m.session_id='ses_f78cd915affe0REGAi8RNl9dHX'
  and json_extract(m.data,'$.role')='assistant' and json_extract(m.data,'$.finish') is not null
  order by m.id desc limit 1;"
```
Output: `msg_087326f73001zMpMOU7SDbRdK5|stop||17009|17009|big-pickle`. So 17,009 of 200,000 (trigger 140,000). Query time: 0.004 s.

### Compaction marker (UNVERIFIED locally; source cited)
- The local database has 0 compaction records: `part` types are only `patch` 6, `reasoning` 44, `step-finish` 52, `step-start` 52, `text` 43, `tool` 41; 0 messages with `summary=1` or `mode='compaction'`.
- Source (bundle strings): compaction creates a user message plus a part `{type:"compaction",auto:w.auto,overflow:w.overflow}`. Then it writes an assistant message `{role:"assistant",mode:"compaction",agent:"compaction",summary:!0,tokens:{output:0,input:0,reasoning:0,cache:{read:0,write:0}},...}`. Its tokens are filled when the summary request finishes, so they read the pre-compaction size.
- Reading right after: the newest finished assistant row is the summary row (`summary: true`, `mode: "compaction"`). Its tokens are NOT the new context. horch must report "compacted, pending" until a newer finished assistant row with `summary != true` exists. The detection query: `json_extract(data,'$.summary') = 1` or a `part` row with `json_extract(data,'$.type') = 'compaction'`.

### Window and threshold
- Model limits: `~/.cache/opencode/models.json` (`limit.context`, `limit.input`, `limit.output`), or a `provider.<p>.models.<id>.limit` override in `opencode.json` / `OPENCODE_CONFIG_CONTENT`.
- Trigger: `limit.input − (compaction.reserved ?? min(20000, maxOutput))` when `limit.input` exists, else `limit.context − min(limit.output, 32000)`. Values: big-pickle 140,000 of 200,000; nemotron-3.5-lightning-free 230,144 of 262,144; nemotron-3-ultra-free 968,000 of 1,000,000 (`compaction-benchmarks.md` 2.4).
- The model per message is in `data.modelID` / `data.providerID`, so horch can look up M per reading.

### Hazards
- In-progress rows: an assistant row exists before it finishes, with tokens 0. Require `data.finish` not null.
- Tool-output pruning (`compaction.prune`) marks old tool parts as compacted without a new summary message. The next finished message shows the smaller number. No marker needed.
- Same database for every OpenCode session on the machine. Always filter by `session_id`.
- `opencode export <id>` gives the same data but starts a process; the SQLite read is cheaper (0.004 s).

---

## 4. pi 0.85.1

### Source and finder
- File: `~/.pi/agent/sessions/--<path>--/<timestamp>_<uuid>.jsonl`, or `PI_CODING_AGENT_SESSION_DIR` (`usage.rs:470-473`). horch passes `--session-id` (`launch.rs:239-250`).
- Finder: `find_pi_session` (`usage.rs:218-227`), file name contains the id.

### Formula (source: installed bundle `.../pi-coding-agent/dist/bundle/chunks/chunk-JVUZSMYM.js`)
- `function calculateContextTokens(usage){return usage.totalTokens||usage.input+usage.output+usage.cacheRead+usage.cacheWrite}`
- `getAssistantUsage(msg)`: role `assistant`, `stopReason` not `aborted` and not `error`, and `calculateContextTokens(usage) > 0`.
- `estimateContextTokens(messages)` = usage of the last such assistant message + `estimateTokens2` of every later message.
- `shouldCompact(contextTokens,contextWindow,settings2){return settings2.enabled?contextTokens>contextWindow-settings2.reserveTokens:!1}`.
- `getContextUsage()`: if the current branch has a `compaction` entry and no valid assistant usage after it, it returns `{tokens:null,contextWindow,percent:null}`.

So:
```
entries = lines on the active branch (see hazards)
row     = last entry with type=="message", message.role=="assistant",
          message.stopReason not in {aborted, error}, and calculateContextTokens(usage) > 0
if a type=="compaction" entry is newer than row -> "compacted, pending"
context = message.usage.totalTokens, or input + output + cacheRead + cacheWrite
```

### Verified example
UNVERIFIED. `~/.pi/agent/sessions` does not exist on this machine
(`ls: /Users/mascott/.pi/agent/sessions: No such file or directory`). pi has never
saved a session here. The formula comes from the installed source above.

### Compaction marker (UNVERIFIED locally; source cited)
- Entry: `{"type":"compaction","id":...,"parentId":...,"timestamp":...,"summary":"...","firstKeptEntryId":"...","tokensBefore":50000}`; optional `usage` (the summary request) and `retainedTail` (`docs/session-format.md:234-240`; `docs/compaction.md:121-134`).
- Reading right after: `null` until the first valid assistant response after the entry. pi RPC docs say the same: "`contextUsage.tokens` and `contextUsage.percent` are `null` immediately after compaction until a fresh post-compaction assistant response provides valid usage data" (`docs/rpc.md:595`).

### Window and threshold
- Window: the model's `contextWindow` in `~/.pi/agent/models.json` (262,144 for `qwen3.8`; 128,000 for the other entry).
- Trigger: `contextWindow − compaction.reserveTokens` (default 16,384) = 245,760 for `qwen3.8`.
- Live push option: `get_session_stats` over RPC returns `contextUsage: {tokens, contextWindow, percent}` (`docs/rpc.md:554-595`). Extensions get `ctx.getContextUsage()` (`docs/extensions.md:1066`). horch launches the TUI, not RPC mode, so the file read is the available path.

### Hazards
- Tree: entries link by `id`/`parentId`; `/tree` creates branches in the same file (`docs/session-format.md:3`). The last line is on the active branch in normal use. After `/tree` navigation the last assistant line can belong to an abandoned branch. Exact fix: walk `parentId` from the newest entry. The `branch_summary` entry marks a branch switch.
- `ToolResultMessage` can carry `usage` for nested LLM work (`docs/session-format.md:92-101`). It is not an assistant message; pi ignores it for context. Filter on `role == "assistant"`.
- 1 line per completed message; streaming partials are not saved (`docs/session-format.md:120`). No duplicates.
- `stopReason: "error"` lines carry zero usage; skip them.

---

## 5. Prime Agent 0.9.4

### Source and finder
- File: `<session-dir>/<session-id>.jsonl`, flat. horch passes a private `--session-dir` per launch and stores the file path in the ledger (`crates/horch-core/src/prime.rs:17-22`).
- Finder: `find_pi_session` accepts a path directly (`usage.rs:218-222`).

### Formula
Same code as pi. `/opt/homebrew/lib/node_modules/prime-agent/dist/bundle/chunk-TOACIHN2.js`:
- `calculateContextTokens(usage)` returns `usage.totalTokens || usage.input + usage.output + usage.cacheRead + usage.cacheWrite`.
- `getContextUsage()` returns `{ tokens: null, contextWindow, percent: null }` after a compaction until a post-compaction assistant usage exists (same logic as pi, printed in full from the bundle).
- `shouldCompact(contextTokens, contextWindow, settings)` is present with the same shape.

So the pi formula applies without change.

### Verified example
UNVERIFIED with a real number. The 2 local files `~/.prime/agent/sessions/01a08728-....jsonl` and `01a087c7-....jsonl` each hold 1 assistant message with `stopReason: "error"` and `totalTokens: 0` (`model: claude-opus-4-7`). The formula excludes both, so the reading is "no value". No horch-launched Prime session file exists under `~/.local/state/horch`.

### Compaction marker (UNVERIFIED locally; source cited)
- `{"type":"compaction",...,"firstKeptEntryId":"...","tokensBefore":50000}` (`prime-agent/docs/session-format.md:238`), same as pi.

### Window and threshold
- Window: bundled model registry, `contextWindow: 1e6` for `anthropic/claude-opus-5`. Trigger: 1,000,000 − 16,384 = 983,616 (`compaction-benchmarks.md` 2.5). Setting: `compaction.reserveTokens` in `~/.prime/agent/settings.json` or `<project>/.prime/agent/settings.json`.

### Hazards
- Same as pi (tree branches, error lines, tool-result usage).

---

## 6. Read cost

Measured on this machine (Apple silicon, warm page cache). Commands:

```
# 50 MB Claude transcript (first 52,428,800 bytes of b44bddb8-....jsonl, 2,009 lines)
time cat /tmp/r4-50mb.jsonl > /dev/null                      # 0.005 s
time jq ... /tmp/r4-50mb.jsonl | tail -1                     # 0.210 s, full parse, prints 50559
time python3 (json.loads every line, keep last usage)        # 0.075 s
time tail -c 262144 ... | tail -n +2 | jq ... | tail -1      # 0.011 s

# Codex rollout, 159,961,105 bytes
time python3 (json.loads every line, keep last token_count)  # 0.308 s, prints (149947, 258400)
time rg '"type":"token_count"' F | tail -1 | jq ...          # 0.027 s
time tail -c 1048576 F | tail -n +2 | rg ... | tail -1 | jq  # 0.030 s
```

- A full parse of 50 MB costs 0.075 s to 0.21 s. A full parse of 160 MB costs 0.31 s. `usage.rs` `read_session` reads the whole file with `read_to_string` (`usage.rs:519`).
- Per poll, per session, a full read is acceptable at a poll interval of 30 s or more with fewer than 10 sessions. A tail read is 10x to 20x cheaper.
- Recommendation: tail-read. Seek to `len − W` with W = 256 KiB, drop the first partial line, scan backwards for the target line. If none is found, double W up to the file size. Codex `compacted` lines reach 4.6 MB and Claude lines reach 1.4 MB, so the doubling is required.
- Cheaper still: remember the last byte offset per file and parse only the new bytes on each poll (the file only grows). A shrink or a new inode means rotation; reset.
- OpenCode: 1 indexed SQLite query, 0.004 s.

---

## 7. Summary table

| harness | source | finder | current-context formula | compaction marker | window / trigger source | verified number |
|---|---|---|---|---|---|---|
| Claude Code | `~/.claude/projects/*/<sid>.jsonl` | `find_claude_transcript` `usage.rs:197` | last assistant (non-sidechain, non-synthetic) `input + cache_creation + cache_read + output` | `system` `subtype: compact_boundary` (+ `compactMetadata.preTokens/postTokens/trigger`), then user `isCompactSummary: true` | not in transcript; teammate env `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (trigger = N − 33,000), default 1,000,000 / 967,000 | VERIFIED 77,788 (0932f5dd); compaction VERIFIED (56c891bc) |
| Codex | `rollout-*-<sid>.jsonl` | `find_codex_rollout` `usage.rs:210` | last `event_msg` `token_count` `info.last_token_usage.total_tokens` | top-level `type: "compacted"` (0.154 also `event_msg` `context_compacted`) | `info.model_context_window` (258,400, effective); trigger 0.9 × 272,000 = 244,800 unless `model_auto_compact_token_limit` | VERIFIED 78,834 of 258,400; compaction VERIFIED (01a0de6e line 387: 227,306 → 12,847) |
| OpenCode | `~/.local/share/opencode/opencode.db` table `message` | ledger `ses_...` (`opencode.rs:26`) | last assistant row with `finish` set: `tokens.total` or `input + output + cache.read + cache.write` | assistant row `summary: true`, `mode: "compaction"`; part `type: "compaction"` | `~/.cache/opencode/models.json` limits + config overrides | VERIFIED 17,009 (big-pickle); compaction UNVERIFIED (source only) |
| pi | `~/.pi/agent/sessions/**/*<sid>*.jsonl` | `find_pi_session` `usage.rs:218` | last assistant (stopReason not error/aborted) `usage.totalTokens` or `input + output + cacheRead + cacheWrite` | entry `type: "compaction"` (`tokensBefore`, `firstKeptEntryId`); value `null` until next response | `~/.pi/agent/models.json` `contextWindow`; trigger = window − `reserveTokens` (16,384) | UNVERIFIED (no local sessions); compaction UNVERIFIED (source only) |
| Prime Agent | ledger stores the file path | `find_pi_session` (direct path) | as pi (same code, `chunk-TOACIHN2.js`) | as pi | bundled registry `contextWindow` 1,000,000; trigger 983,616 | UNVERIFIED (2 local files, both error with 0 tokens); compaction UNVERIFIED |

Counts: current-context number VERIFIED for 3 of 5 (Claude, Codex, OpenCode). Compaction marker VERIFIED for 2 of 5 (Claude, Codex).

## 8. Recommendation input

- Add a reader per harness that returns `{tokens: Option<u64>, window: Option<u64>, compacted_at: Option<marker>, pending: bool}`. `pending = true` when a compaction marker is newer than the last usable response. Do not reuse the cumulative `read_*` functions in `usage.rs`; they sum, and this needs "last".
- Claude: include `output_tokens`. It matches the harness's auto-compact count (`Ox`). The statusLine percentage leaves it out; the difference is 1 response's output.
- Codex: read `last_token_usage.total_tokens`, not `input_tokens` and not `token_usage_record`. The Codex default trigger (244,800) is below 300,000. A fixed 300,000 threshold never fires for Codex. Use a per-harness threshold or a percentage of the trigger.
- OpenCode: 1 SQLite query per poll, opened with `-readonly` (or `SQLITE_OPEN_READONLY`). Skip rows without `finish`. Treat `summary: true` as pending.
- pi and Prime: honor `stopReason` and the compaction entry exactly as `getContextUsage()` does. Follow `parentId` from the newest entry if `/tree` branching matters.
- Read method: tail-read with a growing window and drop the first partial line, or keep a per-file byte offset and parse only appended bytes. A full read costs up to 0.31 s for a 160 MB file.
- Window M for "N of M": Codex has it in the transcript. Claude, pi, Prime and OpenCode need it from config horch already controls (teammate env, models.json, bundled registry). Store M in the teammate/roster data, not in the reader.
- After horch triggers a compaction, confirm it by the marker (Claude `compact_boundary`, Codex `compacted`, OpenCode `summary` row, pi/Prime `compaction` entry), not by the number. The number is stale or null until the next response on Claude, OpenCode, pi and Prime. Codex writes a new `token_count` at once.
- Orchestrator self-watch: the orchestrator is `Session::Unmanaged` and has no ledger `session_id` (`00-shared-context.md`). The Claude finder needs that id. This overlaps with `ai_docs/designs/telemetry-and-balancing.md` (DRAFT); brief 03 owns it.

## 9. Seen in passing (outside scope, not fixed)

- `usage.rs:360` says Codex `token_count` events are "written twice each"; `telemetry-sources.md` X1 already reports this is false on 0.157.1.
- `read_codex` takes the last `token_count` and misses a final `token_usage_record` (`telemetry-sources.md` X1). Not re-checked here.
- `read_pi` counts every line with `usage`, including `stopReason: "error"` lines and `toolResult` usage, and does not count compaction `usage` stored on `compaction` entries. The cumulative total can be slightly off. Not verified on a live file.
