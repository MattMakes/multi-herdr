# Research 03: where context watching and compaction plug into horch

Brief: `ai_docs/plans/context-compaction/03-research-horch-integration.md`.
Revision examined: `a4dcee2` (main), working tree with untracked `ai_docs/` only.
herdr: 0.8.2 (`herdr --version`). Date: 2026-09-28. Author: researcher-6.

Scope: horch Rust code, teammate prose, bundled skills, and the herdr surface.
Harness internals (how to measure context, how each CLI compacts) belong to
briefs 01 and 02 and are not researched here. Facts from
`00-shared-context.md` "Findings from round 1" are reused, not re-verified.

Evidence labels: **[code]** = read from source at the cited line.
**[live]** = observed by a command run on this machine today.
**[doc]** = from `herdr-docs/*.md`. **UNVERIFIED** = not proven here.

## Short answer

- The orchestrator has no ledger record and no `HORCH_*` identity env vars
  except `HORCH_WORKSPACE_ID` and `HORCH_TEAMMATES_DIR`. But herdr already
  knows its session id: `herdr pane get <orchestrator pane>` returns
  `agent_session.value` for Claude and Codex panes, because the herdr
  claude/codex integrations are installed (v8). horch already parses that
  field (`Pane::agent_session_id`, `herdr.rs:48`). No launch change is needed
  to find the orchestrator's transcript.
- herdr classifies every pane as `idle | working | blocked | done | unknown`
  (`agent_status` in `pane get`/`agent list`). horch does not parse that
  field today. `herdr agent wait <pane> --until idle` exists.
- `horch tell <role> "<text>"` types the text RAW. It adds no `[role]`
  prefix. So `horch tell sonnet-1 "/compact ..."` already sends a slash
  command. The prefix is a prose convention only.
- `handoff` (the `whats-next` adaptation) is already in all 4 phase catalogs
  and reaches both fleet orchestrators. The gaps are prose (nobody is told to
  use it at a context threshold) and its fixed default output path.
- Per-teammate auto-compact thresholds need no Rust change for Claude:
  the teammate `env:` map reaches the CLI (`launch.rs:33-40`).
- Baseline: `CARGO_TARGET_DIR=/tmp/researcher-target cargo test --workspace`
  passes 340 of 340.

## Component map

```
horch fleet ─ recipes.rs:197 ──► herdr workspace create + pane run
                                   └─► horch pane-launch (recipes.rs:348)
                                         ├ Mailbox::register "orchestrator" (mailbox.rs:139)
                                         ├ prompts::agent_prompt (_base/fleet-orchestrator.md + orchestrator.md)
                                         ├ skills::Bundle::install (phase: plan + orchestrate [+ skill-creator])
                                         └ launch::command_with_skills(Session::Unmanaged)  ◄ no ledger record
horch spawn ─ spawn.rs:57 ──► Ledger::add_with_phase (session_id minted for claude/pi)
                             └─► brief.json ─► horch worker (worker.rs:29)
                                   ├ export HORCH_* (worker.rs:70-95)
                                   ├ launch::command_with_skills(Fresh|Resume|Unmanaged)
                                   └ start_harvest (codex/opencode/prime session id → ledger)
horch tell/assign ─ messaging.rs ──► Herdr::send_line = send-text, 1-2 s, Enter, 1 s, Enter
horch cost ─ cost.rs:89 ──► usage::read_session (cumulative tokens; not context size)
herdr agent_status / agent_session  ◄── herdr screen manifests + installed hooks
```

## 1. Orchestrator identity

**Launch path [code].** `pub fn fleet` at `crates/horch/src/cmd/recipes.rs:197-224`:
- `herdr.workspace_create(label, cwd, true)` at `recipes.rs:205`.
- `std::env::set_var("HORCH_WORKSPACE_ID")` and `("HORCH_PROJECT_DIR")` at
  `recipes.rs:206-207`. These set vars in the `horch fleet` process only.
  A herdr pane does not inherit that process's environment
  (comment at `recipes.rs:176-177`), so they do not reach the pane.
- `herdr.pane_run(root_pane, pane_command("orchestrator", kind, model))` at
  `recipes.rs:211-214`. `pane_command` (`recipes.rs:163-185`) builds
  `horch pane-launch --role orchestrator --kind <kind> [--model M] [--teammates-dir D]`,
  quoted by `PaneShell::host().command_line` (`paneshell.rs:34`).

`pub fn pane_launch` at `recipes.rs:348-441`:
- `Mailbox::register(&herdr, role)` at `recipes.rs:355`. This writes
  `$TMPDIR/herdr-orchestration-<ws>/orchestrator.id` and sets
  `HORCH_WORKSPACE_ID` in the process (`mailbox.rs:139-156`).
- Sets `HORCH_TEAMMATES_DIR` from `--teammates-dir` (`recipes.rs:359-361`).
- Reads `HORCH_PROJECT_DIR` (`recipes.rs:362-366`), but nothing sets it in
  the pane, so this branch does not fire for a fleet orchestrator.
- Teammate file: `orchestrator` or `orchestrator-codex` (`recipes.rs:375-381`).
- `launch::apply_env(&teammate)` at `recipes.rs:407`: exports the teammate `env:` map.
- `launch::command_with_skills(&teammate, Session::Unmanaged, ...)` at
  `recipes.rs:422-428`. `Unmanaged` means Claude gets no `--session-id`
  (`launch.rs:381-389`) and Codex gets no resume id (`launch.rs:540`).

**Env vars the orchestrator agent actually has [live].** `ps eww` on the
child of every `horch pane-launch --role orchestrator` process on this
machine (13 panes, Claude and Codex) shows exactly these horch/herdr names:
`HERDR_BIN_PATH`, `HERDR_ENV`, `HERDR_PANE_ID`, `HERDR_SOCKET_PATH`,
`HERDR_TAB_ID`, `HERDR_WORKSPACE_ID`, `HORCH_TEAMMATES_DIR`, `HORCH_WORKSPACE_ID`.
There is no `HORCH_ROLE`, `HORCH_RECORD_ID`, `HORCH_SESSION_ID`, or
`HORCH_PROJECT_DIR`. Consequence: `horch note` and `horch done` fail in the
orchestrator pane (`messaging.rs:69-72`, `messaging.rs:82-84` require
`HORCH_RECORD_ID`).

**How horch can learn the orchestrator's session id and transcript.**

Option A - herdr `agent_session`, no launch change [live]:
- `herdr pane get w2E:p1` (this fleet's orchestrator, per
  `$TMPDIR/herdr-orchestration-w2E/orchestrator.id`) returns
  `"agent_session":{"agent":"claude","kind":"id","source":"herdr:claude","value":"0cdde350-3f25-46a9-a776-6f37174a1c45"}`.
- The transcript exists at
  `~/.claude/projects/-Users-mascott-projects-multi-herdr/0cdde350-3f25-46a9-a776-6f37174a1c45.jsonl`.
- Codex orchestrators report the same shape: `herdr agent list` shows
  `w24:p1 codex ... agent_session 01a0ccf8-869c-7b02-b7ac-934f6a0d4864`.
- The source is the herdr hooks: `herdr integration status` prints
  `claude: current (v8)` and `codex: current (v8)`. pi and opencode are
  `not installed`.
- horch already reads it: `Pane::agent_session_id()` at
  `crates/horch-core/src/herdr.rs:48-60`, used by the codex harvest at
  `crates/horch/src/cmd/worker.rs:240-243`.
- Lookup chain: `Mailbox::pane_for("orchestrator")` (`mailbox.rs:159-163`)
  → `Herdr::pane_get` (`herdr.rs:348`) → `agent_session_id()` →
  `usage::find_claude_transcript` (`usage.rs:197`) or
  `usage::find_codex_rollout` (`usage.rs:210`).
- Gotcha: the Claude hook lives in the operator's `settings.json`
  (`herdr-docs/integrations.md:137`). A teammate that sets
  `setting_sources` without `user` drops that hook, and herdr then has no
  `agent_session` for that pane. The orchestrator keeps all settings
  (comment `teammates/orchestrator.md:23-27`), so it is not affected today.
- Gotcha: the Claude orchestrator's session id changes after `/clear`, and
  herdr reports a new one on the next SessionStart
  (UNVERIFIED for `/compact`; brief 02 owns it).

Option B - ledger record, as in the DRAFT design
(`ai_docs/designs/telemetry-and-balancing.md:34-45`): `horch fleet` writes a
record, Claude gets a minted `--session-id` (change `Session::Unmanaged` at
`recipes.rs:424` to `Session::Fresh(&id)`), Codex reuses `start_harvest`
(`worker.rs:209-298`). This also makes `horch note`/`horch done` work for the
orchestrator if `pane_launch` exports `HORCH_RECORD_ID` and `HORCH_ROLE`.
This is a larger change and overlaps the DRAFT design's unit 1
(`telemetry-and-balancing.md:216`). Option A needs no ledger change.

Already present: `horch cost --session claude:<id>` prices a session outside
the ledger, "e.g. the orchestrator's" (`crates/horch/src/main.rs:139-142`,
`crates/horch/src/cmd/cost.rs:102-120`).

## 2. Worker identity

**Ledger record [code].** `struct Record` at `crates/horch-core/src/ledger.rs:35-58`:
`record_id`, `session_id: Option<String>` ("`null` until known: codex only
reveals its session id after launch", `ledger.rs:37`), `agent`, `tier`
(teammate name), `model`, `effort`, `phase`, `role`, `status`, `task`,
`history`, `created_at`, `updated_at`. A record matches by `record_id` or
`session_id` (`ledger.rs:65-67`). There is no field for context size or a
compaction event today.

**Env vars [code, live].** `export_brief` at `crates/horch/src/cmd/worker.rs:70-95` sets
`HORCH_ROLE`, `HORCH_TEAMMATE`, `HORCH_AGENT`, `HORCH_MODEL`,
`HORCH_RECORD_ID`, `HORCH_SESSION_ID`, `HORCH_RESUME`, `HORCH_TASK`,
`HORCH_PROJECT_DIR`, and conditionally `HORCH_STATE_DIR`,
`HORCH_CLAUDE_BIN`, `HORCH_CODEX_BIN`, `HORCH_TEAMMATES_DIR`.
`HORCH_WORKSPACE_ID` comes from `Mailbox::register` (`mailbox.rs:154`).
This pane's env confirms all of these names [live].

**Where the session id is filled, per harness:**

| Harness | Mints id? | Where | Ledger value |
|---|---|---|---|
| Claude | yes | `spawn.rs:120-124` mints; `launch.rs:382-384` passes `--session-id` | UUID at spawn |
| pi | yes | same mint; `launch.rs:243-245` passes `--session-id` | UUID at spawn |
| Codex | no | `worker.rs:168-176` starts `start_harvest`; herdr `agent_session` first (`worker.rs:240-243`), else `codex::find_rollouts` (`worker.rs:263-265`) | UUID after launch, via `Ledger::set_session` (`ledger.rs:296`) |
| OpenCode | no | harvest, `opencode::find_sessions` (`worker.rs:249-252`) | `ses_...` id |
| Prime | no | harvest, `prime::find_session(daemon.sessions_dir())` (`worker.rs:254-260`) | session FILE PATH |

Flags: `Agent::mints_session_id` = Claude, pi (`teammates.rs:231-233`);
`Agent::harvests_session_id` = Codex, OpenCode, Prime (`teammates.rs:237-239`).
The harvest polls every 3 s for 60 tries (`worker.rs:234-236`), so a session
id can stay `null` for up to 180 s, or forever if the harvest fails
(logged at `worker.rs:285-296`).

Per-role lookup for a context watcher: mailbox role → pane
(`mailbox.rs:159`); ledger role → latest working record → `session_id`
(the same selection `Ledger::assign` uses, `ledger.rs:371-393`).

## 3. Idle detection

**What herdr offers [doc, live].**
- Every pane with a detected agent has `agent_status`: `idle`, `working`,
  `blocked`, `done`, or `unknown` (`herdr-docs/cli-reference.md:256`,
  `:291`). `done` is "the same underlying idle state after unseen background
  work completes" (`cli-reference.md:291`). So `idle` and `done` both mean
  "between turns".
- `herdr agent list`, `agent get`, `pane get`, `pane list` return it
  [live: `pane get w2E:p1` → `"agent_status":"done"`].
- `herdr agent wait <target> [--until STATUS]... [--timeout MS]` returns
  when the status matches; default set is idle, done, blocked
  (`cli-reference.md:289`, `socket-api.md:643-644`).
- `pane.agent_status_changed` socket event (`socket-api.md:611`, `:619`)
  gives push notification. horch has no socket client; it shells out to the
  CLI (`herdr.rs:306-328`).

**Which harnesses herdr classifies [doc, live].**
`herdr-docs/agents.md:11-31` state-authority table and
`herdr-docs/integrations.md:57-58`:

| Harness | herdr state authority | On this machine |
|---|---|---|
| Claude Code | screen manifest; hook gives session only | classified [live: 19 claude panes idle/working/done] |
| Codex | screen manifest; hook gives session only | classified [live: 3 codex panes idle] |
| pi | lifecycle hooks when installed, else screen manifest | hook `not installed`; screen manifest applies (UNVERIFIED live) |
| OpenCode | lifecycle plugin when installed, else screen manifest | plugin `not installed`; screen manifest applies (UNVERIFIED live) |
| Prime Agent | not in herdr's agent table; Prime has a built-in herdr reporter (`integrations.md:87`) | UNVERIFIED live |

Limits: `blocked` is strict; an unknown prompt shape shows as `idle`
(`agents.md` "Blocked state"). `unknown` means herdr cannot classify
(`cli-reference.md:291`).

**What horch parses today [code].** `struct Pane` at
`crates/horch-core/src/herdr.rs:27-41` has `pane_id`, `workspace_id`,
`tab_id`, `agent_session`. It has no `agent_status` and no `agent` field.
There is no `agent wait` wrapper. `Herdr::wait_output` (`herdr.rs:653-661`)
waits on screen text, not on status. Adding
`#[serde(default)] pub agent_status: Option<String>` to `Pane` is safe:
unknown fields are already ignored (test `herdr.rs:841`).

**Idle is not a stopping point by itself.** A worker that waits for an
orchestrator answer is `idle` mid-task. A natural stopping point needs a
second signal. Candidates in existing state: the latest ledger `history`
event is `note` or `done` (`ledger.rs:395-416`), or the worker's last
`[role]` line was `DONE:`/`NOTE:`. Which signal to use is a design decision
for brief 04.

## 4. Injection

**`horch tell` [code].** `messaging::tell` at `crates/horch/src/cmd/messaging.rs:18-36`
resolves the mailbox, finds the pane for the role, and calls
`herdr.send_line(&target, message)`. The message is the CLI trailing args
joined with spaces (`main.rs:56-63`, `main.rs:301`, `joined` at
`main.rs:282-284`). **No prefix is added.** The `[role]` prefix is written
by the sender because the prose tells it to (`teammates/_base/fleet-worker.md:46`).
The only code-built prefix is in `horch done`:
`format!("[{role}] DONE: {summary}")` at `messaging.rs:92`.

**`horch assign` [code].** `messaging::assign` at `messaging.rs:63-66`:
`Ledger::assign(role, task)` (records the task, `ledger.rs:371-393`), then
`tell(role, task)`. Same raw text. Note: `assign` on a `/compact` line would
overwrite the ledger `task` with `/compact ...`. Use `tell`, not `assign`.

**`Herdr::send_line` [code].** `crates/horch-core/src/herdr.rs:680-691`:
1. `herdr pane send-text <pane> <message>` (`herdr.rs:378-381`).
2. sleep 1 s, or 2 s when `message.len() > 1500`.
3. `herdr pane send-keys <pane> enter`.
4. sleep 1 s.
5. `herdr pane send-keys <pane> enter` again ("no-op-safe retry").

**Bracketed paste [doc, code].** `send-text` is the low-level, non-submitting
write; `pane run` and `agent prompt` honor live bracketed-paste mode
(`herdr-docs/cli-reference.md:201`, `:289`). horch deliberately avoids
`pane run` for TUIs (`herdr.rs:370-372`, `herdr.rs:672-677`). So `tell`
types the text as keystrokes, not as a bracketed paste. That is what a slash
command needs to be recognised as typed input.

`paneshell.rs` is not on the tell path. It only quotes the command line that
`pane run` submits to a fresh shell (`paneshell.rs:1-9`, `:34`).

**Can horch send a raw `/compact ...` line?** Yes, today, with no code change:
`horch tell <role> "/compact <instructions>"`. Risks (UNVERIFIED, brief 02
owns the harness side):
- The second Enter lands while compaction runs. It is harmless if the input
  box is empty, as the comment at `herdr.rs:676-677` assumes.
- A slash-command popup (Codex) can consume the first Enter as "select";
  the second Enter then submits. This is plausible, not tested.
- Text typed while the target is `working` goes into the input box. Whether
  each CLI queues or submits it is harness behavior.
- The orchestrator can target itself: `orchestrator` is a registered role
  (`recipes.rs:355`). It then types into its own pane during its own turn.

**Code change for a dedicated raw send.** Smallest: a new subcommand, for
example `horch compact <role> [instructions]`, in `main.rs` `enum Command`
(`main.rs:32`) and dispatch (`main.rs:299-418`), with the body in
`messaging.rs`. It would (1) resolve the pane as `tell` does, (2) optionally
check `agent_status` is idle/done (new `Pane.agent_status` field or a new
`Herdr::agent_wait` wrapper next to `wait_output` at `herdr.rs:653`),
(3) pick the per-harness command from the ledger record's `agent`
(`ledger.rs:39`), and (4) call `send_line`. Alternative primitive:
`herdr agent prompt <pane> <text> --wait` submits text and Enter
atomically and waits for settle (`cli-reference.md:289`); horch has no
wrapper for it. Which primitive works per harness is a brief 02 question.

## 5. Command surface

Clap definitions: `enum Command` at `crates/horch/src/main.rs:32-280`;
dispatch at `main.rs:299-418`; modules listed in `crates/horch/src/cmd/mod.rs:1-13`.

(a) Context report - natural homes:

| Candidate | Definition | Fit |
|---|---|---|
| New `horch context [--json]` | new variant in `enum Command` (`main.rs:32`), new `cmd/context.rs`, add to `cmd/mod.rs` | Best fit. Live per-pane view: role, pane, agent, `agent_status`, current context tokens, threshold. Includes the orchestrator via Option A in section 1. |
| `horch sessions` | `Sessions { json }` at `main.rs:87-92` → `ledgercmd::sessions` (`ledgercmd.rs:68`) → `Ledger::render` (`ledger.rs:447`) | Ledger view of all history. Adding a live column means transcript reads on every call; it has no orchestrator row. |
| `horch cost` | `Cost {..}` at `main.rs:119-143` → `cost::cost` (`cost.rs:89`), rows built at `cost.rs:176-260` via `usage::read_session` (`usage.rs:503`) | Cumulative spend, not current context. Already accepts `--session claude:<id>` for the orchestrator. Could gain a `context` column once `usage.rs` exposes a current-context reader. |

The reader belongs in `crates/horch-core/src/usage.rs` next to
`read_claude` (`usage.rs:260`), `read_codex` (`usage.rs:368`),
`read_pi` (`usage.rs:420`), reusing the locators at `usage.rs:197-218`.
OpenCode has no reader (`00-shared-context.md`).

(b) Compact action: new `horch compact <role> [instructions]` variant in
`enum Command` (`main.rs:32`), body in `crates/horch/src/cmd/messaging.rs`
beside `tell` (`messaging.rs:18`). No existing subcommand fits; `tell` is the
primitive it wraps. `horch ledger` subcommands (`ledgercmd.rs:17-60`) could
gain a `Compacted { key }` event if the ledger should record compactions
(via `Ledger::append_event`, `ledger.rs:417`).

## 6. Prose surface

**Orchestrator loop.** `teammates/_base/fleet-orchestrator.md` (132 lines),
shared by both flavors (`orchestrator.md:5`, `orchestrator-codex.md:5`):
- `== Channel ==` at lines 18-28 (lists `assign`, `tell`, `inbox`).
- `== Core loop ==` at lines 66-79. A "check context, compact at a
  stopping point" step fits here, for example between step 7 (verify DONE)
  and step 8.
- `== Worker lifecycle ==` at lines 103-112.
- `== Protect your context ==` at lines 114-130. The orchestrator's own
  self-compaction rule fits here.
- Flavor persona blocks: `teammates/orchestrator.md:43-66`,
  `teammates/orchestrator-codex.md` body. Harness-specific compact commands
  could go in each persona, but a single table in the shared base is simpler.
- The playbook skill `skills/orchestrate/SKILL.md` (orchestrator-only) is
  another home for the detailed procedure; the base prose then only points to it.

**Worker lifecycle.** `teammates/_base/fleet-worker.md` (92 lines):
- `Communication and lifecycle` list at lines 44-57. A rule such as "when
  the orchestrator asks, write a handoff with the handoff skill, report it,
  then wait" fits here.
- `== Scope ==` at lines 59-68.
- Task blocks `task_fresh`/`task_resume`/`task_idle` in frontmatter
  lines 24-35.

The fixed `horch orchestration` recipe uses `teammates/orchestration-orchestrator.md`
and `teammates/orchestration-worker.md` (no base). Its goldens must not change
(`the_orchestration_recipe_briefings_are_unchanged`, `golden_prompts.rs:163-180`).
Leave that recipe out of scope or accept a golden update there.

**Tests that fail when this prose changes** (`crates/horch-core/tests/golden_prompts.rs`).
Goldens are never regenerated (`golden_prompts.rs:26-30`). A change adds a
named "sanctioned" block to the test, not a new golden file:
- `every_worker_briefing_differs_only_where_sanctioned` (`golden_prompts.rs:88-160`)
  compares against `tests/golden/worker-{sonnet,opus,codex-sol,codex-terra,smoke}-{task,idle,resume}.txt`
  (15 files). Any `fleet-worker.md` edit needs a new sanctioned replacement here.
- `the_orchestrator_briefing_differs_only_where_sanctioned` (`golden_prompts.rs:183`)
  compares against `tests/golden/fleet-orchestrator.txt`. Any
  `fleet-orchestrator.md` or `orchestrator.md` body edit needs a new block here.
- `the_codex_orchestrator_differs_from_the_claude_one_only_in_its_tier_block`
  (`golden_prompts.rs:370`). An edit to the shared base passes; an edit to
  only one persona fails.
- `execpolicy_blocks_are_unchanged` (`golden_prompts.rs:352`) with
  `tests/golden/execpolicy-*.txt` (3 files). This changes only if new
  commands (for example `horch compact`, `horch context`) are added to the
  Codex execpolicy in `teammates/_base/codex-execpolicy.md` or
  `codex-orchestrator-execpolicy.md`. The Codex orchestrator cannot run a
  command its execpolicy does not name (`recipes.rs:409-417`), so a new
  `horch` subcommand the orchestrator must run needs a rule there.
- Unit tests in `crates/horch-core/src/prompts.rs:177-320`, for example
  `briefings_use_horch_subcommands` (`prompts.rs:177`) and
  `codex_rules_match_the_commands_workers_are_told_to_run` (`prompts.rs:210`).

## 7. Skill surface

**How `handoff` reaches a pane [code].**
- `phase_skills` at `crates/horch-core/src/skills.rs:23-37` lists `handoff`
  in all 4 phases: research (`:25`), plan (`:26`), implementation (`:27`),
  validation (`:28-35`). `docs/phase-skills.md` table agrees.
- `selected(teammate)` (`skills.rs:75-97`) = teammate `skills` ∪
  `phase_skills(phase)`. A teammate with `phase: null` and no `handoff` in
  `skills` gets no handoff.
- `Bundle::install` (`skills.rs:140`) writes the selected folders per launch;
  `briefing` (`skills.rs:257-325`) names teammate `skills` as "expected" and
  the rest of the phase as "Also available". This pane's briefing shows
  `handoff` under "Also available" [live].
- Skill files are compiled in by `crates/horch-core/build.rs` (skills dir at
  `build.rs:48`; `BUNDLED_SKILL_FILES` read at `skills.rs:41`).
  Editing `skills/handoff/SKILL.md` changes the binary; `skills/provenance.json`
  records the upstream hash (`source_sha256`), not the local file hash, so
  no provenance test fails on a local edit (no test reads `provenance.json`:
  `grep -rn provenance crates/` returns nothing).

**Who has it today [code].**
- Both fleet orchestrators: `phase: plan` (`orchestrator.md:7`,
  `orchestrator-codex.md:7`). Pinned by
  `skills_orchestrate_is_attached_by_name_to_the_orchestrator_only`
  (`skills.rs:557-571`): `["create-plan","handoff","orchestrate","pre-flight","skill-creator"]`.
- Every roster worker has a phase (`grep ^phase: teammates/*.md`), so every
  worker has it. Exceptions: `smoke` (`agent: none`, no phase) and
  `_template.md` (`phase: null`, line 85).
- The fixed-recipe `orchestration-orchestrator` (`phase: plan`) and
  `orchestration-worker` (`phase: implementation`) also have it.

**What change makes it available everywhere.** For the phase catalogs,
nothing: it is already in every phase. To also cover `phase: null` custom
teammates, add it unconditionally in `selected` (`skills.rs:75-80`); that
changes the expected lists in the tests at `skills.rs:416-470` and
`skills.rs:557-571`. To make it "expected" (named with its description)
instead of "also available", add `handoff` to `skills:` in each teammate
file, or special-case it in `Bundle::briefing` (`skills.rs:267-296`).

**Gotcha: fixed output path.** `skills/handoff/SKILL.md` step 7 writes
"the assigned handoff path or `ai_docs/handoffs/whats-next.md`". Workers
share one checkout (memory `fleet-workers-share-one-checkout`). Two workers
compacting at once would both write `ai_docs/handoffs/whats-next.md`. The
compact instruction should pass a per-role path, for example
`ai_docs/handoffs/<role>-whats-next.md`, or the skill default should change.

## 8. Launch-time backstop

Per-teammate knobs that reach the CLI today:

| Knob | Code | Reaches | Notes |
|---|---|---|---|
| `env:` map | field `teammates.rs:527`; exported by `launch::apply_env` (`launch.rs:33-40`); called in `worker.rs:110` and `recipes.rs:407` | every harness, orchestrator included | `ANTHROPIC_API_KEY` is refused by the roster check (`teammates.rs:920-921`) and removed from every command (`launch.rs:46`, `launch.rs:58-61`). |
| `args:` | field `teammates.rs:525`; appended per builder: claude `launch.rs:392`, codex `launch.rs:539`, pi/prime `launch.rs:255`, opencode `launch.rs:166` | every harness | codex `-c key=value` fits here. |
| `settings:` (Claude) | field `teammates.rs:510`; fleet panes merge it in `Bundle::claude_settings` (`skills.rs:194-230`), passed as `--settings` | Claude | A settings key such as `autoCompactWindow` would go here. Setting `settings:` replaces the teammate's JSON base, and horch still adds its switches on top (`skills.rs:210-229`). |
| OpenCode config overlay | `OPENCODE_CONFIG_CONTENT` built in `skills.rs:232-255` and `launch.rs:174-197` | OpenCode | Compaction keys, if any, would join this overlay (brief 02). |
| Codex private home | `codex::Rules::install` → private `CODEX_HOME` (`codex.rs:55-80`) | Codex | `config.toml` is linked from the operator's home (`codex.rs:185-224`); a per-teammate value is safer as `args: ["-c", ...]`. |
| Prime daemon | `prime::Daemon::install` (`prime.rs:39`); flags added at `worker.rs:143-153` | Prime | Its own session dir per pane. |

Per the shared context, Claude's lever is `CLAUDE_CODE_AUTO_COMPACT_WINDOW`
(env) or `autoCompactWindow` (settings). So a Claude teammate can set
`env: {CLAUDE_CODE_AUTO_COMPACT_WINDOW: "300000"}` with no Rust change.
The orchestrator gets its teammate `env:` too (`recipes.rs:407`).
The equivalent keys for Codex, pi, Prime and OpenCode are brief 02's output.
A dedicated field (for example `auto_compact_tokens:`) would need a new
`Teammate` field (`teammates.rs:422-570`), a per-agent mapping in each
builder in `launch.rs`, a roster check, and a `_template.md` entry.

## 9. Tests

Must stay green:
- `CARGO_TARGET_DIR=/tmp/researcher-target cargo test --workspace`.
  **Baseline 2026-09-28 at a4dcee2: 340 passed, 0 failed.**
  herdr-docs-sync 40, herdr-install 37, horch (bin) 51, horch-core (lib) 207,
  golden_prompts 5, doc-tests 0.
- `crates/horch-core/tests/golden_prompts.rs` (5 tests, part of the above).
- Unit tests that pin the areas above: `herdr.rs:693-851` (response parsing),
  `ledger.rs:490-828`, `launch.rs:550-1354` (argv per harness),
  `skills.rs:370-625` (catalogs), `prompts.rs:150-320`, `teammates.rs`
  roster checks, `cost.rs:405-488`, `usage.rs` tests.
- `HORCH_TEAMMATES_DIR=$PWD/teammates <built horch> teammates --check`.
  Baseline [live]: `roster ok: 25 teammates, 20 offered to the orchestrator`, exit 0.
- `horch smoke messaging | fleet | tile` (`crates/horch/src/cmd/smoke.rs`).
  These create panes; not run here, per the brief. `smoke messaging` is the
  relevant one if `send_line` or `tell` changes.

Gotcha (from shared context, confirmed): `cargo test` does not produce
`target/debug/horch`; run `cargo build -p horch` with the same
`CARGO_TARGET_DIR` to get a binary for `teammates --check`.

## Overlap with `ai_docs/designs/telemetry-and-balancing.md` (DRAFT)

- Section 3 "every pane is a ledger record, the orchestrator included"
  (`:34-45`) is Option B in section 1 here. Option A (herdr `agent_session`)
  gets the orchestrator's transcript without it.
- Section 4 collector (`:47-75`) moves transcript readers to an
  offset-aware API (`:58`). A current-context reader in `usage.rs` should
  fit that API, or it will be written twice.
- Section 5 metadata tokens (`:154`): herdr `pane.report_metadata`
  `--token ctx=312k` could show context per pane in the sidebar
  (`herdr-docs/cli-reference.md:245-262`).
- Section 7.4 (`:201-207`) already tells a tight orchestrator to write a
  handoff with `horch:handoff`. The same prose section in
  `fleet-orchestrator.md` is where both rules would live.

## Change map

| Surface | File | What changes | Risk |
|---|---|---|---|
| herdr pane status | `crates/horch-core/src/herdr.rs:27-41` | Add `agent_status: Option<String>` (and `agent`) to `Pane`; optional `agent_wait` wrapper near `wait_output` (`:653`) | Low. `serde(default)`; unknown fields already ignored. |
| Orchestrator identity | `crates/horch-core/src/herdr.rs:48`, `mailbox.rs:159` (reuse only) | Find orchestrator session via `pane_for("orchestrator")` → `pane_get` → `agent_session_id()` | Low. Depends on herdr claude/codex hooks installed; pi/opencode hooks are not installed. |
| Orchestrator ledger record (optional) | `crates/horch/src/cmd/recipes.rs:197-224, 348-441`; `ledger.rs` | Write a record, mint Claude `--session-id`, harvest Codex id, export `HORCH_RECORD_ID`/`HORCH_ROLE` | Medium. Overlaps DRAFT design unit 1; `sessions`/`spawn --resume` must treat the record specially. |
| Current-context reader | `crates/horch-core/src/usage.rs:197-519` | New per-harness "last context occupancy" readers beside `read_claude`/`read_codex`/`read_pi` | Medium. Semantics per harness from brief 01; whole-file reads are slow on large transcripts. |
| Context report command | `crates/horch/src/main.rs:32`, `cmd/mod.rs`, new `cmd/context.rs` | `horch context [--json]` listing role, pane, agent, status, context tokens, threshold | Low. New code only. |
| Compact action command | `crates/horch/src/main.rs:32`, `crates/horch/src/cmd/messaging.rs:18` | `horch compact <role> [instructions]`: idle check, per-agent command, `send_line` | Medium. Keystroke timing and slash popups per harness are UNVERIFIED (brief 02). |
| Codex execpolicy | `teammates/_base/codex-orchestrator-execpolicy.md`, `codex-execpolicy.md` | Allow `horch context` / `horch compact` for the Codex orchestrator | Low. Updates `execpolicy-*.txt` goldens and `prompts.rs:210` test. |
| Orchestrator prose | `teammates/_base/fleet-orchestrator.md:66-79, 114-130` (+ optional `skills/orchestrate/SKILL.md`) | Rule: check `horch context`; above 300,000 tokens, wait for a stopping point, request handoff, compact; same for itself | Medium. Needs a new sanctioned block in `golden_prompts.rs:183`. |
| Worker prose | `teammates/_base/fleet-worker.md:44-57` | Rule: on a compact request, write a handoff to a per-role path, report it, then wait | Medium. Needs a new sanctioned block in `golden_prompts.rs:88-160` (15 goldens). |
| Handoff skill | `skills/handoff/SKILL.md` step 7; optionally `skills.rs:75-97` | Per-role default path; optionally always select `handoff` | Low. Catalog tests at `skills.rs:416-470, 557-571` change if selection changes. |
| Launch backstop | teammate `env:` in `teammates/*.md` (no Rust); optional new field in `teammates.rs` + `launch.rs` builders | Set per-teammate auto-compact threshold (e.g. `CLAUDE_CODE_AUTO_COMPACT_WINDOW`) | Low for `env:`; medium for a new field (5 builders, roster check, template). |
| Ledger event (optional) | `crates/horch-core/src/ledger.rs:417`, `cmd/ledgercmd.rs:17-60` | Record a `compact` history event per session | Low. |

## Open questions

- Does herdr keep reporting the same `agent_session` value after `/compact`
  in Claude Code and Codex? (Brief 02.)
- Do pi, OpenCode and Prime panes get a reliable `agent_status` without the
  herdr integrations installed? Not observed live: no such pane runs now.
- Is `send_line` (send-text + 2 Enters) or `herdr agent prompt` the right
  primitive for a slash command on each harness? (Brief 02.)
