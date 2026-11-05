# Live check: skill-intake

Live checks for the `skill-intake` feature (spec
`ai_docs/specs/skill-intake/spec.md`, section 12.5). The gate never runs
them. Each section is a run by a worker, recorded by hand. This file starts
with step 0, the sandbox probe (spec section 13, Q7).

## Sandbox probe (Q7)

**Verdict: fails — ticket 02 is needed.** No horch command works from a
Claude pane with a `sandbox:` block. All 4 commands exit 1.

### What the probe proves

Spec H-10 says no test shows that `horch tell`, `horch done`, `horch ledger
progress` and `horch spawn` work inside a sandboxed pane. This probe runs
them in 2 real sandboxed Claude panes.

### How it ran

Date: 2026-11-05. horch 0.4.3, Claude Code 2.1.318, macOS 26.5.1 (Seatbelt).
Each probe is a throwaway hidden teammate in a scratch worktree
(`horch ledger worktree scratch probe-q7`). Neither is landed. Both ran with
`horch spawn <name> --exact --run skill-intake`, with `HORCH_TEAMMATES_DIR`
set to the scratch `teammates/` folder. `--run` gives the pane a ledger
worker row, so `horch ledger progress` reaches the ledger and does not fail
for lack of a worker.

| probe | shape | model, effort | sandbox |
|---|---|---|---|
| `probe-sanitizer` | `intake-sanitizer` (spec 6.4) | sonnet, low | `hidden: true`; `tools: [Read, Grep, Glob, Write, Edit, Bash]`; no `network.allowedDomains`; `allowRead` and `allowWrite` `~/.local/state/skill-intake/drafts`; `denyRead` `.../contaminated`, `.../clean`, `~/.ssh`, `~/.aws`, `~/.config`, `~/Library/Keychains`; the interpreter and `git *` deny rules of 6.4 |
| `probe-importer` | `skill-importer` (spec 9.2) | sonnet, low | `hidden: true`; `network.allowedDomains` `github.com`, `codeload.github.com`, `objects.githubusercontent.com`; `allowRead` and `allowWrite` `~/.local/state/skill-intake` |

Both have `base: fleet-worker`, `agent: claude`, `permission_mode: auto`,
`inherit_plugins: false`, `mcp_servers: {}`, `fallbacks: []`,
`disallowed_tools` with `Agent`, `WebFetch`, `WebSearch` and `NotebookEdit`,
and the `allowed_tools` list of `sonnet`. Each pane writes its results to a
file inside its own writable side root, because no horch command works to
report them. The pane read the file path from its task text or persona.

To re-run (ticket 02 does this after its change): create the 2 teammates
from the table, make `~/.local/state/skill-intake/drafts/probe/` and
`~/.local/state/skill-intake/workspace/probe/`, spawn each with a task text
that names the exact commands and the results file, then read the 2 files.
Write the commands in the task text itself. A first `probe-importer` run
that said "your persona names the commands" ran none of them: the model did
not find the persona text.

### Result

| command | probe | exit | error text or denied path |
|---|---|---|---|
| `horch tell orchestrator "..."` | `probe-sanitizer-1` | 1 | `horch: pruning acked mail: reading /Users/matthewscott/.local/state/horch/mail/w29/acked: Operation not permitted (os error 1)` and `horch: creating /Users/matthewscott/.local/state/horch/mail/w29/pending/.1793863405930-01793863405930611000-56495-0.json.tmp: Operation not permitted (os error 1)` |
| `horch ledger progress "..."` | `probe-sanitizer-1` | 1 | `horch: opening the task ledger /Users/matthewscott/.horch/ledger.sqlite3: unable to open database file: /Users/matthewscott/.horch/ledger.sqlite3: Error code 14: Unable to open the database file` |
| `horch done "..."` | `probe-sanitizer-1` | 1 | `closing: reporting to the orchestrator` then `horch: no record matches 6a984dea-d544-48d4-a13a-2bd282076f00` |
| `horch spawn smoke` | `probe-importer-2` | 1 | `horch: opening the task ledger /Users/matthewscott/.horch/ledger.sqlite3: unable to open database file: /Users/matthewscott/.horch/ledger.sqlite3: Error code 14: Unable to open the database file` |

`probe-importer-2` also ran `horch done "sandbox probe finished"`: exit 1,
`closing: reporting to the orchestrator` then `horch: no record matches
b6910216-cac8-4b78-b2e0-54d3f3a3b2e7`. No `smoke` pane was created.

Evidence from an earlier `probe-importer-1` run, which ran none of the probe
commands: the pane reported that its own `horch tell` failed twice and its
`horch note` failed once, all with "Operation not permitted" on
`~/.local/state/horch/`. This is the pane's own report, not a recorded
command output.

The `horch done` error is not a plain denial. The text names a record id,
not a path. The cause is inferred, not proven: `horch done` reads the
project record file `~/.local/state/horch/-Users-matthewscott-repo-multi-herdr.json`,
the read fails under the sandbox, and the lookup finds no record. Ticket 02
must check this on its re-run.

### What ticket 02 must add

Denied and named by this probe:

| kind | path | why |
|---|---|---|
| read and write | the horch state root, `$HORCH_STATE_DIR` (`~/.local/state/horch`) | `horch tell` reads `mail/<workspace>/acked` and creates `mail/<workspace>/pending/*.tmp`; `horch done` very likely reads the project record file here |
| read and write | the ledger folder, `~/.horch` (`<home>/.horch/ledger.sqlite3`, `crates/horch-core/src/taskledger/store.rs`) | `horch ledger progress` and `horch spawn` open `ledger.sqlite3`. SQLite also creates `ledger.sqlite3-wal` and `-shm`, and the folder holds `locks/` and `evidence/`, so the grant is the folder, not the file |

Not denied in any output, so this probe does not name them:

| kind | path | status |
|---|---|---|
| read and write | the horch data root, `$HORCH_DATA_DIR` (`~/.local/share/horch`) | no command reached it. Spec Q7 option 1 lists it. By the ticket 02 rule ("exactly the paths 01 named") it stays out until the re-run shows a denial |
| unix socket | `$HERDR_SOCKET_PATH` (`~/.config/herdr/herdr.sock`) | no command reached it. Every command failed earlier, so the socket stays unproven. The re-run must show whether `horch tell` and `horch spawn` need `network.allowUnixSockets` |

Warnings for ticket 02:

- `~/.config` is in the `denyRead` of the intake role shape. The herdr socket
  is under `~/.config/herdr`. An `allowUnixSockets` entry may need no read
  grant, but the probe cannot say. Check on the re-run.
- The first denial can hide the next one. Each command stops at its first
  failure, so the list above is the first layer. Expect the re-run to find
  more.
- The probe panes are stuck after a failed `horch done`. Stop them with
  `horch ledger stop <worker> "<reason>"`.

### Cost and side effects

The probe started 3 short Claude sessions, 1 more than the ticket allows:
`probe-sanitizer-1`, `probe-importer-1` (no probe command ran; repeated) and
`probe-importer-2`. Sessions: sonnet, low effort. No other model session
started. The probe left a worker row for each pane in run `skill-intake`;
all 3 were stopped with `horch ledger stop`. The probe wrote only under
`~/.local/state/skill-intake/`.

### Re-probe after ticket 02

**Verdict: all 4 commands work.** Date: 2026-11-05. The same 2 probe
teammates ran from `.worktrees/_scratch/probe-q7/teammates`, spawned by a
debug build of the ticket 02 branch with `--exact --run skill-intake`. The
commands inside the panes ran the installed horch 0.4.3: only the launch
settings changed. The task text named the commands, as the section above
says.

The launch adds these entries to every sandboxed Claude pane. Each comes
from the roots horch resolves at launch, not from a fixed string:

| key | entry | source |
|---|---|---|
| `filesystem.allowRead`, `filesystem.allowWrite` | `/Users/matthewscott/.local/state/horch` | `Paths::state_root` (`$HORCH_STATE_DIR`, else the XDG default) |
| `filesystem.allowRead`, `filesystem.allowWrite` | `/Users/matthewscott/.horch` | the folder of `Paths::ledger_db` (`$HORCH_LEDGER_DB`, else `<home>/.horch/ledger.sqlite3`) |
| `network.allowUnixSockets` | `/Users/matthewscott/.local/state/horch` | `Paths::state_root`. Seatbelt matches a socket entry as a subpath, so this covers `channels/<workspace>.sock` |
| `network.allowUnixSockets` | `/Users/matthewscott/.config/herdr/herdr.sock` | `$HERDR_SOCKET_PATH` of the spawning process. No entry when it is unset |

Round 1 (filesystem entries only; panes `probe-sanitizer-2`, `probe-importer-3`):

| command | exit | error text |
|---|---|---|
| `horch tell orchestrator "..."` | 0, not delivered | `connect to channel relay /Users/matthewscott/.local/state/horch/channels/w29.sock` failed, then `herdr pane send-text ... PermissionDenied` |
| `horch ledger progress "..."` | 0 | none |
| `horch done "..."` | 1 | `herdr pane get w29:pC failed ... PermissionDenied`; "the summary is recorded, but this pane may still be open" |
| `horch spawn smoke` | 1 | `launch of ... failed splitting a pane: herdr pane get w29:pD failed ... PermissionDenied` |

Round 1 proves 2 facts:

- The state root caused the "no record matches" error of `horch done`. With
  the state root open, the error is gone and the summary is recorded.
- The next layer is Unix socket connects: the channel relay socket under the
  state root, and the herdr socket. The `~/.config` read path is not needed
  for the herdr socket: the sanitizer keeps `denyRead ~/.config` and connects.

Round 2 (all entries; panes `probe-sanitizer-3`, `probe-importer-4`):

| command | probe | exit | output |
|---|---|---|---|
| `horch tell orchestrator "..."` | `probe-sanitizer-3` | 0 | `queued, not yet acknowledged (notification written; awaiting horch inbox acknowledgement)` |
| `horch ledger progress "..."` | `probe-sanitizer-3` | 0 | none |
| `horch done "..."` | `probe-sanitizer-3` | 0 | the pane closed; record status `done` |
| `horch spawn smoke` | `probe-importer-4` | 0 | `spawned smoke-2 (pane w29:pG)` |
| `horch done "..."` | `probe-importer-4` | 0 | the pane closed; record status `done` |

A successful `horch done` closes its pane, so the pane cannot write its exit
code. The evidence is the record file: `probe-sanitizer-3`,
`probe-importer-4` and `smoke-2` have status `done`, and their panes and
processes are gone. In round 1, a failed `horch done` left the pane open.

Not needed: the data root `$HORCH_DATA_DIR` (`~/.local/share/horch`). No
command in either round reached it, so the launch does not add it.

Cost: 4 more sonnet low-effort sessions (2 per round) and 1 `smoke` pane,
which runs no model. Round 1 panes were stopped with `horch ledger stop`.
Round 1 results files are kept as `*-results-round1.txt` beside the round 2
files under `~/.local/state/skill-intake/`.

### Re-probe after ticket 20 (`horch_access: report`)

**Verdict: 3 allowed commands exit 0, 4 denials hold.** Date: 2026-11-05.
The probe teammate `probe-report` is a copy of `intake-sanitizer` with
`effort: low` (sonnet), so it has `horch_access: report`. It ran from
`.worktrees/_scratch/probe-t20/probe-teammates`, spawned by a debug build
of the T-20 branch with `--exact --run skill-intake`. The pane's `horch`
was the same debug build. The task text named the commands, as above.

The launched `--settings` of `probe-report-1` (read from the `claude`
process arguments) hold only the role's side root and 1 socket:

| key | entries |
|---|---|
| `filesystem.allowRead`, `filesystem.allowWrite` | `~/.local/state/skill-intake/drafts` |
| `network.allowUnixSockets` | `/Users/matthewscott/.local/state/horch/report/<record id>.sock` |

No state root, ledger folder, relay socket or herdr socket. The `claude`
process had `HORCH_REPORT_SOCKET` set to the same path. The socket file was
mode 0600.

Round 1 (`probe-report-1`, commit `f5bfc2eb`) and round 2
(`probe-report-2`, commit `ef7958d3`):

| # | command | round 1 | round 2 | output |
|---|---|---|---|---|
| 1 | `horch ledger progress "..."` | 0 | 0 | none; the ledger event shows the text for the pane's worker row |
| 2 | `horch tell orchestrator "..."` | 0 | 0 | none; the orchestrator mail has `from: probe-report-2` and the text tagged `[probe-report-2]` by the parent |
| 3 | `herdr pane list` | 1 | 1 | `Error: Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" }` |
| 4 | `horch spawn smoke` | 1 | 1 | `horch: this pane has horch_access: report. Only ... work here` |
| 5 | `env -u HORCH_REPORT_SOCKET horch spawn smoke` | 1 | not run | `horch: opening the task ledger /Users/matthewscott/.horch/ledger.sqlite3: unable to open database file ... Error code 14` |
| 6 | `: >> ~/.horch/ledger.sqlite3` | 1 | 1 | the shell's open for write failed |
| 7 | `touch ~/.local/state/horch/probe-t20-write` | 1 | 1 | `touch: ... Operation not permitted`; the file does not exist |
| 8 | `horch done "..."` | 1 | 0 | round 1: `the report socket closed without an answer`; round 2: `horch done: the worker is closing this pane` |

The 4 denials of the ticket: the herdr socket (3), `horch spawn` (4, and 5
without the report variable), a write to the ledger database (6) and a
write under the state root (7).

Round 1 found 1 defect. The parent ran `horch done`, the record went to
`done` and the pane closed, but the parent died with the pane before it
could answer. The client read end of file and exited 1. Commit `ef7958d3`
makes end of file after `done` a success; after any other verb it stays an
error. Round 2 shows exit 0. The record file shows status `done` for
`probe-report-1` and `probe-report-2`, and both panes are gone.

Round 2 did not run command 5: no line for it is in the results file. Round
1 shows its result.

Not fixed: the parent dies with the pane, so it does not remove its socket
file after `horch done`. The 2 probe socket files were removed by hand. The
next launch of the same record replaces the file.

Cost: 2 sonnet low-effort sessions. No `smoke` pane started. The watchdog
recorded each probe worker as `no_task` (it claimed no task). Results files:
`~/.local/state/skill-intake/drafts/probe/report-results-round1.txt` and
`report-results.txt`.

### Re-probe after ticket 21 (`spawn_only`)

**Verdict: 3 controller refusals hold, the allowed spawn works, the role's
`tell` reaches the controller, and its `tell` to the orchestrator is
refused.** Date: 2026-11-05. Code at commit `b79bc663`.

The probe controller `probe-importer` is a copy of `skill-importer` with
`hidden: true`, `model: sonnet`, `effort: low`, no `system_prompt` and
`spawn_only: ["probe-report"]`. The role is the T-20 `probe-report`
(`horch_access: report`). Both ran from
`.worktrees/_scratch/probe-t20/probe-teammates`. A copy of the T-21 debug
build at `~/.local/state/skill-intake/drafts/probe/t21-bin/horch` spawned the
controller with `--exact --run skill-intake`. Each pane ran that copy by its
full path, because the installed horch 0.4.3 has no report mode.

Controller `probe-importer-5` (pane `w29:pY`):

| # | command | exit | output |
|---|---|---|---|
| 1 | `horch spawn smoke --exact` | 1 | `horch spawn refused: probe-importer-5 (teammate probe-importer) may spawn only [probe-report], and 'smoke' matches none` |
| 2 | `HORCH_TEAMMATES_DIR=/tmp horch spawn probe-report --exact` | 1 | `horch spawn refused: probe-importer-5 (teammate probe-importer) has spawn_only [probe-report], so HORCH_TEAMMATES_DIR=/tmp is refused; its brief names <scratch>/probe-teammates` |
| 3 | `horch spawn --resume probe-x` | 1 | `horch spawn refused: probe-importer-5 (teammate probe-importer) has spawn_only [probe-report], so --resume is refused` |
| 4 | `horch spawn probe-report --exact --run skill-intake "<task>"` | 0 | `spawned probe-report-3 (pane w29:pZ)` |
| 5 | (received) | - | `[probe-report-3] probe-report hello controller` reached the controller pane |

Role `probe-report-3` (pane `w29:pZ`):

| # | command | exit | output |
|---|---|---|---|
| 1 | `horch tell probe-importer-5 "..."` | 0 | none |
| 2 | `horch tell orchestrator "..."` | 1 | `horch: this pane reports only to probe-importer-5, not to orchestrator` |
| 3 | `horch done "..."` | 0 | the pane closed |

The record file shows status `done` for `probe-importer-5` and
`probe-report-3`, and both panes are gone. The orchestrator mail of `w29`
has `[probe-report-3] DONE: probe-report done`: `horch done` of a role still
reports as before. No mail has the text of the refused `tell`.

Side effects: each spawn by the debug build printed 2 warnings that the
watchdogs of runs `skill-intake` and `one-page-design` run another build.
Both say nothing was signalled. The role's report socket file stayed after
`horch done` (the T-20 "not fixed" item) and was removed by hand.

Cost: 2 sonnet low-effort sessions. No `smoke` pane started. Results files:
`~/.local/state/skill-intake/workspace/probe/t21-importer-results.txt` and
`~/.local/state/skill-intake/drafts/probe/t21-report-results.txt`.

### Re-probe after ticket 27 (own skill bundle)

**Verdict: each pane reads its bundle 2 ways and fails to write it 3
ways.** Date: 2026-11-05. Claude Code 2.1.318, macOS (Seatbelt). Code at
commit `769dd9cf`.

The T-17 live check stopped `skill-importer-1` at a prompt: the Read tool
could not open the skill bundle under the state root
(`permissions.blockReadsOutsideWorkingDirectories`).

#### Option choice (`claude -p` probes)

4 headless `claude -p --permission-mode acceptEdits` runs, with a
hand-made `--settings` and a probe bundle at
`~/.local/state/horch/skill-bundles/probe-t27/`. `acceptEdits` allows any
edit in a working directory, so only a deny rule or the sandbox stops a
write. No auto-mode classifier runs. Grep and Glob are not tools in Claude
Code 2.1.318 ("No such tool available"), so Read and Bash are the read
paths.

| run | Read | `cat` | Edit | Write | `echo >` |
|---|---|---|---|---|---|
| no grant, `full` | denied (`blockReadsOutsideWorkingDirectories`) | allowed | **allowed** | **allowed** | **allowed** |
| option (a), `full` | allowed | allowed | denied | denied | denied |
| option (b), `full` | denied (`blockReadsOutsideWorkingDirectories`) | allowed | denied | denied | denied |
| option (a), `report` | allowed | allowed | denied | denied | denied |

- Option (a): `permissions.additionalDirectories` of the bundle, plus
  `permissions.deny` `Edit(/<bundle>/**)` and `Write(/<bundle>/**)`
  (`//` starts an absolute rule path), plus `sandbox.filesystem.allowRead`
  and `denyWrite` of the bundle.
- Option (b): the same, with a `permissions.allow` rule `Read(/<bundle>/**)`
  in place of `additionalDirectories`. An allow rule does not pass
  `blockReadsOutsideWorkingDirectories`, so (b) fails.
- Edit and Write fail with `File is in a directory that is denied by your
  permission settings.` `echo >` fails with `operation not permitted`: the
  sandbox `denyWrite` wins over the `full` pane's state-root `allowWrite`.
- The first row shows the gap before this ticket: a `full` pane could edit
  its bundle with every tool.

Horch takes option (a), for the launch's own bundle only
(`overlay_bundle_paths` in `crates/horch-core/src/harness/claude.rs`).

#### Live panes

The probe teammates `probe-t27-report` (`horch_access: report`) and
`probe-t27-full` (`horch_access: full`) are copies of the T-20
`probe-report` (sonnet, low effort, `permission_mode: auto`,
`skills: [skill-intake]`). They ran from
`.worktrees/_scratch/probe-t27/probe-teammates`. A copy of the T-27 debug
build at `~/.local/state/skill-intake/drafts/probe/t27-bin/horch` spawned
each with `--exact --run skill-intake`. The task text named the steps.

The launched `--settings` (read from the `claude` process arguments) add 5
entries, each for the pane's own bundle `<state>/skill-bundles/<record id>`:

| key | entry |
|---|---|
| `sandbox.filesystem.allowRead` | the bundle |
| `sandbox.filesystem.denyWrite` | the bundle |
| `permissions.additionalDirectories` | the bundle |
| `permissions.deny` | `Edit(//<bundle>/**)`, `Write(//<bundle>/**)` |

The `report` pane has no state root and no ledger folder, as after T-20.

| # | step | `report-1` | `full-1` | `report-2` | `full-2` |
|---|---|---|---|---|---|
| 1 | Read `S/SKILL.md` | allowed | allowed | allowed | allowed |
| 2 | `cat S/SKILL.md \| head -5` | allowed | allowed | allowed | allowed |
| 3 | Edit `S/SKILL.md` | denied | denied | denied | denied |
| 4 | Write `S/probe-written.txt` | denied | denied | denied | denied |
| 5 | `echo ... > S/probe-echoed.txt` | denied | denied | denied | denied |

`S` is `<bundle>/skills/skill-intake`. Steps 3 and 4 fail with `File is in
a directory that is denied by your permission settings.` Step 5 fails
first at the auto-mode classifier: `Permission for this action was denied
by the Claude Code auto mode classifier`. Round 2 used a neutral echo text
and a task line that names the probe; the classifier still denied it. So
in a live pane the classifier is the first layer for step 5, and the
sandbox `denyWrite` (shown by the `claude -p` runs above) is the second.
After both rounds, each of the 4 bundles had its `name: skill-intake` line
and no probe file.

Not closed: a `full` pane still has the whole state root in `allowWrite`,
so Bash can write the bundle of another worker. This ticket opens and
closes only the pane's own bundle.

Cost: 4 short `claude -p` sonnet sessions and 4 sonnet low-effort panes.
Results files: `~/.local/state/skill-intake/drafts/probe/t27-report-results.txt`,
`t27-full-results.txt`, `t27-report-results-round2.txt` and
`t27-full-results-round2.txt`.

### Re-probe after ticket 28 (every skill bundle, `full` pane)

**Verdict: a `full` pane reads its own bundle, fails to write its own
bundle and another bundle with Edit, Write and Bash, and its `horch tell`,
`horch ledger progress`, `horch spawn smoke` and `horch done` work.**
Date: 2026-11-05. Claude Code 2.1.318, macOS (Seatbelt). Code at commit
`d22e110d`.

The probe teammate `probe-t28-full` is a copy of `skill-importer` with
`hidden: true`, `model: sonnet`, `effort: low`, no `system_prompt` and no
`spawn_only` (`horch_access: full`, `permission_mode: auto`,
`skills: [skill-intake]`). It ran from
`.worktrees/_scratch/probe-t28/probe-teammates`. A copy of the T-28 debug
build at `~/.local/state/skill-intake/drafts/probe/t28-bin/horch` spawned it
with `--exact --run skill-intake`, and the pane ran that copy by its full
path. "Another bundle" is `<state>/skill-bundles/probe-t28-other/`, made
by hand with 1 `SKILL.md`.

The launched `--settings` (read from the `claude` process arguments) add 3
entries for the state root's bundle folder, before the T-27 entries for
the pane's own bundle:

| key | entry |
|---|---|
| `sandbox.filesystem.denyWrite` | `<state>/skill-bundles` |
| `permissions.deny` | `Edit(//<state>/skill-bundles/**)`, `Write(//<state>/skill-bundles/**)` |

`sandbox.filesystem.allowWrite` still holds the state root and the ledger
folder.

#### Live pane `probe-t28-full-1` (pane `w29:p1M`)

`O` is the pane's own bundle, `X` is the other bundle.

| # | step | result | output |
|---|---|---|---|
| 1 | Read `O/skill-intake/SKILL.md` | allowed | `name: skill-intake` |
| 2 | `head -3 O/skill-intake/SKILL.md` | allowed | `---` |
| 3 | Edit `O/skill-intake/SKILL.md` | denied | `File is in a directory that is denied by your permission settings.` |
| 4 | Write `O/probe-written.txt` | denied | as step 3 |
| 5 | `echo probe > O/probe-echoed.txt` | denied | the auto mode classifier |
| 6 | Read `X/SKILL.md` | denied | `blockReadsOutsideWorkingDirectories` |
| 7 | Edit `X/SKILL.md` | denied | as step 3 |
| 8 | Write `X/probe-written.txt` | denied | as step 3 |
| 9 | `echo probe > X/probe-echoed.txt` | denied | the auto mode classifier |
| 10 | `horch tell opus-hardening-10 "..."` | exit 0 | the text reached the spawner pane |
| 11 | `horch ledger progress "..."` | exit 0 | none |
| 12 | `horch spawn smoke --exact` | exit 0 | `spawned smoke-3 (pane w29:p1N)` |
| 13 | `horch done "..."` | exit 0 | the pane closed; record status `done` |

Steps 10 to 12 also print a warning that the run's watchdog runs another
build of horch (the installed 0.4.3). Nothing was signalled.

The sweep stamp `<state>/.sweep-stamp` was set 2 hours back before the
pane started, so the sweep was due. After step 12 the stamp kept its old
time and the output had no `state sweep` line: a sandboxed caller skips
the sweep and writes no stamp (`sweep_state` in
`crates/horch/src/cmd/spawn.rs`).

#### Sandbox layer (`claude -p`)

In the live pane the auto mode classifier stops steps 5 and 9 first, as in
T-27. 2 headless `claude -p --permission-mode acceptEdits` runs used the
pane's launched `--settings` file, so no classifier ran:

| step | result | output |
|---|---|---|
| Edit `X/SKILL.md` | denied | `File is in a directory that is denied by your permission settings.` |
| Write `X/headless-written.txt` | denied | as Edit |
| `echo probe > X/headless-echoed.txt` | denied | `operation not permitted`, exit 1 |
| `rm -rf X` | denied | `rm: X/SKILL.md: Operation not permitted` |
| `echo probe > <state>/probe-t28-control.txt` | allowed | exit 0 |

The control shows that the rest of the state root stays writable. After
all runs, `X/SKILL.md` had its `name: probe-other` line and no probe file
was in `X` or `O`.

Claude Code 2.1.318 warns at start, for each `Write(...)` deny rule: `is
not matched by file permission checks — only Edit(path) rules are`. The
`Edit(...)` rule covers the Write tool (step 8 and the headless Write). The
`Write(...)` rules of T-27 and T-28 have no effect.

Cost: 1 sonnet low-effort pane, 1 `smoke` pane (no model) and 2 short
`claude -p` sonnet sessions. Results file:
`~/.local/state/skill-intake/drafts/probe/t28-full-results.txt`.

### Re-probe after ticket 31 (`allowRead` paths as working directories)

**Verdict: after the fix, a sandboxed auto-mode pane runs the T-17 command
with no prompt. The quarantine Read deny holds, a path outside the sandbox
lists stays closed to Read and Bash, and Edit and Write fail on an
`allowRead`-only path.**
Date: 2026-11-05. Claude Code 2.1.318, macOS (Seatbelt).

#### Cause

`overlay_sandbox` sets `permissions.blockReadsOutsideWorkingDirectories`.
Claude Code checks the path arguments of a Bash command against the working
directories, whatever `sandbox.filesystem.allowRead` says. A command that
names a path under `~/.local/state/skill-intake` stops at:
`Reads outside the working directories are blocked
(permissions.blockReadsOutsideWorkingDirectories). Add the directory with
/add-dir, or remove that setting. Do you want to proceed?`
A plain `cat <path>` did not stop; `python3 -I $S/scripts/intake.py ...
--task <path>` did.

#### Fix

`overlay_sandbox_dirs` in `crates/horch-core/src/harness/claude.rs` adds
each `filesystem.allowRead` and `allowWrite` path of the teammate's
`sandbox:` block, with `~` expanded, to `permissions.additionalDirectories`.
A path in `allowRead` and not in `allowWrite` also gets the deny rule
`Edit(//<path>/**)`, which covers the Write tool too (T-28 finding).
horch's own paths (state root, ledger folder, sockets) are not added. The
fix is for every sandboxed teammate, so the `_template.md` promise holds:
`allowRead` re-opens a path for Bash and for the Read tool.

#### Probe

The probe teammate `probe-t31` is a copy of `skill-importer` with
`hidden: true`, `model: sonnet`, `effort: low`, no `system_prompt` and no
`spawn_only`. Its `allowRead` adds `~/.local/state/probe-t31-ro`, a path
not in `allowWrite`. It ran from `.worktrees/_scratch/probe-t31/teammates`
with `--exact --run skill-intake`. Copies of the debug build before and
after the fix spawned it. Probe files:

- `C` = `~/.local/state/skill-intake/contaminated/probe-t31`, with
  `task.txt` and `quarantine/secret.txt`.
- `O` = `~/.local/state/probe-t31-outside/out.txt`, in no sandbox list.
- `R` = `~/.local/state/probe-t31-ro/ro.txt`, `allowRead` only.
- `S` = the pane's own bundle `skills/skill-intake` folder.

The launched `--settings` after the fix add, before the bundle entries:

| key | entries |
|---|---|
| `permissions.additionalDirectories` | `~/.local/state/skill-intake`, `~/.local/state/probe-t31-ro`, `~/.claude/skills` (expanded) |
| `permissions.deny` | `Edit(//<home>/.local/state/probe-t31-ro/**)` |

| # | step | before | after |
|---|---|---|---|
| 1 | Bash `S=...; python3 -I $S/scripts/intake.py spawn auditor --help --task C` | prompted (text above) | allowed, `usage: intake spawn ...` |
| 2 | Bash `cat C/task.txt` | allowed | allowed |
| 3 | Read `C/task.txt` | denied, `blockReadsOutsideWorkingDirectories` | allowed |
| 4 | Read `C/quarantine/secret.txt` | denied, `File is in a directory that is denied by your permission settings.` | denied, as before |
| 5 | Bash `cat O` | denied, `Operation not permitted` | denied, as before |
| 6 | Read `O` | denied, `blockReadsOutsideWorkingDirectories` | denied, as before |
| 7 | Bash `cat R` | allowed | allowed |
| 8 | Edit `R` | allowed (the file changed) | denied, `File is in a directory that is denied by your permission settings.` |
| 9 | Write `R/../new.txt` | allowed (the file was made) | denied, as step 8 |

Panes: `probe-t31-1` (before, steps 2 to 9), `probe-t31-3` (before, step 1;
the prompt was declined with Esc) and `probe-t31-2` (after, steps 1 to 9).
A headless `claude -p --permission-mode auto` run with the before pane's
launched `--settings` gave the same step 1 denial, and with the fix the
command ran.

Step 4 shows that the `Read(~/.local/state/skill-intake/contaminated/*/quarantine/**)`
deny of `skill-importer` wins over the new working directory. Steps 8 and 9
show a gap before this ticket: the Edit and Write tools are outside the OS
sandbox, so in auto mode they wrote an `allowRead`-only path. The new
`Edit(...)` rule closes it. After the probe, `R` holds its first text and
no probe file is in its folder.

Not closed: a `denyRead` path inside an `allowRead` path would be open to
the Read tool. No teammate has that shape today.

Cost: 3 sonnet low-effort panes and 2 short `claude -p` sonnet sessions.
Results files: `~/.local/state/skill-intake/drafts/probe/t31-before-results.txt`
and `t31-after-results.txt`.

### Re-probe after ticket 33 (git through the sandbox proxy)

**Verdict: fixed.** Date: 2026-11-05. Claude Code 2.1.318, macOS
(Seatbelt). In a sandboxed `skill-importer` pane, `intake`'s git failed with
`Proxy CONNECT aborted`. Plain `git` in the same pane worked.

#### Facts from the docs and the binary

- `~/.claude-code-docs/docs` (dated 2025-10-31) has no `allowedDomains`,
  `strictAllowlist` or `allowManagedDomainsOnly`. The facts below come from
  the schema text and code in the 2.1.318 binary.
- `strictAllowlist`: "When true, the sandbox runtime deterministically
  denies hosts not in allowedDomains instead of prompting." It applies to
  sandboxed commands only, not to WebFetch.
- With `strictAllowlist` (or an admin sandbox mandate), Claude Code takes
  `allowedDomains` only from policy, `--settings` (flagSettings) and user
  settings. It ignores entries from project and local settings. horch gives
  the list through `--settings`, so `github.com` stays allowed.
- The operator's `~/.claude/settings.json` and `settings.local.json` have
  no `sandbox` key. This host has no managed settings file and no
  `com.anthropic.claudecode` preferences domain.
- In the sandbox, the proxy URL in `HTTP(S)_PROXY` and `ALL_PROXY` carries
  per-session credentials. The sandbox also sets
  `GIT_CONFIG_PARAMETERS='http.proxyAuthMethod=basic' 'credential.http://127.0.0.1:<port>.helper='`.

#### Proven cause

`_git_env()` in `intakelib/acquire.py` keeps the proxy variables but drops
`GIT_CONFIG_PARAMETERS`. Without `http.proxyAuthMethod=basic`, git sends its
first CONNECT with no `Proxy-Authorization` and waits for a 407. The sandbox
proxy closes the connection instead, and git prints `Proxy CONNECT aborted`.
`strictAllowlist`, `allowUnixSockets` and the allowlist are not the cause:
plain `git` and `curl` to `github.com` pass with the same block.

1-change isolation (headless `claude -p --permission-mode auto` with the
same `sandbox` block as the pane):

| run | change | result |
|---|---|---|
| c | plain `git ls-remote` | exit 0, `de8494f3...` |
| c0 | c with `GIT_CONFIG_PARAMETERS` unset | `Proxy CONNECT aborted` |
| d | git with `_git_env()` | exit 128, `Proxy CONNECT aborted` |
| d1 | d with `GIT_CONFIG_PARAMETERS` added back | exit 0 |
| d2 | d with only `-c http.proxyAuthMethod=basic` | exit 0 |

#### Fix

`_git_config()` adds `-c http.proxyAuthMethod=basic`. `_git_env()` is not
changed: `GIT_CONFIG_PARAMETERS` can carry any git config, so it stays
out. No `*_API_KEY` reaches git. The domains not listed stay denied.
Test: `test_git_sends_proxy_credentials_on_the_first_connect` runs git
against a local proxy that closes a CONNECT with no credentials. It failed
before the fix with the live error text, and passes after it.

#### Probe

Pane `probe-t33-1`, a copy of `skill-importer` with `hidden: true`,
`model: sonnet`, `effort: low`, `skills: []`, no `system_prompt` and no
`spawn_only`. It ran from `.worktrees/_scratch/probe-t33/teammates` with
`--exact --run skill-intake`, spawned by a copy of the branch's debug build.
Its launched `sandbox.network` was the same as the failing pane's:
`allowUnixSockets` (state root, herdr socket), the 3 GitHub domains,
`strictAllowlist: true`. "before" and "after" are copies of `intakelib`
from before and after the fix, run in the same pane.

| step | command | before | after |
|---|---|---|---|
| 2a | `env \| grep -i -E 'proxy\|ssl\|cert'` | proxy variables and `GIT_CONFIG_PARAMETERS` set (credentials not recorded) | same |
| 2b | `curl https://github.com` | `200` | `200` |
| 2c | `git ls-remote https://github.com/MNICKZ/docs-driven-workflow` | exit 0, `de8494f3...` | exit 0 |
| 2d | the same with `_git_config()` and `_git_env()` | exit 128, `Proxy CONNECT aborted` | exit 0, `de8494f3...` |
| 2e | `curl https://example.com` | denied, `CONNECT tunnel failed, response 403`, `host is not on the allow list` | denied, as before |

A headless `claude -p --permission-mode auto` run with the same `sandbox`
block ran `intake acquire https://github.com/MNICKZ/docs-driven-workflow
--ref de8494f3b9b4c622f29a3dc0e6d8eb9162c7a8db` with the fixed scripts
and no prompt. It exited 0, and the provenance records commit `de8494f3...`.

Cost: 1 sonnet low-effort pane and 5 short `claude -p` sonnet sessions.

### Ticket 34 (each role's status file is inside its sandbox)

T-17 live, task `20261105-133523-1d17be`: `intake-analyst-1` wrote its
spec under `drafts/`, but "Status file write is denied". The analyst
status file was `contaminated/<task>/status/analyst-<n>.json`. The
`intake-analyst` sandbox writes only `~/.local/state/skill-intake/drafts`.

#### Paths per role before the fix

`B` is `~/.local/state/skill-intake`. `<t>` is the task id.

| role | brief | status file | `_io` write paths | allowRead | allowWrite |
|---|---|---|---|---|---|
| auditor | `B/contaminated/<t>/briefs/` | `B/contaminated/<t>/status/` | `C/sweep/audit.jsonl`, `C/sweep/opaque` | contaminated | contaminated |
| analyst | `B/contaminated/<t>/briefs/` | `B/contaminated/<t>/status/` (denied) | `D/drafts` | contaminated, drafts | drafts |
| sanitizer | `B/drafts/<t>/briefs/` | `B/drafts/<t>/status/` | `D/drafts`, `D/quarantine-cr` | drafts | drafts |
| writer | `B/clean/<t>/briefs/` | `B/clean/<t>/status/` | `K/out`, `K/report.json`, `K/persona.md` | clean | clean |
| reviewer | `B/clean/<t>/briefs/` | `B/clean/<t>/status/` | `K/out`, `K/review.json` | clean | clean |

Only the analyst status file was outside its allowWrite. The domain
write set had the same error: `"C/status/analyst-*.json"`.

#### Fix

- `roles.STATUS_SIDE` puts each status file in the side that the role
  writes. The analyst status file is `D/status/analyst-<n>.json`. The
  analyst brief stays in `C/briefs/`.
- `domain.WRITE_SETS["analyst"]` names `"D/status/analyst-*.json"`.

#### Test

`TeammateSandboxTest` in `scripts/skill-intake/tests/test_roles.py`
reads the real `teammates/intake-<role>.md` sandbox blocks, with `~` set
to the test HOME. For each of the 5 roles it checks:

- the brief and every `_io` read path are inside allowRead and outside
  denyRead;
- the status file and every `_io` write path are inside allowWrite;
- the status file matches the role's `domain.WRITE_SETS` patterns, and
  each pattern's side root is inside allowWrite.

Before the fix the test failed for the analyst only. After the fix it
passes for all 5 roles.

### Re-probe after ticket 35 (a sandboxed `full` pane reads its roster)

**Verdict: fixed.** Date: 2026-11-05. Claude Code 2.1.318, macOS
(Seatbelt). T-17 live, pane `skill-importer-5`, task
`20261105-133927-c967a7`: `intake spawn auditor` ran `horch spawn`, which
exited 1 with `reading /Users/matthewscott/repo/multi-herdr/teammates/_base:
Operation not permitted (os error 1)`.

#### Cause

The brief carries `teammates_dir`, and the pane gets `HORCH_TEAMMATES_DIR`.
`overlay_horch_paths` opened only the state root, the ledger folder and the
sockets to a `full` pane. With `blockReadsOutsideWorkingDirectories`, Bash
could not read a roster folder outside the pane's working directories.

#### Fix

Commit `0b69bb56`. `overlay_roster_dirs` in
`crates/horch-core/src/harness/claude.rs` runs for a sandboxed `full`
pane. For `$HORCH_TEAMMATES_DIR` and `~/.config/horch/teammates`, each when
it is a folder, it adds:

- `sandbox.filesystem.allowRead` and `permissions.additionalDirectories`
  (read, ticket 31 rule);
- `sandbox.filesystem.denyWrite` (Bash cannot write, R6);
- the deny rule `Edit(//<dir>/**)` (Edit and Write tools). No `Write(...)`
  rule: it has no effect (T-28 finding).

`LaunchEnv.roster_dir` is the `HORCH_TEAMMATES_DIR` of the agent: the
briefed value in `child_env`, else the value of the launching process. A
`report` pane gets nothing new. Tests:
`a_full_sandboxed_launch_reads_but_does_not_write_its_roster` (red before
the fix) and `only_a_full_launch_with_a_roster_opens_one`.

#### Probe

The probe teammate `probe-t35` is a copy of `skill-importer` with
`hidden: true`, `model: sonnet`, `effort: low`, `skills: []`, no
`system_prompt` and no `spawn_only`. `R` = `~/.local/state/probe-t35-roster`,
the probe roster (`probe-t35.md` and an empty `_base/`), outside every
sandbox list of the teammate. Copies of the debug build before and after the
fix spawned it with `HORCH_TEAMMATES_DIR=R --exact`, outside a ledger run.
The `horch` in the pane is the installed 0.4.3. The launched `--settings`
after the fix add `R` to `allowRead`, `denyWrite` and
`additionalDirectories`, and add `Edit(//<home>/.local/state/probe-t35-roster/**)`.

| # | step | before (`probe-t35-before`) | after (`probe-t35-after`) |
|---|---|---|---|
| 1 | `echo $HORCH_TEAMMATES_DIR` | `R` | `R` |
| 2 | `horch teammates` | exit 1, `reading R/_base: Operation not permitted (os error 1)` | exit 0 |
| 3 | `horch spawn smoke --exact --no-tile ...` | exit 1, as step 2 | exit 0, `spawned smoke-4` |
| 4 | Edit `R/probe-t35.md` | not run | denied, `File is in a directory that is denied by your permission settings.` |
| 5 | Write `R/probe-written.txt` | not run | denied, as step 4 |
| 6 | `echo probe > R/probe-echoed.txt` | not run | denied by the auto mode classifier |
| 7 | `horch tell orchestrator ...` | not run | exit 0 |
| 8 | `horch note ...` | not run | exit 0 |
| 9 | `horch ledger progress ...` | not run | exit 1, `HORCH_WORKER_ID is not set` (no ledger run, not the sandbox) |

Steps 6 and 9 did not reach the sandbox. A headless `claude -p` run with the
after pane's launched `--settings` and pre-approved Bash rules checked them
at the OS layer:

| command | result |
|---|---|
| `echo probe > R/headless-echoed.txt` | denied, `operation not permitted`, exit 1 |
| `horch ledger progress ...` (worker id of `opus-hardening-14`) | exit 0 |
| `cat R/probe-t35.md` | exit 0 |

After the probe, `R/probe-t35.md` holds its first text and `R` has no probe
file. Every `horch` command in the pane printed a warning that the
`skill-intake` watchdog runs another build of horch.

Cost: 2 sonnet low-effort panes, 1 `smoke` pane (no tokens) and 1 short
`claude -p` sonnet session. Results files:
`~/.local/state/skill-intake/drafts/probe/t35-before-results.txt` and
`t35-after-results.txt`.
