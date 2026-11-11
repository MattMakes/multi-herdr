## Summary

Scope: how `horch` wires each agent CLI and herdr today, where each login comes from at run time, and each place where the code differs by OS. Measured at worktree tip `99b05f19` (branch `fleet/fleetd-design`). "The design" is `ai_docs/plans/fleetd-design/source-design.md`. All paths below are under `crates/` unless they start with `teammates/` or `docs/`.

`NOT MENTIONED` rows that would cause lost work, a stuck fleet or a security problem if fleetd dropped them, most important first:

1. S1 (security). `ANTHROPIC_API_KEY` removal covers more than the `claude` launch: every child through `scrub_child_env` (`horch-core/src/runtime/process.rs:190`), every `herdr pane run` line (`horch-core/src/workspace/paneshell.rs:11`), detached children (`runtime/process.rs:277`), updaters, probes, and a roster check that rejects a teammate file that sets it (`roster/validation.rs:481`). The design names only the `claude` launch (line 190). Section 4.
2. S2 (security). herdr can resume agent panes after a server restart from the herdr server's own environment, which horch does not scrub. `horch` warns when `resume_agents_on_restore` is not false (`horch/src/cmd/restore.rs:62`). The design (line 158) relies on herdr resume and does not mention this.
3. S3 (security). `agy` children lose `GEMINI_API_KEY`, `GOOGLE_API_KEY` and `GOOGLE_GEMINI_BASE_URL` (`horch-core/src/harness/antigravity.rs:26`), so `agy` stays on the operator's Google login. The roster check rejects them (`roster/validation.rs:495`).
4. S4 (stuck fleet). The private `CODEX_HOME` is a directory of symlinks to every entry of the real codex home except `rules/` (`harness/codex.rs:325`). `auth.json` therefore resolves to the operator's login, and `sessions/` to the real rollouts. The design keeps a private `CODEX_HOME` (line 530) but says the wrapper builds it "from the profile" (line 206). A home without the `auth.json` link has no login.
5. S5 (stuck fleet). The Prime agent dir links `auth.json`, also when it is missing, so a refreshed token writes back to the operator's file (`harness/prime.rs:33`, `:907`). The design has no Prime, pi or OpenCode adapter at all.
6. S6 (stuck fleet). Codex hook trust: every codex launch gets `--dangerously-bypass-hook-trust` (`harness/codex.rs:1379`). The design lists hook pre-trust as unconfirmed (line 187).
7. S7 (security). Agent CLI updates wait out a release cooldown read from the npm registry; a CLI with unreadable release dates is held back (`harness/cooldown.rs:1`). The design has docs sync on a version change (line 759) and no cooldown.
8. S8 (stuck fleet). Pane auto-update is off: `DISABLE_AUTOUPDATER=1` for claude (`teammates/_base/harness-defaults.md:12`), `-c check_for_update_on_startup=false` for codex (`harness/codex.rs:1353`). `herdr update` runs only on operator request, because it can stop the herdr server and close every pane (`harness/update.rs:30`).
9. S9 (security). A sandboxed Claude teammate never starts on a host that cannot sandbox it: macOS needs `/usr/bin/sandbox-exec`, Linux needs `bwrap` and `socat`, other OSes are refused (`harness/claude.rs:1082`). The design names bubblewrap only for codex (line 186).
10. S10 (stuck fleet). Trust is written into the operator's real stores (`.claude.json`, `$CODEX_HOME/config.toml`, `~/.gemini/antigravity-cli/settings.json`) under a lock, atomically, never for `/`, home or an ancestor of home, and never over an explicit codex `untrusted` (`harness/trust.rs:1`, `:471`). The design seeds trust in private homes (line 188): `CHANGED`, but the safety rules are not mentioned.
11. S11 (lost work on Windows). The worker channel relay and the report socket need Unix sockets. On Windows a send fails and `horch_access: report` is refused (`messaging/channel.rs:104`, `messaging/report.rs:237`, `execution/lifecycle.rs:1046`).
12. S12 (stuck fleet on Windows). The machine probe returns unknown disk, memory and load on Windows (`runtime/machine.rs:322`). The D8 free-disk floor and load guard have no input there.
13. S13 (stuck fleet). The quota probes read limits through each CLI's own client, so horch holds no credential (`routing/quota.rs:9`, `routing/quota_probe.rs:83`, `:140`, `:195`). D2 depends on these readings. The design mentions quota only as budget (line 803).
14. S14. The design assumes systemd or launchd, `WatchdogSec`, `/proc`, inotify and Unix signals to a process group (section 8). The word "Windows" has 0 hits in the design. D1 makes Windows first-class.
15. S15. The OpenCode, pi and Prime adapters (launch, skills, resume, context and quota readers) are `NOT MENTIONED`. Dropping them removes the `opencode-zen` and `local` pools (`routing/quota.rs:46`).

Counts: 178 OS-specific `cfg` sites in production code, 340 more in tests (518 lines from the ticket grep, 103 files). The ticket grep misses `cfg(any(` (13 sites) and `cfg!(` (35 sites); section 7 counts them separately.

## 1. Method

- Step 1: remote is `git@git-alt:MattMakes/multi-herdr.git` and email is `m4ttplu5@gmail.com`. Both match.
- Code read: `horch-core/src/harness/*`, `workspace/herdr.rs`, `workspace/paneshell.rs`, `runtime/*`, `procid.rs`, `fsx.rs`, `routing/quota*.rs`, `telemetry/context.rs`, `telemetry/readers.rs`, `usage.rs`, `roster/permission.rs`, `roster/validation.rs`, `teammates/_base/harness-defaults.md`.
- Each `NOT MENTIONED` was checked with `grep -n -i` on the design for the CLI, flag or concept name.
- A row that is test-only is excluded. A production site is a site before the file's `#[cfg(test)] mod` line.

## 2. Wiring map (step 3)

Harnesses: `claude`, `codex`, `agy` (`antigravity.rs`), `opencode`, `pi`, `prime`, and `none` (smoke teammate, launches nothing, `harness/none.rs:15`). The shared launch flow is `harness/launch.rs:519` (`run_flow_code`): skills, briefing, trust, window decision, adapter `prepare`, command, scrub, run, discovery, `finish`.

| Concern | Harness | Implementation | Design |
| --- | --- | --- | --- |
| Launch command | all | 1 builder per harness behind `Harness::command`; callers never match on the kind (`harness/mod.rs:121`). `build_command` adds the env scrub (`mod.rs:224`) | CHANGED: wrapper builds flags, exec mode default (line 533, D7) |
| Launch command | claude | variadic flags first, then `--model`, settings, session, `args`, prompt last (`harness/claude.rs:155`). `--append-system-prompt-file` (`claude.rs:43`) | CHANGED: `claude -p --output-format stream-json` in exec (line 533) |
| Launch command | codex | `resume` subcommand first, `-c model=`, `-c tui.resume_cwd="current"`, `--search` on every mode (`codex.rs:1332`, `:1254`) | CHANGED: `codex exec --json` (line 533) |
| Launch command | agy | `--model`, `--effort`, mode flags, `--conversation`, `args`, `--prompt-interactive` (`antigravity.rs:76`) | CHANGED: `agy -p --output-format stream-json` (line 533) |
| Launch command | opencode | `--model`, `OPENCODE_CONFIG_CONTENT`, `--auto`, `--pure` unless `inherit_plugins`, `--session`/`--prompt` (`opencode.rs:190`) | NOT MENTIONED |
| Launch command | pi, prime | `--model`, `--thinking`, tool flags, `--no-skills --no-prompt-templates --no-themes`, `-- <prompt>` (`pi.rs:134`, `:184`) | NOT MENTIONED |
| Model and effort | claude | `--model`, `--effort` dropped for a model without effort (Haiku) (`claude.rs:249`) | KEPT (line 497) |
| Model and effort | codex | `-c model="..."`, `-c model_reasoning_effort="..."` (`codex.rs:1340`, `:1358`) | KEPT (line 497) |
| Model and effort | agy | `--model`; `--effort` only for a bare id, id suffix `-low/-medium/-high` otherwise (`antigravity.rs:30`, `:79`) | KEPT (line 497) |
| Model and effort | opencode, pi, prime | `--model`; pi and prime `--thinking` (`pi.rs:142`) | NOT MENTIONED |
| Permission or sandbox | claude | `--permission-mode` (`claude.rs:257`); `sandbox:` overlay in `--settings` with forced keys (`claude.rs:769`, `:782`); horch paths opened per `horch_access` (`claude.rs:837`) | CHANGED: `dontAsk` plus allow rules, `--permission-prompts none` (line 540) |
| Permission or sandbox | codex | write pane: fleet permission profile, `-a never`, state root as workspace root, private TMPDIR (`codex.rs:1130`, `:627`); plan pane `:read-only -a on-request` (`codex.rs:1233`) | CHANGED: `-a never`, drop `broad_ledger_rule` (line 538) |
| Permission or sandbox | codex | execpolicy rules for fleet commands in a private `rules/` (`codex.rs:1`, `:302`); `codex sandbox` runs checks in the write profile (`codex.rs:1155`) | CHANGED (line 538) |
| Permission or sandbox | agy | `Plan`/`AcceptEdits`/`Auto`=`--sandbox --dangerously-skip-permissions`/`Bypass`; others refused (`roster/permission.rs:62`) | CHANGED: `--mode auto` (line 540) |
| Permission or sandbox | opencode | `--auto` for write modes; `Plan`, `Manual`, `DontAsk` refused (`roster/permission.rs:44`) | NOT MENTIONED |
| Permission or sandbox | pi, prime | no approval gate; `permission_mode` rejected by the roster check (`pi.rs:130`) | NOT MENTIONED |
| Config home | claude | no `CLAUDE_CONFIG_DIR`; inline `--settings` overlay merged with operator settings (`claude.rs:302`, `:365`); harness defaults (`teammates/_base/harness-defaults.md:9`) | KEPT (line 530) |
| Config home | codex | private `CODEX_HOME` of symlinks, set on the child only (`codex.rs:325`, `:763`); Windows: shared rules file (`codex.rs:361`) | KEPT (line 530); symlink detail NOT MENTIONED |
| Config home | agy | operator `HOME` (no private home) | CHANGED: private `HOME` (line 530) |
| Config home | opencode | `OPENCODE_CONFIG_CONTENT` merged with the inherited value (`opencode.rs:212`) | NOT MENTIONED |
| Config home | prime | private agent dir of links, generated `settings.json` and `models.json` (`prime.rs:30`, `:532`) | NOT MENTIONED |
| Config home | pi | operator `~/.pi/agent` not read or written (`pi.rs:9`) | NOT MENTIONED |
| Skills | claude | bundle as `--plugin-dir` plus settings overlay; skills show as `horch:<id>` (`claude.rs:27`, `:67`, `:1134`) | KEPT (line 173) |
| Skills | codex | `skills` link in the private home (`codex.rs:212`); Windows refuses phase skills (`codex.rs:223`) | KEPT (line 173) |
| Skills | opencode | `skills.paths` in `OPENCODE_CONFIG_CONTENT`; `OPENCODE_DISABLE_EXTERNAL_SKILLS=1` (`opencode.rs:113`) | NOT MENTIONED |
| Skills | pi, prime | `--skill <dir>` (`pi.rs:97`, `prime.rs:1077`); capability table `capabilities.rs:9` | NOT MENTIONED |
| Skills | agy | none (`SkillExposure::None`) | NOT MENTIONED (design line 173 lists `.agents/skills`) |
| MCP servers | claude | inline `--mcp-config` with the horch channel server, `--strict-mcp-config` when `mcp_servers` is set (`claude.rs:197`, `:217`) | KEPT (line 530) |
| MCP servers | codex | only the horch channel, as `-c mcp_servers.horch.*` with 14 passed env names (`codex.rs:1400`) | NOT MENTIONED |
| MCP servers | others | `mcp_servers` and `mcp_config_files` rejected by `generic_validate` (`harness/mod.rs:247`) | NOT MENTIONED |
| Hooks | all | horch installs no hooks in any CLI. Codex gets `--dangerously-bypass-hook-trust` (`codex.rs:1379`); codex `-c notify=[]` for workers (`harness-defaults.md:20`) | CHANGED: stop hook and liveness hooks added (lines 538-539) |
| Resume | claude | horch mints the id: `--session-id` fresh, `--resume` later (`claude.rs:379`) | KEPT (line 180) |
| Resume | codex | id found after launch from rollout files by launch token and cwd (`codex.rs:509`, `:805`) | KEPT (line 180) |
| Resume | agy | id read from `~/.gemini/antigravity-cli/cache/last_conversations.json` by workdir and mtime (`antigravity.rs:34`, `:59`) | KEPT (line 180) |
| Resume | opencode | `opencode session list --format json` by directory and time (`opencode.rs:35`); prompt typed after resume (`opencode.rs:149`) | NOT MENTIONED |
| Resume | pi | `--session-id` creates or resumes (`pi.rs:200`) | NOT MENTIONED |
| Resume | prime | private `--session-dir`; the session file path is the handle (`prime.rs:1023`, `:1099`) | NOT MENTIONED |
| Context reader | claude, codex, pi, prime, opencode | tail of the main transcript, 256 KiB window, marker cache (`telemetry/context.rs:1`, `:273`, `:320`, `:341`, `:386`); opencode through `sqlite3` | NOT MENTIONED |
| Context reader | agy | none (`usage.rs:775` excludes it) | CHANGED: wrapper tracks usage (line 179) |
| Usage or quota reader | claude | `claude -p --model haiku --input-format stream-json` plus 1 `get_usage` control request (`routing/quota_probe.rs:83`) | NOT MENTIONED |
| Usage or quota reader | codex | `codex app-server`: `account/rateLimits/read` (`quota_probe.rs:140`); rollout snapshot (`quota_probe.rs:414`) | NOT MENTIONED |
| Usage or quota reader | agy | `agy -p /usage --output-format json` (`quota_probe.rs:195`) | NOT MENTIONED |
| Usage or quota reader | opencode | no probe; `FreeUsageLimitError` starts a cooldown (`routing/quota.rs:16`) | NOT MENTIONED |
| Usage or quota reader | pi | `pi --version` plus `ollama list` (`quota_probe.rs:282`) | NOT MENTIONED |
| Usage or quota reader | all | token readers per transcript (`telemetry/readers.rs:11`) | KEPT (line 84, "telemetry") |
| Trust prompt | claude, codex, agy | `ensure_trusted` before `prepare` (`launch.rs:576`, `trust.rs:471`) | CHANGED: seeded in private homes (line 188) |
| Trust prompt | opencode, pi, prime | no trust dialog (`trust.rs:53`) | NOT MENTIONED |
| Update check | all 6 | `horch update` and the fleet start step: per-CLI lock, 24 h freshness, no kill at timeout, release cooldown (`harness/update.rs:1`, `:48`, `:149`) | NOT MENTIONED |
| Update check | claude, codex | pane self-update off (`harness-defaults.md:12`, `codex.rs:1353`) | NOT MENTIONED |
| Update check | herdr | operator request only, attached to the terminal (`update.rs:653`) | NOT MENTIONED |
| Compact window | claude, codex, prime | `CLAUDE_CODE_AUTO_COMPACT_WINDOW` in settings env (`claude.rs:548`); `model_auto_compact_token_limit` (`codex.rs:1463`); Prime `models.json` (`prime.rs:36`) | KEPT (line 531) |
| Unwanted model calls | claude, codex, opencode, prime | prompt suggestion off, `tui.auto_recap=false`, title off, `RLM_MAX_DEPTH=1`, `autoRefine` off, attribution off (`harness-defaults.md:9`) | NOT MENTIONED |
| Remote Control | claude | `remoteControlAtStartup=false` in every worker pane (`claude.rs:1154`) | NOT MENTIONED |
| Daemon | prime | per-launch daemon socket under a private `TMPDIR`, stopped on finish; herdr agent released (`prime.rs:1`, `:1048`) | NOT MENTIONED |

## 3. herdr wiring (step 4)

horch calls herdr only through its CLI, never through the socket API (`workspace/herdr.rs:1`). Every call reads the `{"result": ...}` JSON envelope (`herdr.rs:176`). A failure carries herdr's stderr as typed `HerdrFailed` (`herdr.rs:61`). A missing binary prints the `herdr-install` hint (`herdr.rs:155`). Calls run in their own process group (Unix) or a job object (Windows) with a timeout or a deadline (`herdr.rs:676`, `:876`).

Expected version: the base surface is verified on herdr 0.6.1 and re-checked on 0.8.0. Tile calls, `pane wait-output` and `pane report-agent` are verified on 0.8.2 (`herdr.rs:4`, `:460`, `:504`). No code checks the herdr version: `horch doctor` checks other tools' minimums (`horch/src/cmd/doctor.rs:684`) but not herdr's. The installed herdr is 0.9.4-preview (decisions.md). The design reads 0.9.5 (line 147).

| Call | Implementation | Callers | Depends on in herdr output | Design |
| --- | --- | --- | --- | --- |
| `workspace list` | `herdr.rs:185`, `:269` | reachability probe (`execution/lifecycle.rs:317`), `arrange.rs:50`, `sweep.rs:631` | exit 0; `workspaces` | CHANGED: socket, `session.snapshot` (line 158) |
| `pane get <id>` | `herdr.rs:195` | `launch.rs:968`, `execution/store.rs:889` | `pane.pane_id`, `agent_status`, `state_change_seq`; accepts internal `p_2` ids | CHANGED (line 158) |
| `pane list --workspace` | `herdr.rs:200` | `arrange.rs:167` | `panes[]` with `tab_id` | CHANGED (line 158) |
| `pane split --direction --no-focus` | `herdr.rs:205` | `execution/service.rs:266`, `sweep.rs:593` | new pane id | KEPT (line 149) |
| `pane run <pane> <line>` | `herdr.rs:220` | `execution/service.rs:288` with `PaneShell` quoting and the forbidden-env prefix (`paneshell.rs:41`) | exit 0 | NOT MENTIONED (design uses `agent start`, line 150) |
| `pane send-text`, `pane send-keys` | `herdr.rs:225`, `:230` | `messaging/delivery.rs:82`, `:94` | exit 0 | KEPT: doorbell only (line 151) |
| `agent prompt <pane> <text>` | `herdr.rs:498` | `messaging/delivery.rs:66` | fails when herdr sees no agent | KEPT (line 151) |
| `pane read --source` | `herdr.rs:245` | `delivery.rs:319`, `horch/src/cmd/compact.rs:1033` | screen text | KEPT (line 152) |
| `pane wait-output --match --source --timeout` | `herdr.rs:464` | `horch/src/cmd/smoke.rs:298` only | exit 0 match, timeout text (`herdr.rs:614`) | KEPT (line 152) |
| `pane close` | `herdr.rs:235` | `execution/lifecycle.rs:341`, `:1002` | exit 0 | NOT MENTIONED |
| `pane report-agent`, `release-agent` | `herdr.rs:510`, `:538` | report: no caller; release: `prime.rs:1048` | `--seq` order | CHANGED: wrapper reports (line 154) |
| `pane layout`, `focus`, `move`, `resize`, `tab list`, `tab focus` | `herdr.rs:249`-`:420` | `workspace/arrange.rs`, `tile.rs` | rects, ids | NOT MENTIONED |
| `workspace create --label --cwd --no-focus`, `workspace close` | `herdr.rs:424`, `:446` | `launch.rs:2261`, `lifecycle.rs:1349`, `sweep.rs:622` | `workspace`, `root_pane` | NOT MENTIONED |
| `notification show` | `herdr.rs:452` | `horch/src/cmd/mail.rs:475` | exit 0 | NOT MENTIONED |
| `integration status` | `herdr.rs:486` | `horch/src/cmd/recipes.rs:951` | text | NOT MENTIONED |
| `HERDR_PANE_ID`, `HERDR_SESSION`, `HERDR_SOCKET_PATH` env | `runtime/context.rs:293`, `:373` | pane identity; socket opened in the Claude sandbox (`claude.rs:854`) | herdr sets them in panes | CHANGED: `herdr --session fleet` (line 159) |
| `resume_agents_on_restore` config check | `horch/src/cmd/restore.rs:62` | `horch doctor`, recipes (`doctor.rs:28`, `recipes.rs:397`) | `[session]` key in herdr config | NOT MENTIONED |
| `herdr update --handoff` | `harness/update.rs:653`, `restore.rs:70` | operator only | keeps panes alive | NOT MENTIONED |
| Status values | `workspace/model.rs:54` | `idle` and `done` count as at the prompt | `agent_status` strings | KEPT (line 153) |

## 4. Login sources (step 5)

No credential is opened here. Names and paths only.

| What needs a login | Source at run time | Expires | How horch reports a sign-in error | Location |
| --- | --- | --- | --- | --- |
| `claude` panes, claude quota probe | operator's claude.ai subscription login in Claude's own store (`$CLAUDE_CONFIG_DIR` or home; on macOS the Keychain). horch sets no `CLAUDE_CONFIG_DIR` and strips `ANTHROPIC_API_KEY` | yes, refreshed by Claude | probe error "rate limits are not available for this login" in harness health (`routing/quota.rs:805`); stall note names "a login, trust or quota prompt" (`taskledger/watchdog.rs:992`) | `launch.rs:268`, `quota_probe.rs:97` |
| `codex` panes, codex quota probe | `$CODEX_HOME/auth.json` (ChatGPT login), reached through the private home's symlink | yes | probe error from `account/rateLimits/read` (`quota.rs:1227` test "not signed in") | `codex.rs:15`, `:325` |
| `agy` panes, google quota probe | operator's Google login in `~/.gemini/`; `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `GOOGLE_GEMINI_BASE_URL` removed | yes | `agy /usage` error, for example "You are not signed in" (`quota.rs:1660`) | `antigravity.rs:19`, `quota_probe.rs:202` |
| `opencode` panes | operator's OpenCode providers and credentials; `--pure` keeps them | provider dependent | `FreeUsageLimitError` cooldown only; no sign-in check | `opencode.rs:203` |
| `prime` panes | operator's Prime `auth.json`, linked into the private agent dir; refreshed through the link; legacy `apiKeys` dropped from the copied settings | yes | none found | `prime.rs:33`, `:788`, `:907` |
| `pi` panes | local Ollama, no login | no | `local` pool probe fails (`quota_probe.rs:282`) | `pi.rs:1` |
| git fetch and push | operator's SSH key and agent (`git-alt` host here); horch adds `ConnectTimeout` and keepalive, never a key | no | git stderr; stall bound kills the group (`vcs/network.rs:1`, `:28`) | `vcs/network.rs:155` |
| `gh` (pr step) | `gh auth login` store | yes | "install GitHub CLI and run gh auth login" (`taskledger/finish/pr.rs:30`) | `pr.rs:22` |
| `horch self-update` | unauthenticated GitHub API | no | HTTP error text | `horch/src/cmd/selfupdate.rs:28` |
| npm registry (cooldown dates) | unauthenticated | no | CLI held back | `harness/cooldown.rs:9` |

Design: claude login KEPT (lines 190, 530). Codex and agy logins are `NOT MENTIONED` beyond "login (to confirm in conformance)" (line 530) and open question line 848. The rest are `NOT MENTIONED`.

## 5. `ANTHROPIC_API_KEY` removal (step 6)

| Site | What it does | Location | Design |
| --- | --- | --- | --- |
| `FORBIDDEN_ENV` | the 1 list: `["ANTHROPIC_API_KEY"]` | `harness/launch.rs:271` | KEPT (line 190) |
| `scrub_child_env` | removes `FORBIDDEN_ENV` and git repo variables from a child | `runtime/process.rs:190` | NOT MENTIONED |
| `Harness::build_command` | every adapter command is scrubbed; no adapter overrides it | `harness/mod.rs:224` | KEPT (line 190) |
| launch flow | scrub again after window and env layers | `harness/launch.rs:526`, `:914` | KEPT (line 190) |
| `inherit_env` | a forbidden key never passes, also from teammate `env:` | `runtime/process.rs:200` | NOT MENTIONED |
| pane shell line | Posix `exec /usr/bin/env -u ANTHROPIC_API_KEY ...`; PowerShell `Remove-Item Env:ANTHROPIC_API_KEY`; a shell that stays `unset`s it first (U-68) | `workspace/paneshell.rs:11`, `:52` | NOT MENTIONED |
| detached children | judge, compaction job, grid tidy | `runtime/process.rs:277` | NOT MENTIONED |
| updaters, herdr update | scrubbed | `harness/update.rs:507`, `:657` | NOT MENTIONED |
| quota probes | claude probe removes it by hand; others through `run_with_deadline` | `routing/quota_probe.rs:110`, `:249` | NOT MENTIONED |
| finish steps, triage, runs | land, pr, complexity, triage apply, run commands | `taskledger/finish/land.rs:98`, `pr.rs:24`, `complexity.rs:298`, `triage/apply.rs:95`, `taskledger/runs.rs:886` | NOT MENTIONED |
| roster check | a teammate `env:` with the key or any `ANTHROPIC_*` fails | `roster/validation.rs:481`, `:1681` | NOT MENTIONED |
| doctor docker probe | removed by hand | `horch/src/cmd/doctor.rs:1980` | NOT MENTIONED |
| herdr restore warning | warns that a herdr-resumed agent could bill the key | `horch/src/cmd/restore.rs:62` | NOT MENTIONED |
| guard tests | 1 scrub place (`arc_88`); every line that names the key removes it (`ctx_27`); e2e for pane line, agy, updaters | `horch-core/tests/arch_scan.rs:711`, `horch/tests/context_cli.rs:453`, `horch-e2e/tests/lifecycle.rs:250`, `update.rs:416` | NOT MENTIONED |

`ANTHROPIC_BASE_URL` is read only as a host name for context policy (`runtime/context.rs:279`). It is not removed.

## 6. Platform code (step 7)

Ticket grep: 518 lines in 103 files. Production sites: 178 (the rest are tests and e2e). Extra forms the grep misses: 13 `cfg(any(...))` and 35 `cfg!(...)` production sites, counted in the "other" row where they are not in another row. The split per concern is a manual grouping of the 178 sites; each count can be off by 1 or 2. 3 of the 178 sites are test helpers outside a `mod tests` block.

| Concern | Sites | Files | macOS | Linux | Windows | Gaps |
| --- | --- | --- | --- | --- | --- | --- |
| Process identity and liveness | 16 | `procid.rs` (12), `fsx.rs:324`-`:337` (3), `horch/src/cmd/spawn.rs:421` | start time from `proc_pidinfo`; `kill(pid,0)`; parent pid via `proc_pidinfo` | `/proc/<pid>/stat` field 22, btime from `/proc/stat`; parent pid from `/proc` | `OpenProcess` + `GetProcessTimes`; `windows_running` | Windows: no parent pid (`spawn.rs:451`); others: no start time |
| Signals and stop | 14 | `procid.rs:118`-`:127`, `runtime/process.rs:62`-`:81`, `taskledger/watchdog.rs:673`, `telemetry/lock.rs:65`, `taskledger/finish/land.rs:183`, `horch/src/cmd/taskledger_watch.rs:30`, `telemetry/mod.rs:46`, `prime.rs:495`, `:642` | SIGTERM/SIGKILL after start-time match; SIGINT/TERM/HUP handlers | same, plus `pidfd_send_signal` | `TerminateProcess` exit 1, no polite stop; watchdog stop and collector stop not implemented (`telemetry/lock.rs:96`); no signal handlers | Windows: no graceful stop, no stop of stale watchdog or collector |
| Process groups | 16 | `workspace/herdr.rs` (12), `runtime/process.rs`, `harness/update.rs:525`, `vcs/network.rs:155`, `land.rs:99`, `horch/src/cmd/taskledger_finish.rs:168`, `procid.rs:65` | `process_group(0)`, `killpg` | same; group members from `/proc` | job object for herdr calls (`herdr.rs:676`); else child only (`process.rs:81`) | Windows: updater, git network bound and checks kill only the direct child |
| File locks | 6 | `fsx.rs:612`-`:725` | `DirLock` by `renamex_np(RENAME_EXCL)`; `flock` breaker guard | `renameat2(RENAME_NOREPLACE)`; `flock` | plain `rename`; breaker guard holds nothing (`fsx.rs:713`) | Windows: lock break race not guarded |
| File permissions | 28 | `fsx.rs:101`-`:139`, `messaging/private.rs` (8), `compaction/jobfile.rs:231`, `update.rs:425`, `trust.rs:832`, `pi.rs:61`, `runtime/process.rs:105`-`:128`, `skills/stable.rs:81`, `materialize.rs:157`, `horch-marketplace/src/source.rs:53`, `horch/src/cmd/compact.rs:649`, `telemetry/store.rs:163`, `place.rs:99`, `selfupdate.rs:376` | 0600/0700 modes; owner check refuses foreign dirs; exec bit | same | modes ignored; owner and mode checks skipped (`private.rs:108`); executable = exists | Windows: no private-file or foreign-owner protection |
| Paths and home dirs | 9 | `taskledger/store.rs:84`, `runtime/paths.rs:50`, `:148`, `runtime/bins.rs:215`, `fsx.rs:346`, `herdr-install/src/main.rs:354` | passwd home; `HOME`; Claude managed settings path | passwd home; `HOME` | `USERPROFILE`; no passwd home, so no ledger counts as shared (`store.rs:112`); host name empty (`fsx.rs:358`) | Windows: shared-ledger guard off; empty host name in lock owner |
| Symlinks | 11 | `harness/codex.rs:156`-`:365`, `prime.rs:854`, `claude_plugins.rs:533`, `herdr-install/src/win.rs` (8) | symlinks | symlinks | no private `CODEX_HOME`, codex phase skills refused (`codex.rs:223`); Prime link variant (`prime.rs:859`); plugin symlinks not copied; junctions in installer | Windows: codex skills and rule isolation not implemented |
| Shell and quoting | 4 | `workspace/paneshell.rs:33`, `horch/src/cmd/taskledger.rs:2518`, `recipes.rs:1654` | Posix `exec /usr/bin/env -u` | same | PowerShell `Remove-Item Env:`, `&` call | none found |
| Sockets and pipes | 45 | `messaging/channel.rs` (24), `messaging/report.rs` (16), `execution/lifecycle.rs:1028`, `prime.rs:349`, `fsx.rs:186` | Unix sockets for relay and report; `O_NONBLOCK` on FIFO | same | relay send fails, report socket refused (`lifecycle.rs:1053`) | Windows: worker channel and `horch_access: report` not implemented |
| Service or daemon start | 2 | `runtime/process.rs:280` (`setsid` detach), `prime.rs:642` (survive SIGHUP) | `setsid` so a pane close does not kill it | same | no detach flags | Windows: detached children die with the pane; no launchd/systemd/service code on any OS |
| Other | 27 | `runtime/machine.rs` (5), `selfupdate.rs:577` (tar vs zip), `build.rs:112`, `telemetry/cursor.rs:84` (file identity), `herdr-install/src/win.rs:165` (user PATH), `install.rs` (7 `cfg!`), `project_notes.rs:99`, `claude.rs:1082` (sandbox host) | `sysctl`, `vm_stat`, `statvfs`, `getloadavg`; `sandbox-exec` | `/proc/meminfo`, `statvfs`; `bwrap` + `socat` | all probes unknown (`machine.rs:322`); file identity by index; Claude sandbox refused | Windows: no disk, memory or load reading |

Process start is `std::process::Command` everywhere; Windows `PATHEXT` lookup is `runtime/process.rs:15`.

## 7. Design assumptions that 1 OS does not have (step 8)

| Design term | Design line | macOS | Linux | Windows |
| --- | --- | --- | --- | --- |
| `systemd` user unit, `Restart=always` | 13, 47, 203, 211, 215, 793, 823 | not present; launchd (`launchctl`, `KeepAlive`) | present | not present; a service or Task Scheduler. horch has no supervisor code on any OS |
| `WatchdogSec=30`, `systemd_notify("WATCHDOG=1")` | 203, 707, 794, 823 | launchd has no watchdog ping; a hung process is not restarted | present (`sd_notify`) | not present |
| `launchd` | 13, 47, 823, 842 | present | not present | not present |
| `/proc/<pid>/stat` | 663 | not present; horch uses `proc_pidinfo` (`procid.rs:185`) | present (`procid.rs:233`) | not present; horch uses `GetProcessTimes` (`procid.rs:340`) |
| `kill -0 pid` | 663 | present | present | not present; horch uses `OpenProcess` (`fsx.rs:331`) |
| `inotify` | 117, 689 | not present; FSEvents or kqueue | present | not present; `ReadDirectoryChangesW`. Design treats events as a speed-up only (line 689) |
| `bubblewrap` | 186 | not present; Seatbelt `sandbox-exec` (`claude.rs:756`) | present when installed (`claude.rs:1091`) | not present; Claude sandbox refused (`claude.rs:1102`) |
| `SIGINT` then `SIGKILL` to the process group | 217, 676 | present (`process.rs:62`) | present, plus pidfd (`procid.rs:127`) | not present; `TerminateProcess` only, job objects only for herdr calls (`herdr.rs:676`) |
| `process group` | 206, 217, 676 | present | present | not present; job object needed |
| Unix socket for herdr | 159 | present | present | herdr pipe name not checked here; horch relay and report sockets are Unix-only (`channel.rs:104`) |

## 8. Gaps and unknowns

- I could not determine the herdr transport on Windows. horch uses only the herdr CLI, so no horch code names it.
- `pane report-agent` has a wrapper (`herdr.rs:510`) and no caller in production code.
- The ticket grep pattern misses `cfg(any(...))` and `cfg!(...)`. D1's per-file counts (codex.rs 30, herdr.rs 29, channel.rs 23, fsx.rs 21) differ from this tip (30, 32, 25, 27) because those counts include tests and the tip moved.
