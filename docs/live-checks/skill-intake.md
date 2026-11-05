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
