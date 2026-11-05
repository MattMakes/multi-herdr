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
