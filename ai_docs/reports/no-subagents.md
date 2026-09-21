# No subagents in a fleet pane

A fleet pane must not start a subagent. A worker that needs more hands sends
`QUESTION:` to the orchestrator. The orchestrator splits the work and spawns.

This report records the evidence for each harness switch. Date: 2026-09-20.
Machine: darwin 25.5.0.

## Step 1. Claude, verified

Version: `claude --version` -> `2.1.278 (Claude Code)`.

Every command ran from `/Users/mascott/projects/multi-herdr`. Every command
carries the prefix `env -u ANTHROPIC_API_KEY`. The key in this shell is
invalid and returns 401.

### Test A. `--disallowedTools Agent`

```
claude --disallowedTools Agent -p 'Use the Agent tool to list the files in this directory. If you cannot, say exactly: AGENT TOOL UNAVAILABLE.' --max-turns 2
```

Output:

```
AGENT TOOL UNAVAILABLE.

The Agent tool is not in this session's tool set, and a search of the deferred
tools found no match. I can list the directory directly with Bash if you want
that instead.
```

### Test B. No flag

```
claude -p 'Use the Agent tool to list the files in this directory. If you cannot, say exactly: AGENT TOOL UNAVAILABLE.' --max-turns 2
```

Output (extract): the session listed the directory and did not say
`AGENT TOOL UNAVAILABLE`.

```
drwxr-xr-x    3 mascott  staff     96 Sep  7 16:53 .claude
...
drwxr-xr-x   27 mascott  staff    864 Sep 19 17:04 teammates
```

### Test C. `--disallowedTools Task`

```
claude --disallowedTools Task -p 'Use the Task tool to list the files in this directory. If you cannot, say exactly: TASK TOOL UNAVAILABLE.' --max-turns 2
```

Output:

```
TASK TOOL UNAVAILABLE.

No Task tool exists in this session, either loaded or deferred.
```

### Test D. No flag, `Task`

```
claude -p 'Use the Task tool to list the files in this directory. If you cannot, say exactly: TASK TOOL UNAVAILABLE.' --max-turns 2
```

Output:

```
TASK TOOL UNAVAILABLE
```

Test D is the control. `Task` is absent with no flag set. The name does not
exist on 2.1.278.

### Test E and Test F. The tool set, listed

```
claude -p 'List the names of every tool you have, comma-separated, nothing else.' --max-turns 1
```

Output (Test E, no flag):

```
Agent, Bash, Edit, ListAgents, Read, ReportFindings, ScheduleWakeup, Skill,
ToolSearch, Write, advisor, CronCreate, CronDelete, CronList, DesignSync,
EnterWorktree, ExitWorktree, LSP, Monitor, PushNotification, RemoteTrigger,
SendMessage, TaskStop, WebFetch, WebSearch
```

Output (Test F, with `--disallowedTools Agent`):

```
Bash, Edit, ListAgents, Read, ReportFindings, ScheduleWakeup, Skill,
ToolSearch, Write, advisor, CronCreate, CronDelete, CronList, DesignSync,
EnterWorktree, ExitWorktree, LSP, Monitor, PushNotification, RemoteTrigger,
SendMessage, TaskStop, WebFetch, WebSearch
```

Result: `--disallowedTools Agent` removes `Agent` from the tool set. The flag
does not leave the tool in place behind a permission prompt. No other tool
name changes.

### Decision

The deny list is `disallowed_tools: [Agent]`. `Task` is not added. The name
does not exist on this version. Test D proves it.

### `CLAUDE_CODE_FORK_SUBAGENT`: unverified, not applied

`ai_docs/plans_to_improve.md:214` names this variable.
`ai_docs/reports/env-research/claude-code.md` does not mention it.
`claude --help` does not list it.

The name is present in the bundle at
`/Users/mascott/.local/share/claude/versions/2.1.278`:

```
var Ipr="tengu_fork_subagent_enabled";
function Opr(){if(a.CLAUDE_CODE_FORK_SUBAGENT===!0)return"env";if(Ce())return"disabled";return"default"}
function Dpr(){let e=ps();if(Pq())return"disabled";if(a.CLAUDE_CODE_FORK_SUBAGENT===!1)return"disabled";...}
```

The variable gates the gate named `tengu_fork_subagent_enabled`. A false value
returns `"disabled"`.

Test G and Test H asked a session to quote the `fork` text in its `Agent` tool
schema, first with no variable and then with
`CLAUDE_CODE_FORK_SUBAGENT=false`. Both sessions reported the same text. The
test showed no difference.

Status: unverified, not applied. No teammate file gets an `env:` block for it.

This costs nothing. `fork` on 2.1.278 is a `subagent_type` value of the `Agent`
tool, not a separate tool. Test F removes `Agent`, so it removes `fork` too.

### Out of scope, recorded for the operator

Test E and Test F show that `ListAgents`, `SendMessage` and `TaskStop` stay
after the deny. These 3 tools address agents that already exist. They do not
start one. The fleet uses this path on purpose. `Monitor`, `CronCreate` and
`ScheduleWakeup` schedule work in the same session. They do not start an agent.

## Step 2. The other harnesses

| Harness | Feature that starts a nested agent | Off switch | How a teammate passes it | State |
|---|---|---|---|---|
| Claude 2.1.278 | `Agent` tool | `--disallowedTools Agent` | `disallowed_tools: [Agent]` | Verified, applied |
| Codex 0.155.1 | feature `multi_agent`, stable, on by default | `-c features.multi_agent=false` | `args: ["-c", "features.multi_agent=false"]` | Verified, applied |
| OpenCode 1.18.2 | subagents `explore` and `general`, reached by the `task` tool | config file key `tools.task: false` | none; horch forwards no OpenCode config file | Verified feature, no switch horch forwards. Not applied |
| pi | none | not applicable | not applicable | Verified none |
| Prime | unknown | unknown | unknown | Not installed. Unverified, not applied |

### Codex 0.155.1, verified

`codex --version` -> `codex-cli 0.155.1`.

`codex features list` prints 142 features. The agent-shaped ones:

```
collaboration_modes                      removed            true
enable_fanout                            removed            false
external_agent_memory_import             under development  false
multi_agent                              stable             true
multi_agent_mode                         removed            false
multi_agent_v2                           stable             false
use_agent_identity                       under development  false
```

`multi_agent` is stable and is on by default. Both off switches flip the
effective state:

```
$ codex features list --disable multi_agent | grep '^multi_agent '
multi_agent                              stable             false
$ codex features list -c features.multi_agent=false | grep '^multi_agent '
multi_agent                              stable             false
```

`codex --help` documents the equivalence: `--disable <FEATURE>` is
"Equivalent to `-c features.<name>=true`", inverted.

WARNING: a codex teammate cannot use `disallowed_tools`.
`Agent::takes_tool_lists` at `crates/horch-core/src/teammates.rs:173` excludes
`Agent::Codex`, so the roster check rejects the field by name. The switch goes
in `args:`.

`codex_command` in `crates/horch-core/src/launch.rs` already passes
`-c key=value` pairs and then appends `teammate.args` verbatim. The `-c` form
is chosen over `--disable` to match that style.

Limit: `codex exec` returned "You've hit your usage limit" on this machine on
2026-09-20. The live tool list is not in this report. The switch is verified
through the effective-state output above, which is the state the feature gate
reads.

### OpenCode 1.18.2, feature verified, switch not available

`opencode agent list` names 2 subagents:

```
explore (subagent)
general (subagent)
```

`opencode --help` lists every flag of the default command. No flag disables a
tool. The flags are `--model`, `--variant`, `--agent`, `--pure`, `--session`,
`--prompt`, `--auto`, `--mini` and the server flags.

OpenCode disables a tool through its config file, not through a flag.
`opencode_command` in `crates/horch-core/src/launch.rs` passes flags only. It
forwards no config file. A teammate file therefore has no field to carry this
switch.

Status: not applied. The prose rule in `teammates/_base/fleet-worker.md` is
the only control on an OpenCode pane today. A later task can add a config-file
field to the OpenCode launch path.

### pi, verified none

`pi --help` crashes on this machine and prints a stack trace. The bundle at
`/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist` carries
the tool registry. It names 4 tools:

```
name:"bash"
name:"edit"
name:"read"
name:"write"
```

A search for `spawn_agent`, `subagent`, `sub_agent` and `dispatch_agent`
returned nothing. pi has no tool that starts a nested agent. `--exclude-tools`
is present in the bundle and is what `disallowed_tools` already renders for pi
at `crates/horch-core/src/launch.rs:172-175`. There is nothing to exclude.

### Prime, unverified

Prime is not installed on this machine. `which prime` fails.
`crates/horch-core/src/launch.rs:165-171` records that Prime 0.9.4 has
`--tools` and `--no-tools` and no `--exclude-tools`, so
`Agent::takes_tool_denylist` excludes it. No subagent feature is confirmed or
refuted.

Status: unverified, not applied.

## The enforcement

Prose alone did not hold. The rule now has 2 further layers:

1. The 11 Claude teammates that the orchestrator can spawn, and the
   orchestrator itself, carry `disallowed_tools: [Agent]`. The 2 Codex
   teammates and the Codex orchestrator carry
   `args: ["-c", "features.multi_agent=false"]`.
2. `Roster::check` in `crates/horch-core/src/teammates.rs` fails a roster
   whose `agent: claude` teammate lacks the deny. `horch teammates --check`
   runs it. The escape hatch is `allow_subagents: true`, which no shipped
   teammate sets.
