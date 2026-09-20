# multi-herdr

"horch" multi-agent orchestration layouts for [herdr](https://herdr.dev), in Rust.

Three binaries, no runtime dependencies beyond herdr itself. No bash, no jq, no
Node required by horch itself. It runs on macOS, Linux and Windows; Codex phase
skills currently require macOS/Linux or WSL. A `justfile` at
the repo root wraps the common ones for people who like typing `just
herdr-fleet`, but it's optional sugar over `horch`, not a dependency.

| binary            | what it does                                              |
|-------------------|-----------------------------------------------------------|
| `horch`           | Build and drive multi-agent herdr workspaces              |
| `herdr-install`   | Install the latest Herdr CLI                              |
| `herdr-docs-sync` | Mirror the Herdr docs into `herdr-docs/`                  |

## Getting started

Build and put the binaries on your PATH:

```bash
cargo build --release
./target/release/herdr-install          # if you do not have herdr yet
./target/release/horch install           # copies horch to ~/.local/bin
```

`horch install` prints what to add to your PATH if the directory is not already
on it. On Windows the default install directory is
`%LOCALAPPDATA%\Programs\horch`.

Then check your setup and verify the machinery end to end:

```bash
horch doctor            # is herdr installed and its server reachable?
horch smoke messaging   # 2-pane check of the messaging primitives, no agents
horch smoke fleet       # full spawn -> ledger -> report -> self-close check
```

Both smoke checks spend no LLM tokens and clean up after themselves. They leave
the scratch workspace open when they fail, so you can look at it.

Day to day: `just install` once, then `herdr-fleet` from any project.
`just update` pulls and reinstalls.

## Running a fleet

```bash
cd ~/your-project
horch fleet            # Claude Code on Fable orchestrates (the default)
horch fleet cc         # the same, said out loud
horch fleet codex      # Codex on Astra orchestrates instead
```

You get one orchestrator pane and nothing else. It reads the roster, breaks the
work down, and spawns exactly the workers each piece needs - then shuts them down
as they finish. No worker is started before there is a job for it.

The flavor picks who orchestrates and nothing else. Both read the same roster
and spawn the same workers, so a Codex orchestrator still reaches for `opus`
when a task wants Claude, and a Claude one still reaches for `codex-sol`.

### One top-tier session per fleet

Fable and Astra are reserved for the orchestrator. `horch spawn` refuses to
start a worker on either tier, whichever flavor is orchestrating - so a Fable
orchestrator cannot start an Astra, an Astra cannot start a Fable, and neither
can clone itself. The rule is matched on the tier name rather than an exact
model slug, so a version bump stays reserved without an edit.

That is why every briefing is written for an Opus or Codex-Sol reader: the
orchestrator does the reasoning that needs the top tier, writes the result to a
file, and hands the execution down. `horch teammates --check` fails any teammate
in the roster that asks for a reserved tier, so the rule cannot be broken by
adding a file.

Everything needs a running herdr server: launch the herdr app, or run
`herdr server` headless.

`horch orchestration` is the simpler alternative: a fixed 5-pane workspace with
no ledger and no dynamic spawning.

### How the panes talk

There is no AI-to-AI protocol here. A message is literally typed into the target
pane's terminal and submitted, as if you switched panes and typed it by hand.

| command                            | who runs it  | what it does                          |
|------------------------------------|--------------|---------------------------------------|
| `horch tell <role> "<message>"`    | anyone       | Type a message into another pane      |
| `horch inbox`                      | anyone       | List reachable roles                  |
| `horch assign <role> "<task>"`     | orchestrator | Record a task on the ledger, then send it |
| `horch spawn <teammate> "<task>"`  | orchestrator | New pane, new agent session           |
| `horch spawn --resume <id> "<task>"` | orchestrator | New pane, resuming an old session   |
| `horch sessions`                   | orchestrator | Read the project session ledger       |
| `horch layout`                     | orchestrator | Report the grid of every tab          |
| `horch tile`                       | orchestrator | Rearrange every pane into the grid    |
| `horch balance`                    | orchestrator | Make every worker column the same width |
| `horch note "<update>"`            | worker       | Record progress on its ledger record  |
| `horch done "<summary>"`           | worker       | Record a summary, report, close its pane |

`horch tile` puts every pane where it belongs: the orchestrator full height down
the left of tab 1, four workers beside it in a 2x2, and six per overflow tab in a
2x3, filling a column top then bottom before opening the next one. `horch spawn`
and `horch done` run it themselves, so the grid lays itself out after every spawn
and every worker that finishes, and neither you nor the orchestrator ever chooses
a pane or a direction; `horch tile --plan` shows what it would move without
moving anything, and `HORCH_TILE=0` (or `horch spawn --no-tile`) switches the
automatic part off.

Panes are moved, never recreated: `herdr pane move` keeps the pane id and the
process, so an agent mid-task is only resized. Tiling also evens the columns out,
which is what `horch balance` does on its own - needed by hand only after you
drag a border yourself when the automatic tiling is off. It is not cosmetic: a
split halves its parent while a departing worker hands its width to one
neighbour, so left alone the two rows stop agreeing on where the columns are.

### Teammates

Every agent this fleet launches is described by one markdown file in
[`teammates/`](teammates/). The file carries the launch settings in its
frontmatter - agent, model, effort, permission mode, tools - and its body is the
prompt. **Adding a file adds someone the orchestrator can pick**; there is no
registry to update.

| teammate               | agent  | model  | for                                          |
|------------------------|--------|--------|----------------------------------------------|
| `product-lead`         | Claude | Opus   | Scope, priorities, acceptance criteria       |
| `researcher`           | Claude | Opus   | R&D; investigating unfamiliar code and prior art |
| `staff-engineer`       | Claude | Opus   | Deep implementation plans a junior can execute |
| `designer`             | Claude | Opus   | UX flows, states, copy, accessibility        |
| `architect-reviewer`   | Claude | Opus   | Boundaries, contracts, coupling. Read-only   |
| `frontend-developer`   | Claude | Opus   | UI work, with a real browser to check it in  |
| `backend-developer`    | Claude | Opus   | JS/TS, Python, Go, C#, Rust services and APIs |
| `qa-engineer`          | Claude | Sonnet | Tests, e2e harnesses, bug reproduction       |
| `sonnet` `opus` `codex-sol` `codex-terra` | | | Generic fallbacks |
| `opencode-ultra` `opencode-pickle` `opencode-lightning` | OpenCode | free tier | Free workers, for public/OSS work |
| `pi`                   | pi     | local  | Runs on your own hardware; nothing leaves the machine |
| `prime`                | Prime Agent | Opus | One persistent Python kernel; long, exploratory runs |

The two orchestrators are teammate files too, and hidden from the roster:
`orchestrator` (Claude, Fable) and `orchestrator-codex` (Codex, Astra). They
share one briefing in `teammates/_base/fleet-orchestrator.md`, and each file
holds only the paragraph naming the tier it is the only session of.

The last row are the *generic fallbacks*, used when no specialist matches. The
two prompts the `orchestration` recipe uses are teammate files as well.

### Starting agents clean

A fresh `claude` inherits every globally-enabled plugin, skill and MCP server.
For a worker with one narrow job that is context bloat, and it is where a
persona gets quietly overruled - an installed skill arrives as an instruction
and beats prompt text.

The principle is subtractive and narrow: **keep the operator's `settings.json`**
(it is already tuned for token economy - `disableWorkflows`,
`disableBundledSkills`, the status line) and switch off only the two things a
fleet worker has no use for.

| field | what it does | how |
|---|---|---|
| `inherit_plugins: false` | switches the operator's globally-enabled plugins off, by name, for this session | `--settings '{"enabledPlugins":{...:false}}'` - merges per key, touches nothing else |
| `mcp_servers: {}` | exactly zero MCP servers | `--mcp-config '{"mcpServers":{}}' --strict-mcp-config` |
| `disable_skills: true` | no slash commands at all - including the built-in `/context` and `/config`, so nothing shipped uses it | `--disable-slash-commands` |

Bundled teammates now use a repo-owned skill catalog instead of machine-local
plugin paths. Each launch gets a small skills-only plugin; bundled Claude skills
and Workflows are disabled by default for these launches. Custom `plugin_dirs`
remain supported. Permission denials, provider settings and the status line are
preserved. See [phase skills and context measurement](docs/phase-skills.md).

`setting_sources: []` (`--setting-sources ""`) also exists, and is the blunt
instrument: it drops the operator's tuning along with the plugins and usually
costs more context than it saves. `horch` keeps the status line alive through it
regardless, since a pane without one is blind on context and cost.

### Skills for each phase

```sh
horch skills --phase research --json
horch spawn codex-sol --phase research "Investigate the migration"
horch spawn sonnet --phase implementation "Implement the accepted plan"
horch spawn --resume RECORD_ID --phase validation "Check the final diff"
```

Research, plan, implementation and validation select portable native skill
catalogs across all five harnesses. Role defaults and explicit `skills` live in
teammate YAML; resumes preserve the phase. Full workflows load on demand.
See [the phase catalog, adapters and context measurements](docs/phase-skills.md).

### Five harnesses, one roster

A teammate's `agent:` picks which CLI its pane runs. They differ in almost
everything - how a prompt is passed, whether horch can name the session before
launch, what "run unattended" means - so `launch.rs` has one builder per agent
and `Agent` answers the rest as capabilities rather than as `if agent == ...`
scattered through the code.

| agent | prompt | model | effort | session |
|---|---|---|---|---|
| `claude`   | trailing positional | `--model` alias | `--effort` | horch mints `--session-id` |
| `codex`    | trailing positional | `-c model="..."` | `-c model_reasoning_effort` | harvested from rollout files |
| `opencode` | `--prompt` flag | `-m provider/model` | `--variant` | harvested from `opencode session list --format json` |
| `pi`       | after `--` | `--model provider/id` | `--thinking` | horch mints `--session-id` |
| `prime`    | after `--` | `--model provider/id` | `--thinking` | horch owns the `--session-dir` and reads it back |

Two of those need more than flags:

**OpenCode** needs `--auto`, or a worker stops at the first permission prompt in
a pane nobody is watching. `permission_mode: plan` has no OpenCode equivalent at
all, so it is refused rather than quietly downgraded.

**Prime Agent supervises its own sessions.** Verified on 0.9.4: even a `--print`
run that failed authentication left a background service alive after the process
exited. herdr already treats a pane as an agent's lifetime, so left alone
`horch done` would close the pane and leave the daemon running - one per spawn,
forever. Every Prime pane therefore gets its own `--daemon-socket`, and horch
stops exactly that service when the CLI exits. `prime-agent shutdown` is not
usable here: it stops every agent on the machine, including other panes in the
same fleet.

pi and Prime have no approval gate at all - their tools simply run, which is what
makes them usable unattended. `permission_mode` is refused on both, because
setting it would imply a restraint that does not exist.

### Free models are paid for with your prompts

The `opencode-*` tiers cost nothing because the provider trains on what it is
sent. That is a fine trade for public and open-source work and a bad one for
anything else, so it is written down in two places rather than assumed:

- their `brief_description` says `TRAINS ON YOUR INPUT`, which is the line the
  **orchestrator** reads when it decides who gets a task;
- `trains_on_input: true` in the teammate file appends a block from
  `_base/fleet-worker.md` to the **worker's own** briefing, telling it to treat
  the pane as public and to report `BLOCKED` rather than read anything
  proprietary into it.

One field, one copy of the prose, and both ends of the decision covered. Set it
on any teammate whose provider trains on input; leave it off for paid, local and
self-hosted models, because a warning that appears everywhere stops being read.

### Starting codex panes clean

Every Codex launch disables startup update checks with
`-c check_for_update_on_startup=false`, selects the current fleet directory on
resume with `-c 'tui.resume_cwd="current"'`, and passes
`--dangerously-bypass-hook-trust`. That last flag runs enabled hooks without a
trust dialog, so use hooks whose sources you have vetted; it does not grant
shell commands extra permissions. These are per-launch settings, not edits to
your global Codex configuration. Verified with Codex CLI 0.154.0; see the
[official CLI reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli)
and [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference).

The bundled Codex workers and orchestrator use `permission_mode: auto`, mapped
to `-s workspace-write -a never`. Commands allowed by the sandbox or explicit
rules can run; actions needing approval fail back to the agent instead of
waiting for a person. This preserves sandbox restrictions. It does not use
`--dangerously-bypass-approvals-and-sandbox`. Custom teammates can still choose
an interactive permission mode. Authentication and any required first-run
setup must already be complete before launching unattended panes.

Codex has no `--tools` or `--settings`; what it can reach outside its sandbox is
decided by execpolicy `prefix_rule`s, and it refuses any command its rules do not
name. So a codex worker needs `horch note` and `horch done` allowed, and a codex
orchestrator needs `horch spawn`, `assign`, `tell`, `inbox`, `sessions` and
`layout` - different sets, and neither should get the other's.

Those rules load from `$CODEX_HOME/rules/*.rules`, and codex has no flag or
config key that points one launch at a different set. Appending to the shared
`~/.codex/rules/default.rules` would therefore grant every codex session on the
machine whatever any fleet ever needed, permanently - the file accumulates and
nothing ever prunes it.

So each codex pane gets a **private `CODEX_HOME`** instead: a directory that
symlinks every entry of the real one except `rules/`, and whose `rules/` holds
that launch's rules and nothing else. Auth, config, plugins and
`sessions/` resolve through the symlinks; the skills link points to the selected
fleet bundle. Other native skill discovery roots can still contribute skills.
Rollouts still land where the ledger's harvest looks for them. The directory
is rebuilt per launch and removed when the pane exits, so two panes in one fleet
can hold different rules and neither leaks to the other - or to the operator's
own codex sessions.

If codex creates something at the top level of that private home that was not
there at launch, the directory is kept and named rather than deleted, so state
is never silently thrown away. On Windows the rules go to the shared file
instead for custom teammates without phase skills. Phase-enabled Codex requires
WSL on Windows and fails before shared rules are written; see the compatibility
notes in [phase skills](docs/phase-skills.md).

### The session ledger

Every worker session is recorded in a per-project JSON ledger, so "what has been
worked on here, by which session" survives pane close and workspace restarts.
That is what lets the orchestrator resume an old session instead of starting
cold. Workers close their own pane when done, but their sessions stay resumable.

```bash
horch sessions              # readable summary, newest first
horch sessions --json       # the raw records
horch ledger path           # where the file lives
```

The ledger lives at `$HORCH_STATE_DIR`, else
`${XDG_STATE_HOME:-~/.local/state}/horch/<slugified-project-path>.json`. The
format is unchanged from the previous bash implementation, so existing ledgers
stay readable and their sessions stay resumable.

## Keeping the docs current

`herdr-docs/` is a mirror of <https://herdr.dev/docs/>. Refresh it any time:

```bash
herdr-docs-sync              # writes to ./herdr-docs
herdr-docs-sync --dry-run    # report what would change
```

It discovers pages from the site's sitemap, converts each to markdown, and
removes local files for pages that no longer exist, so the directory stays a
faithful mirror. If any page fails, nothing is deleted that run - a partial
picture is not a safe basis for deleting documentation.

## Installing Herdr

```bash
herdr-install                # latest stable (preview on Windows)
herdr-install --dry-run      # report what would be installed
herdr-install --channel preview
```

It reads the same release manifests as `herdr update`, so installs and updates
agree on what "latest" means, and verifies the published sha256 when the channel
provides one.

On macOS and Linux it writes one binary to `~/.local/bin/herdr`. On Windows it
uses the preview channel - the only one publishing a Windows asset while native
Windows support is in beta - and unpacks the release with its ConPTY runtime into
a versioned directory, pointing `current` and the PATH directory at it by
junction, so an update never overwrites a running `herdr.exe`.

## Environment

| variable             | effect                                                        |
|----------------------|---------------------------------------------------------------|
| `HORCH_CLAUDE_BIN`   | Which Claude CLI to launch. Defaults to `cpx` if on PATH, else `claude` |
| `HORCH_CODEX_BIN`    | Which Codex CLI to launch. Defaults to `codex`                 |
| `HORCH_OPENCODE_BIN` | Which OpenCode CLI to launch. Defaults to `opencode`           |
| `HORCH_PI_BIN`       | Which pi CLI to launch. Defaults to `pi`                       |
| `HORCH_PRIME_BIN`    | Which Prime Agent CLI to launch. Defaults to `prime-agent`     |
| `HORCH_STATE_DIR`    | Where ledgers live                                             |
| `HORCH_PROJECT_DIR`  | Which project a ledger belongs to. Defaults to the cwd         |
| `HORCH_WORKSPACE_ID` | Target workspace, when not running inside a herdr pane         |
| `CODEX_HOME`         | The codex home a private one is mirrored from. Defaults to `~/.codex` |
| `HERDR_INSTALL_DIR`  | Where `herdr-install` puts herdr                               |

## Layout

```
crates/
  horch-core/         Shared machinery: herdr client, mailbox, ledger, layout,
                      prompts, codex glue, pane-shell quoting
  horch/              The horch CLI
  herdr-install/      The Herdr CLI installer
  herdr-docs-sync/    The documentation mirror
herdr-docs/           The mirrored documentation
```

## Tests

```bash
cargo test              # unit tests, no herdr server needed
horch smoke messaging   # against a live herdr server
horch smoke fleet
horch smoke tile
```

The smoke checks are the acceptance tests: they exercise the real herdr socket
API, the mailbox, the ledger and pane self-close, and they are what to run after
a herdr upgrade.
