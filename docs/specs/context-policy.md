# Spec: the fleet context policy (CTX) and the harness defaults (HDF)

| | |
|---|---|
| Status | Slice 1 implemented (Claude and Codex end to end; workstream W1, units u0 to u7). Slice 2: the OpenCode, pi and Prime readers (u8), `--json` (u10a) and Prime's agent dir (u9) are implemented; the slice-2 live check (part C) has run: OpenCode compacts in place; pi and Prime stay on the fresh route. Slice 3 (`compact_at`, the native count; u11a) is implemented. |
| Date | 2026-10-06 |
| Authors | product-lead-1 (requirements), staff-engineer-1 and staff-engineer-2 (design), architect-reviewer-1 (review), backend-developer units u0 to u7 (build). |
| Evidence | The W1 requirements and design (`ai_docs/plans/wave2/w1/requirements.md`, `design.md`), the 14 review decisions (`review-decisions.md`), and the live check `scripts/live/context.sh` with its result in [`docs/live-checks/context.md`](../live-checks/context.md). [SEEN] marks a fact read on the operator's Mac on 2026-10-06. |

---

## 1. Problem

Every fleet session, the orchestrator included, fills its context window
until its harness compacts it by itself. The harness picks the moment: in
the middle of an edit, a build or a test run, with no handoff written first.
What the summary drops is lost: the plan step, the files touched, the open
questions, and for the orchestrator the map of which worker owns which plan.
Accuracy falls earlier still: the measured long-context decline of the
fleet's models starts between 32K and 256K tokens, but the harnesses compact
between 140,000 and 983,616 tokens. Before W1 nobody could see how full a
session was: `horch cost` and `horch usage` show cumulative spend, not the
current context.

## 2. Goals and non-goals

Goals:

- G1. Every fleet session works in the part of its window where its model is
  still accurate, and it never loses its task state when its context is
  reset.
- G2. The operator sees, per session, the current context, the harness's own
  auto-compact trigger and where its setting comes from, and the watch
  threshold (`horch context`).
- G3. A session is compacted, or replaced by a fresh session, at a stopping
  point it chooses, after it writes a handoff, and before its harness
  compacts it by itself.
- G4. horch never changes a native window that the operator set. A fleet
  default applies only where the operator set none.
- G5. Per-harness launch switches (W2's background calls switched off) live
  in 1 data file, not in every teammate file (HDF).

Non-goals (this time):

- N1. A watcher process or daemon, or a push signal from herdr. The
  orchestrator's checkpoints and a worker's own `horch note` cover working
  sessions.
- N2. Harness hooks and extensions (Claude `SessionStart`/`PreCompact`,
  Codex `hooks.json`, an OpenCode compaction plugin, a pi extension, a Prime
  `compact` skill).
- N3. Changing the operator's `~/.claude/settings.json`,
  `~/.codex/config.toml` or any global harness config. Fleet values land per
  launch.
- N4. Antigravity measurement and compaction: no usage source is known. It
  shows as `not-read`.
- N5. Automatic tuning of windows or thresholds. Re-tuning is the
  `tune-fleet` skill's job.
- N6. Lowering `opencode-ultra`'s context limit, and pi `/tree` branch
  walking.
- N7. herdr sidebar context badges.
- N8. The fixed `horch orchestration` recipe: it is frozen with its goldens.
- N9. Measuring whether earlier compaction improves task quality.

## 3. The threshold rule

```
native trigger  = the harness's own auto-compact trigger (rules below)
base            = the teammate's compact_at, else 300,000 (CTX-21)
threshold       = min(base, floor(0.8 x native))          base when native is unknown
headroom        = native - threshold                      at least 20,000 (roster check)
```

Native trigger rules (`compaction::window::native_trigger`):

| harness | rule | formula |
|---|---|---|
| claude | window minus reserve | `min(setting or W, W) - 33,000`; `W` = 200,000 for a model with a `haiku` segment, else 1,000,000 |
| codex | Codex limit | with the transcript's window `w`: `min(setting or no limit, floor(w x 18 / 19))`; without `w`: the setting (unknown when also unset) |
| pi, prime | window minus reserve | `min(setting or W, W) - 16,384`; `W` from the model table (`anthropic/claude-opus-5-5` and `anthropic/claude-opus-5` 1,000,000, `ollama/qwen3.8` 262,144) |
| opencode | per model | `opencode/big-pickle` 140,000; `opencode/nemotron-3.5-lightning-free` 230,144; `opencode/nemotron-3-ultra-free` 968,000 |
| antigravity, none | unknown | threshold 300,000; the row is `not-read` |

`compact_at` is a teammate frontmatter key only (no env variable).
`horch teammates --check` rejects a value outside 50,000 to 1,000,000.
`horch context`, `horch context --windows`, `horch compact` and the
`horch note` check all use the same base. The tables below assume no
`compact_at`.

The 0.8 factor leaves the session room to finish its step and write a
handoff (about 10,000 to 40,000 tokens, not measured), and the orchestrator
checks only at checkpoints. The 20,000 floor stops a small window from making
the watch useless: a teammate whose fleet window leaves less fails
`horch teammates --check` (CTX-08). An operator value below the floor is not
a roster error: `horch context --windows` prints `WARN` after its headroom.

### 3.1 Effective on the operator's Mac ([SEEN] 2026-10-06)

The operator sets `CLAUDE_CODE_AUTO_COMPACT_WINDOW=500000` in
`~/.claude/settings.json`; `~/.codex/config.toml` sets no
`model_auto_compact_token_limit`.

| tier (teammates) | setting | source | native trigger | threshold | headroom |
|---|---|---|---|---|---|
| Claude orchestrator, every flavor | 500,000 | operator `~/.claude/settings.json` | 467,000 | 300,000 | 167,000 |
| Claude `opus` (48) | 500,000 | operator | 467,000 | 300,000 | 167,000 |
| Claude `sonnet` (13) | 500,000 | operator | 467,000 | 300,000 | 167,000 |
| Codex `gpt-5.6-sol` (5) | 200,000 | fleet, key `codex/gpt-5.6-sol` | 200,000 | 160,000 | 40,000 |
| Codex `gpt-5.6-terra` (1) | 150,000 | fleet, key `codex/gpt-5.6-terra` | 150,000 | 120,000 | 30,000 |
| Codex `gpt-5.6-luna` (1) | - | harness | 244,800 with window 258,400 (unknown before the first `token_count`) | 195,840 (300,000 while unknown) | 48,960 |
| `orchestrator-codex` on `gpt-6-astra` | - | harness | as luna | as luna | as luna |
| `orchestrator-codex` on `gpt-5.6-sol` (`horch fleet sol`) | 200,000 | fleet, key `codex/gpt-5.6-sol`: Codex has no orchestrator key, so the Codex orchestrator takes its model's window | 200,000 | 160,000 | 40,000 |
| `opencode-ultra` (slice 2) | - | harness | 968,000 | 300,000 | 668,000 |
| `opencode-lightning` (slice 2) | - | harness | 230,144 | 184,115 | 46,029 |
| `opencode-pickle` (slice 2) | - | harness | 140,000 | 112,000 | 28,000 |
| `pi` `ollama/qwen3.8` (slice 2) | - | harness | 245,760 | 196,608 | 49,152 |
| `prime` `anthropic/claude-opus-5-5` (slice 2) | 200,000 | fleet (`compact_window` in `teammates/prime.md`) | 183,616 | 146,892 | 36,724 |
| `antigravity` | - | - | unknown | none (`not-read`) | - |

The operator's 500k Claude window plus the 300k watch is the policy the
operator asked for.

### 3.2 Fleet defaults (where the operator sets nothing)

The values are data: `windows` in `teammates/_base/context-windows.md`,
keyed `<harness>/<teammate name>` first, then `<harness>/<model>`. A
teammate's own `compact_window` wins over the file.

| key | setting | native trigger | threshold | headroom |
|---|---|---|---|---|
| `claude/orchestrator` | 300,000 | 267,000 | 213,600 | 53,400 |
| `claude/opus` | 200,000 | 167,000 | 133,600 | 33,400 |
| `claude/sonnet` | 150,000 | 117,000 | 93,600 | 23,400 |
| Claude with no value anywhere | - | 967,000 | 300,000 | 667,000 |
| `codex/gpt-5.6-sol` | 200,000 | 200,000 | 160,000 | 40,000 |
| `codex/gpt-5.6-terra` | 150,000 | 150,000 | 120,000 | 30,000 |
| `prime` (`compact_window: 200000`, slice 2) | 200,000 | 183,616 | 146,892 | 36,724 |

Example of the floor: a Claude window of 100,000 gives native 67,000,
threshold 53,600 and headroom 13,400, so the roster check fails it.

### 3.3 Where a window comes from

At launch the harness adapter resolves the operator's own value first, and
passes the fleet value only when there is none:

- Claude, first hit wins, each source only when the teammate's
  `setting_sources` allows it: managed settings (the `RuntimeContext` path
  `claude_managed_settings`: `$HORCH_CLAUDE_MANAGED_SETTINGS`, else the
  platform path), `<workdir>/.claude/settings.local.json`,
  `<workdir>/.claude/settings.json`, the user `settings.json` (under
  `CLAUDE_CONFIG_DIR` or `~/.claude`), then the process env. A settings file
  that also sets `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` gives an unknown trigger.
  The fleet value goes into the single `--settings` overlay as
  `env.CLAUDE_CODE_AUTO_COMPACT_WINDOW`; the overlay keeps the operator's
  other env keys (live check A3).
- Codex: `model_auto_compact_token_limit` in `$CODEX_HOME/config.toml`
  (`[profiles.<p>]`, the dotted profile key, then the top level). The fleet
  value is `-c model_auto_compact_token_limit=<n>` before the prompt, on
  fresh and resume launches.
- Prime (slice 2): a provider the operator's `models.json` defines at all is
  the operator's; else a fleet agent dir with a `models.json` that holds
  only the override. Each launch gets its own agent dir,
  `<state_root>/prime/<slug>-<uuid>/agent/`, passed as
  `PRIME_AGENT_CODING_AGENT_DIR`:
  - The source dir is the teammate's own `PRIME_AGENT_CODING_AGENT_DIR`,
    else the inherited one, else `~/.prime/agent`. An inherited value under
    `<state_root>/prime/` is another launch's agent dir and is ignored.
  - Every source entry is a link, except `settings.json` and `models.json`
    (generated) and `*.lock` and `*.tmp` (not linked). `auth.json` is always
    linked, also when the source has none, so a login lands in the
    operator's file.
  - The `settings.json` copy drops the legacy `apiKeys`.
  - horch reads the operator's and the project's files only as regular
    files, with a size bound, so a FIFO in the project does not block the
    launch.
  - `Daemon::finish` removes the agent dir when it stops the pane's daemon.
    Each launch sweeps the agent dirs of earlier launches with no daemon
    socket and no live launcher.

Prime agent dir security notes (u9 security review,
`ai_docs/plans/wave2/w1/u9-security-review-result.md`):

- S1, accepted residual risk (low likelihood): the per-launch agent dir
  splits Prime's auth lock. The sync path locks `<agent dir>/auth.json.lock`;
  the refresh path locks the real file. A `/login`, `/logout`, `mcp add` or
  `mcp remove` in a fleet Prime pane can be lost during a token refresh in
  another Prime process. `teammates/prime.md` tells the pane not to run them.
  The upstream fix: Prime's `withLock` locks the realpath.
- S6: the settings copy is not free of credentials. Prime settings can hold
  MCP `headers` and a legacy `apiKeys`. horch drops `apiKeys`, links the
  operator's `settings.json` when it inserts no fleet key, and removes the
  agent dir when the pane's daemon stops.
- S9: when the agent dir build fails, horch prints 1 stderr line and the
  pane runs on the source dir: no fleet window, and the record says
  `applied: false`.

The launch records its decision on the ledger record (§7.2). `horch context`
shows the recorded source; a record without one is resolved now and shows
`(now)`.

## 4. Protocol

### 4.1 Worker, compaction in place (Claude, Codex)

1. Orchestrator checkpoint (after it handles each worker message, and before
   each spawn): `horch context --over` shows `sonnet-1 ... over`.
2. `horch compact sonnet-1 --request`: horch types the `request` message
   into the pane and records `compact-requested`; the row becomes
   `requested`. The orchestrator does not ask twice in 1 compaction cycle.
3. The worker reaches a stopping point: a plan step and its check are done,
   or it waits for an answer; never mid-edit, mid-build or mid-test.
4. It writes `ai_docs/handoffs/sonnet-1-whats-next.md` with the
   `horch:handoff` skill, runs
   `horch note "handoff: ai_docs/handoffs/sonnet-1-whats-next.md"` (if that
   fails, it goes on), sends
   `[sonnet-1] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-1-whats-next.md`,
   and ends its turn.
5. The orchestrator runs `horch compact sonnet-1`. It returns as soon as the
   job has started.
6. The job waits for 2 idle polls in a row, types `/compact <instructions>`
   (Claude) or `/compact` (Codex), waits for the compaction marker in the
   transcript, waits for idle again, types the `resume` line, waits for the
   next reading, records `compacted`, and sends
   `[horch] NOTE: sonnet-1 compacted. Context 312857 -> 18207 tokens. Handoff: ai_docs/handoffs/sonnet-1-whats-next.md.`
   to the orchestrator pane.

Self-warning path: steps 1 and 2 are replaced by a `horch note` that prints
the `warning` message. That call is the stopping point, so the worker does
step 4 at once. The orchestrator handles `COMPACT-READY` as in step 5.

Codex workers and `horch note` (review decision 2): before W21 a Codex
worker's sandbox could block the ledger lock, so `horch note` could fail in a
Codex worker pane. Live check B2 now passes for `workspace-write` Codex
panes (below), so the `horch note` check watches them as it watches every
other worker. The rule "the orchestrator checkpoint is the only watch for
Codex workers" stays only for a `read-only` Codex pane
(`permission_mode: plan`). The worker briefing tells a worker to go on to
`COMPACT-READY` when `horch note` fails.

B2 result (2026-10-07, Codex 0.160.0, `-s workspace-write`): an execpolicy
`allow` lifts the sandbox only for a command that matches its prefix. A plain
`horch note "x"` matches and writes. A compound command such as
`horch note "x" 2>&1; echo $?` runs in the sandbox, and the ledger lock in the
state root fails with "Operation not permitted". The fix: every Codex pane,
worker and orchestrator, gets `--add-dir <state root>` from the Codex
`prepare`. That is the only extra writable root. The ledger, its lock and
temp files, the context cache and the compact job dirs are all under it.
`horch tell` writes under the temp dir, which the sandbox already allows.
`horch` only reads the data dir. A `codex exec` run with the fleet's private
`CODEX_HOME` and `--add-dir` records the compound `horch note`. A `read-only`
Codex pane (`permission_mode: plan`) ignores `--add-dir`, so its
`horch note` can still fail.

B2 re-run in fleet Codex worker panes after W21 (2026-10-07, CTX-13.1,
CTX-13.3): PASS.

- The compound `horch note` records the note (`codex-sol-8`, exit 0).
- The warning prints and records `context-warned` (`codex-sol-9` at 85,668
  tokens with a threshold of 80,000; spawned with a scratch roster copy, Codex
  window 100,000).
- Round trip by `horch compact --request`: 23,880 -> 7,717 tokens.
- Round trip by the warning: 89,581 -> 7,264 tokens.
- `horch compact --force` waited 32 s for idle, then typed 1 bare `/compact`:
  30,189 -> 6,885 tokens.

Codex writes its token count after each model turn, not during the turn. So
the warning comes at the first `horch note` after the turn that crosses the
threshold.

Security note on `--add-dir <state root>` (W21; orchestrator decision:
accepted). Every Codex pane can now write the whole state root: the ledgers
of all projects, the private `codex-home/<role>` dirs and the
`skill-bundles/<execution_id>` dirs of live panes. A Claude worker has no OS
sandbox and can already write there, so a Codex pane gets the same access,
not more. Codex reads its rules only at startup, and each launch builds a
new private home, so a write into a live pane's `codex-home` does not change
that pane. Follow-up, not in this branch: move `codex-home` and
`skill-bundles` out of the state root, then narrow `--add-dir` to the
ledger and the dirs that `horch` writes.

### 4.2 Fresh-session route (pi and Prime; the fallback for all)

OpenCode moves to the in-place route in slice 2 (live check C5 PASS). pi
and Prime stay on this route:

- pi: the in-place round trip is not verifiable yet. In a short session pi
  answers `/compact` with "Nothing to compact (session too small)".
- Prime: herdr does not detect `prime-agent` (agent `null`), so the
  compaction job cannot see idle. A forced job failed at `wait-idle` after
  120 s and typed nothing. Follow-up: horch reports the Prime pane state
  with `herdr pane report-agent`, or herdr detects `prime-agent`.

On `COMPACT-READY <path>` when `horch compact` refuses with "fresh route",
on a `[horch] BLOCKED:` line, or on a row in state `compact-lost`: the
orchestrator tells the worker to run `horch done` with a summary that names
`<path>`, then spawns the same teammate with a task that names the plan file
and `PRIOR WORK: read <path> first`. horch types nothing into a pane that
may be busy, so this route works when herdr reports `unknown` for the pane.

### 4.3 Orchestrator (both flavors)

Stopping point: every worker message answered, no spawn half-done, no
`DONE:` under verification. The orchestrator writes
`ai_docs/handoffs/orchestrator-whats-next.md` (every live role from
`horch sessions`, its plan file, every open question, every `COMPACT-READY`
line not yet acted on) and runs `horch compact orchestrator`. Only when it
prints `scheduled` does it end the turn with no further tool call. On a
`[horch] BLOCKED:` line it stays in the turn, goes on with the work, and
tries again at the next stopping point (the native trigger is the
backstop). The job types the line only when the pane is idle, so a Codex
orchestrator never rejects it. The `[horch] NOTE:` report arrives in the
orchestrator's own pane.

### 4.4 Per-harness route

`in_place` in `teammates/_base/context-windows.md` lists the harnesses whose
compaction live check passed (CTX-17). An operator can take a harness out
with an overlay copy of that file at
`~/.config/horch/teammates/_base/context-windows.md`, with no rebuild; the
messages stay in `_base/context-messages.md`.

| harness | command typed | keep-list as argument | busy behaviour | sends | route |
|---|---|---|---|---|---|
| claude | `/compact <instructions>` | yes | queued | 1 | in place |
| codex | `/compact` | no | rejected | up to 3 | in place |
| pi | `/compact <instructions>` | yes | aborts the turn | 1 | fresh |
| prime | `/compact <instructions>` | yes | queued | 1 | fresh |
| opencode | `/compact` | no | ends the turn | up to 3 | in place (slice 2) |
| antigravity | - | - | - | 0 | not watched (`not-read`) |

### 4.5 The compaction-instructions block (CTX-19, CTX-20)

Every fleet briefing carries exactly 1 `== Compact instructions ==` block:
`teammates/_base/fleet-worker.md` (with `== Context compaction ==`, the
worker protocol) and `teammates/_base/fleet-orchestrator.md` (with
`== Context watch ==`). It tells any summary to keep the role, the plan or
brief path, the handoff path, the files touched, the decisions, the open
questions and their answers, and the report target; the orchestrator's
block also keeps the roster and the role-to-plan ownership map. horch-driven
compaction on a harness that accepts an argument passes the `instructions`
message as the argument, so the keep-list holds there even when a harness
ignores the block. The live check plants a canary in the block and reports
per harness whether a summary honours it.

## 5. CLI contract

```
horch context [--over] [--windows] [--json]
horch compact <role> --request
horch compact <role> [--force] [--timeout <secs>]
horch compact <role> --foreground [--force] [--timeout <secs>]   (internal: the detached child)
horch note <text>                                                  (the context check, CTX-10)
```

### 5.1 `horch context`

Rows: every live record of the current project's ledger with
`kind == orchestrator` first (newest first), then 1 live worker record per
role (the newest `updated_at`), by role. No herdr call. The threshold of a
row is `threshold(policy::watch_base(<the teammate's compact_at>), native)`.

```
ROLE          HARNESS  MODEL             CONTEXT     WINDOW   NATIVE  SOURCE          THRESHOLD  LAST COMPACT              COMPACTIONS  STATE
orchestrator  claude   claude-opus-5-5   33,352   1,000,000  467,000  operator (now)    300,000  2026-09-20T10:05:00.000Z  0n/1h        ok
codex-sol-1   codex    gpt-5.6-sol      227,306     258,400  200,000  fleet             160,000  -                         0n/0h        over
opus-1        claude   claude-opus-5-5   10,486*  1,000,000  467,000  operator (now)    300,000  2026-09-20T10:05:00.000Z  1n/0h        pending
sonnet-1      claude   claude-opus-5-5  311,225   1,000,000  467,000  operator (now)    300,000  -                         0n/0h        over
sonnet-2      claude   sonnet                 -   1,000,000  467,000  operator (now)    300,000  -                         -            no-transcript
reasons:
  sonnet-2: no-transcript: no transcript found
```

- `*` = provisional (the harness's post-compaction figure); `-` = unknown.
  Numbers have `,` groups. MODEL is the transcript's model, else the
  record's. WINDOW is the transcript's model window, else the model table's.
  NATIVE is the native trigger. COMPACTIONS (CTX-25) is
  `<native>n/<horch>h`, `-` when the transcript does not read: a marker is
  horch-driven when a `compacted` event is at or after it and at most 15
  minutes after it; each event matches 1 marker; the rest are native.
  SOURCE is the recorded decision's source, or
  `operator (now)`, `fleet (now)`, `harness (now)` when the record has none.
- STATE, first match: `compact-lost` (the job dir says the job is lost),
  `no-session`, `not-read`, `no-transcript`, `unknown`, `compacting` (a
  live job), `pending` (a marker newer than the last usable response),
  `requested` (asked in this cycle), `over`, `ok`.
- `reasons:` holds 1 line per row in `compact-lost`, `no-session`,
  `not-read`, `no-transcript` or `unknown`. The `compact-lost` reason is
  `the compaction job is lost at step <step>; log <path>`.
- A reader that finds a lost job claims it; the 1 reader that claims it
  records 1 `compact-failed` event.
- Exit 0, also when rows are unreadable. Exit 1 only when the ledger cannot
  be read (1 stderr line).

`--over`: the header and only the rows in `over`, `requested` or
`compact-lost`, with their `reasons:` lines. With none it prints exactly
`no session is over its threshold` (no header), exit 0.

`--windows`: 1 row per roster teammate on a harness with a known trigger
rule, as a launch from the current directory would decide now: TEAMMATE,
HARNESS, MODEL, SETTING, SOURCE, NATIVE, THRESHOLD, HEADROOM (with `WARN`
under 20,000), DETAIL (a file path with `~` for the home,
`context-policy windows <key>`, `teammate <name> compact_window`, or
`harness default`).

`--json` (CTX-24): a JSON array on stdout, 1 object per row, in the order
of the text table. Every row is in it: `--json` ignores `--over`, and
`--windows` wins over `--json` (the windows table, no JSON). The keys, in
this order:

| key | value | `null` when |
|---|---|---|
| `role` | the record's role | never |
| `record_id` | the record id | never |
| `harness` | `claude`, `codex`, `opencode`, `pi`, `prime`, `antigravity` or `none` | never |
| `model` | MODEL of the text row | never |
| `session_id` | the record's harness session id | not yet harvested |
| `transcript` | the transcript path that was read | the transcript does not read |
| `pane` | the record's pane id, when herdr lists it | herdr does not answer, or does not list the pane |
| `pane_status` | herdr's `agent_status` of that pane | as `pane`, or herdr gives no status |
| `tokens` | CONTEXT of the text row | unknown (`-`), for example a pi `pending` row |
| `provisional` | `true` for the harness's post-compaction figure | the transcript does not read |
| `pending` | `true` when a marker is newer than the last usable response | the transcript does not read |
| `window` | WINDOW | unknown |
| `native_trigger` | NATIVE | unknown |
| `native_setting` | the window setting of the decision (Claude `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, Codex `model_auto_compact_token_limit`, Prime and pi `contextWindow`) | no source sets one |
| `native_source` | `operator`, `fleet` or `harness`, without `(now)` | never |
| `native_detail` | the detail of the decision | never |
| `threshold` | THRESHOLD | never |
| `last_compaction` | `{at, trigger, pre_tokens, post_tokens}`; a member is `null` when the transcript does not give it | no marker, or the transcript does not read |
| `native_compactions` | the native count of COMPACTIONS (CTX-25) | the transcript does not read |
| `horch_compactions` | the horch count of COMPACTIONS (CTX-25) | the transcript does not read |
| `state` | STATE, `compact-lost` included | never |
| `reason` | the text of the row's `reasons:` line, without the role and the state | the row has no `reasons:` line |
| `route` | `in-place` (the harness is in `in_place`) or `fresh` | never |
| `handoff` | `ai_docs/handoffs/<role>-whats-next.md` resolved against the record's `workdir`, else the project dir | never |

`pane` and `pane_status` come from 1 `herdr pane list` call per workspace
(the record's `workspace_id`, else the caller's). A failed call gives `null`
for both, not an error. The text table makes no herdr call. Exit codes are
those of the text table.

### 5.2 `horch compact <role> --request`

- Resolves the live record of `<role>` in the caller's herdr workspace
  (`Ledger::live_for_role(role, Some(<workspace>))`) and its pane.
- Asked already in this compaction cycle (rule A: a `compact-requested` or
  `context-warned` event at or after the later of the newest `compacted`
  event and the transcript's last marker): prints
  `already asked <role> at <at>`, exit 0, types nothing.
- Else types the rendered `request` message when the pane is ready, records
  `compact-requested` `tokens <n> threshold <t>`, prints
  `asked <role> to write <handoff>`, exit 0.
- Refusals, exit 1, 1 stderr line `horch compact: <reason>`:
  - `no live record for role <role> in workspace <ws>`
  - `role <role> has no pane in workspace <ws>`
  - `role <role> runs harness none, which has no compact command`
  - `refused: the orchestrator is not asked; it compacts itself at its stopping point`

### 5.3 `horch compact <role>`

1. Validate: the live record in the workspace, its pane, a harness with a
   compact command. Refusals as in §5.2, with
   `role <role> runs harness <h>, which has no compact command` for a
   harness with no command.
2. Job liveness: a live job prints
   `compaction of <role> already running; log <path>`, exit 0. A lost job is
   claimed (1 event); the start clears its dir.
3. Route, unless `--force`: a harness not in `in_place` exits 1 with
   `horch compact: refused: <harness> uses the fresh route; tell <role> to run horch done`.
4. Handoff guard (CTX-14), unless `--force`. The path is
   `ai_docs/handoffs/<role>-whats-next.md` resolved against the record's
   `workdir`, else the project dir; the request time is the newest
   `compact-requested` or `context-warned` event of the record. Exit 1 with
   1 of:
   - `horch compact: refused: handoff <path> is missing`
   - `horch compact: refused: handoff <path> is older than the request at <at>`
   - `horch compact: refused: handoff <path> is <m> minutes old (limit 60)` (no request: the 60-minute rule)
   Nothing is typed.
5. Start: under the lock of `<state_root>/compact/`, the parent spawns
   `<exe> compact <role> --foreground [--force] [--timeout N]` detached
   (`spawn_detached`: a new session, stdin null, stdout and stderr appended
   to `job.log`, the forbidden env keys scrubbed) and writes `spawned`.
6. Start handshake: the parent polls every 50 ms for up to `START_TIMEOUT`
   (5 s). The wait ends as soon as `job.json` shows a step past `start`, so
   only a failed start waits the full time.
   - `job.json` past `start`: prints `compaction of <role> scheduled; log <path>`, exit 0.
   - The child exits 0 first (a second job for the role): the `already running` line, exit 0.
   - The child exits 3 (`EXIT_REPORTED`) first: it has recorded
     `compact-failed` and sent its report; the parent prints the last
     `[horch] BLOCKED:` line of `job.log`, exit 1.
   - Any other exit, or no job file at the timeout: the parent kills the
     unreaped child (SIGKILL), clears the job dir, records `compact-failed`
     `step start: <reason>` (for example `the job did not start in 5 s`),
     prints the rendered `failed` line to stdout, exit 1.

`--foreground` (the child): it creates `job.json` at step `start` first,
with `create_new` (a second job for the role gets `AlreadyExists` and exits
0 with no event). Then it checks again (steps 1, 3 and 4 with the same
flags), writes step `wait-idle`, starts the shared heartbeat, builds the job
plan (`instructions` or `instructions-orchestrator` by record kind), runs the
job, and deletes `job.json`, `heartbeat` and `spawned` on every exit
(`job.log` stays). Exit codes:

| exit | when | event and report |
|---|---|---|
| 0 | the compaction is done; or `AlreadyExists` | `compacted` and the `reported` line; none for `AlreadyExists` |
| 3 (`EXIT_REPORTED`) | a refusal or an I/O error at step `start`, or a job step failure | `compact-failed` `step <step>: <reason>` and the `failed` line, sent by the child |

### 5.4 `horch note <text>` (CTX-10)

1. Record the note exactly as before W1; its error is returned as before.
2. The check, inside a catch-all with a silent panic hook: the record of
   `HORCH_RECORD_ID` is live; its row is built as `horch context` builds it;
   it warns when `compaction::policy::should_warn` (the row state is
   `over`). Any error or panic means no warning. A marker-cache write that
   fails changes nothing.
3. On a warning: record `context-warned` `tokens <n> threshold <t>` (its
   error ignored) and print the rendered `warning` (a worker) or
   `warning-orchestrator` (an orchestrator record), 1 line on stdout.
   Nothing on stderr. Exit 0.

## 6. Data files

### 6.1 `teammates/_base/context-windows.md` (operator-tunable)

`windows` (the fleet values of §3.2) and `in_place` (`[claude, codex, opencode]`). An
operator may copy the whole file to
`~/.config/horch/teammates/_base/context-windows.md`; that changes no message.

### 6.2 `teammates/_base/context-messages.md` (the protocol text)

`messages`: every line horch types for the context watch. Rust only fills the
placeholders `{role}`, `{tokens}`, `{threshold}`, `{handoff}`, `{pre}`,
`{post}`, `{step}`, `{reason}`, `{log}`. Each message is 1 line (a newline
would submit early). Keys: `request`, `warning`, `warning-orchestrator`,
`instructions`, `instructions-orchestrator`, `resume`, `reported`, `failed`.
Operators do not copy this file; `horch teammates --check` warns when an
overlay replaces it. The briefings name these prefixes, and each starts (or
is in) the matching message:

| prefix in a briefing | message |
|---|---|
| `NOTE: Your context is` | starts `request` |
| `NOTE: Context warning` | starts `warning`, `warning-orchestrator` |
| `NOTE: COMPACT-READY` | `request` and `warning` contain `[{role}] NOTE: COMPACT-READY {handoff}` |
| `[horch] NOTE:` | starts `reported` |
| `[horch] BLOCKED:` | starts `failed` |

Roster checks: each base sets only its own keys; every message key exists,
has no newline and no unknown placeholder; every `windows` key names a
harness; every `in_place` entry names a harness with a compact command; a
teammate sets its window with `compact_window`, never with `env:`,
`settings:` or `args:`; a fleet window leaves at least 20,000 headroom.

### 6.3 `teammates/_base/harness-defaults.md` (HDF)

`defaults`: entries selected by `harness` and optionally by `base`, each
with `env`, `args`, `settings` (Claude `--settings` keys), `config`
(OpenCode `OPENCODE_CONFIG_CONTENT` keys), `agent_settings` (Prime agent-dir
`settings.json` keys) and an optional `force` with its reason. The roster
attaches the entries whose `base` matches to each teammate at load; the
builder selects them by the teammate's final harness when the command is
built (so the Codex orchestration pane of a Claude teammate gets the Codex
defaults).

Merge order, per key, the first that has a value wins:

1. The operator's own value for the key, unless the entry has `force`
   (Claude: the settings chain of §3.3; Codex: `config.toml`; OpenCode and
   Prime: a named env variable in the inherited allow-list).
2. The teammate's own value.
3. horch's fixed switches (Claude `enabledPlugins`, skill switches,
   `remoteControlAtStartup`, sandbox).
4. The harness default, in file order.

The inherited environment is an allow-list (review decision 6):
`runtime::context::Inherited` holds named fields only, and no
`FORBIDDEN_ENV` name (`ANTHROPIC_API_KEY`) is ever captured. A non-Claude
`env` default key must be in `runtime::context::DEFAULT_ENV_YIELD`.

### 6.4 The marker cache

`<state_root>/context/markers/<first 16 hex of sha256(transcript path)>.json`
with `{"v": 1, "path", "scanned_to", "marks": [...]}`. A file shorter than
`scanned_to`, another `v` or a cache that does not parse starts the scan from
0. The write is best effort: a cache dir that cannot be written (a Codex
sandbox, a read-only state dir) never hides the reading. Deleting the
directory is always safe.

## 7. Ledger

### 7.1 Events (CTX-22)

Free-text history events on the session's record, shown by
`horch sessions` with no format change:

| event | text | written by |
|---|---|---|
| `compact-requested` | `tokens <n> threshold <t>` | `horch compact <role> --request` |
| `context-warned` | `tokens <n> threshold <t>` | the `horch note` check |
| `compacted` | `<pre> -> <post> tokens; handoff <path>` (`unknown` for a missing number) | the job |
| `compact-failed` | `step <step>: <reason>` | the job, the child, the parent at a failed start, or the first reader of a lost job (`step <step>: the job process is lost; log <path>`) |

Handoff cost = `pre` of `compacted` minus `tokens` of `compact-requested`.

### 7.2 The record field `compact_window` (CTX-05)

The launch's window decision, stored by the ledger's own type
(`execution::legacy::LedgerWindow`, explicit serde names; `execution` does
not import `compaction`). Optional: absent before W1, and a resume rewrite
sets it to none until the relaunch writes it again.

```json
"compact_window": {"tokens": 200000, "source": "fleet", "detail": "context-policy windows codex/gpt-5.6-sol", "applied": true}
```

`source` is `operator`, `fleet` or `harness`; an unknown string (a newer
binary's value) loads as unknown, and the reader resolves now. `applied` is
true only when the built command really carries the value (read back from
the command); else the detail says `not applied: <reason>`.

## 8. The compaction job and its heartbeat

### 8.1 Job dir

`<state_root>/compact/<workspace>-<role>/`:

| file | written by | content |
|---|---|---|
| `job.log` | the parent creates it before the spawn; the child appends | 1 line per step: `<UTC> <step> <text>` |
| `spawned` | the parent, right after the spawn | `{"pid", "at"}`: the spawn time for the liveness rule |
| `job.json` | the child, `create_new`, then rewritten at each step | `{"v": 1, "role", "record_id", "started_at", "step"}` |
| `heartbeat` | the child, through `crate::heartbeat`, every 2 s | `{"pid", "at", "started"}` |
| `lost` | the first reader that finds the job lost, `create_new` | `{"at"}` |

Steps: `start`, `wait-idle`, `send`, `wait-marker`, `wait-idle-after`,
`resume`, `wait-reading`, `report`. Idle is the 1 predicate
`workspace::model::at_prompt` (`idle` or `done`) on 2 polls in a row, 2 s
apart; `working`, `blocked`, `unknown`, no status or a failed poll start the
count again. Timeouts: idle `--timeout` (default 1,800 s), Codex resend
after 30 s with no marker (at most 3 sends), marker 600 s, idle after the
marker 600 s, reading 180 s.

### 8.2 Liveness and lost jobs

`compaction::jobfile::liveness`: no `job.json` and no starting job is none;
a fresh heartbeat of a running pid (or a spawn less than 30 s ago with no
heartbeat yet) is live; `job.json` with a dead pid or a stale heartbeat is
lost, at the step `job.json` names. A lost job is row state `compact-lost`,
`horch context --over` prints it, and exactly 1 reader records the
`compact-failed` event. The next `horch compact <role>` clears the dir.

### 8.3 The shared heartbeat (`crates/horch-core/src/heartbeat.rs`)

The judge job (CMP-16) and the compaction job are the 2 allowed background
jobs, and both use 1 liveness scheme: `heartbeat::start` writes a beat every
2 s, and `heartbeat::liveness` decides running, lost or not started from the
beat, the pid and the spawn time (stale after 30 s). The CMP-16 guard test
(`cmp_16_only_judge_and_compaction_jobs_detached`) keeps a named allow-list
of the 2 jobs.

The live check A8 proves that a detached child outlives the end of a Claude
Bash tool call and of a `codex exec` call. If it ever fails, the fallback is
a job that runs in its own herdr pane.

## 9. Failure modes

| event | behaviour | visible as |
|---|---|---|
| transcript missing | state `no-transcript`; `horch note` silent | `reasons:` line |
| session id not yet harvested (Codex, up to 180 s) | `no-session` | `reasons:` line |
| a harness update changes the transcript shape | no usable line, so `unknown`; never 0 | `reasons:` line; the live check FAILs on a re-run |
| a line being written at the tail | invalid JSON skipped; the previous usable line used | none |
| marker cache corrupt, old or the file truncated | scan again from 0 | none; 1 slower read |
| marker cache dir not writable | the reading is returned; the cache is not written | none |
| the operator-setting resolver misses a new Claude source | a wrong SOURCE or NATIVE | live check A2 compares `/autocompact` with `--windows` and FAILs |
| an old horch rewrites the ledger | `compact_window` dropped | SOURCE shows `(now)` until the next launch |
| the `horch note` check errors or panics | caught; no output; the note is recorded; exit 0 | nothing |
| `horch note` fails in a `read-only` Codex pane (`permission_mode: plan` ignores `--add-dir`) | the worker goes on to `COMPACT-READY`; no warning for that worker | the orchestrator checkpoint only; `workspace-write` Codex panes pass B2 |
| the pane never idle | the job fails `wait-idle`, `compact-failed`, `[horch] BLOCKED:` | orchestrator line, ledger |
| `/compact` rejected (Codex busy) | sent again after 30 s, at most 3 sends | `job.log` lines |
| the marker never appears | fails `wait-marker` after 600 s | `[horch] BLOCKED:` line |
| a concurrent `horch tell` garbles the line | no marker, so BLOCKED | `[horch] BLOCKED:` line |
| the detached job never starts | the parent kills the child at the start timeout, clears the dir, records `compact-failed` `step start: ...`, prints the `failed` line, exit 1 | the line on stdout, in the orchestrator's turn; ledger |
| the child refuses on its re-check, or `job.json` fails with an I/O error | the child records `compact-failed` `step start: <reason>`, reports `failed`, exits 3; the parent prints the line, exit 1 | `[horch] BLOCKED:` line; ledger |
| the job is killed later | `job.json` with a dead pid or a stale heartbeat: `compact-lost`; 1 `compact-failed` event | `horch context --over` row and its reason; ledger |
| 2 `horch compact` for 1 role | the second sees a live job (or `AlreadyExists`) and exits 0 `already running` | stdout line |
| handoff missing or stale | refused; nothing typed | 1 stderr line |
| the worker ignores the request or the warning | the row stays `requested`; the native trigger compacts later; the block keeps the state | `horch context` |
| the handoff costs more than the headroom | a native compaction during the handoff; the block keeps the state | LAST COMPACT |
| herdr does not detect the agent (Prime: agent `null`) | the job never sees idle and fails `wait-idle`; it types nothing. Prime stays on the fresh route, which types nothing | `[horch] BLOCKED:` line on a forced job |
| pi answers `/compact` with "Nothing to compact (session too small)" | the in-place round trip is not verified; pi stays on the fresh route | live check C5 |
| a pi or Prime summary drops the compact-instructions block (C3: pi ignored the canary; Prime not verifiable, its RPC compact timed out after 30 s) | the keep-list argument of `horch compact` still carries the state | live check C3 |

## 10. Requirements

Header rule: `| ID | Requirement | Phase | Tests |`. Phase `W1` is not an
A or B phase, so `scripts/check-req-coverage.sh` always checks these IDs.
Test names are exact test function names. Slice 2 adds CTX-24 and the
slice-2 tests of CTX-02, CTX-03, CTX-04, CTX-05 (Prime), CTX-17 and CTX-20;
slice 3 adds CTX-21 and CTX-25.

### 10.1 CTX: context policy

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| CTX-01 | `horch context` lists the orchestrator records first, then 1 row per live worker role of the project, with role, harness, model, context, window, native trigger, its source, threshold, last compaction time and state. | W1 | ctx_01_lists_orchestrator_first_then_live_workers, ctx_01_done_worker_has_no_row, ctx_01_cli_table_from_fixture_home |
| CTX-02 | Current context is the harness's comparison number from the newest completed response, never a cumulative total. Claude and Codex in slice 1; OpenCode, pi, Prime in slice 2. | W1 | ctx_02_claude_last_main_chain_response_with_output, ctx_02_claude_last_message_iteration, ctx_02_codex_last_token_count_total_and_window, ctx_02_pi_skips_error_and_tool_result_lines, ctx_02_prime_after_compaction, ctx_02_opencode_newest_finished_row |
| CTX-03 | After a compaction and before the next response: `pending`, with the harness's post figure marked provisional, or unknown. Never 0, never the pre value. | W1 | ctx_03_claude_pending_uses_post_tokens_provisional, ctx_03_claude_new_response_ends_pending, ctx_03_codex_compacted_is_pending_until_token_count, ctx_03_pi_pending_is_unknown |
| CTX-04 | An unreadable session shows `no-session`, `no-transcript`, `not-read` or `unknown`, never a number, with a 1-line reason. | W1 | ctx_04_no_session_id_is_no_session, ctx_04_missing_transcript_is_no_transcript, ctx_04_harness_without_reader_is_not_read, ctx_04_states_print_a_reason_line, ctx_04_opencode_without_sqlite3_is_not_read |
| CTX-05 | Every fleet pane runs with its effective native window: the operator's value where any operator source sets one, else the fleet default of its tier. horch never overrides an operator value and never edits operator files. The launch records the decision. Prime (slice 2): each launch gets its own agent dir (§3.3): source dir from the teammate env, then the inherited value, then `~/.prime/agent`, a launch dir under the state root ignored; `*.lock` and `*.tmp` not linked; `auth.json` always linked; `apiKeys` dropped; bounded reads of regular files only; the dir removed when the daemon stops; old dirs swept. | W1 | ctx_05_claude_settings_chain_precedence, ctx_05_claude_overlay_gets_fleet_window_only_without_operator_value, ctx_05_codex_operator_limit_from_profile_or_top_level, ctx_05_codex_fleet_limit_flag_only_without_operator_value, ctx_05_launch_records_window_decision, ctx_05_old_ledger_loads, ctx_05_recorded_applied_matches_the_command, ctx_05_prime_agent_dir_links_and_fleet_models, ctx_05_prime_operator_override_wins, ctx_05_prime_inherited_agent_dir_is_the_source, ctx_05_prime_launch_records_the_applied_window, ctx_05_prime_teammate_agent_dir_is_the_source, ctx_05_prime_inherited_launch_dir_is_ignored, ctx_05_prime_transient_entries_and_missing_auth, ctx_05_prime_settings_copy_drops_api_keys, ctx_05_prime_project_fifo_does_not_block, ctx_05_prime_finish_removes_the_agent_dir, ctx_05_prime_launch_sweeps_old_agent_dirs |
| CTX-06 | The window a pane uses is proved per tier by a re-runnable live check that reads the harness's own report (`/autocompact` for Claude) and compares it with `horch context --windows`. | W1 | ctx_06_windows_view_lists_claude_and_codex_teammates, ctx_06_live_check_covers_every_tier |
| CTX-07 | Threshold = `min(base, floor(0.8 x native))`, base 300,000; unknown native gives base. | W1 | ctx_07_threshold_is_min_of_base_and_eight_tenths, ctx_07_unknown_native_uses_base, ctx_07_native_trigger_per_rule, ctx_07_compaction_pure_modules_do_no_io |
| CTX-08 | The roster check fails a teammate whose fleet window leaves headroom below 20,000. | W1 | ctx_08_roster_check_fails_headroom_below_floor, ctx_08_builtin_roster_passes_the_floor |
| CTX-09 | `horch context --over` prints only `over`, `requested` and `compact-lost` rows, else exactly `no session is over its threshold`, exit 0. The orchestrator briefing names it at 2 checkpoints; the Codex orchestrator may run it. | W1 | ctx_09_over_prints_only_over_and_requested, ctx_09_over_prints_compact_lost, ctx_09_over_with_none_prints_exact_line, ctx_09_orchestrator_briefing_names_the_checkpoints |
| CTX-10 | `horch note` warns a session at or over its threshold, once per compaction cycle, after recording the note; a check failure changes nothing; the rule is `policy::should_warn`; a failed cache write does not hide the reading. Live check B2 passes for `workspace-write` Codex panes; for a `read-only` Codex pane the orchestrator checkpoint is the only watch. | W1 | ctx_10_rule_a_cycle_starts_at_newest_compaction, ctx_10_should_warn_is_classify_over, ctx_10_cache_write_failure_keeps_the_reading, ctx_10_note_warns_once_over_threshold, ctx_10_note_silent_on_unknown_pending_or_error, ctx_10_note_warns_again_after_compaction, ctx_10_note_warns_with_a_read_only_cache_dir |
| CTX-11 | The orchestrator asks a worker to prepare; the worker answers `[<role>] NOTE: COMPACT-READY <path>`; the row shows `requested`. | W1 | ctx_11_requested_state_after_ask, ctx_11_request_types_message_and_records_event, ctx_11_live_for_role_filters_by_workspace, ctx_11_worker_briefing_explains_compact_ready, ctx_11_briefing_prefixes_start_the_rendered_messages |
| CTX-12 | Handoff path `ai_docs/handoffs/<role>-whats-next.md`; distinct per role; the handoff skill defaults to it. | W1 | ctx_12_handoff_path_per_role_is_distinct, ctx_12_handoff_skill_names_the_role_path |
| CTX-13 | horch types the per-harness command only after 2 idle polls, confirms by the transcript marker, then types the resume line. | W1 | ctx_13_job_waits_for_two_idle_polls, ctx_13_job_confirms_by_marker_then_types_resume, ctx_13_codex_resends_at_most_three_times, ctx_13_claude_gets_one_send |
| CTX-14 | horch refuses a compaction whose handoff, resolved against the record's workdir, is missing, or older than the newest request or warning of the record, or (with neither) older than 60 minutes, unless forced. | W1 | ctx_14_handoff_freshness_rule, ctx_14_compact_refuses_missing_or_stale_handoff, ctx_14_handoff_resolves_against_the_record_workdir |
| CTX-15 | Any job failure, a job that does not start, and a child that refuses each end in 1 `[horch] BLOCKED:` line with step, reason and log, and 1 `compact-failed` event. A lost job is row state `compact-lost` with 1 `compact-failed` event. The detached job survives the end of the tool call and of the turn (live check). | W1 | ctx_15_idle_timeout_fails_with_step_and_reason, ctx_15_failure_reports_blocked_line_and_event, ctx_15_lost_job_classifies_compact_lost, ctx_15_compact_start_handshake_fails_without_job_file, ctx_15_foreground_refusal_records_failed_and_reports, ctx_15_first_reader_of_a_lost_job_records_one_event, ctx_15_live_check_proves_detached_survival |
| CTX-16 | Fresh-session route for any harness not in `in_place`, and as the fallback. | W1 | ctx_16_compact_refuses_fresh_route_harness, ctx_16_orchestrator_briefing_names_the_fresh_route |
| CTX-17 | Each harness's reader and command have a re-runnable live check; a harness is compacted in place only when listed in `in_place`. Slice 2 (part C, 2026-10-07): the pi, Prime and OpenCode readers match the harness number (C2); OpenCode passes `horch note`, `agent_status` and the round trip (C5), so `in_place` is `[claude, codex, opencode]`; pi passes `horch note` and `agent_status`, but its round trip is not verifiable; Prime passes `horch note`, but herdr gives no `agent_status`. pi and Prime stay fresh. | W1 | ctx_17_in_place_only_for_listed_harnesses, ctx_17_live_check_covers_claude_and_codex, ctx_17_live_check_covers_slice_two_harnesses |
| CTX-18 | The orchestrator compacts itself by the same protocol, on both flavors. | W1 | ctx_18_compact_targets_the_orchestrator_pane_and_record, ctx_18_orchestrator_briefing_has_self_compaction |
| CTX-19 | Every fleet briefing carries exactly 1 compaction-instructions block; the orchestrator's names the roster and the ownership map. | W1 | ctx_19_every_fleet_briefing_has_one_keep_list_block |
| CTX-20 | The live check proves per harness whether a summary honours the block (canary); horch-driven compaction passes the keep-list as the argument where accepted. | W1 | ctx_20_compact_line_has_keep_list_only_where_accepted, ctx_20_live_check_plants_a_canary |
| CTX-21 | A teammate's `compact_at` (frontmatter only, no env variable) sets its watch base in place of 300,000; `horch teammates --check` rejects a value outside 50,000 to 1,000,000; `horch context`, `horch context --windows`, `horch compact` and the `horch note` check use it. | W1 | ctx_21_compact_at_range_checked, ctx_21_compact_at_sets_base |
| CTX-22 | Events `compact-requested`, `context-warned`, `compacted` (`<pre> -> <post> tokens; handoff <path>`), `compact-failed` (`step <step>: <reason>`). | W1 | ctx_22_job_records_compacted_with_exact_text, ctx_22_request_and_warning_event_texts |
| CTX-23 | A successful compaction sends exactly 1 `[horch] NOTE: <role> compacted. ...` line. | W1 | ctx_23_job_reports_success_once |
| CTX-24 | `horch context --json` prints a JSON array, 1 object per row, with the 24 keys of §5.1 in that order; an unknown value is `null`, never a guess; `pane` and `pane_status` come from 1 `herdr pane list` call per workspace and are `null` when herdr does not answer; `--json` ignores `--over`; `--windows` wins. | W1 | ctx_24_json_has_every_key_and_nulls, ctx_24_json_pane_from_one_pane_list |
| CTX-25 | Each row counts the session's compactions: COMPACTIONS `<native>n/<horch>h` in the table, `native_compactions` and `horch_compactions` in `--json`. A marker is horch-driven when a `compacted` event is at or after it and at most 15 minutes after it; each event matches 1 marker; the rest are native. | W1 | ctx_25_counts_native_and_horch_compactions, ctx_25_cli_counts_native_and_horch_compactions |
| CTX-26 | `horch context` with 10 sessions under 1 s; the `horch note` check under 0.5 s. | W1 | ctx_26_tail_read_is_bounded, ctx_26_marker_cache_reads_only_new_bytes |
| CTX-27 | No CTX code reads, sets or passes `ANTHROPIC_API_KEY`; `runtime/context.rs` captures no `FORBIDDEN_ENV` name; live checks unset it. | W1 | ctx_27_no_api_key_in_context_policy_code |

### 10.2 HDF: harness defaults

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| HDF-01 | `teammates/_base/harness-defaults.md` holds per-harness `env`, `args`, `settings`, `config` and `agent_settings` defaults; the roster attaches the entries whose base matches to each teammate, and the builder selects them by the teammate's final harness. | W1 | hdf_01_entries_attach_by_harness_and_base, hdf_01_brief_carries_harness_defaults, hdf_01_defaults_follow_the_final_harness |
| HDF-02 | Merge order: operator value (unless `force`), then teammate value, then horch's fixed switches, then the harness default. The operator values come from an allow-list of named env variables and from a managed-settings path in `RuntimeContext`. | W1 | hdf_02_teammate_env_wins_over_default, hdf_02_codex_default_args_skip_a_key_the_teammate_sets, hdf_02_codex_default_yields_to_operator_config_unless_forced, hdf_02_claude_settings_default_yields_to_operator_chain, hdf_02_opencode_config_default_merges_under_teammate_json, hdf_02_inherited_env_is_an_allow_list, hdf_02_managed_settings_path_comes_from_the_context |
| HDF-03 | The roster check fails a teammate line that repeats a matching default, a file that sets `harness_defaults`, and a malformed entry. | W1 | hdf_03_repeat_of_a_default_fails_the_check, hdf_03_malformed_entry_fails_the_check, hdf_03_builtin_roster_has_no_repeats |
| HDF-04 | Moving W2's 77 copies into the defaults changes no launch except the position of the Codex default `-c` pairs; the oracle world pins every default `env` key. | W1 | hdf_04_migrated_launch_env_and_args_match_w2, hdf_04_oracle_world_pins_every_default_env_key |

## 11. Acceptance

The acceptance criteria are those of the W1 requirements
(`ai_docs/plans/wave2/w1/requirements.md`, "Acceptance criteria"), with 2
changes that the operator's decision (the operator's tuned window wins) made:

- CTX-05.1 becomes: with the operator's `~/.claude/settings.json` at
  500000, every Claude pane's `/autocompact` prints `500k tokens` and
  `horch context` shows SOURCE `operator`; in a scratch `CLAUDE_CONFIG_DIR`
  with no window, the launch argv carries the fleet value and
  `/autocompact` with that argv prints `150k` (sonnet), `200k` (opus),
  `300k` (orchestrator). Live check A2 runs both halves.
- CTX-07.1: the table values come from §3.1 and §3.2 of this spec, not
  from the requirements table.

Live-only criteria (CTX-02.4, CTX-05, CTX-06, CTX-11.1, CTX-13.1 to
CTX-13.3, CTX-15 survival, CTX-16.1, CTX-17.1, CTX-18, CTX-20, CTX-22.1,
CTX-23.1, CTX-26) are steps of `scripts/live/context.sh`; their results are
in [`docs/live-checks/context.md`](../live-checks/context.md). If part B
or part C FAILs at the compaction step of a harness, that harness leaves `in_place`
until its check passes.
