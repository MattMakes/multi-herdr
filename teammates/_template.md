---
# ─── identity ────────────────────────────────────────────────────────────────
# `name` must equal the filename without .md. It is the id used by
# `horch spawn <name>`, stored in the ledger, and shown in `horch inbox`.
name: my-teammate

# THE ONLY FIELD THE ORCHESTRATOR READS WHEN CHOOSING A TEAMMATE.
# One line, <= 120 characters. Every teammate's brief_description is
# concatenated into the orchestrator's briefing, so this is a direct tax on the
# orchestrator's context: N teammates = N lines it carries for the whole run.
# Write it as a selection cue ("when would I reach for this?"), not a résumé.
brief_description: One line, <=120 chars, describing when to pick this teammate.

# true for the fallbacks (sonnet, opus, codex-sol, codex-terra).
# Specialists leave this out. The roster renders specialists first, generics
# second, so the orchestrator reads "match, else fall back".
generic: false

# true = loaded and spawnable by name, but never offered in the roster.
# For fixtures and machinery (see smoke.md), so they cost the orchestrator
# nothing. Distinct from a leading `_`, which means "not parsed at all".
hidden: false

# Name globs (`*` and `?`, case-sensitive). When set, the orchestrator is
# offered this teammate only when the fleet's project has a matching file or
# directory at the top level or one level down; any one match is enough.
# `horch spawn <name>` still works in any project. Leave it out (the default)
# and the teammate is offered everywhere. For domain teammates, e.g.
#   offer_when: ["*.uproject"]
#   offer_when: ["*.xcodeproj", "*.xcworkspace", "Package.swift"]
offer_when: []

# Host tools this teammate cannot work without. `horch doctor` checks each one
# when the project is offered this teammate. Values: xcode (`xcodebuild` on
# PATH, with `sudo xcodebuild -runFirstLaunch` done), blender (`blender` on
# PATH or `BLENDER_PATH`, and `blender --version` runs).
requires: []

# ─── inherited base prompt ───────────────────────────────────────────────────
# Names a file in _base/. The base carries the orchestrator-facing protocol -
# horch tell / horch note / horch done, the blocked-and-wait rule, the
# lifecycle. Every teammate inherits it so no persona can accidentally omit
# the only channel the worker has. The persona below is substituted into the
# base at {persona}; the task/resume paragraph at {task_briefing}.
base: fleet-worker

# ─── which CLI, which model ──────────────────────────────────────────────────
# agent: claude | codex | opencode | pi | prime | antigravity | none
#   ("none" = the smoke fake, spends no tokens)
# Session handling is DERIVED from agent and is not configurable here:
#   claude   -> horch mints the session id, passes --session-id / --resume
#   pi       -> same, via pi's --session-id ("create it if missing")
#   codex    -> codex mints its own id, horch harvests it from the rollout file
#   opencode -> harvested from `opencode session list --format json`
#   prime    -> no --session-id at all; horch gives it a --session-dir it owns
#               and reads back the session that appears there
#   antigravity -> agy mints its own id; horch reads it back from
#               ~/.gemini/antigravity-cli/cache/last_conversations.json
agent: claude

# claude:   a family alias (sonnet | opus) so it tracks the latest.
# codex:    a literal slug (gpt-5.6-sol | gpt-5.6-terra); no alias mechanism.
# opencode: provider/model, e.g. opencode/big-pickle. `opencode models` lists
#           them; the `opencode/...` provider is the free tier.
# pi/prime: provider/model too, e.g. ollama/qwen3.8 or anthropic/claude-opus-5-5.
# Not fable, and not gpt-6-astra: both top tiers are reserved for whichever
# orchestrator is running, and `horch spawn` refuses to start a worker on
# either. A fleet has exactly one top-tier session. Reach for opus or
# codex-sol instead.
model: opus

# ─── usage limits ────────────────────────────────────────────────────────────
# Teammates whose LAUNCH settings (agent, model, args, env, ...) a spawn
# borrows when this teammate's usage pool is exhausted, broken or cooling -
# or tight while a fallback has twice the headroom. This teammate's persona,
# base, phase and skills stay. First usable entry wins; a fallback's own
# fallbacks are never followed. Each must run on a different pool, and never
# on a trains-on-input (opencode) teammate. `horch route <name>` shows what a
# spawn would do right now.
fallbacks: []

# claude   -> `--effort <level>`   (low | medium | high | xhigh | max)
#             Not on haiku: Haiku 4.5 has no effort setting.
# codex    -> `-c model_reasoning_effort="<level>"`
#             (none | low | medium | high | xhigh | max). Never `minimal` (an
#             API error on gpt-5.6) or `ultra` (fans out, multiplies spend).
#             REQUIRED on an offered codex teammate: unset, the pane inherits
#             the operator's ~/.codex/config.toml.
# opencode -> the build agent's `variant`, via OPENCODE_CONFIG_CONTENT
#             (the TUI has no --variant flag). Only for models that define
#             variants; the free `opencode/*` models have none, and `--check`
#             refuses the field there.
# pi/prime -> `--thinking <level>` (off | minimal | low | medium | high | xhigh | max)
# Same field, different mechanism per agent, and `horch teammates --check`
# validates it per agent. `horch spawn --effort` overrides it for one spawn.
# Levels are not comparable across vendors: "high" on codex is not "high" on
# claude. See ai_docs/reports/model-guide-2026-09.md for why each teammate
# runs at the level it does.
effort: xhigh

# claude only. Sets CLAUDE_CODE_SUBAGENT_MODEL, i.e. the model this teammate's
# OWN subagents run on. Only worth setting when the teammate delegates heavily
# (see staff-engineer.md). Ignored for codex.
subagent_model:

# ─── skills ──────────────────────────────────────────────────────────────────
# phase selects a portable repo-owned catalog on every harness that exposes
# skills (not antigravity: agy has no skills-dir switch).
# Values: research | plan | implementation | validation. null means no catalog.
# `horch spawn --phase` overrides this; resume keeps its recorded phase.
# skills adds named bundled skills to the phase catalog. Bodies load on demand.
# plugin_dirs is a separate Claude-only extension for custom operator plugins.
phase: null
plugin_dirs: []
skills: []

# available_skills offers further catalog skills by NAME ONLY: each one is
# materialized with the launch and named under "Also available" in the
# briefing, without its description. Use it for a related skill that is worth
# one name in the briefing, not a full description. The harness still lists
# every materialized skill's description in its own skill list, so keep the
# list short. A name in both skills and available_skills fails --check.
available_skills: []

# operator_skills copies skills from a directory on the operator's machine
# into the launch bundle, on every harness that exposes skills. They are
# never compiled in and never copied into this repo. Example, for skills that
# `xcrun agent skills export` wrote:
#   operator_skills:
#     dir: ~/.agents/skills
#     names: [swiftui-whats-new-27, test-modernizer]
# - `~/` expands against the launch's home.
# - Each name must be <dir>/<name>/SKILL.md, with a matching `name:`.
# - They are EXPECTED skills, named with their descriptions, like skills:.
# - A name that is also a bundled or marketplace skill fails: one bundle
#   directory cannot hold both.
# - A subagent skill (device-interaction, or a body that says "SUBAGENT
#   skill" or "Agent tool") fails: fleet panes start no subagents.
# - Launch copies each directory and checks the copy's digest.
# `horch teammates --check` fails on a missing dir or name.
operator_skills: null

# claude only. claude.ai-synced skills are off in every fleet pane; set
# `inherit_claudeai_skills: true` to keep them. (horch puts
# syncClaudeAiSkills: false in its --settings overlay; that hides the
# anthropic-skills:<name> entries for this session only and moves nothing.)
# disabled_skills switches further skills off by name, e.g. ["dev-prime"],
# as skillOverrides "off" entries. Settings merge per key, so the operator's
# own skillOverrides still apply. Verified against Claude Code 2.1.278: this
# hides a ~/.claude/skills entry, not only a plugin skill.
# The bundled Claude teammates use it for one thing: the operator's stale
# herdr-orchestrator and herdr-worker skills, whose descriptions trigger on
# "herdr" and "horch" and whose content predates this horch CLI. The repo
# carries the real briefing in teammates/_base/, so those copies only mislead
# a pane. A plugin's copy keeps its plugin prefix and is a different name;
# switch those off with the prefix, or with inherit_plugins: false.
inherit_claudeai_skills: false
disabled_skills: []

# claude only. Plugins whose NAMED skills this teammate is expected to use:
#   plugin_skills:
#     code: [review-git-changes, complexity-sweep]
# - The named skills go into the worker's briefing, each with its SKILL.md
#   description, as skills it is expected to use. (The bundled `skills:` above
#   get the same treatment.) A bare list of names reads as optional.
# - Every OTHER skill of that plugin is switched off for this session, as a
#   `"<plugin>:<skill>": "off"` skillOverrides entry. So a marketplace plugin
#   that ships twenty skills exposes only the two this role should use.
# - The plugin is found in plugin_dirs first, then in the operator's
#   ~/.claude/plugins/installed_plugins.json. An installed one stays enabled
#   even with inherit_plugins: false.
# `horch teammates --check` fails on an unknown plugin or skill, and when the
# teammate has neither a phase nor skills (the briefing could not name them).
plugin_skills: {}

# claude only, orchestrator only. Starts Claude's Remote Control bridge, as
# `remoteControlAtStartup: true` in the --settings overlay. Every other claude
# pane gets the key set to false, so an operator or org default cannot turn it
# on in a worker. `horch teammates --check` fails a non-orchestrator that sets
# it.
remote_control: false

# ─── startup mode ────────────────────────────────────────────────────────────
# How much the teammate may do without asking. Unset = inherit the operator's
# settings, which for a worker in an unwatched pane usually means it stops on
# the first prompt nobody is there to answer. Be explicit.
#
# claude -> --permission-mode <value>
#           acceptEdits | auto | bypassPermissions | manual | dontAsk | plan
# codex  -> no single equivalent; mapped onto its sandbox + approval pair:
#           plan               -s read-only        -a on-request
#           acceptEdits        -s workspace-write  -a on-request
#           auto               -s workspace-write  -a never
#           bypassPermissions  --dangerously-bypass-approvals-and-sandbox
#           manual, dontAsk    no equivalent -> load-time error
permission_mode: acceptEdits

# ─── tools ───────────────────────────────────────────────────────────────────
# Claude only. Codex has no per-tool control, so setting any of these on an
# agent: codex teammate is a load-time error; use `args` and its sandbox flags.
#
# tools             -> --tools: THE AVAILABLE SET, replacing the default.
#                      omit the key   = do not pass the flag (default set)
#                      ["default"]    = --tools default (everything)
#                      [] (empty)     = --tools "" (nothing)
#                      ["Read","Bash"]= exactly those
# allowed_tools     -> --allowedTools: pre-approved, no permission prompt.
#                      Supports patterns: "Bash(git *)".
# disallowed_tools  -> --disallowedTools: denied outright. On claude 2.1.278
#                      this takes the tool out of the session's tool set; it is
#                      not a permission prompt.
#
# FLEET RULE: no pane spawns subagents. A subagent's work never reaches the
# ledger or the grid, and splitting the work is the orchestrator's decision. A
# worker that needs more hands sends `QUESTION:` to the orchestrator instead.
# So every `agent: claude` teammate the orchestrator can spawn, and the
# orchestrator itself, carries `disallowed_tools: [Agent]`, and
# `horch teammates --check` fails one that does not. `Agent` is the only
# subagent tool name on this version; `Task` does not exist.
#
# allow_subagents -> true waives that check for this one teammate. Nothing
#   shipped sets it. It denies nothing by itself: it only stops `--check`
#   asking for the deny, so a waived teammate really can spawn subagents.
#
# Other harnesses carry the same rule through their own switch. Codex uses
# `args: ["-c", "features.multi_agent=false"]`. See
# `ai_docs/reports/no-subagents.md` for the evidence per harness.
tools:
allowed_tools: []
disallowed_tools: [Agent]
allow_subagents: false

# ─── starting clean: plugins, skills, MCP ────────────────────────────────────
# A fresh `claude` inherits everything the operator has installed globally:
# every enabled plugin, every skill, every MCP server. For a worker with one
# narrow job that is context bloat - and it is also where a persona gets
# overruled, because an installed skill arrives as an instruction and wins.
#
# The principle: keep the operator's settings.json, which is already tuned
# for token economy (disableWorkflows, disableBundledSkills, statusLine...),
# and subtract only the two things a fleet worker has no use for.
#
# inherit_plugins -> false switches the operator's globally-enabled plugins
#   OFF for this session, by name, via a --settings override. Nothing else in
#   settings.json changes. plugin_dirs then adds back only what is needed.
#   Measured on this machine: ddd ~3.2k tokens always-on, herdr ~485,
#   code ~450. Loading a 20-skill plugin to use two of its skills is the
#   exact bloat this field exists to avoid.
#
# mcp_servers -> see below. Empty means zero MCP servers.
#
# disable_skills -> --disable-slash-commands. Despite the help text ("Disable
#   all skills") this is the whole slash-command dispatcher: it also removes
#   the BUILT-IN /context, /config, /help, and there is no allowlist. Nothing
#   shipped sets it. Never combine it with phase, skills, or plugin_dirs; `--check` rejects
#   that combination.
inherit_plugins: true
disable_skills: false

# setting_sources -> --setting-sources <list>. Which of the operator's settings
#   files load: user, project, local. Omit = all of them. [] = NONE.
#   This is the blunt instrument. It also drops disableWorkflows,
#   disableBundledSkills and the rest of the operator's tuning, so it usually
#   costs MORE context than it saves. Only the status line survives it (horch
#   re-injects statusLine via --settings). Prefer inherit_plugins.
setting_sources:

# settings -> --settings <path>. Your own settings file. Replaces horch's
# --settings overlay entirely (the plugin switch-off, the skill switches and
# the statusLine passthrough), so it has to carry those itself; --check
# insists on statusLine. A teammate with a phase or skills is the exception:
# the skill bundle merges horch's switches into this file.
settings:

# sandbox -> the `sandbox` object of Claude's settings, put in the --settings
#   overlay (code.claude.com/docs/en/settings-reference, "Sandbox settings").
#   Claude only. The OS (Seatbelt on macOS, bubblewrap on Linux) holds every
#   Bash command and its children to it, so `sh -c` or `unset` cannot step
#   out. Omit = no sandbox. When set, horch always adds enabled: true,
#   failIfUnavailable: true, allowUnsandboxedCommands: false,
#   network.strictAllowlist: true and
#   permissions.blockReadsOutsideWorkingDirectories: true (the Read tool is
#   outside the sandbox, and this key closes the home directory to it and
#   to Bash). Re-open paths with filesystem.allowRead. The launch refuses a
#   host that cannot sandbox. `horch teammates --check` fails a non-claude
#   agent, a settings file, a value against a forced key, excludedCommands
#   and filesystem.disabled. See app-release-preparer.md.
sandbox:

# mcp_servers -> --mcp-config '{"mcpServers": {...}}' --strict-mcp-config
#   Omit the key = leave the operator's MCP configuration alone.
#   {} (empty)   = exactly zero MCP servers. `--strict-mcp-config` is what
#                  makes this subtractive rather than additive.
#   {...}        = only these.
#
# NO SECRETS. These files are committed. A server that needs an API key goes in
# mcp_config_files, pointing at a file the operator owns and gitignores.
mcp_servers:
mcp_config_files: []

# ─── escape hatches ──────────────────────────────────────────────────────────
# Everything the named fields above do not cover, so a new agent-CLI flag is
# not a schema change. Appended verbatim, after the derived args.
#   claude e.g. ["--permission-mode", "acceptEdits"]
#   codex  e.g. ["-s", "workspace-write"] or ["-p", "some-codex-profile"]
args: []
# env: set on the CLI's process, never through a shell. A value that starts
# with `~/` expands against the launch's home. NO SECRETS: point at a file the
# operator owns instead (see app-release-preparer.md).
env: {}

# ─── first instruction ───────────────────────────────────────────────────────
# Optional. Rendered as the very last line the teammate reads, after the task.
# Use it for a mandatory first move. Placeholders: {role} {task} {model}
# {session} {project_dir} {record_id} {plan_file}
# Does this teammate's provider train on what it is sent? True for the free
# tiers, where the prompts are the payment. It appends the shared warning block
# from `_base/fleet-worker.md` to the briefing, so the constraint reaches the
# WORKER as an instruction - `brief_description` only reaches the orchestrator.
# Leave false for anything paid, local, or self-hosted.
trains_on_input: false

first_instruction:
---

Everything below the frontmatter is the PERSONA: who this teammate is and what
it is for. It replaces {persona} in the base prompt.

Keep it to identity, scope, and standing constraints. Do not restate the
communication protocol - the base already carries it, and a second copy that
drifts is worse than no copy. Do not put the task here; the task arrives per
spawn.
