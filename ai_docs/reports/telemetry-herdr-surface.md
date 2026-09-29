# Telemetry space: herdr surface and horch state (evidence report [H])

- Date: 2026-09-29
- Author: the fleet orchestrator (cloud session)
- Scope: evidence for the telemetry-space design. No code changed.
- Sources: vendored herdr docs (`herdr-docs/*.md`), horch source, and
  `ai_docs/reports/horch-tile-herdr-surface.md` (verified on the operator's Mac
  against herdr 0.8.2, 2026-09-20).
- Limits: herdr is not installed in this container and github.com is blocked.
  Nothing here was run against a live herdr. Docs claims are what the docs say,
  not what a binary does.

Provenance tags: **[docs]** herdr-docs file:line, **[code]** horch source
file:line, **[prior-report]** a report verified on the operator's Mac,
**[operator]** only knowable on the operator's machine.

## 1. horch state

1. **State root.** Ledgers live in `$HORCH_STATE_DIR` if set and non-empty,
   else `${XDG_STATE_HOME:-$HOME/.local/state}/horch` **[code]**
   `crates/horch-core/src/ledger.rs:111-122`. On Windows `USERPROFILE` stands
   in for `HOME` **[code]** `ledger.rs:132-138`. The project a ledger belongs to
   is `$HORCH_PROJECT_DIR`, else the cwd **[code]** `ledger.rs:124-130`.

2. **One ledger JSON per project, named by a lossy slug.** The file is
   `<state_root>/<slug(project)>.json` **[code]** `ledger.rs:148-152`. The slug
   maps every byte that is not ASCII alphanumeric to `-`, matching
   `tr -c 'A-Za-z0-9' '-'` **[code]** `ledger.rs:94-109`, tests at
   `ledger.rs:538-544`. The mapping is not reversible, so the project path
   cannot be recovered from the file name. Example collision: `/a/b-c` and
   `/a/b/c` both become `-a-b-c.json`. So do `/a/b_c`, `/a/b.c` and `/a/b c`.
   A `Record` holds no project path field **[code]** `ledger.rs:34-59`, so the
   file content does not recover it either.

3. **Lock idiom.** A read-modify-write takes `<ledger>.json.lock` with
   `std::fs::create_dir` (atomic; stock macOS has no `flock`) **[code]**
   `ledger.rs:172-205`. It retries every 100 ms. After 150 failed attempts
   (about 15 s of waiting) it removes the lock dir and tries again
   **[code]** `ledger.rs:190-194`. Note: "stale" is judged by how long the
   waiter has waited, not by the lock's age. The guard removes the dir on drop
   **[code]** `ledger.rs:76-85`. Writes go through a temp file and rename
   **[code]** `ledger.rs:207-211`. (`horch tile` uses a different lock: a
   `create_new` file `tile.lock` in the mailbox dir, 20 s wait, 60 s
   mtime-based stale break **[code]** `crates/horch/src/cmd/tilecmd.rs:30-32,362-416`.)

4. **Mailbox dir per workspace.** `std::env::temp_dir()/herdr-orchestration-<workspace_id>`
   **[code]** `crates/horch-core/src/mailbox.rs:74-87`. It dies with the
   machine's temp dir, not with panes **[code]** `mailbox.rs:4-5`. It holds:
   `<role>.id` (public pane id) **[code]** `mailbox.rs:127-129,150-153`;
   `<role>.brief.json` (role, teammate, agent, model, record id, session id,
   task, project dir, state dir, bin overrides, resolved teammate) **[code]**
   `mailbox.rs:21-58,131-133`; `.seq-<teammate>` counters, `.<role>.launch-marker`,
   `<role>.harvest.log` **[code]** `mailbox.rs:218-239`; and `tile.lock`
   **[code]** `tilecmd.rs:377-381`. The workspace id comes from
   `HORCH_WORKSPACE_ID`, else from `herdr pane get $HERDR_PANE_ID` **[code]**
   `mailbox.rs:96-113`.

5. **`horch cost`.** It is after-the-fact and per project: it opens only
   `Ledger::open()` (the ambient project's ledger) **[code]**
   `crates/horch/src/cmd/cost.rs:89-91`. Extra sessions can be added by hand
   with `--session <agent>:<id>` **[code]** `cost.rs:102-121`. For every record
   it calls `usage::read_session` **[code]** `cost.rs:193-194`, which finds the
   transcript by session id and reads the WHOLE file with `read_to_string`,
   every run, with no cache or offset **[code]**
   `crates/horch-core/src/usage.rs:503-520`. Claude search scans
   `~/.claude/projects/*` **[code]** `usage.rs:197-207`; codex and pi walk their
   session trees recursively **[code]** `usage.rs:210-246`. opencode is not read
   (its store is SQLite) and is reported as `not_read` **[code]**
   `usage.rs:13,498,515`. There is no cross-project roll-up.

6. **Env vars a worker pane gets.** herdr panes do not inherit the spawner's
   env, so values travel in the brief **[code]** `mailbox.rs:40-42`. `horch worker`
   exports into its own process before launching the agent **[code]**
   `crates/horch/src/cmd/worker.rs:70-96`: `HORCH_ROLE`, `HORCH_TEAMMATE`,
   `HORCH_AGENT`, `HORCH_MODEL`, `HORCH_RECORD_ID`, `HORCH_SESSION_ID`,
   `HORCH_RESUME`, `HORCH_TASK`, `HORCH_PROJECT_DIR`, and when set
   `HORCH_STATE_DIR`, `HORCH_CLAUDE_BIN`, `HORCH_CODEX_BIN`,
   `HORCH_TEAMMATES_DIR`. `horch register` adds `HORCH_WORKSPACE_ID` **[code]**
   `mailbox.rs:154`. Other HORCH_* read by horch: `HORCH_TILE` (0/false/no/off
   disables auto-tile) **[code]** `tilecmd.rs:423-428`; `HORCH_OPENCODE_BIN`,
   `HORCH_PI_BIN`, `HORCH_PRIME_BIN` (bin overrides). herdr itself injects
   `HERDR_ENV`, `HERDR_SOCKET_PATH`, `HERDR_WORKSPACE_ID`, `HERDR_TAB_ID`,
   `HERDR_PANE_ID` **[docs]** `socket-api.md:211`.

7. **How herdr is invoked.** Always as a child process, `Command::new("herdr")`,
   resolved on `PATH`. The name is hard-coded at **[code]**
   `crates/horch-core/src/herdr.rs:307` (`output`, used by every JSON call),
   `:339` (`server_reachable`), `:655` (`wait_output`) and `:665`
   (`integration_status`). `doctor` also checks `which("herdr")` **[code]**
   `crates/horch/src/cmd/doctor.rs:40`. No env var overrides the binary (no
   `HORCH_HERDR_BIN`; grep finds none). horch never talks to the socket
   directly. Because the child inherits horch's env, herdr's own
   `HERDR_SESSION` / `HERDR_SOCKET_PATH` do pick the session **[docs]**
   `socket-api.md:490-495`.

## 2. herdr capabilities

| Topic | What the docs say | Cite |
| --- | --- | --- |
| Session model | A session is "a persistent Herdr server namespace". Named sessions are "separate runtime namespaces" with their own panes, tabs, workspaces, sockets and runtime state; they share the global config. | [docs] `concepts.md:48-60`, `persistence-remote.md:13-25` |
| Session commands | `herdr --session <name>`; `herdr session list/attach/stop/delete [--json]`. | [docs] `cli-reference.md:10-11,97-106` |
| Socket paths | Default `~/.config/herdr/herdr.sock`; named `~/.config/herdr/sessions/<name>/herdr.sock`. | [docs] `socket-api.md:479-488` |
| Picking a session | Order: CLI `--session`, then `HERDR_SOCKET_PATH`, then `HERDR_SESSION`, then default. | [docs] `socket-api.md:490-497`, `cli-reference.md:439-440` |
| How many sessions | No number is given. One socket serves one session. No "8" and no session cap appears anywhere in `herdr-docs/`. | [docs] (absence; grep) |
| Cross-session view | Not documented. `session.snapshot` covers the connected session only. | [docs] `socket-api.md:95,111` |
| Headless server | `herdr server` "runs the headless server explicitly. Use it for supervised or service-style setups." | [docs] `cli-reference.md:79-87` |
| `workspace create` | `[--cwd PATH] [--label TEXT] [--env KEY=VALUE] [--focus] [--no-focus]`. Returns `.result.workspace.workspace_id`, `.result.tab.tab_id`, `.result.root_pane.pane_id`. | [docs] `cli-reference.md:112,120-126`, `agent-automation.md:20,28` |
| Focus default | "Workspace and tab creation, and pane splitting, leave focus unchanged by default. `--focus` selects the new layout; `--no-focus` states the default explicitly." | [docs] `cli-reference.md:154` |
| `workspace list` shape | Not shown in the docs. horch reads `.result.workspaces[].workspace_id` and `active_tab_id` only. List responses carry a `tokens` map. | [code] `herdr.rs:100-106,148-150`; [docs] `socket-api.md:591` |
| Find by label | No lookup-by-label command. `workspace get`, `pane get`, `tab get` take ids. Labels can be set (`workspace rename`, `pane rename`) but not queried. Only agents have a name alias usable as a target (`agent start <name>`, `agent rename`), unique among live agents, cleared on exit. A caller can still filter `workspace list` client-side. | [docs] `cli-reference.md:111-117,146,161,169,283`, `agent-automation.md:36-38` |
| `pane run` | Submits text plus Enter atomically, honouring bracketed paste. | [docs] `cli-reference.md:196,201` |
| `pane split` | `[<pane_id>] --direction right\|down [--ratio] [--cwd] [--env] [--focus] [--no-focus]`; new id at `.result.pane.pane_id`. | [docs] `cli-reference.md:170,179` |
| `pane list` | `[--workspace <id>]`. Real shape carries `pane_id`, `workspace_id`, `tab_id`, `agent`, `agent_status`, `focused`, `cwd`, `agent_session`. | [docs] `cli-reference.md:159`; [prior-report] `horch-tile-herdr-surface.md:59-67` |
| `agent_session` | Read-only object on `pane get/list`, `agent get/list` when an official integration reported a native session; omitted otherwise. Shape `{source, agent, kind, value}`. | [docs] `cli-reference.md:233`, `socket-api.md:536-549`; [prior-report] `horch-tile-herdr-surface.md:66` |
| `workspace report-metadata` | `<workspace_id> --source ID [--token NAME=VALUE] [--clear-token NAME] [--seq N] [--ttl-ms N]`; socket `workspace.report_metadata`. | [docs] `cli-reference.md:116`, `socket-api.md:96,585-591` |
| Token limits | "A report may mention at most 16 token keys, and a pane or workspace may retain at most 32 keys. Token names are 1–32 ASCII letters, digits, underscores, or hyphens." | [docs] `socket-api.md:583` |
| Value limits | Token values (and title, display_agent, state labels) are capped "at 80 characters". Empty value clears the key. | [docs] `socket-api.md:593`, `cli-reference.md:258` |
| Source limits | `source` at most 80 chars, `[A-Za-z0-9:._-]`; at most 32 distinct sequenced sources per pane or workspace for its lifetime (not released on clear). | [docs] `socket-api.md:595,599`, `cli-reference.md:262` |
| TTL / persistence | `ttl_ms` 1..86400000. "Token metadata is not restored after a server restart." Changes emit `workspace.metadata_updated`. | [docs] `socket-api.md:591,597` |
| Sidebar render | Workspace tokens render as `$name` in Space rows; at most 16 rows and 16 tokens per row per layout. | [docs] `configuration.md:289,291,345`, `config-reference.md:463-465` |

## 3. What horch calls today

All calls go through `crates/horch-core/src/herdr.rs`. Callers are in
`crates/horch/src/cmd/*.rs` and `mailbox.rs`. M = mutates layout, focus or pane
lifetime.

| Subcommand | herdr.rs line | Main callers | Purpose | M |
| --- | --- | --- | --- | --- |
| `workspace list` | 340, 431 | doctor.rs:46, tilecmd.rs:171, smoke.rs:412 | reachability probe; active tab for focus restore | |
| `workspace create --label [--cwd] [--no-focus]` | 630-643 | recipes.rs:205,291; smoke.rs:73,142,464 | fleet / orchestration workspace | yes |
| `workspace close` | 647 | smoke.rs:105,200,712 | tear down smoke workspaces | yes |
| `pane get` | 349 | mailbox.rs:108,144; messaging.rs:98; spawn.rs:206; tilecmd.rs:574 | internal id to public id, workspace id | |
| `pane list --workspace` | 354 | tilecmd.rs:119; layoutcmd.rs:24; balancecmd.rs:89 | enumerate panes in one workspace | |
| `pane split --direction --no-focus` | 359-367 | spawn.rs:210; recipes.rs:294-297; smoke.rs | new worker pane | yes |
| `pane run` | 374 | spawn.rs:213; recipes.rs:211,329; smoke.rs | launch a command in a fresh shell | |
| `pane send-text` / `send-keys enter` | 379, 384 (via `send_line` 680-688) | messaging.rs:35 (`horch tell`) | type into a TUI pane | |
| `pane close` | 389 | messaging.rs:115 (`horch done`); smoke.rs:682 | worker closes itself | yes |
| `pane read --source` | 399 | smoke.rs:115 | smoke check | |
| `pane layout --pane\|--current` | 404-405 | tilecmd.rs:130,595; balancecmd.rs:96; layoutcmd.rs:37; spawn.rs:225 | tab geometry, focus, zoom | |
| `tab list --workspace` | 423 | tilecmd.rs:118; layoutcmd.rs:23 | tabs of one workspace | |
| `tab create` | 444-452 | none outside herdr.rs (tile avoids it) | wrapper only | yes |
| `tab close` | 466 | none (documented destructive) | wrapper only | yes |
| `tab focus` | 473 | tilecmd.rs:238,330,338 | restore focus after tile | yes (focus) |
| `pane focus --direction --pane` | 484-489 (via `pane_focus_walk` 507) | tilecmd.rs:342; smoke.rs:520,605 | walk focus to a pane | yes (focus) |
| `pane move --tab --split [--target-pane] [--ratio] --no-focus` | 565-588 | tilecmd.rs:232 | place a worker in the grid | yes (move) |
| `pane move --new-tab --label --no-focus` | 594 | tilecmd.rs:206 | park tab | yes (move) |
| `pane resize --pane --direction --amount` | 612-619 | tilecmd.rs:280; balancecmd.rs:53 | even columns | yes (tile) |
| `wait output --match --timeout` | 656 | smoke.rs:101 | smoke check | |
| `integration status` | 666 | recipes.rs:266 | warn on missing integrations | |

Not called anywhere: `pane swap`, `workspace focus`, `workspace rename`,
`workspace report-metadata`, `pane report-metadata`, `agent *`, `session *`,
`server`, event subscriptions **[code]** (grep of `crates/`).

## 4. Dependencies

From the Cargo.toml files **[code]**:

| Crate | Dependencies |
| --- | --- |
| workspace (`Cargo.toml`) | shared: anyhow 1, clap 4 (derive, env), serde 1 (derive), serde_json 1, serde_yaml 0.9 |
| `horch-core` | anyhow, serde, serde_json, serde_yaml, chrono 0.4 (clock, std), uuid 1 (v4); unix: libc 0.2; dev: tempfile 3 |
| `horch` | horch-core, anyhow, clap, serde, serde_json, tempfile 3; unix: libc 0.2; dev: serde_yaml |
| `herdr-install` | anyhow, clap, serde, serde_json, ureq 3 (rustls, json), sha2 0.10, zip 2 (deflate); windows: winreg 0.52; dev: tempfile |
| `herdr-docs-sync` | anyhow, clap, ureq 3 (rustls), scraper 0.21, ego-tree 0.9, htmd 0.1, quick-xml 0.37; dev: tempfile |

Confirmed absent from every Cargo.toml and from `Cargo.lock`: no TUI crate
(ratatui, crossterm), no async runtime (tokio), no SQLite crate (rusqlite,
libsqlite3-sys), no OTLP / opentelemetry crate **[code]** (grep).

Network: in this cloud container `static.crates.io` is blocked by the egress
policy. `curl https://static.crates.io/crates/...` fails with
`CONNECT tunnel failed, response 403` (re-checked 2026-09-29); the sparse
index `index.crates.io` answers 200. So `cargo fetch` of any crate not already
cached fails, and no new crate can be added or built here until the
environment's network policy allows it.

## 5. Implications for a telemetry space

- **A file-based data plane beats a herdr-hosted one.** Token metadata is
  display-only, capped at 16 keys per report, 32 kept per workspace, 80-char
  values, 32 sources per lifetime, and is lost on server restart **[docs]**
  `socket-api.md:583,593,597,599`. It cannot hold time series or per-call
  cost. horch already keeps durable state in plain files under a known root
  (facts 1-4). Telemetry should write there and use herdr tokens, if at all,
  only as a short summary view.
- **herdr cannot aggregate across sessions.** Each session is its own socket
  and namespace **[docs]** `concepts.md:60`, `socket-api.md:483-495`. A
  collector that must see several sessions has to read files, or call herdr
  once per session with `HERDR_SESSION` set (horch's hard-coded `herdr`
  inherits env, fact 7).
- **The collector needs its own workspace, created with `--no-focus`.** herdr
  already defaults to no focus **[docs]** `cli-reference.md:154`, and
  `workspace_create(.., focus=false)` passes `--no-focus` **[code]**
  `herdr.rs:636-638`. Its own workspace keeps it out of the fleet's mailbox
  and tile scope, since both are keyed by workspace id (fact 4).
- **`horch tile` must never run on the collector's workspace, and never have
  the collector in a fleet workspace.** Tile works on exactly one workspace:
  `--workspace`, the workspace of `--pane`, else the caller's own **[code]**
  `tilecmd.rs:566-585`. It reads every tab and pane of it **[code]**
  `tilecmd.rs:117-135` and treats EVERY pane that is not the orchestrator as a
  worker to move **[code]** `tilecmd.rs:57-72`. A collector pane in a fleet
  workspace would be parked and re-gridded. Tile also refuses a workspace with
  no registered or inferable orchestrator **[code]** `tilecmd.rs:141-162`,
  which is a safe failure for a collector workspace, but only if it never
  registers the role `orchestrator`.
- **`horch balance` picks one tab, not a workspace.** It resolves a pane
  (`--pane`, first pane of `--workspace`, else `HERDR_PANE_ID`) and resizes
  that pane's tab only **[code]** `balancecmd.rs:82-96`. It only acts on a
  settled 2-row grid **[code]** `balancecmd.rs:101-107`,
  `crates/horch-core/src/balance.rs:162`. A single-pane collector tab is left
  alone, but nothing names it as excluded.
- **Auto-tile hooks fire on spawn and done.** `horch spawn` calls
  `after_change` on the spawner's mailbox workspace **[code]** `spawn.rs:229`,
  and `horch done` starts a detached `horch tile|balance --workspace <ws>`
  **[code]** `messaging.rs:113`, `tilecmd.rs:613-626`. The collector must not
  be started with `horch spawn` into a fleet workspace.
- **Cost data must be incremental.** `horch cost` re-reads whole transcripts
  per run, one project at a time, and skips opencode (fact 5). A live
  collector should tail by byte offset and key rows by session id, not by the
  lossy ledger slug (fact 2).
- **Do not trust `agent_session` alone for attribution.** It is present only
  when an official integration reported it **[docs]** `socket-api.md:536-549`;
  the ledger's session id is the authoritative join key **[code]**
  `usage.rs:4-5`.

## 6. Conflicts with the design

| Design statement | Verdict | Evidence |
| --- | --- | --- |
| "herdr sees 1 of 8 sessions per socket" | **Partly not in the docs.** One socket = one session is documented. The figure "8" is not in the docs; no session count or cap is given anywhere. Needs **[operator]** evidence or should be dropped. | [docs] `socket-api.md:479-495`, `concepts.md:48-60`, `persistence-remote.md:25` |
| "...and cannot find a pane by label" | **Consistent, with one caveat.** No lookup by workspace/tab/pane label. But agents can be targeted by a unique live name (`agent start <name>`, `agent rename`), cleared when the agent exits. | [docs] `cli-reference.md:283`, `agent-automation.md:36-38`, `agents.md:105-114` |
| "`workspace create --no-focus` exists" | **Confirmed.** Also: no-focus is already the default. | [docs] `cli-reference.md:112,123,154` |
| "report_metadata limits: 16 keys, 80-char values" | **Incomplete.** 16 is the per-REPORT key limit; a workspace may RETAIN up to 32 keys. 80 chars is correct for values and for `source`. Also missing: token names 1-32 chars, 32 sources per lifetime, not restored after restart. ("16" also appears as sidebar rows/tokens-per-row, a different limit.) | [docs] `socket-api.md:583,593,595,597,599`, `configuration.md:291` |
| "herdr is hard-coded in herdr.rs" | **Confirmed.** Lines 307, 339, 655, 665; no env override. Session selection still works via inherited `HERDR_SESSION` / `HERDR_SOCKET_PATH`. | [code] `herdr.rs:307,339,655,665`; [docs] `socket-api.md:490-495` |
| "horch cost re-reads whole transcripts, per project" | **Confirmed.** `read_to_string` of each transcript every run; only the ambient project's ledger. | [code] `usage.rs:518`, `cost.rs:90` |

Side findings (not design claims, but they touch the same surface):

- `workspace_create(.., focus=true)` passes no flag at all **[code]**
  `herdr.rs:636-638`. Per the docs the default is no focus **[docs]**
  `cli-reference.md:154`, so `horch fleet` / `horch orchestration`
  (`recipes.rs:205,291`) may not focus their new workspace on 0.8.x. Whether
  that is true on the operator's herdr is **[operator]**.
- `wait_output` calls `herdr wait output` **[code]** `herdr.rs:656`. The docs
  only list `herdr pane wait-output` **[docs]** `cli-reference.md:316`. Used
  only by `horch smoke`. Unverified here.
- The code says `HERDR_PANE_ID` holds an internal id like `p_2` **[code]**
  `mailbox.rs:93`, `herdr.rs:346-347`; the docs call it the public pane id
  **[docs]** `cli-reference.md:443`. horch round-trips it through `pane get`,
  so either works. Docs also offer `HERDR_WORKSPACE_ID`, which horch does not
  use **[docs]** `cli-reference.md:445`.
