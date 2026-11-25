# Sandboxed pane horch access: what a steered pane can do

Review of ticket T-19 (run `skill-intake`). Date: 2026-11-05. Code at
commit `6e5c021b` (T-2, "Sandbox: open horch's state root, ledger folder and
sockets to sandboxed Claude panes").

## Scope and threat model

Threat: a skill-intake role worker (auditor or analyst) reads hostile skill
text. A prompt injection steers it. The steered role runs any command its
Bash allows, inside Claude Code's OS sandbox (Seatbelt on macOS, bwrap on
Linux).

T-2 (`overlay_horch_paths`, `crates/horch-core/src/harness/claude.rs:819`)
adds these entries to every Claude teammate that has a `sandbox:` block:

| sandbox key | entry | source |
|---|---|---|
| `filesystem.allowRead`, `filesystem.allowWrite` | state root (`~/.local/state/horch`) | `LaunchEnv::state_root` = `Paths::state_root` |
| `filesystem.allowRead`, `filesystem.allowWrite` | ledger folder (`~/.horch`) | `LaunchEnv::ledger_dir` = parent of `Paths::ledger_db` |
| `network.allowUnixSockets` | state root (subpath: covers `channels/<ws>.sock`) | `LaunchEnv::state_root` |
| `network.allowUnixSockets` | `$HERDR_SOCKET_PATH` (`~/.config/herdr/herdr.sock`) | `LaunchEnv::herdr_socket` |

The grant is the same for every sandboxed teammate. It is not limited to
skill-intake roles, to one workspace, to one run or to one project.

Method: static analysis of `crates/` and `herdr-docs/`, plus the T-2
re-probe evidence in `docs/live-checks/skill-intake.md`. No exploit ran
against a live pane. No new pane was started for this review.

Severity scale (for a steered contaminated role):

- Critical: the role leaves the OS sandbox, or makes unsandboxed code or an
  unsandboxed agent act for it, with no human step.
- High: the role changes what another agent reads or does, or breaks the
  clean-room wall, with no human step.
- Medium: the role reads data outside its side root, or blocks fleet work.
- Low: a narrow race or a nuisance.

## Process fact used by several risks

A pane process is a child of the herdr server, not of the process that asked
for the pane. The parent chain of this review's own shell is:
`zsh <- claude <- horch (worker) <- herdr (server) <- herdr (client) <- zsh <- login`.
So a pane that a sandboxed process asks herdr to create runs outside the
caller's Seatbelt profile. The `horch` worker process that is the parent of
`claude` is also outside the sandbox: Claude Code sandboxes only its Bash
tool and that tool's children.

---

## R1: Run any command outside the sandbox through the herdr socket

**Capability.** The herdr socket has no authentication and exposes the full
herdr API. A sandboxed pane that can connect to it can:

- create a pane and run a command in it (`pane.split`, then `herdr pane run
  <pane> <command>`; or `layout.apply` with an argv `command`). That process
  is a child of the herdr server, so it has no sandbox: full network, full
  home directory, `~/.ssh`, `~/.config/gh`.
- run `horch spawn <teammate> --exact "<task>"` for a teammate with no
  sandbox (for example `opus`), or with `HORCH_TEAMMATES_DIR=<a folder it
  wrote>` for a teammate file it wrote itself (any `permission_mode`, no
  `sandbox:`).
- persist: `plugin.link` registers a plugin whose startup hooks run at every
  herdr start (`plugins.json` next to `session.json`).

**Evidence.**

- `herdr-docs/socket-api.md:99` lists `pane.split`, `pane.send_text`,
  `pane.read`, `pane.close` and the plugin methods; `:211` says
  process-launching methods accept `env`; `:352-354` plugin startup hooks
  and the persistent registry; `:461-466` the transport is newline JSON on a
  Unix socket with no auth step.
- `herdr-docs/cli-reference.md:170` (`pane split ... --env`), `:196`
  (`pane run <pane_id> <command>`).
- `crates/horch-core/src/harness/claude.rs:834` (`let sockets =
  [&env.state_root, &env.herdr_socket];`) adds the socket.
- `docs/live-checks/skill-intake.md`, "Re-probe after ticket 02", round 2:
  `horch spawn smoke` from sandboxed pane `probe-importer-4` exited 0 and
  started pane `w29:pG`. That is the same herdr call path.
- `crates/horch-core/src/runtime/context.rs:374`: `HORCH_TEAMMATES_DIR` from
  the environment selects the roster.
- Spec section 6.1 H-1: `horch spawn` has no caller check.

**Severity.** Critical. Status: CONFIRMED (spawn from a sandboxed pane works;
the spawned process is outside the caller's sandbox by the process chain
above).

**Status: C1 done in commit `f5bfc2eb` (ticket T-20).** See "C1 to C3 as
built" below.

**Proposed change (C1).** Do not give a role pane the herdr socket or the
state-root socket subpath. Add a teammate key `horch_access: report`
(default stays `full`). For `report`, `overlay_horch_paths` grants only 1
socket: `<state_root>/report/<record_id>.sock`. The pane's own `horch`
worker process (the unsandboxed parent of `claude`) creates and serves that
socket. It accepts 3 verbs and nothing else:

- `progress <text>`: the parent records `horch ledger progress` for its own
  worker row;
- `tell <text>`: the parent sends the text to the brief's report target (the
  controller role) only, tagged with the brief's role;
- `done <text>`: the parent runs the `horch done` path for its own record.

The `horch` CLI uses that socket when `HORCH_REPORT_SOCKET` is set and
refuses every other subcommand in that mode. Identity comes from the socket
(1 per pane, made by the parent), never from the environment.

Why not the ticket's "herdr socket proxy that only allows the commands horch
tell needs": `horch tell` needs `pane.send_text` to an arbitrary pane id and
`horch done` needs `pane.close`. A proxy must then know which pane is the
allowed target and which is the caller, which is the brief. That is the
report socket with a larger surface (raw herdr JSON).

The controller (`skill-importer`) keeps `full`: it runs `horch spawn`. It
reads only `intake` output, never source text (spec 6.3). See C6.

**Files the change touches.** `crates/horch-core/src/harness/claude.rs`
(`overlay_horch_paths`), `crates/horch-core/src/harness/launch.rs`
(`LaunchEnv`: record id, report socket path), `crates/horch-core/src/roster/`
(the `horch_access` key on `Teammate`, parse and validation),
`crates/horch-core/src/execution/lifecycle.rs` (serve the socket for the
life of the agent), a new `crates/horch-core/src/messaging/report.rs`
(protocol), `crates/horch/src/cmd/messaging.rs` (`tell`, `done` client mode),
`crates/horch/src/cmd/taskledger.rs` (`progress` client mode),
`crates/horch-core/src/runtime/context.rs` (`HORCH_REPORT_SOCKET`),
`teammates/_template.md` (document the key), the 5 `teammates/intake-*.md`
files when they exist.

---

## R2: Type text into any pane, as the operator or as any role

**Capability.** Through the herdr socket the pane can type into every pane
of every workspace: the orchestrator, the skill-intake controller, a
clean-side writer or reviewer, a worker of another project, and an
operator's plain shell (where Enter runs a command outside any sandbox).

The `horch` binary makes the forgery easy:

- `env -u HORCH_ROLE horch tell orchestrator "<text>"` sends an untagged
  line. The code comment says an untagged line "reads as the human
  operator".
- `HORCH_ROLE=<any role> horch tell ...` sends a line tagged as that role.

**Evidence.**

- `herdr-docs/socket-api.md:99` (`pane.send_text`, `pane.send_keys`,
  `pane.send_input`), `herdr-docs/cli-reference.md:196-201` (`pane run`
  submits text plus Enter).
- `crates/horch/src/cmd/messaging.rs:43-50` (doc comment: untagged line
  reads as the operator), `:62` (sender is `ctx.worker.role`, from
  `HORCH_ROLE`), `:184` (`None => text.to_string()`: no tag).
- Spec 6.1 H-5: `horch tell <ROLE>` reaches any registered role and wakes an
  idle session.

**Severity.** High. Status: CONFIRMED by code. The orchestrator and the
operator's shells have no sandbox, so a typed instruction can become
unsandboxed action. It needs the target agent to obey the text; R1 does not.

**Proposed change.** C1 removes the socket. With C1 the only text path out
of a role pane is `tell` to the brief's report target, tagged by the parent.

**Files the change touches.** As C1.

---

## R3: Read the screen of any pane

**Capability.** `herdr pane read <pane> --source recent --lines N` and
`agent.read` return the visible text and scrollback of any pane: other
workers' live sessions, the orchestrator, other projects, operator shells.
This is a live view of other sessions' transcripts.

**Evidence.** `herdr-docs/socket-api.md:99` (`pane.read`, `agent.read`),
`:625-633` ("Reading panes"). `pane.list` and `session.snapshot`
(`:111`) give every pane id.

**Severity.** Medium alone (the role has no network, so it needs R1, R2 or
R6 to move the data out). For skill-intake it also lets a clean-side role
read a contaminated pane, but a clean role is not the steered party in this
threat model.

**Proposed change.** C1.

**Files the change touches.** As C1.

---

## R4: Edit the task ledger directly

**Capability.** `~/.horch` is writable, so the pane can open
`~/.horch/ledger.sqlite3` with `sqlite3` (on the PATH on macOS, and not on
the role deny list in spec 6.4) and change any row of any run of any
project:

- set `tasks.land_check` of another task. `horch ledger finish` of that
  task's worker runs it as `$SHELL -c <land_check>`. For a Claude worker
  that process is not sandboxed. This is code execution outside the sandbox.
- set `tasks.task_file` to a ticket the pane wrote. The next claimer is told
  to "read the ticket file ... and follow it exactly".
- set task, step and worktree status: mark finish steps passed or a task
  done, or make the watchdog reap or ignore a worker.
- write the `evidence/` folder that finish steps read.

**Evidence.**

- `crates/horch-core/src/taskledger/schema.rs:80` (`task_file TEXT`), `:92`
  (`land_check TEXT`), `:295-312` (`task_steps` with `status`,
  `evidence_path`, `verdict`).
- `crates/horch-core/src/taskledger/finish/land.rs:226-231`
  (`check_command` reads `cx.task.land_check`), `:236-252`
  (`check_process`: `Command::new(&cx.tools.shell)` then `-c <command>`; the
  Codex sandbox wraps it only when `sandboxed` is true, which is the Codex
  case).
- `crates/horch/src/cmd/taskledger.rs:2156` and
  `crates/horch/src/cmd/worktree.rs:218`: the evidence root is the ledger
  folder's `evidence/`.
- `crates/horch-core/src/harness/claude.rs:833` adds `env.ledger_dir` to
  `allowWrite`.

**Severity.** Critical. Status: CONFIRMED by code (`land_check` path).
Not reproduced against the live ledger.

**Status: C2 done in commit `f5bfc2eb` (ticket T-20).**

**Proposed change (C2).** For `horch_access: report`, do not add the ledger
folder to `allowRead` or `allowWrite`. `progress` and `done` go through the
report socket (C1); the unsandboxed parent writes the ledger.

Why not the ticket's "ledger writes only through the horch binary": inside
the sandbox the `horch` binary and `sqlite3` have the same file rights, so
the sandbox cannot let one write and stop the other. And the binary trusts
the environment for identity (R5).

**Files the change touches.** `crates/horch-core/src/harness/claude.rs`
(`overlay_horch_paths`). The rest is in C1.

---

## R5: horch commands trust the environment for identity

**Capability.** Even without raw file or socket access, the `horch` binary
lets the pane act as another worker, or as the orchestrator, by setting
environment variables:

- `HORCH_ROLE` sets the sender tag of `horch tell` (R2).
- `HORCH_WORKER_ID` and `HORCH_RECORD_ID` select the ledger row and session
  record that `horch ledger progress`, `horch ledger done` and `horch done`
  change.
- `horch ledger waive` refuses a non-orchestrator caller, but the caller is
  `HORCH_WORKER_ID`. Set it to the orchestrator's worker id (readable from
  the ledger) and the check passes.
- `horch ledger cancel`, `requeue`, `unblock`, `add` and `stop` have no
  caller check. `stop` signals the PIDs that a row records.

**Evidence.**

- `crates/horch-core/src/runtime/context.rs:330-338` (`role`, `record_id`,
  `worker_id` from `HORCH_ROLE`, `HORCH_RECORD_ID`, `HORCH_WORKER_ID`).
- `crates/horch/src/cmd/taskledger.rs:948-958` (`waive_cmd`: caller from
  `worker_id_opt(ctx)`, then a kind check on that row).
- `horch ledger --help`: `cancel`, `requeue`, `unblock` are labelled
  "(orchestrator)" in text only; no other caller check appears in
  `crates/horch/src/cmd/taskledger.rs` (search for "only the orchestrator"
  finds 1 hit, in `waive_cmd`).

**Severity.** High. Status: CONFIRMED by code. With C1 and C2 the role pane
has no ledger or herdr access, so the `horch` binary cannot reach the ledger
from it, and R5 leaves the skill-intake threat model. It stays true for
every unsandboxed worker.

**Proposed change (C4).** Follow-up, not needed for skill-intake: resolve
the caller from herdr's `pane.current` for the caller's pane, and check the
ledger row's recorded pane, instead of `HORCH_WORKER_ID`; gate the
orchestrator commands the same way.

**Files the change touches.** `crates/horch/src/cmd/taskledger.rs`
(`worker_id_opt`, `waive_cmd`, `cancel`, `requeue`, `unblock`, `stop`
handlers), `crates/horch/src/cmd/messaging.rs` (`tell_outcome`).

---

## R6: Change what other workers load from the state root

**Capability.** The state root is writable. These files there are read
later by an unsandboxed process or agent:

- `skill-bundles/<execution_id>/skills/<id>/SKILL.md`: the fleet copy of each
  worker's skills. A worker loads a skill body when its step comes up, so a
  change reaches a live worker. This review's own brief says to use the
  fleet copy under `~/.local/state/horch/skill-bundles/...`. Files are mode
  0644 and owned by the same user: only the sandbox stood in the way.
- `herdr-orchestration-<ws>/<role>.brief.json`: `claude_bin`, `codex_bin`,
  `bin_overrides`, `teammates_dir`, `task` and `report_to`. `horch worker`
  applies the binary overrides at launch and `horch restore` reads the brief
  to relaunch a pane. A changed `claude_bin` runs an attacker binary,
  without a sandbox, the next time that role starts, resumes or is restored.
- `herdr-orchestration-<ws>/<role>.id`: the pane id of a role. A changed
  `orchestrator.id` sends every worker's tell push to another pane.
- `claude/system-prompts/<name>-<hash>.md`: the launch rewrites the file
  when its bytes differ, then `claude` reads it. Only a write in that window
  has an effect.

**Evidence.**

- `crates/horch-core/src/skills/materialize.rs:31` and
  `crates/horch-core/src/skills.rs:132` (bundle root under the state root).
- `crates/horch-core/src/messaging/mailbox.rs:62` (registry dir
  `herdr-orchestration-<ws>` under the state root), `:218-232` (`.id` and
  `.brief.json` files), `:371` (`read_brief`; `private::check` tests mode
  and owner only).
- `crates/horch-core/src/execution/lifecycle.rs:850-876` (`load_brief`, then
  `self.ctx.apply_overrides(&brief.overrides())`).
- `crates/horch-core/src/messaging/brief.rs:108-118` (`overrides`: `claude_bin`
  and `codex_bin` fill the binary overrides).
- `crates/horch/src/cmd/restore.rs:131` (restore reads the brief).
- `crates/horch-core/src/harness/claude.rs:144-155` (`install_system_prompt`).

**Severity.** Critical (skill bundles and brief binaries). The `.id` file is
High. The system prompt file is Low (a race). Status: CONFIRMED by code.

**Status: C3 done in commit `f5bfc2eb` (ticket T-20).**

T-27 (`769dd9cf`): a sandboxed pane reads its own bundle (`additionalDirectories`, `allowRead`) and cannot write it (`deny` Edit/Write, `denyWrite`); a `full` pane can still write other bundles through the state root.

T-28 (`d22e110d`): a sandboxed `full` pane gets `<state>/skill-bundles` in `denyWrite` and `Edit`/`Write` deny rules, so it writes no bundle with any tool; its `horch spawn` skips the state sweep and writes no stamp.

T-35 (`0b69bb56`): a sandboxed `full` pane reads the roster folders `horch spawn` loads (`$HORCH_TEAMMATES_DIR`, `~/.config/horch/teammates`) through `allowRead` and `additionalDirectories`, and cannot write them (`denyWrite`, `Edit` deny).

**Proposed change (C3).** For `horch_access: report`, do not add the state
root to `allowRead` or `allowWrite`. With C1 the pane writes nothing there.

Why not the ticket's "per-pane mailbox path instead of the whole state
root": with C1 the parent enqueues the durable mail, so the pane needs no
mailbox path. A per-pane outbox dir would still need a reader that trusts
the dir as the sender, which is the same binding as the socket, with more
files to check.

Follow-up (C5): integrity for unsandboxed readers, which matters for Codex
panes (the state root is writable from a Codex pane, `paths.rs:91-94`,
#181): make bundle files read-only and check their catalog hash at load;
take binary overrides from the spawn command only, not from the brief file
on restore.

**Files the change touches.** C3: `crates/horch-core/src/harness/claude.rs`
(`overlay_horch_paths`). C5: `crates/horch-core/src/skills/materialize.rs`,
`crates/horch-core/src/execution/lifecycle.rs`,
`crates/horch/src/cmd/restore.rs`.

---

## R7: Forge, hide or delete mail and relay messages

**Capability.**

- Write a JSON file into `mail/<ws>/pending/` (orchestrator mail) or
  `mail/<ws>/roles/<role>/pending/` with any `from` and `text`. The courier
  and `horch inbox` deliver it as that sender.
- Move or delete pending mail: hide a role's `DONE` from the controller, or
  a `QUESTION:` from the orchestrator.
- Connect to `channels/<ws>.sock` and send `Message { from, text }`. The
  relay writes a `notifications/claude/channel` notification with
  `meta.role = from` into the orchestrator session.
- Write into `messages/<ws>/`, the spool that long tells point to.

**Evidence.**

- `crates/horch-core/src/messaging/mail.rs:34-60` (mail dirs under the state
  root), `:100-140` (`enqueue_with_questions`: a pending JSON file with
  `from` as given).
- `crates/horch-core/src/messaging/channel.rs:611-614` (`SocketRequest::Message
  { from, text }` delivered as is), `:700-712` (`deliver`: `meta.role` is
  `from`).
- `crates/horch-core/src/messaging/message.rs:52` (spool dir).
- `crates/horch-core/src/harness/claude.rs:834` (state root as socket
  subpath covers `channels/`).

**Severity.** High. Status: CONFIRMED by code.

**Proposed change.** C1 (no relay socket) and C3 (no mail or spool dir).

**Files the change touches.** As C1 and C3.

---

## R8: Break the skill-intake clean-room wall

**Capability.** Every sandboxed role, contaminated and clean, gets the same
read and write grant on the state root and ledger folder. So:

- a contaminated role (auditor, analyst) writes source text to a file under
  the state root, and a clean role (writer, reviewer) reads it;
- a contaminated role types source text into a clean role's pane (R2) or
  sends it in a `horch tell` that the clean pane receives;
- a contaminated role writes its brief or status text where the controller
  reads it outside `intake role-result`.

The domain snapshot (spec 6.10) hashes only the 3 side roots, so a write to
the state root or ledger folder is not an incident.

**Evidence.** Spec `ai_docs/specs/skill-intake/spec.md` 6.4 (per-role
`allowRead`/`allowWrite` lists are the wall), 6.10 (domain snapshot covers
side roots only, and "the wall holds without the guard hook" in fleet mode).
`crates/horch-core/src/harness/claude.rs:819-850` (the same 2 directories
for every sandboxed teammate).

**Severity.** High for skill-intake. The wall is the product's reason for
separate role workers. Status: CONFIRMED by design and code.

**Proposed change.** C1 and C3: a `report` pane shares no writable or
readable path with another role. The `tell` verb reaches only the
controller, never a clean role.

**Files the change touches.** As C1 and C3. Also the 5 `teammates/intake-*.md`
files (set `horch_access: report`) and spec 6.4 and 6.10 text.

---

## R9: Read other sessions' data in the state root and ledger folder

**Capability.** Read, across all projects and runs:

- `herdr-orchestration-<ws>/*.brief.json`: every worker's task text,
  project dir and report target;
- `mail/` and `messages/`: every message to the orchestrator and every long
  tell;
- `compact/*/job.log`, `telemetry/events-*.jsonl`, the session record files
  `<project>.json` (1016 records in this project's file);
- `~/.horch/ledger.sqlite3` (task summaries, events) and `~/.horch/evidence/`
  (review outputs, check logs).

Claude transcripts are under `~/.claude/projects`, which T-2 does not open.
Handoff files under a project's `ai_docs/handoffs/` are not opened either.
Live screens are R3.

**Evidence.** The paths above exist under the live state root and ledger
folder (listed by name only for this review; contents not read beyond
top-level JSON keys). Owners: `messaging/mailbox.rs:62`,
`messaging/mail.rs:37`, `messaging/message.rs:52`,
`compaction/jobfile.rs:82`, `cmd/taskledger.rs:2156`.

**Severity.** Medium. The role has no network, so it needs R1, R2 or R7 to
move the data out. Status: CONFIRMED.

**Proposed change.** C2 and C3.

**Files the change touches.** As C2 and C3.

---

## R10: Stop or block fleet work

**Capability.** Delete or corrupt `ledger.sqlite3`, `locks/` or the
watchdog logs; close panes (`pane.close`); stop workers (`horch ledger stop`
signals recorded PIDs); fill the state root disk.

**Evidence.** `herdr-docs/socket-api.md:99` (`pane.close`);
`horch ledger --help` (`stop`); write grants as in R4 and R6.

**Severity.** Medium. Status: CONFIRMED by code.

**Proposed change.** C1, C2 and C3.

**Files the change touches.** As C1, C2 and C3.

---

## C1 to C3 as built (T-20, commit `f5bfc2eb`)

- `horch_access: full | report` on `Teammate` (`roster/teammate.rs`).
  Default `full`. `horch teammates --check` fails `report` without
  `sandbox:` (`roster/validation.rs`).
- `overlay_horch_paths` (`harness/claude.rs`): `full` is unchanged.
  `report` adds no state root and no ledger folder, and only
  `<state_root>/report/<record_id>.sock` to `network.allowUnixSockets`. The
  launch also sets `HORCH_REPORT_SOCKET` to that path. A `report` launch
  with no ledger record gets no socket and no variable.
- The `horch worker` parent (`execution/lifecycle.rs`) binds the socket
  (mode 0600, folder 0700) before the agent starts and removes it when the
  agent exits. It runs each checked request as `horch` with the brief's
  environment, so the role and record come from the brief.
- Protocol (`messaging/report.rs`): 1 JSON line per connection, at most
  16384 bytes, then 1 JSON reply line. Verbs: `progress {text}`,
  `tell {to, text}`, `done {text}`. Unknown verbs, unknown keys, an empty
  text and an oversize line are refused. `tell` names its target so the CLI
  keeps its `horch tell <role> <text>` shape. The parent refuses every `to`
  except the brief's report target: `orchestrator` when the brief reports to
  the orchestrator, none otherwise. The brief has no other controller field.
- CLI (`crates/horch/src/main.rs`, `cmd/messaging.rs`, `cmd/taskledger.rs`):
  with `HORCH_REPORT_SOCKET` set, `horch ledger progress`, `horch tell` and
  `horch done` (without `--unlanded`) go to the socket. Every other command
  exits 1 before it reads any horch state.
- The 5 `teammates/intake-*.md` set `horch_access: report`.
  `teammates/skill-importer.md` keeps `full`.

## C6: The controller keeps full access

**Status: C6 done in commit `0726747f` (ticket T-21).** The roster key is in
`19c2d53f` and the brief controller field in `e0e2e12e`. Live check:
`docs/live-checks/skill-intake.md`, "Re-probe after ticket 21".

`skill-importer` needs `horch spawn`, so it keeps `horch_access: full` and
R1 to R10 apply to it if it is steered. It reads only `intake` output and
status files through `intake role-result` (spec 6.3, 6.6), so hostile text
reaches it only through a gap in those commands.

As built:

- `spawn_only: [<glob>, ...]` on `Teammate` (`roster/teammate.rs`).
  `horch teammates --check` fails an empty pattern, a path separator and a
  pattern that matches no teammate. `skill-importer` sets
  `spawn_only: ["intake-*"]`.
- `horch spawn` (`cmd/spawn.rs`) finds the caller from herdr: the role
  registered on the caller's pane, in the pane's own workspace, then that
  role's brief. It does not read `HORCH_ROLE`. A caller with `spawn_only`
  is refused, before any ledger row, for a name that matches no glob, for
  `--resume`, and for a `HORCH_TEAMMATES_DIR` other than the one its brief
  names. A caller without the key, and a pane with no registered role (the
  orchestrator, an operator shell), are unchanged.
- The spawned worker's brief has `controller: <caller role>`
  (`messaging/brief.rs`). `report::target` (`messaging/report.rs`) makes it
  the only `tell` target of a `report` pane. `horch done` still reports to
  the orchestrator, as `report_to` says.

Limit: a steered controller still has `full` access. It can unset
`HERDR_PANE_ID`, delete its brief, or call the herdr socket directly, and
`horch spawn` then sees no caller. C6 stops a steered controller that uses
`horch spawn` as written. C4 (caller identity from herdr, not the
environment) and a `report`-like mode for the controller close the rest.

## Git writes from a sandboxed pane (T-9, run `external-imports`)

Date: 2026-11-24. horch 0.5.3, Claude Code 2.1.331, agy 1.5.16, backend hc.

### Paths a sandboxed worker can write

Claude, `horch_access: full` (`overlay_horch_paths`, `harness/claude.rs`):

| path | access | why |
|---|---|---|
| the persona's own `sandbox.filesystem.allowWrite` entries | write | the persona's work folder |
| state root (`~/.local/state/horch`), except `skill-bundles/` | write | mail, briefs, run files of `horch` commands |
| ledger folder (`~/.horch`) | write | `horch ledger progress`, `claim`, `finish` write the ledger |
| git common directory (`git rev-parse --git-common-dir` in the project) | write | objects and refs of a commit |
| `<repository root>/.worktrees` | write, and `permissions.additionalDirectories` | the files of the ledger worktrees |
| the roster folder and the skill bundle | read only | the pane reads them; it must not change them |
| `<state root>/messages` | Read tool (`permissions.additionalDirectories`), Edit and Write tools denied | the pane reads a truncated horch message from its spool file; Bash already reads the state root |

Claude, `horch_access: report`: only the persona's own entries. The report
socket is the only way out (C1 to C3).

agy: horch adds no path. `--sandbox` gives write access to the agy
workspace only.

### What the harness blocks after these grants

The harness rules win over the grants above:

- Claude Code 2.1.331 always denies writes to `<cwd>/.git/worktrees`,
  `.git/config`, `.git/hooks`, `.git/commondir`, `.git/modules` and
  `.git/info/exclude`. `allowWrite` cannot remove the deny. Only the
  admin folder of a worktree that is the session's cwd, or that is in
  `permissions.additionalDirectories` when the sandbox is built, is exempt.
- agy 1.5.16 `--sandbox` makes the `.git` folder of every repository read
  only. Its changelog says so.

### Effect on the 4 ledger commands

| command | Claude sandbox | agy sandbox |
|---|---|---|
| `horch ledger worktree new` (`git worktree add`) | fails: creates `.git/worktrees/<name>` | fails: writes `.git/refs/heads/<branch>.lock` |
| `git add`, `git commit` in a ledger worktree | fails unless the worktree is an exempt worktree: writes `.git/worktrees/<name>/index.lock` | fails: `.git` is read only |
| `horch ledger finish` | the detached run is a child of the sandboxed Bash, so land and cleanup fail on the same paths | same |

### Root cause

- Claude: the harness denies `.git/worktrees` itself. horch already opens
  the git common directory, so no grant can fix it.
- agy: the harness makes `.git` read only, and horch passes agy no path.
- The hc launch path is not the cause. The live `skill-importer-1` settings
  list the git common directory and `.worktrees` in `allowWrite`.

### Other defects found

- A failed `git worktree add -b` left the new branch, so the next attempt
  failed with "a branch named ... already exists". Fixed in T-9:
  `worktree_ops::add_worktree` deletes the branch with
  `git update-ref -d refs/heads/<branch> <commit>` when git made it and the
  add failed. `finish::create_worktree` and `worktree_ops::create` use it.
- A sandboxed Claude pane could not open a spooled message
  (`<state root>/messages/<workspace>/*.md`) with the Read tool.
  `blockReadsOutsideWorkingDirectories` is on, and the folder was in
  `sandbox.filesystem.allowRead` only. A truncated orchestrator message then
  stopped `skill-importer-1` on a permission prompt. Fixed in T-9 for
  `horch_access: full` (see the table above). Still open: a `report` pane has
  no state root, so it cannot read a spooled message at all. See "Broker
  design (O1)", "Related: spooled messages".
- Open, for a later run: in the Claude sandbox of `skill-importer-1` (hc
  backend), hc `pane_get` and `horch tell <worker>` fail with "Operation not
  permitted (os error 1)"; `horch tell orchestrator` works.

Status: the 4 ledger commands still fail in a sandboxed pane. The
workaround for run `external-imports` is O3: the orchestrator makes the
worktrees and the commits. The target design is O1 below. O2
(`sandbox.excludedCommands` for `horch ledger` and `git`) is rejected: an
unsandboxed `git` with agent arguments (`git -c core.pager=...`) leaves the
sandbox.

## Broker design (O1)

Not built. This section is the input for the O1 tickets.

### Goal

A sandboxed worker in a ledger run creates its worktree, commits, lands and
drops worktrees. The unsandboxed `horch worker` parent of the pane does
every `.git` write for it. The agent never runs code outside the sandbox,
and the parent never runs code from the worktree.

### Who gets a broker

The parent starts a broker when all of these are true:

- the brief has a ledger worker id (`brief.worker_id`);
- the teammate has a `sandbox:` block (Claude) or runs agy with `--sandbox`;
- the teammate has `horch_access: full`. A `report` pane does no ledger work.

### Socket contract

- Path: `<broker root>/<record id>.sock`. The broker root is a sibling of
  the state root (`~/.local/state/horch-broker`), not inside it. A `full`
  pane can connect to every socket under the state root, so a socket there
  would let a pane reach the broker of another pane.
- Folder mode 0700, socket mode 0600. The parent binds the socket before the
  agent starts and removes it when the agent exits, as for the C1 report
  socket (`messaging/report.rs`, `execution/lifecycle.rs`).
- The launch adds only this 1 path to `network.allowUnixSockets` and sets
  `HORCH_BROKER_SOCKET` to it.
- Framing: 1 JSON object on 1 line per connection, at most 16384 bytes, read
  timeout 10 s. The parent sends 1 JSON reply line. The parent serves 1
  request at a time.
- Request: `{"verb": "<verb>", ...fields}`. Unknown verbs and unknown fields
  are refused (`deny_unknown_fields`).
- Reply: `{"ok": true, ...result}` or `{"ok": false, "error": "<text>"}`.
- Identity: the socket is the identity. The parent acts for the worker id,
  task and worktree of its own brief and ledger rows. A request never names
  a worker, a task, a run or a path outside its worktree.

### Requests

| verb | fields | the parent does | reply fields |
|---|---|---|---|
| `worktree_new` | `name`, optional `from` | `worktree_ops::create` for its own worker | `path`, `branch`, `base` |
| `worktree_drop` | `name`, `reason`, `keep_branch` | `worktree_ops::drop_worktree` in its own run | `status` |
| `commit` | `message`, `paths` (list) | `git add -- <paths>` and `git commit -m <message>` in its own open worktree | `commit` |
| `land_rebase` | none | rebase its own worktree branch on the base | `head` |
| `land_advance` | `tested` (commit id) | fast-forward the base to `tested` | `landed` |
| `worktree_remove` | none | remove its own landed worktree (cleanup step) | none |

The finish steps that run code (`check`, `recheck`, the check inside
`land`) stay in the agent's sandbox. With `HORCH_BROKER_SOCKET` set,
`horch ledger finish` runs them as today and calls `land_rebase`, then the
check, then `land_advance`, then `worktree_remove`. The `pr` step is out of
scope: it needs `gh` and the network.

### What the parent validates

Every request:

- size, JSON shape and verb as above;
- the worker still owns its task (`tasks.current_worker_id`), and the task
  is not done or cancelled.

Per field:

- `name`: `valid_name` (1 to 60 letters, digits, `-`, `_`).
- `from`: a ref that `git rev-parse --verify` resolves, with no leading `-`
  and no newline.
- `message`: not empty after trim, at most 4096 bytes, no NUL.
- `paths`: 1 to 1000 entries. Each is relative, has no `..` part, no leading
  `-`, is not `.git` and does not start with `.git/`. After the parent
  resolves symlinks, each path is inside the worktree.
- `tested`: equals the head of the recorded worktree branch, descends from
  the current base, and the base can fast-forward to it.

Git execution for every request:

- The parent takes the worktree path, branch and git common directory from
  the ledger row, never from the worktree. It runs git with
  `--git-dir=<common>/worktrees/<admin name>` and `--work-tree=<path>`, and
  first checks that `<common>/worktrees/<admin name>/gitdir` names
  `<path>/.git`. The `<path>/.git` file is in the writable worktree, so the
  agent can point it at a folder with its own `config`.
- The parent adds `-c core.hooksPath=<empty folder>`,
  `-c core.fsmonitor=false` and `-c protocol.allow=never`, and never passes
  `--recurse-submodules`.
- The parent runs git with its own environment, with every `GIT_*` variable
  removed. It never uses the agent's environment.
- Paths go after `--`. No agent text becomes a git option.

### Threat model

Attacker: a steered sandboxed worker with `horch_access: full`, same user id
as the operator.

| threat | mitigation | residual |
|---|---|---|
| code runs outside the sandbox through git in the parent (hooks, `core.fsmonitor`, filters, a forged `.git` file) | hooks off, fsmonitor off, explicit `--git-dir` from the ledger with the back-pointer check, no submodules, no agent environment | filter and diff drivers need config; both harnesses deny `.git/config` writes. A new git feature that runs a program from a tracked file needs a new `-c` switch |
| the parent runs worktree code | the broker never runs a check, a build or a script; those stay in the sandbox | none |
| a pane acts on another worker's task or branch | 1 socket per record outside the state root; identity from the socket; only rows of its own worker | a `full` pane can still write the ledger (R4) |
| a pane moves the run branch or `main` | `land_advance` fast-forwards only the base of its own task, only to the tested head of its own branch | today the git common directory is in `allowWrite`, so a pane can move refs directly. O1 removes the common directory from `allowWrite`; `.worktrees` stays |
| a pane forges a passed check | none in the broker | check evidence is in the ledger folder, which a `full` pane can write (R4). C4 or a `report`-like ledger mode closes it |
| argument injection | field checks above, paths after `--` | none |
| denial of service | size limit, read timeout, 1 request at a time | a pane can block only its own broker |

### Files that change

- `crates/horch-core/src/messaging/broker.rs` (new): request and reply
  types, `handle(line, &Identity, &Executor) -> Reply`, field validation,
  the socket server. Share the server loop with `messaging/report.rs`.
- `crates/horch-core/src/execution/lifecycle.rs`: start and stop the broker
  next to `start_report_server`; the executor calls the ledger functions in
  process.
- `crates/horch-core/src/harness/launch.rs`: `LaunchEnv::broker_socket`;
  set `HORCH_BROKER_SOCKET`.
- `crates/horch-core/src/harness/claude.rs`: add the broker socket to
  `allowUnixSockets`; with a broker, leave the git common directory out of
  `allowWrite`.
- `crates/horch-core/src/harness/antigravity.rs`: pass the socket variable;
  open the socket if agy needs a setting for it.
- `crates/horch-core/src/taskledger/git.rs`: a hardened runner with an
  explicit git dir and work tree, the `-c` switches and a clean environment.
- `crates/horch-core/src/taskledger/worktree_ops.rs`,
  `crates/horch-core/src/taskledger/finish/mod.rs`,
  `crates/horch-core/src/taskledger/finish/land.rs`: use the hardened runner;
  split land into rebase, check and advance.
- `crates/horch/src/main.rs`, `crates/horch/src/cmd/taskledger.rs`,
  `crates/horch/src/cmd/worktree.rs`,
  `crates/horch/src/cmd/taskledger_finish.rs`: client mode when
  `HORCH_BROKER_SOCKET` is set; a new `horch ledger commit -m <message> --
  <paths>` subcommand.
- The worker prompt text that tells a worker to commit
  (`crates/horch-core/src/prompts.rs` and its golden tests): a sandboxed
  worker commits with `horch ledger commit`.
- This document and a live check in `docs/live-checks/`.

### Test seams

- `broker::handle` is a pure function with a fake executor: size, shape,
  unknown verb, unknown field, each field rule, a refused request runs
  nothing.
- Path validation: `..`, an absolute path, a leading `-`, `.git/`, and a
  symlink that leaves the worktree are refused.
- Hardened runner, with a temporary repository and linked worktree:
  - a `pre-commit` hook that writes a marker file leaves no marker;
  - a worktree `.git` file that points at a folder whose `config` sets
    `core.fsmonitor` to a marker command is refused, and no marker appears;
  - a `GIT_DIR` in the caller's environment is ignored.
- Land: `land_advance` refuses a non-fast-forward, a commit other than the
  branch head, and a branch other than the recorded branch.
- Server: socket mode 0600, folder 0700, the socket is gone after the agent
  exits (copy the report socket tests).
- Claude overlay: with a broker socket, `allowUnixSockets` gains only that
  path, and `allowWrite` has `.worktrees` but not the git common directory.
- CLI: with `HORCH_BROKER_SOCKET` pointing at a fake server, `horch ledger
  worktree new` sends 1 request and runs no git.
- Live check: a sandboxed Claude pane and a sandboxed agy pane each run
  `horch ledger worktree new`, `horch ledger commit` and `horch ledger
  finish` with no orchestrator help.

### Open questions for the tickets

- Can an agy `--sandbox` command connect to a Unix socket, and which agy
  setting allows it? Check before the agy ticket.
- Does agy let a sandboxed command read `<state root>/messages`?

### Related: spooled messages

A `report` pane cannot read `<state root>/messages`. Add a `message` verb to
the report socket that returns the text of 1 spooled message addressed to
the pane's role, or do not truncate messages to a `report` pane.

## Ranked list

| rank | risk | severity | change | when |
|---|---|---|---|---|
| 1 | R1 run any command outside the sandbox (herdr socket, `horch spawn`) | Critical | C1 report socket, no herdr or relay socket for `report` panes | done, `f5bfc2eb` |
| 2 | R4 ledger edit (`land_check` runs unsandboxed, `task_file` redirect) | Critical | C2 no ledger folder for `report` panes | done, `f5bfc2eb` |
| 3 | R6 skill bundles and brief binaries of other workers | Critical | C3 no state root for `report` panes | done, `f5bfc2eb` |
| 4 | R8 clean-room wall breach | High | C1 + C3, `horch_access: report` on the 5 intake roles | done, `f5bfc2eb` |
| 5 | R2 type into any pane, forged or untagged sender | High | C1 | done, `f5bfc2eb` |
| 6 | R7 forge or hide mail and relay messages | High | C1 + C3 | done, `f5bfc2eb` |
| 7 | R5 environment-asserted identity in `horch` | High | C4 caller from herdr pane | follow-up (out of the role threat model after C1 and C2) |
| 8 | R3 read any pane | Medium | C1 | done, `f5bfc2eb` |
| 9 | R9 read other sessions' state and ledger data | Medium | C2 + C3 | done, `f5bfc2eb` |
| 10 | R10 stop or block fleet work | Medium | C1 + C2 + C3 | done, `f5bfc2eb` |
| - | R6 integrity for unsandboxed readers (Codex panes) | - | C5 | follow-up |
| - | controller keeps full access | - | C6 spawn allowlist | done, `0726747f` |

C1, C2 and C3 are 1 change: the `horch_access: report` mode. It replaces
the 4 T-2 grants for a role pane with 1 per-pane socket that the
unsandboxed `horch` parent serves. Until it lands, a skill-intake role pane
has the same reach as an unsandboxed worker.

## Limits of this review

- No exploit ran against a live pane, the live ledger or the live state
  root. R1 rests on the T-2 re-probe (`horch spawn` from a sandboxed pane
  exited 0) and on the process chain. The other risks rest on code paths.
- Linux bwrap was not checked. The grants are the same keys, so the same
  findings are expected.
- Whether `filesystem.allowRead` re-opens a path to the Read tool under
  `blockReadsOutsideWorkingDirectories` was not tested. Bash `cat` is enough
  for every read risk above.
