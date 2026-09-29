# Design: context watch and early compaction for the horch fleet

Status: ACCEPTED FOR IMPLEMENTATION (staff-engineer-1, 2026-09-28).
Amended 2026-09-28 (staff-engineer-2, brief
`ai_docs/plans/context-compaction/06-amend-note-warning.md`, approved by the
operator): the context warning in `horch note` (§5.4).
Plan: `ai_docs/plans/context-compaction/05-implementation.md`.
Brief: `ai_docs/plans/context-compaction/04-design-and-plan.md`.

This document is self-contained. A reader with only a clone of the repository
can implement and cloud-verify it. The research reports are cited as
`R01`, `R02`, `R03`:
- R01 = `ai_docs/reports/context-compaction/01-context-measure.md`
- R02 = `ai_docs/reports/context-compaction/02-compaction-triggers.md`
- R03 = `ai_docs/reports/context-compaction/03-horch-integration.md`

They are untracked today. Every fact the implementation needs is copied here.

Harness versions the facts apply to: Claude Code 2.1.284, Codex 0.157.1,
OpenCode 1.18.2, pi 0.85.1, Prime Agent 0.9.4. horch revision `a4dcee2`.

---

## 1. The request

The operator wants the fleet to compact earlier:
- The orchestrator watches the context size of every worker and of itself.
- When a session crosses 300,000 tokens, the orchestrator waits for a natural
  stopping point in that session's work, then compacts the session.
- Before the compaction, the session writes a "whats-next" handoff. In this
  repository that is the `handoff` skill (`skills/handoff/SKILL.md`).
- Compaction differs per harness: Claude Code, Codex, OpenCode, pi, Prime Agent.

## 2. Summary of the decisions

| # | Decision | One-line reason |
|---|---|---|
| D1 | New `horch context [--json] [--over] [--session <agent>:<id>]`. It reads current context per harness from the transcript tail, and lists every live worker and the orchestrator. | The data is already on disk; `usage.rs` finds each transcript. A tail read costs 0.03 s (R01 §6). |
| D2 | Threshold = `min(base, floor(0.8 × native trigger))`. `base` = teammate `compact_at`, else env `HORCH_COMPACT_AT`, else 300,000. No new backstop in v1. | Codex, pi and 2 OpenCode models auto-compact below 300,000 (R01 §2, R02 §6). The operator's own `CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000` already is a Claude backstop at 467,000. |
| D3 | The orchestrator polls with `horch context --over` at each checkpoint: after it handles a worker message, and before each spawn. No watcher process. Also, each `horch note` checks the calling worker's own context and prints a warning when it is over its threshold, once per compaction cycle (§5.4). | No new process lifecycle. The fleet rule forbids background agents. The output is 1 line when nothing is over. `horch note` never reaches the orchestrator, so a worker on a long quiet task is otherwise checked only by the native backstop, which writes no handoff. |
| D4 | The session decides its own stopping point. The orchestrator asks with `horch compact <role> --request`. The worker answers `[<role>] NOTE: COMPACT-READY <path>`. | Only the session knows if it is mid-edit or mid-test. herdr `idle` alone is not a stopping point (R03 §3). |
| D5 | Handoff path `ai_docs/handoffs/<role>-whats-next.md`. The skill default changes to this path. The worker records it with `horch note`. | The old default collides between workers in the shared tree (R03 §7). The ledger gets a pointer, not the content. |
| D6 | `horch compact <role>` starts a detached job. The job waits until the pane is idle, types the per-harness compact line, waits for the transcript marker, waits for idle again, then types a resume line that names the handoff file. | Codex rejects, OpenCode ends the turn, pi aborts when busy (R02 §2.2, §3.2, §4.2). No harness starts a new turn after a manual compaction (R02 recommendation 3). |
| D7 | The orchestrator compacts itself the same way: `horch compact orchestrator`, then it ends its turn. The detached job waits for idle, so the same path works for Claude and Codex. | Claude queues a self-sent `/compact`, but Codex rejects it (R02 §1.3, §2.3). Waiting for idle is correct for both. |
| D8 | Success = a compaction marker appended after the job's start offset. The context number is not the proof. | Claude, OpenCode, pi and Prime show a stale or null number until the next response (R01 §8). |
| D9 | New Rust: `horch-core/src/context.rs`, `horch/src/cmd/context.rs`, `horch/src/cmd/compact.rs`; small changes to `herdr.rs`, `ledger.rs`, `teammates.rs`, `prompts.rs`, `horch/src/cmd/messaging.rs`. New data file `teammates/_base/compaction.md` holds every message text. Prose: 1 new section in each fleet base. | Prompts are data; Rust only substitutes `{placeholders}`. |
| D10 | Out of scope: watcher daemon, orchestrator ledger record, new backstop levers, harness hooks and extensions, pi `/tree` branches, herdr sidebar metadata, the fixed `orchestration` recipe. | Keep v1 small. Each item is listed in §13 with its lever. |

---

## 3. Measurement (decision 1)

### 3.1 The reading

A reader returns one `Reading` per session:

```rust
pub struct Reading {
    pub tokens: Option<u64>,      // current context; None = unknown or pending
    pub pending: bool,            // a compaction marker is newer than the last usable response
    pub window: Option<u64>,      // model context window, if the transcript has it (Codex only)
    pub model: Option<String>,    // model named in the transcript, if any
    pub last_compaction: Option<Compaction>,
}
pub struct Compaction {
    pub at: String,               // ISO-8601 UTC, from the marker line
    pub trigger: Option<String>,  // "manual" | "auto" (Claude only)
    pub pre_tokens: Option<u64>,
    pub post_tokens: Option<u64>,
}
```

"Current context" is the number the harness compares with its own
auto-compact trigger (R01 intro). It is not the cumulative spend that
`crates/horch-core/src/usage.rs` reads for `horch cost`. The new readers do
not change `usage.rs`; they reuse its finders (`find_claude_transcript`,
`find_codex_rollout`, `find_pi_session`) and `Locations::from_env`.

A transcript reader sees only the last completed response. The harness also
adds an estimate for later messages. So horch lags by at most 1 tool result or
1 user message (R01 intro). The 0.8 headroom in §4 absorbs this lag.

### 3.2 Claude Code

- File: `$HOME/.claude/projects/<any-dir>/<session_id>.jsonl`. Finder:
  `usage::find_claude_transcript(home, sid)`.
- A usable line has all of: `type == "assistant"`, `isSidechain != true`,
  `message.model != "<synthetic>"`, `message.usage` is an object, and
  `input_tokens + cache_creation_input_tokens + cache_read_input_tokens > 0`.
  Missing numeric fields count as 0.
- Formula for the LAST usable line (R01 §1, binary functions `Ox`, `hs`, `kTe`):

  ```
  u = message.usage
  if u.iterations is a non-empty array:
      it = the last element of u.iterations whose "type" is "message"
           (an element with no "type" also counts as "message")
      if it exists and it.input_tokens + it.cache_creation_input_tokens
                      + it.cache_read_input_tokens > 0:
          u = it
  tokens = u.input_tokens + u.cache_creation_input_tokens
         + u.cache_read_input_tokens + u.output_tokens
  ```

  NEW FACT (not in R01): usage lines with `iterations` exist in local
  transcripts. In session `56c891bc-8693-44fc-896d-ce87e0b6f6fa`, 41 of 446
  assistant lines have 3 iterations: `message`, `advisor_message`, `message`.
  The top-level usage is then the SUM of both `message` iterations. Example:
  top level `4 / 2351 / 457723 / 180`; iterations `message 2/433/228645/101`,
  `advisor_message 231470/0/0/1814`, `message 2/1918/229078/79`. Claude Code
  2.1.284 `kTe` takes the last non-advisor iteration:
  `let s=e.iterations.findLast((r)=>!pa(r)); if(!ma(s))return n; return {...s}`.
  So the correct reading is 2 + 1918 + 229078 + 79 = 231,077, not 460,258.
  The exact `pa` predicate is minified; this design treats every type other
  than `message` as skipped. The operator's settings now set
  `CLAUDE_CODE_DISABLE_ADVISOR_TOOL=1`, so new transcripts rarely have it.
- `output_tokens` is included (R01 §1 correction: auto-compact uses `Ox`,
  which adds output).
- Streaming writes 1 line per content block with the same `message.id` and the
  same usage. The last line wins; no deduplication is needed (R01 §1 hazards).
- `message.model` gives the model (for example `claude-opus-5-5`).
- Compaction marker (R01 §1, verified): a line
  `{"type":"system","subtype":"compact_boundary","timestamp":"2026-09-07T21:42:04.545Z","compactMetadata":{"trigger":"auto","preTokens":993784,"postTokens":10486,...},...}`,
  then a line `{"type":"user","isCompactSummary":true,...}`.
  `trigger` is `manual` after `/compact` (R02 live test A: `trigger manual,
  preTokens 34981, postTokens 6136`).
- Pending: a `compact_boundary` line is after the last usable line. Then
  `tokens = compactMetadata.postTokens` (a provisional value) and
  `pending = true`. The first usable line after the marker ends the pending
  state. Verified pair: 991,860 before, 33,352 after (R01 §1).
- `last_compaction`: `at = timestamp`, `trigger`, `pre_tokens = preTokens`,
  `post_tokens = postTokens` of the newest `compact_boundary` line.
- Compaction keeps the same session file (the marker is inside it). herdr may
  report a new session id after `/clear`, not after `/compact` (R03 §1).

### 3.3 Codex

- File: `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<ts>-<session_id>.jsonl`.
  `CODEX_HOME` defaults to `$HOME/.codex`. Worker panes use a private
  `CODEX_HOME` whose `sessions` is a symlink to `~/.codex/sessions`
  (`crates/horch-core/src/codex.rs:10-16`). Finder:
  `usage::find_codex_rollout(loc.codex_sessions, sid)`.
- A usable line: `type == "event_msg"`, `payload.type == "token_count"`,
  `payload.info != null`.
- `tokens = payload.info.last_token_usage.total_tokens`.
  `window = payload.info.model_context_window` (258,400 for gpt-5.6-sol and
  gpt-5.6-terra; this is 95% of the 272,000 catalog window). Do not use
  `input_tokens`: right after a compaction Codex writes `input_tokens: 0,
  total_tokens: 12847`. Do not use `token_usage_record` lines: the compaction
  request's record shows the pre-compaction size (R01 §2).
- Model: Codex has no per-response model field that this design relies on.
  Use the ledger record's model, or the teammate's model.
- Compaction marker (R01 §2, verified, 0.155.1 and 0.157.1): a top-level line
  `{"timestamp":"2026-09-26T16:06:42.431Z","type":"compacted","payload":{"message":"","window_number":1,"latest_token_usage_record":{"usage":{"total_tokens":229420,...}},"replacement_history":[...],...}}`.
  Codex 0.154.0 also wrote `event_msg` `context_compacted`; ignore it.
  `type == "compacted"` is the only marker.
- A `compacted` line can be 4,617,383 bytes (R01 §2 hazards).
- After a compaction Codex writes a new `token_count` at once. Verified pair:
  227,306 before; `compacted`; then `total_tokens` 12,847 (R01 §2).
- Pending: a `compacted` line after the last usable line → `tokens = None`,
  `pending = true`. This state is short.
- `last_compaction`: `at = timestamp`, `trigger = None`,
  `pre_tokens = payload.latest_token_usage_record.usage.total_tokens`,
  `post_tokens` = `total_tokens` of the first usable line after the marker, or
  `None`.

### 3.4 pi and Prime Agent (same session format)

- pi file: `$PI_CODING_AGENT_SESSION_DIR`, else
  `$HOME/.pi/agent/sessions/--<cwd-slug>--/<timestamp>_<uuid>.jsonl`. Finder:
  `usage::find_pi_session(loc.pi_sessions, sid)` (file name contains the id).
- Prime file: the ledger stores the session FILE PATH as the session id
  (`crates/horch-core/src/prime.rs:17-22`). `find_pi_session` accepts a path.
- A usable line (R01 §4, pi source `calculateContextTokens`,
  `getAssistantUsage`): `type == "message"`, `message.role == "assistant"`,
  `message.stopReason` is not `"aborted"` and not `"error"`, and
  `ctx(usage) > 0`, where
  `ctx(usage) = usage.totalTokens if > 0, else usage.input + usage.output + usage.cacheRead + usage.cacheWrite`.
  Lines with `role == "toolResult"` can carry `usage`; skip them.
- `tokens = ctx(message.usage)` of the last usable line.
- Model: `message.provider + "/" + message.model` when both exist (for example
  `ollama/qwen3.8`, `anthropic/claude-opus-5-5`).
- Compaction marker: `{"type":"compaction","id":"...","parentId":"...","timestamp":"2026-09-20T12:00:00.000Z","summary":"...","firstKeptEntryId":"...","tokensBefore":50000}`
  (pi `docs/session-format.md:234-240`; Prime `docs/session-format.md:238`).
- Pending: a `compaction` line after the last usable line → `tokens = None`,
  `pending = true`. This is how pi and Prime report it themselves:
  `getContextUsage()` returns `tokens: null` until the next response
  (pi `docs/rpc.md:595`). null means "just compacted", never 0 tokens.
- `last_compaction`: `at = timestamp`, `pre_tokens = tokensBefore`,
  `post_tokens` = ctx of the first usable line after it, or `None`.
- Limitation: pi `/tree` can leave the last line on an abandoned branch. v1
  reads file order and does not walk `parentId` (§13).
- Evidence level: pi and Prime formulas come from the installed source; no
  real non-error session exists on the operator's Mac (R01 §4, §5).

### 3.5 OpenCode

- Store: SQLite `${XDG_DATA_HOME:-$HOME/.local/share}/opencode/opencode.db`,
  WAL mode. horch has no SQLite crate. The reader shells out:

  ```
  sqlite3 -readonly -json "$DB" \
    "select id, data from message where session_id = '<sid>' order by id desc limit 50"
  sqlite3 -readonly -json "$DB" \
    "select id, data from message where session_id = '<sid>'
       and json_extract(data,'$.summary') = 1 order by id desc limit 1"
  ```

  `<sid>` must match `^ses_[A-Za-z0-9]+$`; refuse anything else (no SQL
  injection). `-json` prints `[{"id":"msg_...","data":"<json text>"}]`; `data`
  is a string that holds JSON. Missing `sqlite3` → state `not-read` with the
  note `sqlite3 not on PATH`.
- Row data fields (R01 §3): `role`, `finish`, `summary`, `mode`,
  `tokens {total, input, output, reasoning, cache {read, write}}`, `modelID`,
  `providerID`, `time {created}` (milliseconds since epoch).
- A usable row: `role == "assistant"`, `finish` is not null, `summary != true`.
- `tokens = data.tokens.total if > 0, else input + output + cache.read + cache.write`
  of the newest usable row (rows are ordered by `id` descending; OpenCode ids
  sort by time).
- Model: `providerID + "/" + modelID` (for example `opencode/big-pickle`).
- Compaction marker: an assistant row with `summary == true` (and
  `mode == "compaction"`). Its tokens are the summary request, so they show
  the pre-compaction size (R01 §3).
- Pending: the newest row with `role == "assistant"` and `finish` set is a
  summary row → `tokens = None`, `pending = true`.
- `last_compaction`: from the second query; `at` = `time.created` as ISO UTC,
  `pre_tokens` = its tokens, `post_tokens = None`.
- Evidence level: the formula is verified (17,009 read in 0.004 s); the
  compaction marker is source-only (R01 §3).

### 3.6 Reading a large file

Transcripts reach 160 MB; single lines reach 4.6 MB (R01 §6). The reader must
not load the whole file for the current number.

`scan_tail(path, want)`:
1. `W = 256 KiB`. `start = len - W` (or 0).
2. Read bytes `[start, len)`. If `start > 0`, drop every byte up to and
   including the first `\n` (the first line is partial).
3. Parse lines from the last to the first. Skip a line that is not valid JSON
   (the harness may be writing it). Stop at the first usable line. Remember
   every marker line seen before it (they are newer than it).
4. If no usable line was found and `start > 0`, double `W` and go to 2.

`last_marker(path, needle, is_marker)`: search backward in 4 MiB chunks
(overlap `needle.len()`) with `memchr::memmem::rfind` for the needle. For each
hit, find the line bounds, parse the line, and confirm the TOP-LEVEL fields
with `is_marker`. A needle inside a JSON string is escaped (`\"type\"`), so it
does not match; a nested object can, so the confirm step is required.
Needles: Claude `"subtype":"compact_boundary"`, Codex `"type":"compacted"`,
pi and Prime `"type":"compaction"`. The worst case (no marker) reads the whole
file once without parsing it.

`memchr` 2.x is already in `Cargo.lock` as a transitive dependency; add
`memchr = "2"` to `crates/horch-core/Cargo.toml`.

### 3.7 The command

```
horch context [--json] [--over] [--session <agent>:<id-or-path>]...
```

Rows, in this order:
1. `orchestrator`: found through the mailbox and herdr (§3.8). Skipped, with 1
   stderr line, when herdr is not reachable or the pane has no
   `agent_session`.
2. Every ledger record with `status == "working"`, one per role (the newest
   `updated_at`, the same selection as `Ledger::assign`,
   `crates/horch-core/src/ledger.rs:371-393`).
3. Every `--session`, in order, with role `session-1`, `session-2`, ...
   Agent is one of `claude`, `codex`, `pi`, `prime`, `opencode`.

`--over` prints only rows with state `over` or `requested`. When there are
none, it prints exactly `no session is over its threshold` and exits 0.

Text columns:

```
ROLE          HARNESS   MODEL                 CONTEXT    WINDOW  PCT  THRESHOLD  LAST COMPACT          STATE
orchestrator  claude    claude-fable-5-1      312,004 1,000,000  31%    300,000  -                     over
sonnet-1      claude    claude-sonnet-5        10,486*1,000,000   1%    300,000  2026-09-28T10:05:00Z  pending
codex-sol-1   codex     gpt-5.6-sol           201,113   258,400  78%    195,840  -                     requested
```

`*` marks a provisional (pending) number. `-` marks an unknown value.

`--json` prints an array. Each element has exactly these keys, in this order:

```json
{
  "role": "sonnet-1",
  "record_id": "r-...",          // null for orchestrator and --session rows
  "harness": "claude",
  "model": "claude-sonnet-5",    // transcript model, else ledger/teammate model, else null
  "session_id": "…",
  "transcript": "/abs/path.jsonl", // null when not found; the DB path for opencode
  "pane": "w2E:p3",              // null when unknown
  "pane_status": "idle",         // herdr agent_status, null when unknown
  "tokens": 10486,               // null when unknown or pending without a provisional value
  "pending": true,
  "window": 1000000,             // null when unknown
  "percent": 1.0,                // round(tokens*1000/window)/10; null when either is null
  "threshold": 300000,
  "native_trigger": 467000,      // null when unknown
  "last_compaction": {"at": "…", "trigger": "manual", "pre_tokens": 312857, "post_tokens": 10486},
  "state": "pending",
  "handoff": "ai_docs/handoffs/sonnet-1-whats-next.md"
}
```

States, first match wins:

| state | condition |
|---|---|
| `no-session` | no session id (a Codex/OpenCode/Prime id is harvested up to 180 s after launch, R03 §2) |
| `not-read` | harness is not one of the 5, or OpenCode without `sqlite3` |
| `no-transcript` | the finder found no file |
| `compacting` | a live job file exists for this role (§6.4) |
| `pending` | `pending == true` |
| `unknown` | `tokens == None` |
| `requested` | tokens ≥ threshold, and the record is already asked: it has a `compact-requested` or `context-warned` event in the current compaction cycle (§5.4 rule A) |
| `over` | tokens ≥ threshold |
| `ok` | otherwise |

### 3.8 Finding the orchestrator

The orchestrator is not a ledger record (`Session::Unmanaged`,
`crates/horch/src/cmd/recipes.rs:422-428`). Its pane has no `HORCH_ROLE` or
`HORCH_RECORD_ID`, so `horch note` and `horch done` fail there (R03 §1). This
design does not add a record (§13). It uses herdr instead (R03 §1 option A,
verified live):

1. `Mailbox::resolve(&herdr)` then `mailbox.pane_for("orchestrator")`
   (`crates/horch-core/src/mailbox.rs:159-163`).
2. `herdr.pane_get(pane)` returns `agent_session`, for example
   `{"agent":"claude","kind":"id","source":"herdr:claude","value":"0cdde350-..."}`.
   `Pane::agent_session_id()` (`crates/horch-core/src/herdr.rs:48-60`)
   already reads the id. The harness is `agent_session.agent` (`claude` or
   `codex`). If that key is missing, try the Claude finder, then the Codex
   finder.
3. Teammate for limits: `orchestrator` for `claude`, `orchestrator-codex` for
   `codex`.

This needs the herdr claude/codex integrations (installed: `claude: current
(v8)`, `codex: current (v8)`, R03 §1).

---

## 4. Threshold and backstop (decision 2)

### 4.1 Rule

```
base      = teammate.compact_at            (new optional frontmatter field, u64)
          else env HORCH_COMPACT_AT        (u64, read by the horch process)
          else 300_000
threshold = if native_trigger is known: min(base, native_trigger * 8 / 10)
            else base
```

Reason for the 0.8 factor: between the check and the compaction, the session
finishes its step and writes the handoff. That costs 10,000 to 40,000 tokens.
The orchestrator also checks only at its checkpoints. On Codex 20% is 48,960
tokens of headroom. A fixed 300,000 never fires on Codex, because Codex
compacts itself at 244,800 (R01 §2). A lower fixed number would compact
1M-window sessions too early. `min` keeps 300,000 wherever it fits.

Reason for no override of the cap: a `compact_at` above the native trigger can
never fire. `horch teammates --check` rejects a `compact_at` below 50,000 or
above 1,000,000.

### 4.2 Window and native trigger per harness

`limits(harness, model, teammate) -> (window, native_trigger)`:

| harness | window | native trigger | source |
|---|---|---|---|
| claude | 200,000 if the model name contains `haiku` or `CLAUDE_CODE_DISABLE_1M_CONTEXT=1`; else 1,000,000 | `min(W, window) - 33,000` where `W` = the auto-compact window (below); `window - 33,000` when `W` is unset | R01 §1; R02 §1.4 (trigger = W − min(max_output, 20,000) − 13,000) |
| codex | `model_context_window` from the transcript, else 258,400 | `window * 18 / 19` (= 90% of the catalog window; 258,400 → 244,800); then `min` with `model_auto_compact_token_limit` if the teammate `args` pass `-c model_auto_compact_token_limit=N` | R01 §2; R02 §2.4 |
| opencode | table below | table below | R01 §3; R02 §3.4 |
| pi | table below | `window - 16,384` | R01 §4 |
| prime | table below | `window - 16,384` | R01 §5 |

Claude `W` lookup, first hit wins. Each source is a string holding an integer
in 100,000..=1,000,000:
1. The teammate `settings` overlay JSON, key `env.CLAUDE_CODE_AUTO_COMPACT_WINDOW`.
2. If the teammate's `setting_sources` is unset or contains `user`:
   `$HOME/.claude/settings.json`, key `env.CLAUDE_CODE_AUTO_COMPACT_WINDOW`.
3. The teammate `env` map.
4. The horch process environment.

The order assumes Claude applies a settings `env` block over the process
environment. That is UNVERIFIED; local check L10 confirms it.

Model table (`const LIMITS`, in `context.rs`; data, like the price table in
`usage.rs`):

| model key | window | native trigger |
|---|---|---|
| `opencode/big-pickle` | 200,000 | 140,000 |
| `opencode/nemotron-3.5-lightning-free` | 262,144 | 230,144 |
| `opencode/nemotron-3-ultra-free` | 1,000,000 | 968,000 |
| `ollama/qwen3.8` | 262,144 | 245,760 |
| `anthropic/claude-opus-5-5` | 1,000,000 | 983,616 |
| `anthropic/claude-opus-5` | 1,000,000 | 983,616 |

A model that is not in the table (and is not Claude or Codex) has window and
native trigger `None`, so its threshold is `base`. The Prime `claude-opus-5-5`
window is inferred from `claude-opus-5` (R01 §5); UNVERIFIED.

### 4.3 Resulting thresholds

| session | native trigger | threshold |
|---|---|---|
| Claude opus/sonnet/fable, operator setting W = 500,000 (today) | 467,000 | 300,000 |
| Claude, no W | 967,000 | 300,000 |
| Claude haiku | 167,000 | 133,600 |
| Codex gpt-5.6-sol / terra | 244,800 | 195,840 |
| pi `ollama/qwen3.8` | 245,760 | 196,608 |
| Prime `anthropic/claude-opus-5-5` | 983,616 | 300,000 |
| OpenCode big-pickle | 140,000 | 112,000 |
| OpenCode nemotron-3.5-lightning-free | 230,144 | 184,115 |
| OpenCode nemotron-3-ultra-free | 968,000 | 300,000 |

### 4.4 Backstop per harness

A backstop is the harness's own auto-compaction, which fires if horch's flow
does not. It must fire after horch's threshold.

| harness | backstop in v1 | value | reason |
|---|---|---|---|
| Claude | the operator's existing `~/.claude/settings.json` `env.CLAUDE_CODE_AUTO_COMPACT_WINDOW = "500000"` | 467,000 | Already above 300,000 and far below 967,000. The operator tuned it; horch does not override it (memory: never drop the operator's tuned settings). No teammate sets `setting_sources`, so every Claude pane gets it. |
| Codex | native | 244,800 | Already 25% above the horch threshold. `model_auto_compact_token_limit` can only lower it (R02 §2.4). |
| pi | native | 245,760 | Already above 196,608. |
| OpenCode lightning, big-pickle | native | 230,144 / 140,000 | Already above the horch threshold. |
| OpenCode ultra | native | 968,000 | No change in v1. Lever for v2: `OPENCODE_CONFIG_CONTENT` `{"provider":{"opencode":{"models":{"nemotron-3-ultra-free":{"limit":{"context":432000,"output":128000}}}}}}` gives 400,000 (R02 §3.4, arithmetic only). It also changes the window OpenCode believes in. |
| Prime | native | 983,616 | No change in v1. Levers for v2: `models.json` `modelOverrides` `contextWindow`, or `compaction.reserveTokens` (R02 §5.4, both UNVERIFIED). |

If the operator removes the Claude setting, the Claude backstop becomes
967,000, and the horch flow still fires at 300,000. A future Claude backstop
at 400,000 is the teammate `settings:` overlay
`{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"433000"}}` (433,000 − 33,000).
Do not use `DISABLE_COMPACT`: it also disables `/compact` (R02 §1.4).

---

## 5. Watch loop and stopping point (decisions 3 and 4)

### 5.1 Who polls, and when

The orchestrator runs `horch context --over` at each checkpoint:
- after it handles any worker message (`DONE:`, `NOTE:`, `QUESTION:`,
  `BLOCKED:`, `ready`);
- before each `horch spawn`.

Reasons:
- Context grows only while a session works, and working workers send messages
  often. The checkpoints follow the growth.
- A watcher process needs a start, a stop, a lock, and a crash story. The
  fleet rules forbid background agents; a plain process is allowed but is
  not needed for v1.
- The call is cheap: about 0.03 s per session (R01 §6), and 1 line of output
  when nothing is over.
- A worker on a long step that sends no message is not seen at these
  checkpoints. `horch note` covers it: each note checks the worker's own
  context and warns the worker when it is over (§5.4). A worker that sends no
  message and runs no `horch note` is covered only by the native backstop
  (§4.4), which writes no handoff.

The worker-side check in `horch note` is an addition, not a replacement. The
orchestrator keeps every checkpoint above. `horch note` does not work in the
orchestrator pane (no `HORCH_RECORD_ID`, §3.8), so the orchestrator checks
itself only with `horch context --over` at its checkpoints. No change there.

### 5.2 Worker flow

1. `horch context --over` lists `sonnet-1 ... over`.
2. The orchestrator runs `horch compact sonnet-1 --request`. horch types the
   `request` message from `_base/compaction.md` into the worker pane and
   records a ledger event `compact-requested`. The row then shows
   `requested`, so the orchestrator does not ask twice.
3. The worker reaches its next stopping point. A worker stopping point is:
   - a plan step is finished and its check has run; or
   - it has just run `horch note`; or
   - it waits for an answer from the orchestrator.
   It is never mid-edit, and never during a build or test run.
   The worker decides, because only it knows. If it waits for an answer when
   the request arrives, it is at a stopping point now.
4. The worker loads `horch:handoff` and writes
   `ai_docs/handoffs/sonnet-1-whats-next.md`.
5. The worker runs `horch note "handoff: ai_docs/handoffs/sonnet-1-whats-next.md"`.
6. The worker sends
   `horch tell orchestrator "[sonnet-1] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-1-whats-next.md"`
   and ends its turn. `NOTE:` keeps the STE keyword list unchanged.
7. The orchestrator runs `horch compact sonnet-1`. It returns at once (§6).
8. horch compacts the pane, types the resume message, and reports
   `[horch] NOTE: sonnet-1 compacted. Context 312857 -> 18207 tokens. Handoff: ai_docs/handoffs/sonnet-1-whats-next.md.`

Self-warning path (no `--request`): steps 1 and 2 are replaced by a
`horch note` call of the worker that prints the `warning` message (§5.4).
That `horch note` call is the stopping point of step 3, so the worker goes on
at once with steps 4, 5 and 6. The orchestrator gets the same
`[sonnet-1] NOTE: COMPACT-READY <path>` line and runs step 7 exactly as
above. Its prose (§8.3, step 2 of the orchestrator list) already handles a
`COMPACT-READY` line that no `--request` caused. The self-warning path saves
the round trip through the orchestrator.

### 5.3 Orchestrator stopping point

The orchestrator is at a stopping point when:
- it has answered every pending worker message; and
- no spawn is half-done (every plan file it wrote is spawned or dropped); and
- it is not in the middle of verifying a `DONE:` (step 7 of its core loop).

### 5.4 Context warning in `horch note`

`horch note <text>` runs in a worker pane. Today it appends a `note` event to
the worker's ledger record (`crates/horch/src/cmd/messaging.rs:71`, wired at
`crates/horch/src/main.rs:304`) and prints nothing. It never reaches the
orchestrator. A `horch note` call is already a worker stopping point (§5.2
step 3). So `horch note` also checks the worker's own context.

Order of operations in `messaging::note`:
1. Record the note exactly as today. An error here is returned exactly as
   today. The note is always recorded before the check.
2. Run the check (below). The check returns `Option<String>`. Any error in the
   check becomes `None`.
3. If the check returns a line, append the ledger event `context-warned` with
   text `tokens <n> threshold <t>` (§7). Ignore an error of this write.
   Then print the line to stdout, followed by 1 newline.
4. Exit 0.

The check:
1. Read the ledger record for `HORCH_RECORD_ID` (`Ledger::read`, match on
   `record_id`). Use its `role`, `agent`, `tier` (the teammate name), `model`
   and `history`. Skip (no warning) when the record is missing or its
   `status` is not `working`.
2. Session id: the record's `session_id`, else env `HORCH_SESSION_ID` when it
   is not empty. A Codex, OpenCode or Prime id is harvested after launch
   (R03 §2), so the record is the better source. No id → no warning.
3. Compute the row for this record with the same function that
   `horch context` uses for a ledger row (§3.7): harness from `agent`,
   `context::read_session`, `context::limits`, and `context::threshold` with
   `base` = teammate `compact_at`, else env `HORCH_COMPACT_AT`, else
   300,000 (§4.1). The teammate comes from `Roster::load()` and the record's
   `tier`. So the number and the threshold are the same as in the
   `horch context` row, when both processes see the same `HORCH_COMPACT_AT`
   and the same roster (§12).
4. Warn only when all of these are true:
   - `tokens` is known, `pending` is false, and `tokens >= threshold`;
   - rule A below says the record is not already asked.
5. The line is the `warning` message of `_base/compaction.md` (§8.2),
   rendered with `compaction_message` and the placeholders `{role}`,
   `{tokens}`, `{threshold}` and `{handoff}`. `{handoff}` is
   `ai_docs/handoffs/<role>-whats-next.md` (§6.1). Rust adds no text.

Rule A (repeat control), shared by `horch note` and the `requested` state of
`horch context` (§3.7):
- `since` = the later of the `at` of the newest `compacted` event of the
  record, and `last_compaction.at` of the current `Reading`. Either can be
  absent. Parse both as UTC times with `chrono` before the comparison. Do not
  compare them as strings: the ledger writes `2026-09-20T10:05:00Z` and the
  transcripts write `2026-09-20T10:05:00.000Z`.
- The record is already asked when it has a `compact-requested` or
  `context-warned` event whose `at` is at or after `since` (any such event
  when `since` is absent).

Why this rule:
- At most 1 warning per compaction cycle. The `horch note "handoff: <path>"`
  call of §5.2 step 5 comes after the `context-warned` event, so it prints no
  second warning.
- No warning after the orchestrator asked with `--request`. The worker
  already has the same instructions.
- A new cycle starts after every compaction. A horch compaction writes a
  `compacted` event. A native compaction (§4.4) writes no ledger event, but
  it writes a transcript marker, so `last_compaction.at` starts the new cycle.
- A worker that ignores the warning is not warned again at each note. The
  orchestrator sees the row as `requested` (not `over`), so it does not send
  a `--request` on top of the warning. The risk row "A worker ignores the
  request" (§12) applies unchanged.

Failure is silent. None of these change the exit code or the output of
`horch note`, and none print to stderr: no ledger record for `HORCH_RECORD_ID`, no
session id, an unknown harness, agent `none`, no transcript, an unreadable
file, missing `sqlite3`, a roster load error, a message render error, a
ledger write error of the `context-warned` event.

Cost: the added work is 1 tail read of the transcript, about 0.03 s (R01 §6),
plus 1 roster load and 1 more ledger read. The roster and ledger costs are
not measured; local check L13 measures the whole call.

The orchestrator: `horch note` fails in the orchestrator pane (no
`HORCH_RECORD_ID`, §3.8), so it never runs this check there. The
orchestrator keeps its own `horch context --over` checkpoints (§5.1, §6.5).

---

## 6. Compaction (decisions 5, 6, 7, 8)

### 6.1 Handoff

- Path: `ai_docs/handoffs/<role>-whats-next.md`, relative to the project
  directory. The orchestrator's path is
  `ai_docs/handoffs/orchestrator-whats-next.md`.
- `skills/handoff/SKILL.md` step 7 changes to: "Write the assigned handoff
  path. With no assigned path, write `ai_docs/handoffs/<role>-whats-next.md`.
  `<role>` is `$HORCH_ROLE`, or `orchestrator` in the orchestrator pane."
- Availability: `handoff` is already in all 4 phase catalogs
  (`crates/horch-core/src/skills.rs:23-37`). Every roster worker has a phase,
  and both fleet orchestrators have `phase: plan`
  (R03 §7). Only `smoke` (agent `none`) and `_template.md` have no phase.
  No catalog change is needed.
- Ledger: the worker records only the path with `horch note`. The handoff
  content stays in the file. The ledger stays small, and the path is enough
  for a fresh spawn.
- Guard: `horch compact <role>` refuses when the handoff file is missing or
  older than 60 minutes, unless `--no-handoff` is given.

### 6.2 Per-harness action

All text goes through `Herdr::send_line`: `herdr pane send-text`, sleep 1 s
(2 s above 1,500 bytes), Enter, sleep 1 s, Enter
(`crates/horch-core/src/herdr.rs:680-691`). The second Enter on an empty
input box does nothing on all 5 harnesses (R02 intro). `send-text` types
keystrokes, not a bracketed paste, so a slash command is seen as typed input
(R03 §4).

| harness | compact line | custom instructions | busy behavior (R02) | after compaction |
|---|---|---|---|---|
| Claude Code | `/compact {instructions}` | yes, the argument (verified live: summary was exactly `banana`) | queued until the turn ends (verified) | idle; no new turn (verified) |
| Codex | `/compact` (bare) | no. `/compact <text>` becomes a model prompt. `compact_prompt` has no effect on the OpenAI provider (R02 §2.1) | REJECTED with Enter: `'/compact' is disabled while a task is in progress.` | idle; no new turn (source) |
| OpenCode | `/compact` (bare) | no. `/compact <text>` becomes a model prompt (R02 §3.1) | runs at the next loop step and ENDS the turn (source) | idle; no new turn (source) |
| pi | `/compact {instructions}` | yes | ABORTS the running turn, then compacts (source) | idle (source) |
| Prime | `/compact {instructions}` | yes, high priority (R02 §5.1) | queued to the next turn boundary (source) | UNVERIFIED whether a typed `/compact` auto-continues |

`{instructions}` is the single-line `instructions` message in
`_base/compaction.md` (§8.2). It points the summary at the handoff file.

Because 3 of 5 harnesses break when busy, the job always waits for idle
first (R02 recommendation 1). Because no harness resumes by itself, the job
always sends the `resume` message after the marker (R02 recommendation 3).
If Prime auto-continues, the resume message arrives during a turn; Prime
queues it as a steer, which is harmless.

Impossible or unsafe cases and the fallback:
- Agent `none` (the `smoke` teammate): horch refuses. There is no session.
- Any job failure (no idle within the timeout, no marker, send error): the job
  reports `[horch] BLOCKED: ...`. The orchestrator then tells the worker to
  run `horch done`, and spawns a fresh worker of the same teammate with the
  task "Read and follow <plan>. PRIOR WORK: read
  ai_docs/handoffs/<role>-whats-next.md first." This is the existing
  fresh-session preference in `_base/fleet-orchestrator.md`.
- OpenCode: bare `/compact` + Enter depends on the autocomplete popup
  (R02 §3.1, UNVERIFIED live). If it fails, the fallback applies.
- pi: `pi --help` crashes on the operator's Mac (R02 "Outside scope"). pi
  panes may not start at all; not a design issue.

### 6.3 The job

```
horch compact <role> [--request] [--force] [--no-handoff] [--foreground] [--timeout <secs>]
```

- `--request`: type the `request` message into the role's pane and record a
  ledger event `compact-requested` with text `tokens <n> threshold <t>`.
  Refused for `orchestrator` (it has no ledger record and asks itself).
  Prints 1 line: `asked <role> to write ai_docs/handoffs/<role>-whats-next.md`.
- Default: validate, then start the job DETACHED and return. Validation:
  role is registered in the mailbox; harness and transcript are resolved
  (§3.7, §3.8); harness is not `none`; no live job for the role (§6.4); the
  handoff guard (§6.1). The detached child is `horch compact <role>
  --foreground <same flags>`, started with `setsid`, stdin null, stdout and
  stderr to the log file. Use the pattern of `settle_after_close`
  (`crates/horch/src/cmd/tilecmd.rs:613-645`). Print 1 line:
  `compaction of <role> scheduled; log <path>`.
- `--foreground`: run the job in this process (the detached child uses it;
  a human can use it to debug).
- `--force`: skip the idle wait, and sleep 5 s instead. Use it only when herdr
  reports `unknown` for the pane (pi, OpenCode, Prime status is UNVERIFIED,
  R03 §3).
- `--timeout`: seconds for the first idle wait. Default 1800.

Job steps (`--foreground`):
1. Write the job file (§6.4).
2. Read `pre` = the current `Reading`. Record `offset` = transcript length in
   bytes (OpenCode: the newest message id).
3. Wait for idle: every 2 s run `herdr pane get <pane>`. Idle means
   `agent_status` is `idle` or `done` on 2 polls in a row. `blocked`,
   `working` and `unknown` are not idle. Stop at `--timeout` → fail.
4. Type the compact line (§6.2).
5. Wait for the marker after `offset` (§3.2-§3.5), polling every 3 s, up to
   600 s. For Codex and OpenCode only: if no marker after 30 s, go back to
   step 3 and send again; at most 3 sends in total. (A rejected Codex
   `/compact` leaves no marker.) Claude, pi and Prime get 1 send only, so a
   queued command never runs twice.
6. Wait for idle again (step 3 rule, up to 600 s). The compaction is finished.
7. Type the `resume` message.
8. Wait up to 180 s for a Reading with `pending == false`. `post` = its
   tokens, else the marker's `post_tokens`, else unknown.
9. Worker only: ledger event `compacted` with text
   `<pre> -> <post> tokens; handoff <path>`.
10. Worker only: `horch tell orchestrator` with the `reported` message.
11. Delete the job file. Exit 0.

On failure at any step: ledger event `compact-failed` (worker only) with the
step and reason; tell the orchestrator the `failed` message; delete the job
file; exit 1. The orchestrator gets the failure line in both cases, also when
the target is the orchestrator itself (it reads it when idle).

If the transcript cannot be found (for example no herdr `agent_session`), the
job cannot see a marker. Then step 5 waits for the pane to leave idle and
return to idle (at least 10 s after the send), and the report says
`completion not verified`.

### 6.4 Job file and log

- Directory: `<state_root>/compact/`, where `state_root` is
  `ledger::state_root()` (`$HORCH_STATE_DIR`, else
  `${XDG_STATE_HOME:-~/.local/state}/horch`).
- Job file: `<workspace_id>-<role>.json` with
  `{"pid":123,"role":"sonnet-1","started_at":"…","step":"wait-idle"}`. The job
  updates `step` as it goes.
- A job is live when the file exists and `kill(pid, 0)` succeeds. A stale file
  (dead pid) is removed and ignored.
- Log: `<workspace_id>-<role>.log`, appended, 1 line per step with a UTC time.

### 6.5 Orchestrator self-compaction (decision 7)

1. `horch context --over` shows the `orchestrator` row as `over`.
2. At its next stopping point (§5.3), the orchestrator loads `horch:handoff`
   and writes `ai_docs/handoffs/orchestrator-whats-next.md`. It includes every
   live role, the plan file of each, and every open question.
3. It runs `horch compact orchestrator`. The job starts detached.
4. It ends its turn at once, with no further tool call.
5. The job sees the pane go idle, types the compact line into the
   orchestrator's own pane, waits for the marker, waits for idle, and types
   the `resume` message. The orchestrator reads its handoff and continues.

Why this works for both flavors: the job waits for idle, so it never types
into a busy Codex pane. Claude would also accept a busy send (it queues,
R02 §1.2), but one path is simpler.

Risks, all local-only checks:
- A worker message can arrive while the orchestrator is idle and start a new
  turn before the job sends. Then step 3 sees `working` and waits again.
- Keystrokes of a worker `horch tell` and the job can interleave in the same
  pane. This risk exists today for any 2 concurrent tells.
- The detached child must survive the Claude Bash tool and the Codex sandbox
  after the command returns. `setsid` survives a herdr pane close
  (`tilecmd.rs:629-632`); the tool cases are UNVERIFIED (L12, L7).
- Messages typed into a Codex pane during compaction: queued or lost is
  UNVERIFIED (R02 §2.5).

### 6.6 Verification after compaction (decision 8)

- Proof: the harness marker appears after the job's start offset:
  Claude `compact_boundary`, Codex `compacted`, pi and Prime `compaction`,
  OpenCode a `summary` row.
- The number: Codex writes a small `token_count` at once. Claude shows
  `postTokens` as provisional. pi, Prime and OpenCode show `pending` until
  the first response after the resume message. The job waits up to 180 s
  for it (step 8).
- Resume: the `resume` message tells the session to read the handoff file and
  continue from its next step. On Claude, `SessionStart` hooks with matcher
  `compact` could inject the same line, but v1 uses the typed message for all
  harnesses (§13).
- The orchestrator can confirm with `horch context`: state `ok`, a new
  `LAST COMPACT` time, and a lower `CONTEXT`.

---

## 7. Ledger changes

New history events on a worker record (free-text `event` names; the
`HistoryEntry` shape is unchanged, `ledger.rs:28-32`):

| event | written by | text |
|---|---|---|
| `compact-requested` | `horch compact <role> --request` | `tokens 312857 threshold 300000` |
| `compacted` | the job, step 9 | `312857 -> 18207 tokens; handoff ai_docs/handoffs/sonnet-1-whats-next.md` |
| `compact-failed` | the job, on failure | `step wait-idle: no idle status in 1800 s` |
| `context-warned` | `horch note`, after the check prints the `warning` message (§5.4) | `tokens 311225 threshold 300000` |

The `context-warned` write uses `record_event` below. Rule A (§5.4) reads
`compact-requested`, `context-warned` and `compacted`.

New `Ledger` API:
- `pub fn live_for_role(&self, role: &str) -> Result<Option<Record>>`: the
  selection `assign` uses. `assign` calls it, so the rule lives in 1 place.
- `pub fn record_event(&self, key: &str, event: &str, text: &str) -> Result<()>`:
  a public wrapper of the private `append_event`.

`horch sessions` renders history already; no render change.

---

## 8. Surface (decision 9)

### 8.1 Rust

| file | change |
|---|---|
| `crates/horch-core/src/context.rs` (new) | `Reading`, `Compaction`; `read_claude`, `read_codex`, `read_pi` (pi and Prime), `read_opencode_rows` (pure, from `sqlite3 -json` text); `read_opencode(db, sid)` (shell-out); `scan_tail`; `last_marker`; `marker_after(harness, path, offset)`; `LIMITS`; `limits(...)`; `threshold(base, native_trigger)`; `read_session(loc, harness, sid) -> Result<(PathBuf, Reading), Missing>` reusing `usage::Missing`. |
| `crates/horch-core/src/lib.rs` | `pub mod context;` |
| `crates/horch-core/Cargo.toml` | `memchr = "2"` |
| `crates/horch-core/src/herdr.rs` | `Pane` gains `#[serde(default)] pub agent_status: Option<String>`; `Pane::is_idle()` (`idle` or `done`); `Pane::agent_name()` reads `agent_session.agent`. Unknown fields are already ignored (test at `herdr.rs:841`). |
| `crates/horch-core/src/ledger.rs` | `live_for_role`, `record_event` (§7). |
| `crates/horch-core/src/teammates.rs` | `Teammate.compact_at: Option<u64>` (`deny_unknown_fields` requires the field); roster check 50,000..=1,000,000; `Base.messages: BTreeMap<String,String>`; roster check: base `compaction` exists and has the keys `request`, `instructions`, `resume`, `reported`, `failed`, `warning`, each a single line with only known placeholders. |
| `crates/horch-core/src/prompts.rs` | `pub fn compaction_message(roster, key, vars: &[(&str, &str)]) -> Result<String>`, using the existing placeholder substitution (unknown placeholder = error). |
| `crates/horch/src/cmd/context.rs` (new) | `horch context` (§3.7). Two `pub(crate)` functions that `messaging.rs` reuses: the row builder for 1 ledger record (no herdr call inside it; the caller fills `pane` and `pane_status`), and `already_asked(history, last_compaction) -> bool` (§5.4 rule A). |
| `crates/horch/src/cmd/messaging.rs` | `note` gains the context check of §5.4 after the ledger write. The check is a private function that returns `Result<Option<String>>`; `note` turns an `Err` into no output. For tests, the check takes the ledger, the `usage::Locations`, the roster and the `HORCH_COMPACT_AT` value as parameters; `note` passes the real ones. No change to `tell`, `assign`, `done` or the clap wiring (`main.rs:304`). |
| `crates/horch/src/cmd/compact.rs` (new) | `horch compact` (§6.3). The job logic is behind a trait `PaneIo { fn status(&self) -> Result<Option<String>>; fn send_line(&self, text: &str) -> Result<()>; }` and a `Probe` closure for readings and markers, with the poll interval as a parameter, so it is unit-tested with fakes. |
| `crates/horch/src/main.rs`, `crates/horch/src/cmd/mod.rs` | 2 new subcommands. |

### 8.2 Data: `teammates/_base/compaction.md` (new)

A `_base` file with no prose body role; its frontmatter `messages` map holds
every text horch types. Placeholders: `{role}`, `{tokens}`, `{threshold}`,
`{handoff}`, `{pre}`, `{post}`, `{step}`, `{reason}`, `{log}`.

```yaml
---
name: compaction
description: >
  Every line horch types for the context watch. Rust only fills the
  placeholders. Each message is one line: a newline would submit early.
messages:
  request: "NOTE: Your context is {tokens} tokens. Your threshold is {threshold} tokens. At your next stopping point, write {handoff} with the horch:handoff skill. Then run: horch note \"handoff: {handoff}\". Then send: horch tell orchestrator \"[{role}] NOTE: COMPACT-READY {handoff}\". Then stop and wait."
  instructions: "Keep the current task, the plan file path, and open questions. The full state is in {handoff}. Keep the summary short."
  resume: "NOTE: Compaction is complete. Read {handoff} now. Then continue your task from its next step. Do not repeat finished steps."
  reported: "[horch] NOTE: {role} compacted. Context {pre} -> {post} tokens. Handoff: {handoff}."
  failed: "[horch] BLOCKED: Compaction of {role} failed at step {step}: {reason}. Log: {log}."
  warning: "NOTE: Context warning. Your context is {tokens} tokens. Your threshold is {threshold} tokens. This horch note call is your stopping point. Do not start new work. Write {handoff} with the horch:handoff skill. Then run: horch note \"handoff: {handoff}\". Then send: horch tell orchestrator \"[{role}] NOTE: COMPACT-READY {handoff}\". Then end your turn and wait."
---
```

The body documents the per-harness table of §6.2 for human readers.

`warning` is printed by `horch note` (§5.4) to the worker's own tool output.
It is not typed into a pane. It starts with `NOTE: Context warning`, not with
`NOTE: Your context is`, so the worker prose (§8.3) can tell it apart from
the `request` message: the `request` asks the worker to finish its step
first, and the `warning` says the worker is at its stopping point now.

Rendered for role `sonnet-1`, tokens `311225`, threshold `300000` and handoff
`ai_docs/handoffs/sonnet-1-whats-next.md`, the `warning` line is exactly:

```
NOTE: Context warning. Your context is 311225 tokens. Your threshold is 300000 tokens. This horch note call is your stopping point. Do not start new work. Write ai_docs/handoffs/sonnet-1-whats-next.md with the horch:handoff skill. Then run: horch note "handoff: ai_docs/handoffs/sonnet-1-whats-next.md". Then send: horch tell orchestrator "[sonnet-1] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-1-whats-next.md". Then end your turn and wait.
```

### 8.3 Prose

`teammates/_base/fleet-worker.md`: insert this section between `== Scope ==`
and `== Message style ... ==`:

```
== Context compaction ==
The orchestrator watches your context size. A line that starts "NOTE: Your
context is" asks you to prepare for compaction. Then:
1. Finish the current plan step and its check. Do not stop mid-edit or
   during a build or test run. If you wait for an answer, go on at once.
2. Load the horch:handoff skill. Write ai_docs/handoffs/{role}-whats-next.md.
3. Run: horch note "handoff: ai_docs/handoffs/{role}-whats-next.md"
4. Send: horch tell orchestrator "[{role}] NOTE: COMPACT-READY ai_docs/handoffs/{role}-whats-next.md"
5. Stop and wait. Do not start new work. horch compacts this session. Then
   it tells you to read the handoff file and continue.
If horch note prints a line that starts "NOTE: Context warning", follow it
at once: do steps 2 to 5. That horch note call is your stopping point.
```

The last 2 lines of the worker section serve the self-warning path (§5.4).
The section still ends with 1 blank line after it (§8.5).

`teammates/_base/fleet-orchestrator.md`: insert this section immediately
before `== Protect your context ==`:

```
== Context watch ==
Watch the context size of every session, yours included:
  horch context --over           sessions over their threshold
  horch context                  every live session and its context size
  horch compact <role> --request ask a worker to write its handoff
  horch compact <role>           compact a session that is ready
Run horch context --over after you handle a worker message and before each
spawn. For a worker in state "over":
1. Run horch compact <role> --request once. The state becomes "requested".
2. On "[<role>] NOTE: COMPACT-READY <path>", run horch compact <role>. It
   returns at once. horch compacts the worker when it is idle, then tells it
   to read its handoff.
3. Lines that start with "[horch]" come from horch. "[horch] NOTE:" reports
   success. On "[horch] BLOCKED:", tell the worker to run horch done. Then
   spawn a fresh worker with its handoff file as prior work.
When the "orchestrator" row is "over", compact yourself at your next
stopping point: every worker message is answered and no spawn is half-done.
1. Load horch:handoff. Write ai_docs/handoffs/orchestrator-whats-next.md.
   Name every live role, its plan file, and every open question.
2. Run horch compact orchestrator.
3. End your turn at once. horch compacts this pane when it is idle, then
   tells you to read the handoff file.
```

Both texts follow the STE rules of the briefings. The orchestrator text names
`horch context` and `horch compact`, which the Codex execpolicy check requires
(§8.4).

`skills/handoff/SKILL.md`: step 7 as in §6.1.

`skills/orchestrate/SKILL.md`: no change. The procedure is short enough for
the base briefing, and one home is easier to keep correct.

### 8.4 Codex orchestrator execpolicy

Add to `teammates/_base/codex-orchestrator-execpolicy.md` `rules:`:

```yaml
  - pattern: '"horch", "context"'
    justification: report the context size of every session
  - pattern: '"horch", "compact"'
    justification: compact a session at a stopping point
```

Without them the Codex orchestrator cannot run the commands
(`recipes.rs:409-417`). `unused_rules` (`teammates.rs:1180-1200`) fails a
rule the briefing never names; §8.3 names both. The worker execpolicy
(`codex-execpolicy.md`) does not change: workers run only `horch note` and
`horch tell orchestrator`, which it already allows. So
`execpolicy_blocks_are_unchanged` (3 worker rules) stays green. The context
warning (§5.4) adds no command: it runs inside `horch note`.

### 8.5 Golden files

The goldens in `crates/horch-core/tests/golden/` are captures of OLD text and
are NEVER regenerated (`golden_prompts.rs:26-30`). A prose change adds a
named, commented "sanctioned" block to the test:
- `every_worker_briefing_differs_only_where_sanctioned`: add a sixth block
  `compaction` with the exact §8.3 worker text (role `r-1`), including its
  last 2 lines about `NOTE: Context warning`, inserted between
  `{scope}` and `{ste}` in the replacement:
  `format!("gotchas, current state.\n{done_summary}\n{scope}{compaction}{ste}")`.
  Update the doc comments ("five" → "six").
- `the_orchestrator_briefing_differs_only_where_sanctioned`: add a block
  `context_watch` with the exact §8.3 orchestrator text plus its trailing
  blank line, inserted before `== Protect your context ==`. Update the
  counts in the comments and the assert message ("ten" → "eleven").
- `the_codex_orchestrator_differs_from_the_claude_one_only_in_its_tier_block`:
  passes unchanged (shared base).
- `the_orchestration_recipe_briefings_are_unchanged`: unchanged (the fixed
  recipe is out of scope).
- `execpolicy_blocks_are_unchanged`: unchanged (§8.4).

### 8.6 Docs

- `README.md`: a section "Context watch and early compaction": the 2
  commands, the threshold rule, the per-harness table (§6.2), the handoff
  path, and 1 sentence on the `horch note` context warning (§5.4).
- `docs/phase-skills.md`: 1 line: `handoff` writes
  `ai_docs/handoffs/<role>-whats-next.md` by default.
- `teammates/README.md` and `teammates/_template.md`: the `compact_at` field.

---

## 9. Verification

### 9.1 Tier 1: cloud-verifiable

Environment: Linux container, fresh clone, no herdr, no other harness, no
operator home. Every command runs from the repository root.

**C0. Toolchain and build dir.**

```
rustc --version || (curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal && . "$HOME/.cargo/env")
rustc --version          # must print 1.85.0 or newer (workspace rust-version)
export CARGO_TARGET_DIR=/tmp/cc-target
cargo fetch              # needs crates.io access; all versions are pinned in Cargo.lock
```

Pass: `rustc` ≥ 1.85 and `cargo fetch` exits 0. If crates.io is blocked, the
cloud tier cannot run; report that and stop.

**C1. Whole test suite.**

```
cargo test --workspace 2>&1 | grep -E '^test result:'
```

Pass: every line reads `test result: ok.` with `0 failed`. The sum of
`passed` is at least 340 (the baseline) plus the new tests.

**C2. Roster check.**

```
cargo build -p horch
HORCH_TEAMMATES_DIR=$PWD/teammates $CARGO_TARGET_DIR/debug/horch teammates --check
```

Pass: prints exactly `roster ok: 25 teammates, 20 offered to the orchestrator`
and exits 0. (`_base/compaction.md` is a base, not a teammate.)

**C3. Golden prompts.**

```
cargo test -p horch-core --test golden_prompts
```

Pass: `test result: ok. 5 passed; 0 failed`.

**C4. Reader and threshold unit tests** (names fixed by the plan):

```
cargo test -p horch-core context::
```

Pass: `0 failed`, and these tests are in the list: `claude_reads_last_usable_line_with_output`,
`claude_skips_sidechain_and_synthetic`, `claude_uses_last_message_iteration`,
`claude_pending_after_boundary_uses_post_tokens`, `claude_post_compaction_reads_new_response`,
`codex_reads_last_token_count_total`, `codex_ignores_null_info_and_usage_records`,
`codex_post_compaction_reads_new_token_count`, `tail_grows_past_a_five_mib_line`,
`pi_skips_error_and_tool_result_lines`, `pi_pending_after_compaction_is_null`,
`prime_uses_pi_format`, `opencode_rows_skip_unfinished_and_summary`,
`opencode_summary_row_is_pending`, `last_marker_ignores_escaped_needles`,
`threshold_caps_at_eight_tenths_of_native_trigger`, `limits_per_harness`,
`claude_auto_compact_window_lookup_order`.

**C5. Job logic with fakes.**

```
cargo test -p horch compact::
```

Pass: `0 failed`, including `job_waits_for_two_idle_polls`,
`job_sends_bare_compact_for_codex_and_opencode`,
`job_sends_instructions_for_claude_pi_prime`, `job_resends_codex_after_rejection`,
`job_never_resends_claude`, `job_sends_resume_after_marker_and_idle`,
`job_records_ledger_events_for_workers_only`, `job_reports_failure_on_timeout`,
`handoff_guard_refuses_missing_or_stale_file`, `request_is_refused_for_orchestrator`.

**C6. `horch context` against checked-in fixtures.**

Fixture home: `crates/horch-core/tests/fixtures/context/home/` (§9.3).

```
BIN=$CARGO_TARGET_DIR/debug/horch
H=$PWD/crates/horch-core/tests/fixtures/context/home
S=$(mktemp -d)
P=$PWD/crates/horch-core/tests/fixtures/context/prime/2026-09-20-prime-post.jsonl
env -u CODEX_HOME -u PI_CODING_AGENT_SESSION_DIR -u HORCH_COMPACT_AT \
  HOME=$H HORCH_STATE_DIR=$S XDG_DATA_HOME=$H/.local/share HORCH_WORKSPACE_ID=none \
  $BIN context --json \
  --session claude:11111111-1111-4111-8111-111111111111 \
  --session claude:22222222-2222-4222-8222-222222222222 \
  --session claude:33333333-3333-4333-8333-333333333333 \
  --session claude:44444444-4444-4444-8444-444444444444 \
  --session codex:01a0de6e-0000-7000-8000-000000000001 \
  --session codex:01a0de6e-0000-7000-8000-000000000002 \
  --session pi:aaaaaaaa-0000-4000-8000-000000000001 \
  --session pi:aaaaaaaa-0000-4000-8000-000000000002 \
  --session prime:$P \
  2>/dev/null | jq -c '.[] | [.role,.harness,.tokens,.pending,.window,.threshold,.native_trigger,.state]'
```

Pass: exactly these 9 lines:

```
["session-1","claude",311225,false,1000000,300000,467000,"over"]
["session-2","claude",10486,true,1000000,300000,467000,"pending"]
["session-3","claude",33352,false,1000000,300000,467000,"ok"]
["session-4","claude",231077,false,1000000,300000,467000,"ok"]
["session-5","codex",227306,false,258400,195840,244800,"over"]
["session-6","codex",12847,false,258400,195840,244800,"ok"]
["session-7","pi",201000,false,262144,196608,245760,"over"]
["session-8","pi",null,true,262144,196608,245760,"pending"]
["session-9","prime",25000,false,1000000,300000,983616,"ok"]
```

The 467,000 in the Claude rows proves the `settings.json` lookup (the fixture
home sets `CLAUDE_CODE_AUTO_COMPACT_WINDOW` to `500000`). If `jq` is missing,
use `python3 -c 'import json,sys; [print(json.dumps([r[k] for k in ("role","harness","tokens","pending","window","threshold","native_trigger","state")],separators=(",",":"))) for r in json.load(sys.stdin)]'`.

Also check `last_compaction` for the 2 post files:

```
... same command ... | jq -c '.[] | select(.role=="session-3" or .role=="session-6") | .last_compaction'
```

Pass:

```
{"at":"2026-09-20T10:05:00.000Z","trigger":"manual","pre_tokens":312857,"post_tokens":10486}
{"at":"2026-09-20T11:00:00.000Z","trigger":null,"pre_tokens":229420,"post_tokens":12847}
```

**C7. `--over` and the empty case.**

```
... same env ... $BIN context --over --session claude:11111111-1111-4111-8111-111111111111 \
  --session claude:33333333-3333-4333-8333-333333333333 | tail -n +2 | wc -l      # 1
... same env ... $BIN context --over --session claude:33333333-3333-4333-8333-333333333333  # prints: no session is over its threshold
... same env ... HORCH_COMPACT_AT=20000 $BIN context --json --session claude:33333333-3333-4333-8333-333333333333 | jq -r '.[0].state'   # over
```

Pass: `1`; the exact sentence; `over`.

**C8. OpenCode CLI path (only when `sqlite3` is on PATH).**

```
command -v sqlite3 && cargo test -p horch-core context::opencode_reads_a_real_database -- --ignored
```

Pass: `1 passed`. The test builds a database in a temp dir from
`fixtures/context/opencode/rows-post.json` with `sqlite3`. Without `sqlite3`,
skip C8 and record "skipped: no sqlite3". The pure row logic is covered by C4.

**C9. No API key anywhere.**

```
git diff --name-only origin/main... | xargs grep -n 'ANTHROPIC_API_KEY' || echo clean
```

Pass: prints `clean`, or only the existing lines in `launch.rs` and
`teammates.rs` that REMOVE or REFUSE the key.

**C10. Context warning in `horch note` (§5.4), with fixtures and no herdr.**

Part 1, unit tests:

```
cargo test -p horch messaging::
cargo test -p horch already_asked
```

Pass: `0 failed` on both, and these tests are in the lists:
`note_warning_fires_over_threshold`,
`note_warning_skips_below_threshold_and_pending`,
`note_warning_skips_after_warned_or_requested`,
`note_warning_fires_again_after_compaction`,
`note_warning_is_silent_on_errors`,
`note_warning_text_comes_from_compaction_base`,
`already_asked_follows_rule_a`.

Part 2, the command against the fixture home of §9.3 and a hand-written
ledger. `HORCH_PROJECT_DIR=/fixture` makes the ledger file
`$S/-fixture.json` (`ledger::slug`). The directory `/fixture` does not need to
exist.

```
BIN=$CARGO_TARGET_DIR/debug/horch
H=$PWD/crates/horch-core/tests/fixtures/context/home
S=$(mktemp -d)
R='"agent":"claude","tier":"sonnet","model":"claude-sonnet-5","status":"working","task":"t","history":[],"created_at":"2026-09-20T09:00:00Z","updated_at":"2026-09-20T09:00:00Z"'
cat > "$S/-fixture.json" <<EOF
[{"record_id":"r-over","session_id":"11111111-1111-4111-8111-111111111111","role":"sonnet-1",$R},
 {"record_id":"r-ok","session_id":"33333333-3333-4333-8333-333333333333","role":"sonnet-2",$R},
 {"record_id":"r-none","session_id":"99999999-9999-4999-8999-999999999999","role":"sonnet-3",$R}]
EOF
W="env -u CODEX_HOME -u PI_CODING_AGENT_SESSION_DIR -u HORCH_COMPACT_AT -u HORCH_TEAMMATES_DIR -u HORCH_SESSION_ID -u HORCH_ROLE HOME=$H HORCH_STATE_DIR=$S HORCH_PROJECT_DIR=/fixture XDG_DATA_HOME=$H/.local/share HORCH_WORKSPACE_ID=none"
$W HORCH_RECORD_ID=r-over $BIN note "step 1 done"; echo "exit $?"
$W HORCH_RECORD_ID=r-over $BIN note "handoff: ai_docs/handoffs/sonnet-1-whats-next.md"; echo "exit $?"
$W HORCH_RECORD_ID=r-ok $BIN note "below"; echo "exit $?"
$W HORCH_RECORD_ID=r-none $BIN note "no transcript"; echo "exit $?"
$W HORCH_RECORD_ID=r-ok HORCH_COMPACT_AT=20000 $BIN note "env base"; echo "exit $?"
jq -r '.[] | .role + " " + ([.history[].event] | join(","))' "$S/-fixture.json"
$W $BIN context --json 2>/dev/null | jq -c '.[] | [.role,.tokens,.state]' | sort
```

Pass: the output is exactly these 13 lines, and nothing is printed to stderr
by the 5 `note` calls:

```
NOTE: Context warning. Your context is 311225 tokens. Your threshold is 300000 tokens. This horch note call is your stopping point. Do not start new work. Write ai_docs/handoffs/sonnet-1-whats-next.md with the horch:handoff skill. Then run: horch note "handoff: ai_docs/handoffs/sonnet-1-whats-next.md". Then send: horch tell orchestrator "[sonnet-1] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-1-whats-next.md". Then end your turn and wait.
exit 0
exit 0
exit 0
exit 0
NOTE: Context warning. Your context is 33352 tokens. Your threshold is 20000 tokens. This horch note call is your stopping point. Do not start new work. Write ai_docs/handoffs/sonnet-2-whats-next.md with the horch:handoff skill. Then run: horch note "handoff: ai_docs/handoffs/sonnet-2-whats-next.md". Then send: horch tell orchestrator "[sonnet-2] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-2-whats-next.md". Then end your turn and wait.
exit 0
sonnet-1 note,context-warned,note
sonnet-2 note,note,context-warned
sonnet-3 note
["sonnet-1",311225,"requested"]
["sonnet-2",33352,"ok"]
["sonnet-3",null,"no-transcript"]
```

What each line proves:
- Line 1: the warning for a worker over its threshold (311,225 ≥ 300,000;
  the `sonnet` teammate has no `compact_at`; the 0.8 cap of 467,000 is
  373,600).
- Line 3: the `handoff:` note prints no second warning (rule A).
- Lines 4 and 5: no warning below the threshold, and silence when the
  transcript is missing. The exit code stays 0.
- Line 6: `HORCH_COMPACT_AT` sets `base` in the worker process. Rule A does
  not block it: the fixture's `compact_boundary` (2026-09-20T10:05:00.000Z)
  starts the cycle, and `sonnet-2` has no ask event after it.
- Lines 8 to 10: every note is recorded, and `context-warned` follows the
  note that printed the warning.
- Lines 11 to 13: `horch context` reads the same record and shows `requested`
  for `sonnet-1`, so the orchestrator does not send a `--request` on top of
  the warning. `sonnet-2` is `ok` again because this call has no
  `HORCH_COMPACT_AT`.

### 9.2 Tier 2: local-only (operator's Mac, herdr and real harnesses)

Build first: `CARGO_TARGET_DIR=/tmp/cc-target cargo build -p horch`, then
put that `horch` first on PATH, or install it as the operator does today.
Every `claude` command runs as `env -u ANTHROPIC_API_KEY claude ...`.

L1. Live reading, Claude worker.
1. `horch fleet`. In the orchestrator, spawn `sonnet` with a short task.
2. In the worker pane, run `/context` after 2 or 3 turns.
3. Run `horch context` in the orchestrator pane.
Expected: the `sonnet-1` row shows the model, window 1,000,000, and a
CONTEXT that differs from `/context` "total" by at most 1 response's output
plus 1 tool result (R01 §1: `/context` omits output).

L2. Live reading, Codex worker. Spawn `codex-sol`. Compare `horch context`
with the Codex status line after a few turns. Expected: same number within
1 tool result; WINDOW 258,400; THRESHOLD 195,840.

L3. Orchestrator row. Expected: `horch context` shows an `orchestrator` row
for `horch fleet` (Claude) and for `horch fleet codex` (Codex), with a
non-null CONTEXT.

L4. Round trip, Claude worker.
1. Run `horch compact sonnet-1 --request`.
2. Expected: the worker finishes its step, writes
   `ai_docs/handoffs/sonnet-1-whats-next.md`, runs `horch note`, and sends
   `[sonnet-1] NOTE: COMPACT-READY ...`.
3. Run `horch compact sonnet-1`. Expected: 1 line `compaction of sonnet-1
   scheduled; log ...`, returned in under 2 s.
4. Expected in the worker pane: `/compact Keep the current task...` typed,
   `Compacting conversation…`, `Compacted`, then the resume line, then the
   worker reads the handoff and continues.
5. Expected in the orchestrator pane: `[horch] NOTE: sonnet-1 compacted.
   Context <pre> -> <post> tokens. ...` with post < pre.
6. `horch sessions` shows the events `compact-requested` and `compacted`.
7. `horch context` shows a new LAST COMPACT time and state `ok`.

L5. Round trip, Codex worker. As L4 with `codex-sol-1`. Expected: bare
`/compact`; `Context compacted` on screen; no `'/compact' is disabled`
message in the log. Also check whether the herdr `agent_session` value stays
the same after compaction.

L6. Codex busy rejection. While `codex-sol-1` works, run
`horch compact codex-sol-1 --force --no-handoff`. Expected: the log shows a
send, no marker in 30 s, an idle wait, and a second send that succeeds.

L7. Orchestrator self-compaction, Codex flavor. `horch fleet codex`. Ask the
orchestrator to write its handoff and run `horch compact orchestrator`, then
end its turn. Expected: the detached job survives the Codex sandbox,
`/compact` runs when the pane is idle, the resume line arrives, and the
orchestrator continues from its handoff.

L8. Orchestrator self-compaction, Claude flavor. As L7 with `horch fleet`.

L9. OpenCode, pi, Prime. Spawn `opencode-pickle`, `pi`, `prime`. Run
`herdr pane get <pane>` while each works and while each is idle. Record the
`agent_status` values. Then run L4 on each. For a pane that shows `unknown`,
use `--force`. Expected: OpenCode bare `/compact` runs through the
autocomplete; pi and Prime compact with instructions. Record any failure.

L10. Claude settings precedence. In a Claude worker pane, run
`echo $CLAUDE_CODE_AUTO_COMPACT_WINDOW` through its Bash tool. Expected:
`500000`, and `horch context --json` shows `native_trigger` 467000 for it.

L11. Speed. With at least 5 live sessions, `time horch context`. Expected:
under 1 s.

L12. Detached job and the Claude Bash tool. Expected in L4 and L8: the job
log reaches `done` after the Bash tool call has returned.

L13. Self-warning round trip (§5.4).
1. Copy `teammates/sonnet.md` to `~/.config/horch/teammates/sonnet.md` and
   set `compact_at: 50000` in the copy. (A pane does not inherit the
   spawner's `HORCH_TEAMMATES_DIR`, `crates/horch-core/src/mailbox.rs:53-55`,
   and this design assumes the same for `HORCH_COMPACT_AT`. So use the
   teammate field.)
2. `horch fleet`. Spawn `sonnet` with a task that has at least 4 steps and
   a `horch note` after each step.
3. Expected: the first `horch note` after the context passes 50,000 tokens
   prints the `NOTE: Context warning` line in the worker's tool output. The
   worker writes `ai_docs/handoffs/sonnet-1-whats-next.md`, runs
   `horch note "handoff: ..."` (no second warning), sends
   `[sonnet-1] NOTE: COMPACT-READY ...`, and ends its turn.
4. `horch sessions` shows exactly 1 `context-warned` event for the record.
   `horch context` shows the row as `requested` before step 5.
5. The orchestrator runs `horch compact sonnet-1`. Expected: L4 steps 3 to 7.
6. In the worker pane, `time horch note timing`. Expected: under 0.5 s.
7. Repeat steps 2 and 3 with a `codex-sol` copy (`compact_at: 50000`).
   Expected: the Codex sandbox lets `horch note` read the rollout, and the
   warning prints.
8. Delete the 2 overlay copies.

### 9.3 How to build the fixtures without real transcripts

Write every fixture by hand from the formats in §3. Keep each file short
(under 20 lines). Use these ids, names and numbers; C6 depends on them.

Home tree `crates/horch-core/tests/fixtures/context/home/`:
- `.claude/settings.json`: `{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"500000"}}`.
- `.claude/projects/-fixture/11111111-1111-4111-8111-111111111111.jsonl`
  (claude pre): 1 user line; 1 assistant line `msg_A` model
  `claude-opus-5-5` usage `4 / 2929 / 308000 / 292` (input / cache_creation /
  cache_read / output) = 311,225; the same `msg_A` line again (streaming
  duplicate); then 1 `isSidechain: true` assistant line with usage
  `1 / 0 / 900000 / 1`; then 1 assistant line with model `<synthetic>` and
  zero usage. Expected 311,225.
- `22222222-2222-4222-8222-222222222222.jsonl` (claude pending): the pre
  lines, then
  `{"type":"system","subtype":"compact_boundary","timestamp":"2026-09-20T10:05:00.000Z","compactMetadata":{"trigger":"manual","preTokens":312857,"postTokens":10486}}`
  and `{"type":"user","isCompactSummary":true,"timestamp":"2026-09-20T10:05:00.100Z","message":{"role":"user","content":"This session is being continued ..."}}`.
  Expected 10,486 pending.
- `33333333-3333-4333-8333-333333333333.jsonl` (claude post): the pending
  lines, then 1 assistant line `msg_B` usage `3 / 30000 / 3000 / 349` = 33,352.
- `44444444-4444-4444-8444-444444444444.jsonl` (claude iterations): 1
  assistant line, top-level usage `4 / 2351 / 457723 / 180`, `iterations`:
  `[{"type":"message","input_tokens":2,"cache_creation_input_tokens":433,"cache_read_input_tokens":228645,"output_tokens":101},
    {"type":"advisor_message","input_tokens":231470,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":1814},
    {"type":"message","input_tokens":2,"cache_creation_input_tokens":1918,"cache_read_input_tokens":229078,"output_tokens":79}]`.
  Expected 231,077.
- `.codex/sessions/2026/09/20/rollout-2026-09-20T10-00-00-01a0de6e-0000-7000-8000-000000000001.jsonl`
  (codex pre): `session_meta`; `token_count` with `"info":null`; `token_count`
  with `last_token_usage {"input_tokens":227000,"cached_input_tokens":200000,"output_tokens":306,"total_tokens":227306}`
  and `model_context_window` 258400; then a trailing
  `{"type":"token_usage_record","payload":{"usage":{"total_tokens":229420}}}`.
  Expected 227,306.
- `...-000000000002.jsonl` (codex post): the pre lines, then
  `{"timestamp":"2026-09-20T11:00:00.000Z","type":"compacted","payload":{"message":"","window_number":1,"latest_token_usage_record":{"usage":{"total_tokens":229420}},"replacement_history":[{"type":"compaction"}]}}`,
  then `token_count` with `{"input_tokens":0,"cached_input_tokens":0,"output_tokens":0,"total_tokens":12847}`,
  window 258400. Expected 12,847.
- `.pi/agent/sessions/--fixture--/2026-09-20T12-00-00-000Z_aaaaaaaa-0000-4000-8000-000000000001.jsonl`
  (pi pre): a `session` header line; 1 assistant `message` line,
  provider `ollama`, model `qwen3.8`, usage
  `{"input":200000,"output":1000,"cacheRead":0,"cacheWrite":0,"totalTokens":201000}`,
  `stopReason` `stop`; then 1 assistant line with `stopReason` `error` and
  zero usage; then 1 `toolResult` line with usage `totalTokens` 999999.
  Expected 201,000.
- `..._aaaaaaaa-0000-4000-8000-000000000002.jsonl` (pi pending): the pre
  lines, then
  `{"type":"compaction","id":"c1","parentId":"m3","timestamp":"2026-09-20T12:00:00.000Z","summary":"...","firstKeptEntryId":"m1","tokensBefore":201500}`.
  Expected null, pending.
- `.local/share/opencode/` stays empty (C8 builds its own database).
- No file has the id `99999999-9999-4999-8999-999999999999`. C10 uses that
  id for the missing-transcript case. Do not add it.

Other fixtures in `crates/horch-core/tests/fixtures/context/`:
- `prime/2026-09-20-prime-post.jsonl`: pi format, provider `anthropic`,
  model `claude-opus-5-5`: an assistant line 280,000; a `compaction` entry;
  an assistant line `totalTokens` 25,000. Expected 25,000.
- `opencode/rows-pre.json`, `rows-pending.json`, `rows-post.json`: arrays in
  the `sqlite3 -json` shape `[{"id":"msg_...","data":"<escaped JSON>"}]`,
  newest first. Include 1 unfinished assistant row (`finish` null, tokens 0)
  and, in pending and post, a summary row
  (`"summary":true,"mode":"compaction"`). Numbers: pre 17,009
  (`opencode/big-pickle`); post 9,500.
- The 5 MiB line for `tail_grows_past_a_five_mib_line` is generated by the
  test in a temp dir, not checked in.

---

## 10. Handing this to a cloud session

Files the cloud session must have:
- This design: `ai_docs/designs/context-compaction.md`.
- The plan: `ai_docs/plans/context-compaction/05-implementation.md`.
- The repository at `main` (`a4dcee2` or later).

Commit both files first. `ai_docs/` is not ignored, and other files under
`ai_docs/` are already tracked, so a plain `git add` works:

```
git add ai_docs/designs/context-compaction.md ai_docs/plans/context-compaction/05-implementation.md
git commit -m "Design and plan: context watch and early compaction"
git push
```

The research reports (R01-R03) are optional context. The design does not
depend on them.

Order of units (details in the plan):
1. U1, U2, U3, U5 in parallel.
2. U4 after U1, U2 and U3.
3. U6 after U4 and U5.
4. U7 (validation, cloud tier) last.
5. The operator runs the local-only checklist (§9.2) on the Mac.

Rules for the cloud session:
- NEVER read, print, set, export or depend on `ANTHROPIC_API_KEY`. Any
  `claude` command is `env -u ANTHROPIC_API_KEY claude ...`. The session
  uses the operator's claude.ai login only.
- Do not start subagents.
- Build and test with `CARGO_TARGET_DIR=/tmp/<unit>-target`.
- Never regenerate a golden file; add a sanctioned block (§8.5).

---

## 11. Compatibility

- Old ledgers: new event names are free text; old records load unchanged.
- Old teammate overlays in `~/.config/horch/teammates` without `compact_at`
  still load (`Option`, `serde(default)`).
- A user `_base` overlay without `compaction.md` falls back to the built-in
  base (overlays only replace files they contain).
- No change to `horch cost`, `usage.rs`, `horch sessions` output format, or
  any launch argument.

## 12. Risks

| risk | effect | mitigation |
|---|---|---|
| herdr reports `unknown` for pi, OpenCode or Prime panes | the job never sees idle | `--force`; L9 records the real values |
| The detached child dies with the tool call | no compaction, job file stale | stale-pid cleanup; L7, L12 |
| Keystroke interleaving with a concurrent `horch tell` | a garbled line in the pane | existing risk; the job sends only when idle |
| A worker ignores the request | the native backstop compacts without a handoff | the orchestrator sees `requested` for a long time and can ask again with `horch tell` |
| Claude settings `env` precedence differs | wrong `native_trigger` shown; threshold stays 300,000 because of `min` | L10 |
| Claude changes the usage shape again | wrong number | fixture tests pin today's shapes; the reader is 1 function per harness |
| A pane does not inherit the spawner's `HORCH_TEAMMATES_DIR` (`mailbox.rs:53-55`). This design assumes the same for `HORCH_COMPACT_AT`: a value set only in the orchestrator's shell may not reach `horch note` | the worker warns at a different number than `horch context` shows (for example 300,000 in the worker, 200,000 in the orchestrator) | the orchestrator's `--request` path still fires at its own number; set a fleet-wide threshold with the teammate `compact_at`, which both processes read; L13 uses `compact_at` |
| A pane does not inherit `HORCH_TEAMMATES_DIR`, so `horch note` loads the built-in roster plus `~/.config/horch/teammates` | a `compact_at` edited in the repository `teammates/` after the build is not seen by `horch note` | rebuild horch, or put the edit in the user overlay; the same limit applies to every worker-side roster load today |
| A worker ignores the context warning | no handoff; later the native backstop compacts | rule A shows the row as `requested`; the orchestrator can ask again with `horch tell` (same as the row above for `--request`) |
| The `horch note` check fails in a way it does not catch (a panic) | `horch note` exits non-zero after the note is recorded | the note is written first; the check uses no `unwrap` on external data; C10 part 2 covers a missing transcript |

## 13. Out of scope for v1 (decision 10)

- A watcher process or daemon, and a push signal from herdr
  (`pane.agent_status_changed`). The orchestrator polls at checkpoints.
- An orchestrator ledger record (R03 §1 option B; DRAFT
  `ai_docs/designs/telemetry-and-balancing.md` §3 and unit 1). This design
  reuses only its idea of treating the orchestrator as a session; it finds the
  orchestrator through herdr instead. When that design lands, `horch context`
  can read the record instead of herdr.
- The telemetry collector and offset-aware readers of
  `telemetry-and-balancing.md` §4. `context.rs` keeps its readers pure
  (`&[u8]`/`&str` in, `Reading` out) so the collector can reuse them.
- herdr sidebar metadata tokens (`pane.report_metadata --token ctx=...`,
  `telemetry-and-balancing.md` §5).
- New backstop levers for Claude, Prime and OpenCode ultra (§4.4).
- Harness hooks and extensions: Claude `SessionStart` matcher `compact` and
  `PreCompact`; Codex `hooks.json` in the pane `CODEX_HOME`; OpenCode plugin
  `experimental.session.compacting`; pi extension `ctx.compact()`; Prime
  built-in `compact` skill (`--skill .../dist/skills/compact`, R02 §5.3).
- Codex custom compact prompt (no effect on the OpenAI provider, R02 §2.1).
- pi `/tree` branch walking (§3.4).
- The fixed `horch orchestration` recipe and its goldens.
- Seen in passing, not fixed (R01 §9): `read_pi` in `usage.rs` counts
  `stopReason: "error"` lines and `toolResult` usage, and misses the usage on
  `compaction` entries; the comment at `usage.rs:360` says Codex writes
  `token_count` twice, which is false on 0.157.1; `read_codex` misses a final
  `token_usage_record`.
