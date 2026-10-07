# Design: the fleet telemetry space and quota-aware balancing

| | |
|---|---|
| Status | Implemented, with the recommended defaults of section 4.2. |
| Date | 2026-09-28 |
| Author | The fleet orchestrator |
| Evidence | [S], [H] and [Q] mark facts from the telemetry-source, herdr-surface and quota-signal research. |
| Build target | Buildable and verifiable in a remote Claude cloud session. Section 16 says how. Section 17 lists what only the operator's Mac can prove. |

---

## Part I - Problem, requirements, decisions

## 1. Problem

On 2026-09-28 the Claude weekly window reads 100% (resets 2026-10-02 14:00Z). The Codex weekly window reads 99% (resets 2026-10-03 19:15Z) [Q]. Nothing in horch saw either coming, and nothing in horch can route around them:

- `horch spawn` has one policy gate, the top-tier refusal at `crates/horch/src/cmd/spawn.rs:136`. It has no teammate substitution [Q].
- The orchestrator has no ledger record and horch never learns its session id (`pane_launch` uses `Session::Unmanaged`). The single largest consumer in a fleet is invisible [S].
- `horch cost` is after-the-fact, per project, and re-reads whole transcripts [H]. It also under-counts Codex, because it misses the final response of every session (205k tokens in the sampled file), and it ignores Claude subagent transcripts [S].

The operator asked for two things:

1. **A singleton telemetry space.** One pane of glass in herdr where every orchestrator and every worker, on every harness (Claude Code, Codex, OpenCode, pi, Prime), reports worker name, task type and token usage, so the operator can see how tokens are used.
2. **Self-balancing.** The orchestrator chooses Claude Code, Codex, a free model, or a self-hosted model on another harness, according to the weekly usage limits.

## 2. Goals and non-goals

Goals:

- G1. Every fleet pane, orchestrators included, on all 5 harnesses and across every project on the machine, is visible live with: project, role, teammate, harness, model, effort, task type, tokens by class, cost, burn rate.
- G2. The space answers "where do tokens go": by phase, teammate, harness, project, plan unit, orchestrator versus worker, cache efficiency, and idle burn.
- G3. Pool headroom (5-hour, weekly, model-scoped weekly) for Claude, Codex, OpenCode free and local sits next to the spend.
- G4. `horch spawn` refuses or substitutes when a pool cannot serve the request. The orchestrator can see and explain the decision before it spawns.
- G5. The whole feature is verifiable without the operator's machine, in a remote cloud session, with hermetic tests (section 16).

Non-goals (v1):

- An OpenTelemetry collector, a web UI, or remote machines.
- Counting sessions that are in no ledger, such as the operator's own interactive Claude sessions. They appear only indirectly, in pool percentages.
- Moving a live orchestrator to another harness. v1 hands off instead (section 13.6).
- Converting fleet tokens into "percent of pool". Pools also drain from non-fleet use, so the space shows both and does not claim to relate them.

## 3. Requirements

Every requirement has an ID. Tests carry the ID in their name (section 16.4), and `scripts/check-req-coverage.sh` fails when an ID has no test.

### Telemetry (TEL)

| ID | Requirement |
|---|---|
| TEL-01 | Every ledger record with a known session id is counted, for agents `claude`, `codex`, `opencode`, `pi`, `prime`. A record that cannot be read is listed with a reason, never counted as 0. |
| TEL-02 | `horch fleet` creates a ledger record for its orchestrator, with a session id, and the orchestrator is counted like any worker. |
| TEL-03 | Token classes are kept separate: `input` (fresh, uncached), `cache_write_5m`, `cache_write_1h`, `cache_read`, `output`, `reasoning`. `reasoning` is a subset of `output`, reported for information. |
| TEL-04 | Claude: usage is deduplicated by `message.id` (the last record wins). `model == "<synthetic>"` is skipped. `<sid>/subagents/*.jsonl` is counted and attributed to the parent record. |
| TEL-05 | Codex: usage comes from `token_usage_record`, 1 per unique `response_id`, so the final response is counted. `token_count` is used only when a rollout has no `token_usage_record`. |
| TEL-06 | OpenCode: usage is read from `opencode.db` in read-only mode, per completed assistant message, for the record's `ses_` id. |
| TEL-07 | Reading is incremental. A restart resumes from saved cursors. No event is ever counted twice, including after truncation, rotation, or a crash between append and cursor save. |
| TEL-08 | Every event carries its task type: `kind` (orchestrator or worker), `phase`, and `plan` (the plan-file slug from the task text). |
| TEL-09 | All ledgers under the state root are read, so every project on the machine is covered. |
| TEL-10 | For the same records and the same time range, `horch usage` totals equal `horch cost` totals, token class by token class, and the costs match to 1e-6 USD. An event whose model has no price is never counted as $0: both commands list it as unpriced, with its model and tokens. |
| TEL-11 | No message content, prompt text, email address, account id or credential is written to any telemetry or quota file. |

### Quota (QUO)

| ID | Requirement |
|---|---|
| QUO-01 | The Claude probe uses the `get_usage` control request through Claude Code's own client. It sends no user message, starts no model turn, and never passes `ANTHROPIC_API_KEY` to the child. |
| QUO-02 | The Codex probe uses `account/rateLimits/read` through `codex app-server`. It starts no thread and no turn. |
| QUO-03 | When a probe fails, the newest on-disk signal is used instead (Claude 429 entries, Codex rollout `rate_limits`), with its age shown. |
| QUO-04 | Windows are identified by duration, never by position. Units are normalized to a fraction 0-1 and UTC timestamps. Model-scoped windows keep their scope. |
| QUO-05 | Pool states follow section 11.4. Every threshold is a setting. |
| QUO-06 | OpenCode Zen enters a cooldown after a `FreeUsageLimitError`. The local pool is `broken` when `pi --version` fails or the model is missing from `ollama list`. |
| QUO-07 | Only the collector probes on a schedule. A CLI probes on demand only when no collector runs and the reading is stale. |

### Space (SPC)

| ID | Requirement |
|---|---|
| SPC-01 | At most 1 collector runs per state root. A stale lock (its pid is dead) is broken and reported. |
| SPC-02 | `horch telemetry` becomes a read-only viewer when the lock is held. Any number of viewers can run. |
| SPC-03 | `horch telemetry ensure` opens the collector in its own herdr workspace with `--no-focus`. It never focuses, moves, splits or tiles any existing pane, and it does nothing when a live collector runs the current binary. A live collector on another binary is stopped (by pid and start time) and replaced. A live collector that horch cannot stop safely (it recorded no start time) is left running: `ensure` warns on stderr with the pid and the way to stop it by hand, reports it live and exits 0. A start leaves 1 `horch telemetry` workspace: it runs the collector in an old collector workspace (labelled `horch telemetry`, 1 pane, not the live collector's) when one exists, and closes the other old ones after the new collector holds the lock. It never closes a workspace with another label or the live collector's workspace. |
| SPC-04 | The screen shows pools, live panes, rollups and insights, with keys `g`, `w`, `p`, `q` (section 12.4). |
| SPC-05 | `horch usage` and `horch quota` print the same data as the screen, as text or `--json`. |
| SPC-06 | `horch fleet` calls `ensure`. If `ensure` fails, the fleet still starts and prints 1 warning line. |

### Balancing (BAL)

| ID | Requirement |
|---|---|
| BAL-01 | Teammates declare `fallbacks: [<teammate>, ...]`. A substituted spawn keeps the original persona, base, phase and skills, and takes the fallback's launch settings (section 13.2). |
| BAL-02 | `horch teammates --check` enforces the fallback rules in section 13.3. |
| BAL-03 | The decision function follows the table in section 13.4 exactly, including the tight-pool headroom rule. |
| BAL-04 | The gate runs in `spawn.rs` after `model_is_spawnable` and before any ledger or pane side effect. `--exact`, `--force` and `HORCH_BALANCE` behave as specified. |
| BAL-05 | A substituted record stores `via` and `substitution_reason`. A resume of that record launches the harness it ran on. |
| BAL-06 | `horch route <teammate> [--json]` prints the decision the gate would make, with no side effects. |
| BAL-07 | The orchestrator briefing gains a `== Usage limits ==` section in `teammates/_base/fleet-orchestrator.md`. `golden_prompts.rs` gains a named sanctioned block for it. |
| BAL-08 | `horch fleet auto` picks the orchestrator flavor from pool states and prints the reason. The default flavor stays `opus`. |
| BAL-09 | No automatic decision ever selects a trains-on-input teammate (`opencode-*`). |

### Non-functional (NFR)

| ID | Requirement |
|---|---|
| NFR-01 | `cargo test --workspace` is hermetic: no network, no real harness binary, no herdr server, no file outside a temp dir. |
| NFR-02 | Budgets: a steady-state tick with 50 live sessions takes under 200 ms. A cold start over 1 GB of transcripts takes under 30 s. Collector RSS stays under 150 MB. The collector keeps at most 320 B in memory per stored event (a test guard), and a steady tick reads only the ledgers and transcripts that changed. A tick holds at most 1 batch of new events (10,000) before it stores them (a test guard), so a first read of a large history does not hold the history. Measured on the 1 GB corpus (1,541,871 events, W18, a loaded host): cold tick 13.8 to 15.9 s before and 14.8 to 23.4 s after batches, with the same thread CPU (12.0 to 12.6 s); the difference is the wait for 1 event-file sync per batch. Peak RSS 3.4 GB before and 0.51 to 0.57 GB after; most of what is left is the index of 1.5 million stored events at about 200 B each. |
| NFR-03 | Existing tests, `horch teammates --check` and the prompt goldens stay green. Every touched `.rs` file is rustfmt-clean. |
| NFR-04 | Linux and macOS both pass (the cloud VM is Linux; the operator runs macOS). No Unix-only call without a Windows `cfg` path. |
| NFR-05 | New runtime crates: only `ratatui` and `crossterm`, in the `horch` crate only. SQLite is reached through the `sqlite3` CLI. |

## 4. Decisions

### 4.1 Design decisions (settled)

| # | Decision | Rejected alternative, and why |
|---|---|---|
| D1 | Transcripts are the token source. | OpenTelemetry: Claude OTel drops the 5m/1h cache split and thinking tokens, and stamps `user.email` and account ids on every record. Codex OTel needs a network listener and has no console exporter [S]. |
| D2 | Limits are read through each harness's own client (`get_usage`, `account/rateLimits/read`): about 1.1 s, no model turn, and horch never handles a credential [Q]. | Direct calls to `/api/oauth/usage` and `/wham/usage`: horch would have to read and hold OAuth tokens. The Claude `statusLine` feed: the operator's own status line already uses that slot [S]. |
| D3 | 1 collector per machine writes files. Everything else reads those files. | A collector per fleet: N fleets would read every transcript N times and probe N times. A herdr-hosted data plane: herdr sees 1 of 8 sessions per socket and cannot find a pane by label [H]. |
| D4 | The collector keeps no state the files cannot rebuild. | An in-memory aggregator: a restart would lose history. |
| D5 | Every pane is a ledger record, the orchestrator included. | Separate bookkeeping for orchestrators: 2 code paths, and `horch cost` would stay blind. |
| D6 | A fallback borrows launch settings and keeps the persona. | Twin teammate files (`researcher-codex`, ...): 8 new files that drift apart. |
| D7 | The gate sits next to the existing top-tier gate, and a pure `decide()` function makes the choice. | Balancing only in the orchestrator's prompt: nothing enforces it, and a cheaper worker model never sees it. |
| D8 | SQLite through the `sqlite3` CLI. | `rusqlite` with bundled SQLite: a C build in every build, for a single reader. |

### 4.2 Operator decisions (defaults used by this document)

| # | Question | Default here | Alternative |
|---|---|---|---|
| OD1 | Balancing mode | `auto`: substitute when exhausted, and when tight with 2x headroom elsewhere | `advise`: print, never substitute |
| OD2 | Free tiers (`opencode-*`) | Never automatic. The orchestrator chooses them explicitly for public work. | Automatic for projects the operator marks public |
| OD3 | Build timing | After the Claude reset on 2026-10-02 14:00Z, or now on extra-usage credits | - |
| OD4 | Location of the space | Its own herdr workspace, `horch telemetry` | A pane in every fleet's grid |
| OD5 | Background probe cadence | 15 min, plus on demand before a spawn when older than 10 min | 5 min |
| OD6 | `horch fleet` default flavor | Stays `opus`; `auto` is opt-in | `auto` by default |

---

## Part II - Design

## 5. Architecture

```
 ~/.local/state/horch/                          harness transcripts (read-only)
 ├── <slug>.json   (1 ledger per project) ─┐    ~/.claude/projects/<slug>/<sid>.jsonl (+ <sid>/subagents/)
 ├── policy.json   (optional settings)     │    <codex home>/sessions/**/rollout-*-<sid>.jsonl
 └── telemetry/                            │    ~/.local/share/opencode/opencode.db (WAL)
     ├── collector.lock/  collector.json   │    ~/.pi/agent/sessions/**, prime session dirs
     ├── cursors.json                      │              │
     ├── events-YYYY-MM-DD.jsonl  <────────┼──────────────┘
     ├── snapshot.json   <─────────────────┤
     └── quota.json      <── probes: claude get_usage · codex app-server · pi/ollama health
                                           │
            ┌──────────────────────────────┴───────────────────────────┐
            │ horch telemetry   (collector + TUI; singleton by lock)   │  lives in herdr workspace
            └──────────────────────────────────────────────────────────┘  "horch telemetry", --no-focus
   readers of the files:
     horch telemetry (viewer)   horch usage   horch quota   horch route   horch spawn (gate)
     the orchestrator (through horch quota / horch usage / spawn output lines)
```

New code:

| path | content |
|---|---|
| `crates/horch-core/src/telemetry/mod.rs` | types: `Event`, `TokenClasses`, `Snapshot`, `Rollup` |
| `crates/horch-core/src/telemetry/readers.rs` | incremental readers, one per harness |
| `crates/horch-core/src/telemetry/cursor.rs` | cursor state, file identity, recent message ids |
| `crates/horch-core/src/telemetry/store.rs` | event append, dedupe index, retention, rollups |
| `crates/horch-core/src/telemetry/collect.rs` | the tick |
| `crates/horch-core/src/quota.rs` | pools, probes, normalization, states, `quota.json` |
| `crates/horch-core/src/balance_policy.rs` | fallback merge, `decide()` (named to avoid `balance.rs`, which is column balancing) |
| `crates/horch-core/src/clock.rs` | `now()` with the `HORCH_NOW` override |
| `crates/horch/src/cmd/telemetry.rs` | `horch telemetry`, `ensure`, `collect --once`, `render` |
| `crates/horch/src/cmd/usagecmd.rs`, `quotacmd.rs`, `route.rs` | the CLIs |
| `crates/horch-e2e/` | fake harness binaries and end-to-end tests (section 16.3) |

Changed code: `usage.rs` (readers move out; pricing stays), `ledger.rs` (fields), `recipes.rs` (orchestrator record, `ensure`, `auto` flavor), `spawn.rs` (gate), `teammates.rs` (`fallbacks`, roster rules), `herdr.rs` (binary seam), `agent.rs` (`herdr_bin`, `sqlite3_bin`), `main.rs` (subcommands), `cost.rs` (calls the shared readers).

## 6. Identity: the ledger

### 6.1 New `Record` fields

All fields use `#[serde(default)]` so that every existing ledger still loads.

| field | type | set by | meaning |
|---|---|---|---|
| `kind` | `"worker"` \| `"orchestrator"` | spawn / fleet | default `worker` |
| `project` | `Option<String>` | spawn / fleet, from `HORCH_PROJECT_DIR` or the cwd | absolute project path. It fixes the lossy slug [H]. |
| `plan` | `Option<String>` | spawn and `horch assign` | slug parsed from the task (rule below) |
| `workspace_id` | `Option<String>` | spawn / fleet, from `HORCH_WORKSPACE_ID` | recorded for the sidebar, which is not planned (§18, M7) |
| `via` | `Option<String>` | spawn gate | the fallback teammate whose launch settings were used |
| `substitution_reason` | `Option<String>` | spawn gate | 1 line, for example `claude 7d 100%, resets 2026-10-02T14:00Z` |

`plan` rule: the first match of `ai_docs/plans/([A-Za-z0-9._-]+)\.md` in the task text, without the extension. Otherwise empty. The rule reads only the task text. `ai_docs/` is the operator's local scratch directory and is not in git, so the named file need not exist. `horch assign` updates `plan` when it changes the task.

### 6.2 The orchestrator record

In `recipes.rs` `fleet()`, before `pane_run`:

1. Create a record: `kind: orchestrator`, `role: orchestrator`, teammate `orchestrator` or `orchestrator-codex`, agent, model and effort of the resolved flavor, phase from the teammate, task `(orchestrating)`.
2. Claude flavors: mint a UUID, pass `--session-id <uuid>`, and store it at once. Codex flavors: reuse the post-launch session discovery that codex workers use (`worker.rs` harvest, `mailbox.harvest_log`) and store the id when found.
3. The launch becomes a managed session. The orchestrator gets `HORCH_RECORD_ID` in its environment like a worker.

Consequences:

- `horch sessions` prints orchestrator records in their own section, above workers.
- `horch spawn --resume <orchestrator record>` refuses with: `record <id> is an orchestrator; restart it with horch fleet`.
- `horch done` is never called by an orchestrator. The record stays `working` until the next `horch fleet` in the same workspace marks it `done` with the summary `superseded`.

## 7. Readers

Each reader implements the following trait:

```rust
pub trait Reader {
    /// Read new complete input since `cursor`, return normalized events, advance `cursor`.
    fn poll(&self, rec: &RecordView, cursor: &mut Cursor, out: &mut Vec<Observation>) -> Result<()>;
}
pub enum Observation { Usage(Event), QuotaSignal(QuotaSignal) }
```

`QuotaSignal` covers Claude 429 entries, Codex `rate_limits` snapshots, Codex `usage_limit_exceeded`, and OpenCode `FreeUsageLimitError`. Readers feed both the event store and the quota module.

### 7.1 Per harness

| agent | files | event id (dedupe key) | token mapping | notes |
|---|---|---|---|---|
| claude | `<home>/.claude/projects/*/<sid>.jsonl`, then `<sid>/subagents/*.jsonl` | `message.id` | `input_tokens` -> input; `cache_creation.ephemeral_5m_input_tokens` -> cw5m; `cache_creation.ephemeral_1h_input_tokens` -> cw1h (if `cache_creation` is absent, `cache_creation_input_tokens` -> cw5m); `cache_read_input_tokens` -> cache_read; `output_tokens` -> output; `output_tokens_details.thinking_tokens` -> reasoning | Records for 1 `message.id` repeat once per content block [S]. The reader emits a message the first time it sees its `message.id`, and holds nothing. A later record of the same id with more tokens emits a correction `<id>+<n>` (`delta: true`) for the growth only, so no token counts twice (section 20, row B1). Claude Code 2.1.292 writes each record after the message completes, with the final usage and `stop_reason` on every record. Subagent events carry `subagent: true`. A synthetic `"error":"rate_limit"` line is a `QuotaSignal::Refusal{pool: claude, text}` [Q]. |
| codex | rollout `rollout-*-<sid>.jsonl` under `<codex home>/sessions` | `payload.response_id` | `usage.input_tokens - usage.cached_input_tokens` -> input; `cached_input_tokens` -> cache_read; `cache_write_input_tokens` -> cw5m; `output_tokens` -> output; `reasoning_output_tokens` -> reasoning | `model` and `effort` come from the latest `turn_context`. Each `token_count.rate_limits` is a `QuotaSignal::Snapshot`. `task_complete.error.codex_error_info == "usage_limit_exceeded"` is a `QuotaSignal::Refusal`. Fallback when there is no `token_usage_record`: difference of consecutive `total_token_usage` values. |
| pi | `~/.pi/agent/sessions/**/*<sid>*.jsonl` (or `PI_CODING_AGENT_SESSION_DIR`) | `<sid>:<byte offset of the line>` | `usage.input` -> input; `cacheWrite` -> cw5m; `cacheRead` -> cache_read; `output` -> output | Only `type == "message"` with `message.role == "assistant"`. `ToolResultMessage.usage` also counts, with `tool_nested: true`. |
| prime | the session file path stored in the ledger | same as pi | same as pi | |
| opencode | `~/.local/share/opencode/opencode.db` | `message.id` | from `message.data`: `tokens.input` -> input; `tokens.cache.write` -> cw5m; `tokens.cache.read` -> cache_read; `tokens.output` -> output; `tokens.reasoning` -> reasoning; `cost` kept as `harness_cost` | Query: `SELECT id, session_id, time_updated, data FROM message WHERE session_id IN (...) AND time_updated > ?1 ORDER BY time_updated`, run through `sqlite3 -readonly -json <db>`. Only messages with `data.time.completed` set. If a message is seen again with different tokens, emit a correction event with `delta: true` for the difference. |

`reasoning` is informational: it is already inside `output` for every harness, and cost uses `output` only.

### 7.2 Shared with `horch cost`

`usage.rs` keeps its price table and `Price` logic. `horch cost` calls the same readers from offset 0 with a throwaway cursor, and sums the events. That is how TEL-10 holds by construction. The 2 known bugs (Codex final response, Claude subagents) are fixed in the readers, so `horch cost` output changes. The changelog line says so.

One pricing function serves both: `usage::cost_of(prices, model, tokens)`, the price-table lookup times the token classes. The collector prices each event with it, `horch cost` prices each model's sum with it, and a reader of the store prices an event stored without a cost with it (section 8.2).

### 7.3 Unread records

A record is listed in `snapshot.unread` with 1 reason when it has: no session id yet; no transcript found; an agent of `none` (the smoke fake); `sqlite3` absent; or a parse error (the reason includes the file and line number). An unread record never contributes a 0.

## 8. Cursors and the event store

### 8.1 Cursor

```json
{ "path": "...", "dev": 16777232, "ino": 9123301, "offset": 482113,
  "recent": [ { "id": "msg_01", "tokens": { ... }, "corrections": 0 } ], "quiet_ticks": 0 }
```

- Only complete lines (ending in `\n`) are consumed. A trailing partial line stays for the next tick.
- If `ino` or `dev` changes, or the size is smaller than `offset`, the reader restarts at offset 0. The dedupe index (8.2) prevents double counting.
- OpenCode cursors store `time_updated` plus the set of message ids emitted at that exact timestamp.
- Windows: file identity uses `(volume serial, file index)` through `std::os::windows::fs::MetadataExt` under `cfg(windows)`.

### 8.2 Store

- `events-YYYY-MM-DD.jsonl` (UTC date of the event). Append-only.
- Dedupe key: `(agent, session_id, event_id)`. The collector keeps an in-memory set of keys for the retention window, rebuilt from the event files at start. Before appending, it drops duplicate keys.
- Memory: the collector keeps each stored event for as long as it runs, so it keeps only what the snapshot folds: the time, the tokens, the cost, the model and the record with its 6 group keys, as interned ids (about 200 B per event). The full events stay on disk. At start it reads each event file line by line.
- Crash safety: append the events, `fsync`, then write the cursor (temp file + rename). A crash between the 2 steps re-reads some input, and the dedupe index drops the repeats. This is what TEL-07 tests. A tick does this once per batch of at most 10,000 new events (`collect::BATCH`): a reader stops at the line that fills the batch, and its cursor points at the next unread line. The cursor file of a batch inside a tick is not synced; the tick's last cursor save is. A power loss keeps the old cursor file or a torn one, which loads as no cursors; both re-read input that is already stored.
- Retention: 35 days (setting `retention_days`). Files older than that are deleted at collector start.
- Pricing: `cost_usd` is the list price when the event was collected, or `null` when the price table did not know the model then. Every reader of the event files (`Store::open`, `store::read_all`) prices a `null` cost from the current table at read time, so a price added later reaches events already stored. A set cost is kept: it is the price at the time. A model the table still does not know stays `null`, and every sum lists it as unpriced, never as $0.

Event line:

```json
{"ts":"2026-09-28T17:40:02Z","project":"/Users/x/p","record_id":"...","session_id":"...","event_id":"msg_01",
 "role":"sonnet-3","teammate":"sonnet","via":null,"kind":"worker","agent":"claude","model":"claude-sonnet-5",
 "effort":"medium","phase":"implementation","plan":"golden-prompts-whitespace","subagent":false,"delta":false,
 "tokens":{"input":60,"cache_write_5m":0,"cache_write_1h":2502,"cache_read":91363,"output":1210,"reasoning":300},
 "cost_usd":0.0412}
```

### 8.3 The tick

Every `tick_ms` (default 2000):

1. Read every `state_root/*.json` ledger whose length or modification time changed. On a parse error, keep the last good copy of that ledger and retry next tick (a ledger write may be in flight).
2. Select records whose `updated_at` is inside the retention window.
3. Resolve each record to its reader and files. A found transcript stays found while it is a file. A record with no transcript is searched for on every tick while its `updated_at` is under 10 minutes old (a new spawn), else every 30 s: a search walks the transcript trees. Claude subagent files are listed again when a directory of the last listing changed. Poll a record only when one of its files changed its length, inode or modification time (OpenCode: the database or its `-wal` file; an empty `-wal` counts as none, because on Linux the first `sqlite3 -readonly` read of a WAL database creates an empty one).
4. Append events, then save cursors (8.2 order), once per batch of new events. A tick that polled nothing saves no cursor.
5. Hand `QuotaSignal`s to the quota module. Run a probe if one is due (section 11.2).
6. Recompute the snapshot and write it with temp file + rename.

`horch telemetry collect --once` runs exactly 1 tick and exits. The tests use it (section 16).

## 9. The snapshot

`telemetry/snapshot.json`, schema version 1:

```json
{
  "schema": 1, "generated_at": "2026-09-28T17:42:00Z",
  "collector": {"pid": 4242, "version": "0.x.y", "started_at": "..."},
  "pools": { "...": "same objects as quota.json, section 11.3" },
  "live": [
    {"project":"/Users/x/multi-herdr","record_id":"...","role":"orchestrator","teammate":"orchestrator",
     "via":null,"kind":"orchestrator","agent":"claude","model":"claude-opus-5-5","effort":"xhigh",
     "phase":"plan","plan":null,"status":"working","task_head":"(orchestrating)",
     "tokens":{...},"cost_usd":21.40,"rate_tokens_per_min":4100,"rate_usd_per_hour":3.9,
     "last_event_at":"2026-09-28T17:41:58Z","idle":false}
  ],
  "rollups": {
    "5h":    {"by_teammate":[...],"by_phase":[...],"by_agent":[...],"by_project":[...],"by_plan":[...],"by_kind":[...]},
    "today": {"...": "same shape"},
    "7d":    {"...": "same shape"}
  },
  "insights": {"orchestrator_share":0.27,"cache_hit":0.91,"idle_spend_share":0.06,"top_plan":"..."},
  "unread": [{"record_id":"...","role":"opencode-ultra-1","reason":"sqlite3 not found"}]
}
```

A rollup row is `{key, tokens{...}, cost_usd, records, done, cache_hit, cost_per_done, unpriced_events}`. `tokens` includes the unpriced events; `cost_usd` does not. Each window's rollup also has `unpriced: [{model, events, tokens{...}}]`, one entry per model with no price (left out when empty). A live row has `unpriced_events` too (left out when 0).

### 9.1 Metric definitions

| metric | definition |
|---|---|
| fresh | `input + cache_write_5m + cache_write_1h` |
| cache hit | `cache_read / (cache_read + fresh)` |
| cost | list price from the `usage.rs` table, dated `prices_as_of`. `harness_cost` (OpenCode) is shown but not summed. |
| rate | tokens (all classes) and USD in the last 5 min, scaled to per minute and per hour |
| live | `status == working`, or any event in the last 10 min |
| idle | `task` equals the idle marker `(idle - awaiting assignment)`, or no event for 10 min while `working` |
| idle spend share | cost of events that arrive while a record is idle, divided by total cost |
| cost per DONE | cost of records with `status == done` divided by their count |
| orchestrator share | cost with `kind == orchestrator` divided by total cost |

## 10. Settings

An optional `state_root/policy.json`. Every key has a compiled default, and an unknown key is an error that names the key.

| key | default | used by |
|---|---|---|
| `tick_ms` | 2000 | collector |
| `retention_days` | 35 | store |
| `probe_interval_min` | 15 | quota (OD5) |
| `probe_on_demand_age_min` | 10 | spawn gate, `horch quota` |
| `stale_after_min` | 30 | quota state `unknown` |
| `tight_used` | 0.85 | state rules |
| `exhausted_used` | 0.98 | state rules |
| `pace_factor` | 1.25 | state rules |
| `pace_min_used` | 0.50 | state rules |
| `headroom_ratio` | 2.0 | tight-pool substitution |
| `opencode_cooldown_min` | 60 | OpenCode Zen state |
| `balance_mode` | `auto` | gate (OD1). `HORCH_BALANCE` overrides it. |

Environment variables:

| variable | meaning |
|---|---|
| `HORCH_BALANCE` | `auto` \| `advise` \| `off` |
| `HORCH_NOW` | RFC 3339 time that replaces the clock everywhere. For tests. horch prints `horch: HORCH_NOW is set` to stderr once. |
| `HORCH_QUOTA_FILE` | read pool states from this file and never probe. For tests and local drills. |
| `HORCH_HERDR_BIN` | herdr binary (new seam; today `herdr` is hard-coded in `herdr.rs`) |
| `HORCH_SQLITE3_BIN` | sqlite3 binary |
| `HORCH_OPENCODE_DB` | path to `opencode.db` |
| existing: `HORCH_STATE_DIR`, `HORCH_CLAUDE_BIN`, `HORCH_CODEX_BIN`, `HORCH_PI_BIN`, `HORCH_PRIME_BIN`, `HORCH_TEAMMATES_DIR`, `CODEX_HOME`, `PI_CODING_AGENT_SESSION_DIR` | unchanged |

`usage::Locations` gains `claude_projects` and `opencode_db`, so every path a reader touches comes from `Locations`.

## 11. Quota

### 11.1 Pools

A teammate's pool is derived. It needs no new frontmatter:

| agent | model | pool |
|---|---|---|
| claude | any | `claude` |
| codex | any | `codex` |
| opencode | `opencode/*` | `opencode-zen` |
| pi | `ollama/*` | `local` |
| pi | other provider `p/*` | the pool of `p` if known, else `unknown` |
| prime | `anthropic/*` | `claude` (conservative: the credential type is unknown [Q]) |

Windows can be scoped to a model family (`seven_day_opus`, `seven_day_sonnet`, `weekly_scoped` "Fable") [Q]. A teammate's state uses the unscoped windows of its pool plus the windows scoped to its model family.

### 11.2 Probes

| pool | method | timeout | fallback |
|---|---|---|---|
| claude | Spawn `claude_bin() -p --model haiku --input-format stream-json --output-format stream-json --verbose` in an empty temp dir. Remove `ANTHROPIC_API_KEY` from the child environment. Write 1 line: `{"type":"control_request","request_id":"horch-quota-<n>","request":{"subtype":"get_usage","skip_behaviors":true}}`. Read stdout lines until an object of `"type":"control_response"` whose request id matches. Take the first nested object that has `rate_limits`. Close stdin and kill the child. | 10 s | the newest `QuotaSignal::Refusal` after the last known reset; else `unknown` |
| codex | Spawn `codex_bin() app-server`. Send `{"id":1,"method":"initialize","params":{"clientInfo":{"name":"horch","version":"<v>"}}}`, then `{"method":"initialized"}`, then `{"id":2,"method":"account/rateLimits/read","params":null}`. Read until the response with `id: 2`. Kill the child. | 10 s | the newest `QuotaSignal::Snapshot` from any rollout, with its age |
| opencode-zen | none exists [Q] | - | `cooling` for `opencode_cooldown_min` after a `FreeUsageLimitError` signal; else `ok` with `no_signal: true` |
| local | `pi_bin() --version` exits 0, and the teammate's model (without `ollama/`) is in `ollama list` | 5 s each | `broken` with the first failing command and its exit status |

Probe rules:

- The Claude probe runs `--model haiku`, so any accidental turn costs the least. The probe never writes a user message. The fake used in tests fails loudly if one arrives (section 16.3).
- Probes run inside the collector every `probe_interval_min`. `horch quota --refresh` and the spawn gate probe only when no live collector holds the lock and the reading is older than `probe_on_demand_age_min`.
- Probe output is parsed field by field into the normalized form, and every other field is dropped. Account ids, emails and spend in dollars never leave the parser (TEL-11).
- The harness version (`claude --version`, `codex --version`, read once per collector start) is stored with each reading.

### 11.3 `quota.json`

```json
{
  "schema": 1, "written_at": "2026-09-28T17:25:03Z",
  "pools": {
    "claude": {
      "state": "exhausted", "reason": "7d at 100% (>= 0.98)",
      "observed_at": "2026-09-28T17:25:02Z", "source": "get_usage", "harness_version": "2.1.283",
      "windows": [
        {"name":"5h","minutes":300,"scope_model":null,"used":0.03,"resets_at":"2026-09-28T19:19:59Z"},
        {"name":"7d","minutes":10080,"scope_model":null,"used":1.00,"resets_at":"2026-10-02T13:59:59Z"},
        {"name":"7d","minutes":10080,"scope_model":"fable","used":0.03,"resets_at":"2026-10-02T13:59:59Z"}
      ],
      "refusal_seen_at": null, "error": null
    },
    "codex": {"state":"exhausted","source":"app-server","windows":[{"name":"7d","minutes":10080,"used":0.99,"resets_at":"2026-10-03T19:15:49Z"}],"ordinary_usage_allowed":true},
    "opencode-zen": {"state":"ok","no_signal":true,"cooling_until":null},
    "local": {"state":"broken","error":"pi --version exited 1"}
  }
}
```

Normalization rules:

- Claude `get_usage` reports `utilization` in 0-100 with ISO resets. Claude stream-json reports 0-1 with epoch resets. Codex reports 0-100 with epoch resets. All become `used` in 0-1 and RFC 3339 UTC [Q].
- A window's name comes from its duration: 300 min -> `5h`, 10080 min -> `7d`, anything else -> `<n>m`. Position (`primary`, `secondary`) is ignored [Q].

### 11.4 States

For each window: `left_h = hours until resets_at`; `elapsed = 1 - left_h / (minutes / 60)`, clamped to [0.01, 1]; `pace = used / elapsed`; `headroom_per_h = (1 - used) / max(left_h, 1)`.

| state | rule. The worst window of the pool, for the teammate's model scope, decides. Rules are checked top to bottom. |
|---|---|
| `broken` | the local probe failed |
| `exhausted` | `used >= exhausted_used`; or a refusal was seen after the start of the current window; or Codex `ordinary_usage_allowed == false` |
| `cooling` | OpenCode Zen within its cooldown |
| `unknown` | no reading younger than `stale_after_min`, and the probe failed or was not allowed |
| `tight` | `used >= tight_used`; or `used >= pace_min_used` and `pace > pace_factor` |
| `ok` | otherwise |

A reading whose `resets_at` has passed is treated as `used = 0` for that window until the next reading.

## 12. The telemetry space

### 12.1 Commands

| command | behavior |
|---|---|
| `horch telemetry` | Try the lock. If acquired: collector + TUI. If held by a live pid: viewer (TUI over `snapshot.json`, refreshed every `tick_ms`). |
| `horch telemetry ensure` | If a live collector holds the lock and runs the current binary: print its location, exit 0. If it runs another binary (section 12.2): stop it, then start a new one as below. Else: `herdr workspace create --label "horch telemetry" --no-focus`, then run `horch telemetry` in its root pane, and wait up to 5 s for `collector.json` to show the new pid. |
| `horch telemetry collect --once` | Takes the lock, runs exactly 1 tick with no TUI, and releases the lock. Exit 2 if a live collector holds the lock. Tests use a private `HORCH_STATE_DIR`. |
| `horch telemetry render [--snapshot F] [--size 120x40] [--group G] [--window W]` | Render 1 frame to stdout as plain text. Used by golden tests and by the cloud verification. |
| `horch usage [--json] [--since T] [--project P] [--by teammate\|phase\|agent\|project\|plan\|kind] [--window 5h\|today\|7d]` | Report from the event store. If no live collector holds the lock, it first runs the same step as `collect --once`, so the store is current. Otherwise it reads the store as it is. Each record lists its `unpriced_models` and `unpriced_events`; the total says "plus N unpriced event(s)", and the report lists the unpriced events per model with their tokens. |
| `horch quota [--json] [--refresh]` | The pool table (section 12.4 header block). `--refresh` probes now, subject to QUO-07. |
| `horch route <teammate> [--json]` | Print the gate decision (section 13.4) with no side effects. |

### 12.2 Singleton lock

- The lock is `telemetry/collector.lock/`, created with `create_dir` (atomic), the same idiom as `Ledger::lock` [H].
- The holder writes `collector.json` with `{pid, started_at, host, herdr_session, workspace_id, pane_id, pid_start, exe}`. `pid_start` is the process start time (`procid::start_time`). `exe` is the binary it runs: `{path, len, mtime_ns}`, the path with symlinks resolved.
- Stale binary: a collector keeps running the binary it started with, so after `just install` it runs old prices and old readers. `horch telemetry ensure` compares the live holder's `exe` with the current executable; `horch install` compares it with the binary it just installed. A holder with another `exe`, or none (a horch older than the field), is stopped with SIGTERM through `procid::signal_same`, which signals only the pid that still has the recorded `pid_start`; a holder without `pid_start` is never signalled. A new collector then starts on the current binary. `horch install` starts none when none was live, and a failed restart only warns. Windows has no signal here: the stale collector is reported.
- Liveness: on Unix, `kill(pid, 0)` succeeds and `started_at` matches the process start time. On Windows, `OpenProcess` succeeds. A lock whose pid is dead is broken, and the breaker prints `horch: removed stale telemetry lock of pid <n>`.
- On exit (normal exit, SIGINT or SIGTERM), the holder removes the lock.

### 12.3 herdr integration

- `ensure` uses only `herdr workspace create --no-focus`, `herdr pane run`, read-only list calls, and `herdr workspace close` of an old collector workspace (SPC-03, W17). It never calls `focus`, `tile`, `split` or `move`, and it never closes a pane or another workspace. The fake herdr in section 16.3 records every argv, and a test asserts this list.
- The space's workspace never contains a fleet worker, so `horch tile` and `horch balance` never touch it. `horch tile` skips any workspace labelled `horch telemetry` as a second guard.
- A viewer can run in any herdr session, because the data comes from files. That is the answer to the 8-session limit [H].
- Not planned (§18, M7): a sidebar where the collector pushes `$spend` and `$pool` tokens with `workspace.report_metadata` onto fleet workspaces in its own herdr session (limits: 16 keys, 80-char values) [H]. The screen and `horch usage` cover it.

### 12.4 Screen

`ratatui` + `crossterm`. Keys: `g` cycles the grouping (teammate, phase, agent, project, plan, kind); `w` cycles the window (live, 5h, today, 7d); `p` cycles the project filter; `q` quits. A frame is a pure function `render(&Snapshot, &ViewState, Rect) -> Buffer`, which is what makes it testable.

```
horch telemetry · 2 fleets · 7 live panes · 17:42Z                     group: teammate   window: 7d
POOL          5h      7d     scoped        state       resets         headroom/h
claude        3%      100%   fable 3%      EXHAUSTED   Thu 14:00Z     0.0%
codex         -       99%    -             EXHAUSTED   Fri 19:15Z     0.0%
opencode-zen  no signal                    ok*         -              -          * public work only
local         -       -      -             BROKEN      pi --version exited 1

LIVE                           model    phase  plan                 fresh   c.read   out    $      tok/min
multi-herdr   orchestrator     opus-5.5 plan   -                    412k    9.8M     61k    21.40  4.1k
              sonnet-3         sonnet   impl   golden-prompts-w…    98k     1.2M     22k     1.10  2.3k

7D BY TEAMMATE                 tokens   $        share   cache hit   $/DONE
opus                           88M      412.00   48%     93%         9.20
orchestrator                   41M      230.00   27%     96%         -
UNREAD  opencode-ultra-1: sqlite3 not found
orchestrators 27% of spend · cache hit 91% · 6% of spend while idle
```

A cost with a `*` leaves out events with no price. The tail then has 1 line for the shown window (5h for `live`): `* UNPRICED  plus <n> event(s), <tokens> tokens, not in $: <model> <n>, ...`.

## 13. Balancing

### 13.1 `fallbacks`

A new frontmatter field: `fallbacks: [codex-sol]`, an ordered list of teammate names. Initial values:

| teammate | fallbacks |
|---|---|
| opus, backend-developer, frontend-developer, researcher, designer, product-lead, staff-engineer, architect-reviewer | `[codex-sol]` |
| sonnet, qa-engineer | `[codex-terra]` |
| codex-sol, codex-network, codex-reviewer | `[opus]` |
| codex-terra | `[sonnet]` |
| codex-luna | `[sonnet, pi]` |
| prime | `[codex-sol]` |
| pi, opencode-* , orchestrator*, orchestration-*, smoke | none |

### 13.2 Merge rule

`merged = original` with these fields taken from the fallback: `agent`, `model`, `args`, `disallowed_tools`, `permission_mode`, `inherit_plugins`, `mcp_servers`, `disabled_skills`, `env`, and every other field that `Agent::takes_*` marks as agent-specific. These fields stay from the original: `name`, `brief_description`, `base`, `persona`, `phase`, `skills`, `generic`, `hidden`. `effort` stays from the original if the fallback's agent accepts that level; otherwise it comes from the fallback. The role name stays `<original>-<n>` (for example `researcher-3`), so the orchestrator's map of roles holds.

### 13.3 Roster rules (`horch teammates --check`)

For each teammate with `fallbacks`, an error for each violation:

1. The fallback exists, is not hidden, and is spawnable (`is_spawnable`).
2. The fallback's pool differs from the teammate's pool.
3. The fallback is not a trains-on-input teammate (every `agent: opencode` teammate). This is BAL-09.
4. The merged teammate passes every existing rule, including the no-subagents rule.
5. No cycles longer than 1 hop matter, because the gate never follows a fallback's own fallbacks. A fallback that has its own `fallbacks` is allowed.
6. Warning, not an error: the persona names a tool that the fallback's agent lacks (list: `Agent`, `Task`, `TodoWrite`, `WebFetch`, `WebSearch`, `NotebookEdit`, `Skill`).

### 13.4 The decision

The code is `crates/horch-core/src/routing/decision.rs`.

```rust
pub enum Decision {
    Spawn      { teammate: String, note: Option<String> },
    Substitute { original: String, via: String, reason: String },
    Refuse     { teammate: String, reason: String, pools: Vec<PoolLine> },
}
pub fn decide(req: &Teammate, roster: &Roster, view: &QuotaView,
              mode: BalanceMode, flags: GateFlags) -> Decision
```

`decide` is pure. The time is not a parameter: it is `view.now`, inside the
`QuotaView`. `RoutingDecision` is a typed view of `Decision`
(`impl From<&Decision> for RoutingDecision`); the JSON stays `Decision`'s.

| requested pool state | mode `off` | mode `advise` | mode `auto` |
|---|---|---|---|
| `ok` | Spawn | Spawn | Spawn |
| `tight` | Spawn | Spawn + NOTE | Substitute if the first fallback whose pool is `ok` has `headroom_per_h >= headroom_ratio x` the requested pool's; else Spawn + NOTE |
| `exhausted`, `broken`, `cooling` | Spawn | Spawn + NOTE | Substitute with the first fallback whose pool is `ok` or `tight`; else Refuse |
| `unknown` | Spawn | Spawn + NOTE | Substitute with the first `ok` fallback; else Spawn + NOTE. Never Refuse on `unknown`. |

Flags and edge cases:

- `--exact` never substitutes. Where the table says Substitute, the result is Spawn + NOTE for `tight` and `unknown`, and Refuse for `exhausted`, `broken` and `cooling`.
- `--force` turns every Refuse into Spawn + NOTE.
- A teammate without `fallbacks` has no candidate, so the "else" branch of its cell applies: Spawn + NOTE for `tight` and `unknown`, Refuse for `exhausted`, `broken` and `cooling`.

Output lines, printed to stdout before the pane id (the orchestrator reads them):

- `NOTE: claude pool tight (7d 88%, pace 1.4x, resets Thu 14:00Z). Fallback codex-sol is ok.`
- `SUBSTITUTED: researcher runs on codex-sol. Reason: claude 7d 100%, resets 2026-10-02T14:00Z.`
- `REFUSED: opus cannot start. claude exhausted (7d 100%, resets Thu 14:00Z). codex exhausted (7d 99%, resets Fri 19:15Z). Options: wait, --force, or choose a teammate yourself.` The exit code is 3.

### 13.5 The spawn gate and `horch route`

- The gate is the private function `gate` in `crates/horch-core/src/execution/plan.rs`. `plan_launch` calls it after the final `Roster::model_is_spawnable` check and before any role, ledger or pane side effect [Q]. `needs_gate` decides whether the gate runs.
- A resume (`--resume`) skips the gate. It launches the recorded `agent` and `model`, which for a substituted record are the fallback's (BAL-05).
- A refusal returns `PlanError::Refused` with the `RoutingDecision` and the REFUSED line. The `horch spawn` command prints the line and exits 3.
- `horch route` (`crates/horch/src/cmd/route.rs`) calls the same `decide()` and prints the same line, plus a table of the pools involved. With `--json` it prints `{decision, teammate, via, reason, line, mode, pools}`.

### 13.6 The orchestrator's part

New section in `teammates/_base/fleet-orchestrator.md`, placed before `== Protect your context ==` (data, not code):

```
== Usage limits ==
Run horch quota before you spawn a batch of workers. It shows each pool: claude, codex, opencode-zen, google, local.
Treat NOTE:, SUBSTITUTED: and REFUSED: lines from horch spawn as facts. Adjust the plan to them.
Use horch route <teammate> to see the decision before you spawn.
When 2 teammates fit the work equally, choose the one whose pool has more headroom per hour.
Use opencode-* only for public or open-source work. Use pi for private, simple work when the local pool is ok.
If your own pool becomes tight, write a handoff with horch:handoff and tell the operator.
```

`horch fleet auto` (a new `FleetFlavor::Auto`): choose `Opus` if the claude pool is `ok` or `tight`; else `Sol` if the codex pool is `ok` or `tight`; else the flavor whose pool resets first. Print `fleet: auto chose sol - claude 7d 100% (resets Thu 14:00Z), codex 7d 12%.` When the orchestrator's own pool becomes `tight`, the snapshot marks its row. The briefing line above tells it to write a handoff.

## 14. Failure modes

| failure | behavior |
|---|---|
| A ledger is mid-write and does not parse | keep the last good copy; retry next tick |
| A transcript is deleted | cursor dropped; events kept; record listed in `unread` if still live |
| A transcript is rewritten (compaction, new inode) | restart at 0; dedupe drops the repeats |
| `sqlite3` is missing | OpenCode records go to `unread` with the reason |
| `opencode.db` is locked | retry next tick; WAL makes this rare [S] |
| A probe hangs | killed at the timeout; fallback signal used; `error` recorded |
| A probe response shape changes | parse error recorded with the harness version; state from the fallback; never `ok` on a parse failure |
| 2 fleets run `ensure` at the same time | 1 wins the lock; the other sees a live holder and prints its location |
| The collector is killed | the lock goes stale; the next `ensure` or `horch telemetry` breaks it and resumes from cursors |
| The clock jumps | windows use `resets_at` from the reading; events keep their transcript timestamps |
| herdr is absent (`ensure`) | `ensure` fails with 1 line; `horch fleet` continues (SPC-06) |

## 15. Security and privacy

- Readers extract usage fields only. They never copy message content, tool input or tool output into any file (TEL-11).
- Probes never touch a credential. Each harness uses its own login. `ANTHROPIC_API_KEY` is removed from the Claude probe's environment (QUO-01).
- Probe responses are parsed into the normalized fields, and everything else is dropped, including account ids, emails and dollar amounts.
- The `task_head` in the snapshot is the first 60 characters of the task text, which is already in the local ledger. Nothing leaves the machine.
- The telemetry files are created with mode `0600` on Unix.

---

## Part III - Build and verification

## 16. Verification in a remote Claude cloud session

### 16.1 What the cloud VM has and lacks

| capability | in a cloud session | consequence for this design |
|---|---|---|
| the repo | cloned from GitHub at the chosen branch | this document and the 3 reports must be committed before the session starts |
| OS | Linux | NFR-04; no macOS-only assumption |
| Rust toolchain | check with `cargo --version`; else install `rustup` in the setup script | - |
| crates.io | with the network level at "Trusted" | needed for `ratatui` and `crossterm` |
| `sqlite3` CLI | may be absent; install with `apt-get install -y sqlite3` in the setup script | `HORCH_REQUIRE_SQLITE=1` makes the OpenCode tests fail instead of skip |
| herdr | absent | the required checks use `fake-herdr`. An optional real check uses `herdr server` (headless) if `herdr-install` can reach GitHub releases. |
| codex, opencode, pi, prime, ollama | absent | fakes only |
| `claude` | present (the session's own CLI) | tests must never reach it. The e2e harness puts the fakes first on `PATH` and sets every `HORCH_*_BIN`. A guard test fails if a real harness binary is used. |
| operator transcripts, ledgers, pools | absent | the fixture corpus (16.3) stands in. Section 17 checks the fixtures against reality. |
| the fleet (`horch fleet`, workers) | not available | the cloud session builds milestone by milestone, alone |

### 16.2 Seams

Every external dependency has a seam, and every seam has a fake:

| dependency | seam | fake |
|---|---|---|
| time | `clock::now()`, `HORCH_NOW` | a fixed RFC 3339 time |
| state root | `HORCH_STATE_DIR` (exists) | a temp dir per test |
| transcript locations | `usage::Locations` (extended) | fixture dirs |
| teammates | `HORCH_TEAMMATES_DIR` (exists), `Roster::builtin()` | the repo roster, or a test roster |
| claude | `HORCH_CLAUDE_BIN` (exists) | `fake-claude` |
| codex | `HORCH_CODEX_BIN` (exists) | `fake-codex` |
| pi | `HORCH_PI_BIN` (exists) | `fake-pi` |
| ollama | `HORCH_OLLAMA_BIN` (new) | `fake-ollama` |
| herdr | `HORCH_HERDR_BIN` (new; `herdr.rs` calls `herdr_bin()`) | `fake-herdr` |
| sqlite3 | `HORCH_SQLITE3_BIN` (new) | the real `sqlite3` on a fixture database |
| quota | `HORCH_QUOTA_FILE` (new) | fixture `quota.json` files |
| TUI | `render()` into a `ratatui` `TestBackend` | text goldens |

### 16.3 Test infrastructure

**Crate `crates/horch-e2e`** (`publish = false`). It holds the fake binaries and the end-to-end tests. The fakes are Rust binaries, so they work on Linux, macOS and Windows. Each fake appends 1 JSON line per call to `$HORCH_FAKE_LOG`: argv, the environment keys it saw, stdin lines, and any violation. Each fake chooses its behavior from `$HORCH_FAKE_SCENARIO`.

| fake | scenarios | violations it records |
|---|---|---|
| `fake-claude` | `usage_ok`, `usage_exhausted`, `usage_scoped`, `usage_hang`, `usage_garbage` | a stdin line with `"type":"user"`; `ANTHROPIC_API_KEY` present in its environment |
| `fake-codex` | `limits_weekly`, `limits_5h_weekly`, `limits_not_allowed`, `hang`, `error` | any method other than `initialize`, `initialized`, `account/rateLimits/read` |
| `fake-herdr` | canned JSON for `workspace list`, `workspace create`, `pane run`, `session list` | any `focus`, `tile`, `split`, `move`, `close`, `swap` call |
| `fake-pi`, `fake-ollama` | `ok`, `crash`, `model_missing` | - |

The e2e tests locate `horch` next to the fake binaries in the same target dir. `just verify` builds all binaries first.

**Fixture corpus** at `crates/horch-core/tests/fixtures/telemetry/`:

```
claude/-work-alpha/11111111-....jsonl        repeated message.id, <synthetic> 429 line, thinking tokens, 1h cache
claude/-work-alpha/11111111-.../subagents/agent-a1.jsonl
codex/sessions/2026/09/27/rollout-...-01a0....jsonl   token_usage_record after the last token_count; rate_limits; usage_limit_exceeded
codex/sessions/2026/09/11/rollout-...-01b0....jsonl   positional primary=5h, secondary=7d (old plan shape)
codex/sessions/2026/09/12/rollout-...-01c0....jsonl   no token_usage_record (older CLI): fallback path
pi/sessions/--work-beta--/..._<uuid>.jsonl   assistant usage, ToolResultMessage usage
prime/<record-dir>/<sid>.jsonl
opencode/opencode.sql                        CREATE TABLE session/message/part + rows; 1 message updated twice
ledgers/-work-alpha.json, -work-beta.json    2 projects; orchestrator record; 1 record per agent; 1 without session id
probes/claude-get-usage-*.jsonl, codex-ratelimits-*.json   probe responses, incl. account ids and emails that must not persist
quota/*.json                                 1 file per pool-state row of section 11.4
EXPECTED.md                                  hand-computed totals per record and per rollup, with the arithmetic
```

Fixture rules:

- Synthetic data only. No line comes from a real transcript.
- Field names and nesting follow the research reports: Claude 2.1.283, Codex 0.157.1, OpenCode 1.18.2, pi 0.85.1, Prime 0.9.4.
- Every fixture file starts with a header comment in `README.md` that names the harness version it models.
- `EXPECTED.md` is the oracle. Its numbers are computed by hand, not by running the code under test.

Appendix A has 1 minimal line per harness.

**Scripts:**

- `scripts/check-req-coverage.sh`: extracts every ID from the tables in section 3, runs `cargo test --workspace -- --list`, and fails for every ID without a test whose name starts with the lowercase ID (`tel_05_`).
- `scripts/verify-telemetry-e2e.sh`: the hermetic scenario in 16.5.
- `just verify`: `cargo build --workspace --bins`, `cargo test --workspace`, `scripts/check-req-coverage.sh`, `scripts/verify-telemetry-e2e.sh`, `rustfmt --check` on the changed `.rs` files, and `HORCH_TEAMMATES_DIR=teammates horch teammates --check`.

### 16.4 Test catalogue (traceability)

Test names start with the requirement ID. U = unit test in the module; I = integration test in `crates/*/tests`; E = e2e test with fakes; G = golden text.

| ID | tests (name prefix: what it proves) | type |
|---|---|---|
| TEL-01 | `tel_01_every_agent_is_read`: 1 record per agent -> events for all 5. `tel_01_unread_has_reason`: no session id, missing file, agent `none` -> listed in `unread`, never 0. | I |
| TEL-02 | `tel_02_fleet_writes_orchestrator_record` (fake-herdr, fake-claude): the record exists with `kind: orchestrator` and a session id that equals the `--session-id` argv. `tel_02_resume_refuses_orchestrator`. | E, U |
| TEL-03 | `tel_03_classes_per_agent`: every class mapped per section 7.1; reasoning is not added to cost. | U |
| TEL-04 | `tel_04_claude_dedupes_message_id`, `tel_04_claude_skips_synthetic`, `tel_04_claude_counts_subagents` | U |
| TEL-05 | `tel_05_codex_counts_final_response`: the sum of `token_usage_record` equals the last `thread_token_usage`, and exceeds the last `token_count`. `tel_05_codex_fallback_without_records`. | U |
| TEL-06 | `tel_06_opencode_reads_completed_messages`, `tel_06_opencode_update_emits_delta` (both need `sqlite3`; they fail under `HORCH_REQUIRE_SQLITE=1` when it is absent) | I |
| TEL-07 | `tel_07_append_in_stages_equals_whole`: split each fixture at every line boundary and at 3 mid-line offsets, tick after each part, and compare with 1 tick over the whole file. `tel_07_restart_resumes`. `tel_07_crash_between_append_and_cursor` (inject a failure after the append) -> no duplicate keys. `tel_07_rotation_resets`. | I |
| TEL-08 | `tel_08_plan_slug_parsed` (table of task texts), `tel_08_kind_and_phase_on_events` | U |
| TEL-09 | `tel_09_two_projects_both_counted` | I |
| TEL-10 | `tel_10_usage_equals_cost`: `horch usage --json` against `horch cost --json` on the same fixtures, per record and per class (E). `tel_10_usage_and_cost_agree_per_record_with_an_unpriced_model`: one fixture with a priced and an unpriced model, before and after the stored costs are erased (U). `tel_10_a_stored_event_without_a_cost_is_priced_at_read_time`, `tel_10_rollups_list_unpriced_events_and_never_count_them_as_zero`, `tel_10_the_screen_lists_unpriced_events` (U) | E, U |
| TEL-11 | `tel_11_no_content_or_identity_persisted`: after a full e2e run, scan every file under the state dir for fixture sentinels (`SENTINEL-CONTENT`, the fixture emails, `acct_`, `user.email`) -> 0 hits | E |
| QUO-01 | `quo_01_claude_probe_protocol`: fake-claude logs exactly 1 stdin line, of subtype `get_usage`; no `user` line; `ANTHROPIC_API_KEY=dummy` set in the parent is absent in the child; `--model haiku` in argv | E |
| QUO-02 | `quo_02_codex_probe_protocol`: exactly `initialize`, `initialized`, `account/rateLimits/read` | E |
| QUO-03 | `quo_03_fallback_on_probe_failure`: `hang` and `error` scenarios -> the rollout snapshot is used, and its age is shown | E |
| QUO-04 | `quo_04_windows_by_duration` (the 09-11 and 09-27 rollouts), `quo_04_units_normalized` (0-1, 0-100, ISO, epoch), `quo_04_scoped_windows` | U |
| QUO-05 | `quo_05_state_table`: 1 case per row of section 11.4, plus the edges (0.849/0.85, 0.979/0.98, pace 1.25), plus `policy.json` overrides | U |
| QUO-06 | `quo_06_opencode_cooldown`, `quo_06_local_broken` (fake-pi `crash`, fake-ollama `model_missing`) | U, E |
| QUO-07 | `quo_07_cli_does_not_probe_when_collector_live`, `quo_07_probe_cadence` (with `HORCH_NOW` stepped) | E |
| SPC-01 | `spc_01_second_collector_is_refused`, `spc_01_stale_lock_is_broken` (a lock with a dead pid) | I |
| SPC-02 | `spc_02_viewer_when_locked` | I |
| SPC-03 | `spc_03_ensure_argv` (fake-herdr log: `workspace create ... --no-focus`, `pane run`, and only read-only calls besides), `spc_03_restart_after_a_dead_collector_leaves_1_telemetry_workspace`, `spc_03_restart_closes_an_old_workspace_it_cannot_reuse`, `spc_03_a_live_collector_keeps_every_workspace` (E), `spc_03_old_collector_workspaces` (U), `spc_03_ensure_noop_when_live` (E: a live collector that records the binary under test), `spc_03_ensure_warns_and_succeeds_for_a_collector_it_cannot_stop` (E). `spc_03_ensure_leaves_a_collector_it_cannot_stop` (U). `spc_03_ensure_restarts_a_collector_on_an_older_binary`, `a_collector_on_another_binary_is_found`, `stop_ends_the_recorded_collector`, `stop_never_signals_a_process_that_is_not_the_recorded_collector` (U, with a fake collector record) | E, U |
| SPC-04 | `spc_04_render_*` goldens at 120x40 and 80x24, for every `g` and `w` value, from `snapshot-fixture.json` | G |
| SPC-05 | `spc_05_usage_json_schema`, `spc_05_quota_json_schema` (every field in sections 9 and 11.3 present, with its type) | E |
| SPC-06 | `spc_06_fleet_survives_ensure_failure` (fake-herdr fails `workspace create` for the label) | E |
| BAL-01 | `bal_01_merge_rule`: a field-by-field table for researcher -> codex-sol | U |
| BAL-02 | `bal_02_roster_rules`: 1 failing roster per rule in section 13.3, plus `bal_02_repo_roster_passes` | U |
| BAL-03 | `bal_03_decision_table`: every cell of the table in section 13.4, x 3 modes, x the flags | U |
| BAL-04 | `bal_04_gate_before_side_effects`: with `HORCH_QUOTA_FILE` set to exhausted-everything, a refused spawn leaves the ledger byte-identical and fake-herdr logs no `pane split`. `bal_04_exit_code_3`. | E |
| BAL-05 | `bal_05_substituted_record_fields`, `bal_05_resume_uses_via` | E |
| BAL-06 | `bal_06_route_matches_gate`: for every quota fixture, `horch route --json` equals the decision the gate logs | E |
| BAL-07 | `bal_07_usage_limits_block_sanctioned` in `golden_prompts.rs`: a named block with its reason | G |
| BAL-08 | `bal_08_fleet_auto_choice`: 4 quota fixtures -> flavor chosen, printed reason | E |
| BAL-09 | `bal_09_never_trains_on_input`: property test: every roster permutation of fallbacks with an `opencode-*` entry fails `--check`, and `decide()` never returns an opencode teammate | U |
| NFR-01 | `nfr_01_no_real_binaries`: the e2e harness asserts that every spawned program's path is inside the fakes dir; a `PATH` without the fakes fails the harness | E |
| NFR-02 | `nfr_02_tick_budget` (`#[ignore]`; run by `just verify-perf`): a generated corpus of 50 sessions and 1 GB; `nfr_02_a_cold_read_holds_one_batch`: a cold read holds at most 1 batch, and a crash after the first batch loses no event and stores none twice | I |
| NFR-03 | the existing suite, `golden_prompts`, `horch teammates --check`, rustfmt on changed files | all |
| NFR-04 | `nfr_04_file_identity` under `cfg(unix)` and `cfg(windows)` | U |
| NFR-05 | `scripts/check-deps.sh`: `cargo tree -e normal` shows no new crate outside the allowed list | script |

### 16.5 The hermetic end-to-end scenario

`scripts/verify-telemetry-e2e.sh` runs this story against a fresh temp state dir, with the fakes first on `PATH`. Each step prints `PASS <step>` or stops with the difference:

1. `HORCH_NOW=2026-09-28T17:00:00Z`. Copy the 2 fixture ledgers and the first half of each transcript into place.
2. `horch telemetry collect --once`. The totals in `snapshot.json` equal `EXPECTED.md` stage 1.
3. Append the second half of each transcript. Step the clock by 2 s. `collect --once`. The totals equal stage 2, and the events contain no duplicate key.
4. Kill a collector started in the background with `SIGKILL` during step 3's tick. Restart it. The totals still equal stage 2.
5. `HORCH_FAKE_SCENARIO=usage_exhausted horch quota --refresh --json`. It prints claude `exhausted`, 7d `1.00`, with the reset from the fixture. The fake log shows no violation.
6. `horch route opus` prints `SUBSTITUTED: opus runs on codex-sol`, for a quota fixture with claude exhausted and codex ok.
7. `horch route opus` prints `REFUSED`, exit 3, for claude and codex both exhausted (today's real state).
8. `horch spawn opus "x"` with the fixture from step 7: exit 3, the ledger is unchanged, and fake-herdr has no `pane split`.
9. `horch telemetry render --size 120x40` equals `golden/telemetry-e2e.txt`.
10. Scan the state dir for sentinels (TEL-11): 0 hits.
11. `horch usage --json` equals `horch cost --json` per record (TEL-10).

### 16.6 Cloud session protocol

**Setup script** (the environment's setup step):

```bash
command -v sqlite3 || (sudo apt-get update && sudo apt-get install -y sqlite3)
cargo --version || (curl -sSf https://sh.rustup.rs | sh -s -- -y)
cargo fetch
```

Network level: Trusted (crates.io, the Ubuntu archive, GitHub).

**Rules for the session:**

- Start from `main` after `orchestrator-prompt-unwrap` is merged, with this document and the 3 reports committed.
- Build 1 milestone per branch: `telemetry/m0-infra`, `telemetry/m1-readers`, and so on. Stack each branch on the previous one when section 18 says it depends on it.
- Run `just verify` before every commit that claims a milestone.
- Do not run a real `claude`, `codex`, `opencode`, `pi` or `ollama` command. Do not call any usage endpoint. Do not edit `~/.claude` or `~/.codex`.
- Do not regenerate a prompt golden. A briefing change gets a named sanctioned block in `golden_prompts.rs`.
- If a fact in this document conflicts with a fixture, stop, and write the conflict in the PR body. Do not change both to agree.

**Evidence in each PR body**, under a `## Verification` heading:

1. The output of `just verify`: the test count before and after, and `0 failed`.
2. The output of `scripts/check-req-coverage.sh` for the milestone's IDs, with the test names found.
3. For M4 and later: the full output of `scripts/verify-telemetry-e2e.sh`.
4. For M6: the rendered frame from step 9, pasted as a code block.
5. A list titled `Not verifiable here`, with the section 17 steps that apply to the milestone.
6. `git diff --stat main...HEAD`, and a statement that no file outside the milestone's list changed.

**Reviewer checklist** (the operator or `codex-reviewer`):

- Every path, binary and clock read goes through a seam from 16.2.
- No reader copies `content`, `text`, `input` or `output` bodies of messages.
- The Claude probe writes exactly 1 stdin line, and removes `ANTHROPIC_API_KEY`.
- The gate is placed before the ledger write in `spawn.rs`.
- `decide()` is pure: no I/O, and time comes in as a parameter.
- Every new briefing line has a sanctioned block with its reason.
- `EXPECTED.md` shows its arithmetic, and the tests compare against it, not against output captured from the code.

## 17. Local acceptance on the operator's Mac

The cloud proves the logic against fixtures. These steps prove the fixtures match reality, and that the pieces work in herdr. `scripts/live/telemetry.sh` is the only script for these steps: it runs each one, prints `PASS`, `FAIL` or `SKIP` per step, and the dated results are in `docs/live-checks/telemetry.md`, with the exact procedure for the steps that need the operator (L3, L5). L7 kills the shared collector, so it runs only with `LIVE_TELEMETRY_KILL_COLLECTOR=1`.

| step | action | pass condition |
|---|---|---|
| L1 fixture drift | For the newest real transcript of each installed harness, extract the set of JSON key paths from the lines that carry usage. Compare with the fixture key paths. | No key path that a reader uses is missing from the real file. New keys are listed, not failed. |
| L2 cost parity | `horch usage --json --since <7 days ago>` and `horch cost --json --since <same>` on the real ledgers | equal per record and per class (TEL-10) |
| L3 probe truth | `horch quota --refresh`, then open `/usage` in an interactive Claude session and `/status` in Codex | every window within 1 percentage point, same reset times |
| L4 probe hygiene | Run `horch quota --refresh` 3 times. List files in `~/.claude/projects/*/` and `~/.codex/sessions/` newer than the first run. | no new transcript contains a user message; no new rollout contains a turn |
| L5 singleton in herdr | `horch fleet` in 2 scratch projects | 1 `horch telemetry` workspace in `herdr workspace list`; the operator's focused pane does not change; the `collector.json` pid is the same after the second fleet |
| L6 live latency | Spawn 1 `sonnet` worker with a 1-line task | its first event is in the store within 5 s after its first usage line is on disk (the script polls the transcript and the store every 0.2 s), and its totals match its transcript. The origin is the disk, not the line's `timestamp`: Claude Code writes a line only after its message completes, seconds after that timestamp, and that lag is outside horch. The limit: 1 tick (2 s) + the tick (under 0.2 s) + 2 polls (0.4 s) is 2.6 s at worst. The script prints the lag from the timestamp as INFO, never as PASS or FAIL |
| L7 crash recovery | `kill -9` the collector during L6, then `horch telemetry ensure` | totals unchanged; no duplicate event keys (`jq` check in the script) |
| L8 gate drill | `HORCH_QUOTA_FILE=<fixture: claude exhausted, codex ok, dates shifted to now> horch route researcher`, then `horch spawn researcher "Reply with DONE"` in a scratch fleet | `SUBSTITUTED` line; the record has `via: codex-sol`; the pane runs codex with the researcher briefing |
| L9 real gate | `horch route opus` on the real pools | the answer follows the pools: `REFUSED` with both reset times when both are exhausted, else `SPAWN` or `SUBSTITUTED`. The script also drills `REFUSED` on the shifted all-exhausted fixture |
| L10 perf | `just verify-perf` on the real state dir | tick under 200 ms after the first; RSS under 150 MB |

## 18. Build plan

| milestone | scope | requirements | depends on | owns (files) |
|---|---|---|---|---|
| M0 infrastructure | `crates/horch-e2e` with the fakes, `clock.rs`, the new seams (`herdr_bin`, `sqlite3_bin`, `ollama_bin`, `Locations` fields), the fixture corpus and `EXPECTED.md`, the scripts, `just verify` | NFR-01, NFR-04 (seams) | prompt-unwrap merged | `crates/horch-e2e/**`, `clock.rs`, `agent.rs`, `herdr.rs` (seam only), `usage.rs` (`Locations` only), fixtures, `scripts/*`, `justfile`, root `Cargo.toml` |
| M1 readers | the trait, 5 readers, cursors, the 2 bug fixes, `horch cost` on the shared readers | TEL-03..07, TEL-10, TEL-11 (readers) | M0 | `telemetry/readers.rs`, `cursor.rs`, `usage.rs`, `cost.rs` |
| M2 identity | the new `Record` fields, the orchestrator record, `plan` parsing, `horch sessions` sections, resume refusal | TEL-02, TEL-08 | M0 | `ledger.rs`, `recipes.rs`, `ledgercmd.rs`, `spawn.rs` (record fields only), `messaging.rs` (assign sets `plan`) |
| M3 quota | pools, probes, normalization, states, `quota.json`, `policy.json`, `horch quota` | QUO-01..07 | M0 | `quota.rs`, `cmd/quotacmd.rs`, `main.rs` |
| M4 store and CLI | store, dedupe index, tick, snapshot, `collect --once`, `horch usage` | TEL-01, TEL-07, TEL-09, SPC-05 | M1, M2 | `telemetry/store.rs`, `collect.rs`, `mod.rs`, `cmd/usagecmd.rs`, `main.rs` |
| M5 balancing | `fallbacks`, merge, roster rules, `decide()`, the gate, `route`, flags, briefing section and golden block, `fleet auto` | BAL-01..09 | M2, M3 | `teammates.rs`, `balance_policy.rs`, `spawn.rs` (gate), `cmd/route.rs`, `teammates/*.md`, `_base/fleet-orchestrator.md`, `golden_prompts.rs`, `recipes.rs` (auto flavor), `main.rs` |
| M6 the space | lock, collector loop, TUI, `render`, `ensure`, the fleet hook, `tile` guard | SPC-01..04, SPC-06, NFR-02 | M3, M4 | `cmd/telemetry.rs`, `recipes.rs` (ensure call), `tilecmd.rs` (guard), `crates/horch/Cargo.toml` |
| M7 (not planned) | sidebar values | - | M6 | none |

M7 is not planned (decision 2026-10-06, wave 2, item U-35). The telemetry space's screen and `horch usage` already show spend and pool state. herdr sidebar metadata is limited (16 keys, 80-character values, and only in the collector's own herdr session), so it cannot carry more than those 2 views. No `telemetry/sidebar.rs` exists.

M1, M2 and M3 can run in parallel after M0: they own disjoint files. `main.rs` and `recipes.rs` are edited by several milestones in small regions. The stacked branches keep those edits serial, and each PR is rebased on its parent before merge.

Local fleet mapping, if the operator builds with the fleet rather than the cloud: M0 `backend-developer`; M1, M2 and M3 each `backend-developer`, in parallel; M4 `sonnet`; M5 `opus`; M6 `frontend-developer`. `codex-reviewer` reviews M1, M3 and M5, and `qa-engineer` reviews M6.

## 19. Risks and open questions

| # | risk or question | mitigation |
|---|---|---|
| R1 | `get_usage` and `account/rateLimits/read` are experimental [Q] | Tolerant parsing, the harness version stored with each reading, the on-disk fallbacks, and a parse failure never yields `ok`. L1 and L3 re-check after every harness update. |
| R2 | The exact `control_response` envelope of `get_usage` is not documented. The parser searches for the first nested object with `rate_limits`. | L3 confirms the real shape. The fixture is updated from L1 output. |
| R3 | Each Claude probe starts a Claude process: SessionStart hooks run, and the session may appear in the session list | Cadence of 15 min (OD5). The probe runs in an empty temp dir. L4 measures what is left behind. |
| R4 | A specialist persona may assume Claude-only tools on a Codex fallback | Roster warning (13.3 rule 6). M5 audits the 8 personas. |
| R5 | The `prime` credential type is unknown [Q] | Mapped to the claude pool until checked |
| R6 | The OpenCode free window is undocumented [Q] | The cooldown is a setting |
| R7 | Claude extra usage is enabled (7.5% of the monthly limit used) [Q]. An exhausted pool may still serve requests on paid credits. | `exhausted` stays exhausted. `--force` is the explicit way to spend credits. |
| R8 | pi cannot start on this machine (Node 22.9 < 22.19) [S] | `local` shows `broken` with the reason, which is the correct state today |
| R9 | Claude `message.usage` repeats were identical on 2.1.283, but older versions grew them [S] | A record that grew emits a correction for the growth (section 20, B1), which is correct for both |

---

## Appendix A - Minimal fixture lines

Claude assistant line (the same `message.id` appears twice in the fixture):

```json
{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","timestamp":"2026-09-28T17:00:01.000Z","requestId":"req_a","message":{"id":"msg_a","model":"claude-sonnet-5","stop_reason":null,"content":[{"type":"text","text":"SENTINEL-CONTENT"}],"usage":{"input_tokens":10,"cache_creation_input_tokens":2000,"cache_read_input_tokens":50000,"output_tokens":300,"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":2000},"output_tokens_details":{"thinking_tokens":120}}}}
```

Claude refusal line:

```json
{"type":"assistant","sessionId":"11111111-1111-4111-8111-111111111111","timestamp":"2026-09-28T17:05:00.000Z","isApiErrorMessage":true,"error":"rate_limit","apiErrorStatus":429,"message":{"model":"<synthetic>","content":[{"type":"text","text":"You've hit your weekly limit · resets Oct 2 at 7am (America/Phoenix)"}]}}
```

Codex rollout lines:

```json
{"timestamp":"2026-09-27T20:44:04Z","type":"session_meta","payload":{"id":"01a0e61c-d73e-74a3-837c-b5aade8b1c38","cwd":"/work/alpha","cli_version":"0.157.1"}}
{"timestamp":"2026-09-27T20:44:05Z","type":"turn_context","payload":{"model":"gpt-5.6-sol","effort":"medium"}}
{"timestamp":"2026-09-27T20:45:00Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"cache_write_input_tokens":0,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":1200},"last_token_usage":{"input_tokens":1000,"cached_input_tokens":800,"cache_write_input_tokens":0,"output_tokens":200,"reasoning_output_tokens":50,"total_tokens":1200},"model_context_window":400000},"rate_limits":{"limit_id":"codex","primary":{"used_percent":99.0,"window_minutes":10080,"resets_at":1791054949},"secondary":null,"plan_type":"prolite"}}}
{"timestamp":"2026-09-27T20:46:00Z","type":"token_usage_record","payload":{"session_id":"01a0e61c-d73e-74a3-837c-b5aade8b1c38","turn_id":"t2","response_id":"resp_2","usage":{"input_tokens":500,"cached_input_tokens":400,"cache_write_input_tokens":0,"output_tokens":100,"reasoning_output_tokens":20,"total_tokens":600},"thread_token_usage":{"total_tokens":1800}}}
{"timestamp":"2026-09-27T20:47:00Z","type":"event_msg","payload":{"type":"task_complete","error":{"message":"You've hit your usage limit. Visit https://chatgpt.com/codex/settings/usage to purchase more credits or try again at Oct 3rd, 2026 7:15 PM.","codex_error_info":"usage_limit_exceeded"}}}
```

pi or Prime assistant line:

```json
{"type":"message","message":{"role":"assistant","provider":"ollama","model":"qwen3.8","usage":{"input":900,"output":150,"cacheRead":0,"cacheWrite":0,"totalTokens":1050,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"stopReason":"stop","timestamp":1790600000000}}
```

OpenCode (`opencode.sql`, excerpt):

```sql
CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
INSERT INTO message VALUES ('msg_o1','ses_alpha',1790600000000,1790600005000,
 '{"role":"assistant","modelID":"nemotron-3-ultra-free","providerID":"opencode","cost":0,"tokens":{"total":1300,"input":1000,"output":300,"reasoning":40,"cache":{"read":0,"write":0}},"time":{"created":1790600000000,"completed":1790600005000}}');
```

Claude `get_usage` probe response, as `fake-claude usage_exhausted` prints it. The envelope follows R2; the identity fields exist so that TEL-11 can prove they are dropped:

```json
{"type":"control_response","response":{"subtype":"success","request_id":"horch-quota-1","response":{"subscription_type":"max","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":3,"resets_at":"2026-09-28T19:19:59Z"},"seven_day":{"utilization":100,"resets_at":"2026-10-02T13:59:59Z"}},"seven_day_opus":null,"seven_day_sonnet":null,"limits":[{"kind":"weekly_scoped","percent":3,"severity":"ok","resets_at":"2026-10-02T13:59:59Z","scope":{"model":{"display_name":"Fable"}},"is_active":true}],"account_uuid":"acct_SENTINEL","email":"sentinel@example.invalid"}}}
```

Codex `account/rateLimits/read` response, as `fake-codex limits_weekly` prints it:

```json
{"id":2,"result":{"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","primary":{"usedPercent":99,"windowDurationMins":10080,"resetsAt":1791054949},"secondary":null,"planType":"prolite"},"accountId":"acct_SENTINEL"}}
```

## Appendix B - Evidence index

| claim | source |
|---|---|
| Ledger fields, `state_root`, per-project files, mkdir lock | [S] Ledger fields; [H] horch state 1-3, 7 |
| Worker env vars `HORCH_*` | [S] Ledger fields |
| Claude transcript shape, repeats, subagents dir | [S] C1 |
| Claude OTel attributes and `user.email` | [S] C2, Empirical checks |
| Codex `token_usage_record` and the under-count | [S] X1 |
| OpenCode tables, WAL | [S] O1 |
| pi and Prime usage shape | [S] pi, Prime Agent |
| Claude windows, `get_usage`, 429 entries | [Q] section 1 |
| Codex windows, positional fields, `account/rateLimits/read` | [Q] section 2 |
| OpenCode `FreeUsageLimitError` | [Q] section 3 |
| Spawn routing points, `spawn.rs:136`, ledger write at 164-178 | [Q] Routing points |
| herdr sessions, labels, metadata tokens, `--no-focus` | [H] herdr capabilities |
| No TUI, async, SQLite or OTLP crates today | [H] Dependencies |

---

## 20. Build notes (2026-09-29, cloud session)

M0 to M6 were written on one branch, `claude/intelligent-heisenberg-vwfjre`, not
one branch per milestone. The prerequisite branch `orchestrator-prompt-unwrap`
does not exist on the remote, so the section 13.6 block is inserted into the
current prose. **Nothing was compiled or run:** this environment's egress
policy refuses `static.crates.io`, so `cargo` cannot download any crate (the
existing workspace does not build here either). Every file was parse-checked
and formatted with `rustfmt`. Run `just verify` where crates.io is reachable;
the render goldens are then written once with `HORCH_BLESS=1` and reviewed.

Where the build departs from sections 1-19, and why:

| # | design | built | why |
|---|---|---|---|
| B1 | Claude: a pending entry per session, emitted on a new `message.id` or 2 quiet ticks | emit on first sight; a repeat with more tokens emits a correction `<id>+<n>` (`delta: true`) | [S] C1: subagent lines carry start-of-stream usage; corrections count growth without a delay, and staged reads equal whole reads by construction |
| B2 | Codex: `token_count` only when a rollout has no `token_usage_record` | the mode is chosen by `session_meta.cli_version` (>= 0.157.0 counts records); a file with no version switches to records at its first record | an incremental reader cannot know the file's future; [S] X1 |
| B3 | `cacheWrite -> cw5m` for pi | `cacheWrite - cacheWrite1h -> cw5m`, `cacheWrite1h -> cw1h` | [S] pi: 0.87 reports the 1h subset |
| B4 | `<sid>/subagents/*.jsonl` | every `*.jsonl` under `<sid>/subagents/`, nested | [S] C1 |
| B5 | Appendix A `get_usage` shape | `seven_day_opus`, `seven_day_sonnet`, `model_scoped[]` and `limits[]` inside `rate_limits`; `severity: normal` | [Q] 1a, from the shipped schema |
| B6 | ratatui + crossterm | the frame is plain text rendered by horch; crossterm only for raw mode and keys | the goldens compare text either way; one crate fewer (NFR-05 still holds) |
| B7 | the next `horch fleet` in the same workspace supersedes an orchestrator | the next `horch fleet` supersedes `working` orchestrator records whose workspace is no longer open | `horch fleet` always creates a new workspace, so "the same workspace" never recurs |
| B8 | lock liveness: pid alive and start time matches | pid alive only (`kill(pid, 0)`, `tasklist` on Windows) | stable Rust has no portable process start time; the lock is also removed on every exit |
| B9 | Windows file identity `(volume serial, file index)` | `(0, creation time)` | the file-index accessors are unstable in Rust |
| B10 | `NOTE: ... resets Thu 14:00Z` | `Fri 14:00Z` | 2026-10-02 is a Friday |
| B11 | stale readings are `unknown` | a `HORCH_QUOTA_FILE` reading is never stale | a drill fixture must keep its meaning on the operator's real clock |
| B12 | - | `unread` reasons carry no machine path | the screen and its goldens must be the same on every machine |
| B13 | - | new seams: `HORCH_PROBE_TIMEOUT_MS`, `HORCH_FAULT=after-append|abort-after-append` (collector), `HORCH_FAKE_FAIL_LABEL` (fake-herdr), `HORCH_BLESS=1` (render goldens) | tests need short timeouts, a crash window and one failing label |
| B14 | settings in `quota.rs` | `policy.rs`; the singleton lock in `telemetry/lock.rs` | both are shared by the collector, the gate and the CLIs |
| B15 | `horch route`: no side effects | it may run the same on-demand probe as the gate (it writes `quota.json` only) | otherwise route and gate could decide on different readings (BAL-06) |
| B16 | `collect --once` | also probes when the collector schedule says a probe is due | it is the collector, for one tick |
| B17 | the codex orchestrator's commands | `horch quota` and `horch route` added to `_base/codex-orchestrator-execpolicy.md` | the briefing names them; a codex orchestrator could not run them otherwise |

Evidence corrections, from the reports: the "8 sessions per socket" figure
is not in the herdr docs [H]; `report_metadata` allows 16 keys per report and
32 per workspace [H]; pi's Node >= 22.19 floor belongs to
`@earendil-works/pi-coding-agent` [Q][S].
