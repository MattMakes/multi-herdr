# teammates/

One file per team member. **Adding a file adds an option the orchestrator can
choose from** — there is no registry to update and nothing to recompile.

> **These files are live.** They are the only place agent briefings exist.
> `crates/horch-core/src/prompts.rs` holds no prompt text; it reads these files
> and substitutes `{placeholder}` spans, nothing more. `tiers.rs` is gone.
>
> ```bash
> horch teammates            # who the orchestrator can pick
> horch teammates --check    # validate before a fleet reads them for real
> horch teammates --new qa   # scaffold qa.md from _template.md
> ```
>
> Edits take effect on the next `just herdr-fleet` with no rebuild, because the
> justfile exports `HORCH_TEAMMATES_DIR` and the roster is read at run time. A
> copy is also compiled in, so an installed `horch` binary works with no repo
> checked out.

## Rules

- **Filename is the id.** `opus.md` → `horch spawn opus`. `name:` in the
  frontmatter must match, and a mismatch is a load-time error rather than a
  quietly-wrong roster.
- **A leading `_` means "not parsed."** `_template.md` and `_base/` are not
  teammates and are never read as such.
- **`hidden: true` means "loaded, but not offered."** The teammate never
  appears in the roster. `smoke.md` uses this, so `horch smoke fleet` keeps
  working without costing the orchestrator a roster line for a fake agent; the
  two orchestrators use it because they are launched into a pane, never spawned
  into one.
- **The top tier is reserved for the orchestrator.** `fable` and `gpt-6-astra`
  are refused by `horch spawn`, whichever flavor is orchestrating: a fleet has
  at most one top-tier session (none under `horch fleet opus` or `sol`). The
  rule is matched on the tier name, so a version bump stays reserved. `horch teammates --check` fails any offered
  teammate that asks for one.
- **`brief_description` is the only field the orchestrator reads** to decide
  between a specialist and a generic. One line, ≤120 characters, enforced at
  load. Everything else — persona, model, effort, skills — is applied when the
  teammate is spawned and is never in the orchestrator's context.
- **Be explicit about mode and tools.** Anything left unset is inherited from
  the operator's `~/.claude/settings.json`, which is written for an attended
  session, not for a worker in a pane nobody is watching. On this machine that
  file denies `ExitPlanMode`, so a plan-mode teammate that says nothing about
  tools cannot leave plan mode. See the `permission_mode` / `tools` block in
  `_template.md`.
- **Personas do not carry protocol.** `_base/fleet-worker.md` owns the
  `horch tell` / `horch note` / `horch done` contract, and
  `_base/fleet-orchestrator.md` owns the spawning and ledger contract; a
  persona that restates either will drift from it. `orchestrator.md` and
  `orchestrator-codex.md` are personas in exactly this sense: each is only the
  paragraph about writing for the workers below it, substituted into the one
  shared briefing at `{persona}`.
- **No fleet pane spawns subagents.** A subagent's work never reaches the
  ledger or the grid, and splitting the work is the orchestrator's decision,
  not a worker's. A worker that needs more hands sends `QUESTION:` to the
  orchestrator. Prose alone did not hold this, so the switch carries it: every
  `agent: claude` teammate the orchestrator can spawn, and the orchestrator
  itself, sets `disallowed_tools: [Agent]`, and `horch teammates --check` fails
  one that does not. `allow_subagents: true` waives the check for one teammate
  and denies nothing by itself; nothing shipped sets it. The Codex teammates
  carry the same rule as `args: ["-c", "features.multi_agent=false"]`.
- **The orchestrator delegates only to workers.** The Claude orchestrator
  also denies `RemoteTrigger`, which starts a cloud agent, and
  `allow_subagents` cannot waive either deny for it. Its briefing forbids a
  nested agent CLI (`claude -p`, `codex exec`) started from a shell.
- **Some capabilities are the orchestrator's alone.** The bundled
  `orchestrate` and `skill-creator` skills and `remote_control: true` are
  refused by `horch teammates --check` on any other teammate. Every Claude
  pane switches off the ambient skill-creator copies (the official plugin and
  the claude.ai-synced skill) and states `remoteControlAtStartup`, `false`
  except on the orchestrator. The Claude orchestrator starts in
  `permission_mode: auto`.
- **Agent-to-agent messages use Simplified Technical English (STE).** The rule
  lives in `_base/fleet-worker.md` and `_base/fleet-orchestrator.md`.

## The team

`product-lead`, `researcher`, `staff-engineer`, `designer`,
`architect-reviewer`, `frontend-developer`, `backend-developer` and
`qa-engineer` are the specialists. Their `phase` selects a small portable
skill catalog embedded in horch; `skills` adds named bundled skills for that
role. Skills are discovered by each harness and read only when relevant.
`product-lead` adds `product-requirements`, `staff-engineer` adds
`system-design` (and names `api-contracts` and `data-migrations`),
`architect-reviewer` adds `architecture-review`, and `backend-developer` adds
`api-contracts` and `data-migrations`.
Claude specialists set `inherit_plugins: false` to switch off globally enabled
plugins while preserving other operator settings and their declared MCP tools.

Researcher, product-lead and designer default to `research`; staff-engineer and
orchestrators to `plan`; reviewers and QA to `validation`; other workers to
`implementation`. Override a task with `horch spawn opus --phase research`.
Resume preserves the recorded phase unless `--phase` overrides it; old records
without a phase use the current teammate default. A phase selects guidance,
not permission mode or tool access.

## The design team

Six more specialists cover visual design. Each carries bundled design skills
by name, not by phase, so `--phase` never hands them to another teammate.

| teammate | phase | agent, model, effort | MCP servers | use for |
|---|---|---|---|---|
| `design-director` | research | claude, opus, high | none | direction contract and storyboard; no production code |
| `design-critic` | validation | claude, opus, high | playwright | scored review with screenshots; never edits code |
| `landing-page-builder` | implementation | claude, opus, medium | playwright, chrome-devtools, context7 | landing and marketing pages |
| `design-system-engineer` | implementation | claude, sonnet, medium | playwright, context7 | tokens, themes, components, brand kit |
| `motion-engineer` | implementation | claude, opus, medium | playwright, chrome-devtools, context7 | GSAP scroll scenes and transitions |
| `visual-prototyper` | implementation | codex, gpt-5.6-sol, medium | none | generated images and a static prototype |

`design-critic` denies `Edit` and `NotebookEdit` but keeps `Write` for its one
critique file. Every builder takes screenshots at 390, 768 and 1440 px before
it reports done. `visual-prototyper` uses Codex image generation when the
session has it, and writes prompts and placeholders when it does not.
`design-system-engineer` runs on sonnet because the direction contract
supplies the judgement; spawn `opus` for a system with no direction. The
orchestrator's pipeline for these seats is in `skills/orchestrate/SKILL.md`
under "Design work".

Inspect catalogs and context estimates with `horch skills --phase validation`.
The built-in roster requires no local plugin paths. Custom Claude teammates can
still declare `plugin_dirs` (`~/` expands at launch); `horch teammates --check`
verifies those paths exist. `disable_skills` conflicts with phase, skills, or
plugin directories.

## The Unreal Engine team

Eleven `ue-*` specialists staff Unreal Engine work. Each sets
`offer_when: ["*.uproject"]`, so the fleet orchestrator is offered them only
when the project has a `.uproject` file at the top level or one level down.
`horch spawn ue-<name>` still works in any project.

| teammate | wave | phase | agent, model, effort | use for |
|---|---|---|---|---|
| `ue-tech-lead` | 1 | plan | claude, opus, high | runs first: writes `.agents/ue-project-context.md`; plans modules and plugins |
| `ue-gameplay-engineer` | 1 | implementation | claude, opus, medium | actors, components, gameplay framework, GAS, tags, data assets |
| `ue-network-engineer` | 1 | implementation | claude, opus, medium | replication, RPCs, prediction, net movement, dedicated servers |
| `ue-technical-artist` | 1 | implementation | claude, opus, medium | materials, Niagara, audio, Sequencer, procedural content |
| `ue-qa-engineer` | 1 | validation | claude, sonnet, high | headless builds, automation tests, crash repros, profiling |
| `ue-code-reviewer` | 1 | validation | claude, opus, high | UE C++ review; never edits |
| `ue-character-engineer` | 2 | implementation | claude, opus, medium | movement modes, animation, cameras, Enhanced Input |
| `ue-ai-engineer` | 2 | implementation | claude, opus, medium | AI controllers, behavior trees, EQS, State Tree, Mass |
| `ue-ui-engineer` | 2 | implementation | claude, opus, medium | UMG, Slate, Common UI, MVVM, gamepad focus |
| `ue-tools-engineer` | 2 | implementation | claude, opus, medium | editor modules, customizations, validators, editor scripts |
| `ue-world-engineer` | 2 | implementation | claude, opus, medium | World Partition, streaming, PCG, collision, save games |

Every one runs on Claude: native Windows refuses a Codex teammate that has
skills, and most Unreal work happens on Windows. Only `ue-tech-lead` and
`ue-code-reviewer`, which read and plan but never build, fall back to
`codex-sol`. The builders and `ue-qa-engineer` have no fallback: a Codex pane
runs with the network off and `workspace-write`, which may block the shared
Derived Data Cache, and nobody has tried it on a UE project yet. When the
Claude pool is out, `horch route` refuses and the orchestrator waits. Add a
Codex fallback after a trial on a real project. Every implementation
teammate and `ue-qa-engineer` carry `ue-build-verify`. `ue-technical-artist`
and `ue-tools-engineer` also carry `ue-editor-scripting`, and so they may
change an asset through editor Python or a commandlet. The others never
touch `.uasset` or `.umap` files and list the asset change for a human.
`available_skills:` names at most 3 related skills per teammate, picked from
the "Related Skills" sections of its own skills.

Each persona carries the same six standing rules: read
`.agents/ue-project-context.md` first, never write asset bytes, check APIs in
the engine headers, one build at a time per working copy, a `DONE:` that
names the target, the configuration and the automation filter with its
result, and the Git LFS lock rules. The `ue-*` skills target UE 5.8, the
latest release on 2026-10-04 (hotfix 5.8.3).

Version control is Git with Git LFS, not Perforce. `.uasset`, `.umap` and
the other binary types are LFS files marked `lockable`. A worker locks an
asset with `git lfs lock` only when the orchestrator assigned that asset,
reports a lock held by someone else as `BLOCKED:`, never runs
`git lfs unlock --force`, and never makes a read-only file writable with
`chmod`. A new worktree needs `git lfs pull`; every worktree of a clone
shares the LFS objects in the common `.git/lfs`. The recommended
`.gitattributes` and the full rules are in
[`skills/ue-build-verify/references/git-lfs.md`](../skills/ue-build-verify/references/git-lfs.md).

Every `ue-*` teammate except `ue-code-reviewer` sets `requires: [git-lfs]`,
and `blender-artist` sets `requires: [blender, git-lfs]`. Their rules tell
them to run `git lfs lock` or `git lfs pull`. `ue-code-reviewer` does not,
because it never builds, edits or commits. When the project is offered one of
them, `horch doctor` checks that `git lfs version` works.

## The Godot team

Nineteen `godot-*` specialists staff Godot 4 work. Each sets
`offer_when: ["project.godot"]`, so the fleet orchestrator is offered them
only when the project has a `project.godot` file at the top level or one
level down. Each also sets `requires: [godot]`, and `horch doctor` then looks
for the engine (`GODOT_PATH`, then `godot` on PATH, then the macOS app
bundle) and checks that `--version` is 4.3 or later. `horch spawn godot-<name>`
still works in any project.

| teammate | wave | phase | agent, model, effort | use for |
|---|---|---|---|---|
| `godot-tech-lead` | 1 | plan | claude, opus, high | runs first: writes `.agents/godot-project-context.md`; plans scenes, autoloads, features |
| `godot-gameplay-programmer` | 1 | implementation | claude, opus, medium | player controllers, state machines, components, combat, game loops, input, physics |
| `godot-systems-programmer` | 1 | implementation | claude, opus, medium | Resources, abilities, inventory, economy, quests, save/load, event bus |
| `godot-ui-developer` | 1 | implementation | claude, opus, medium | Control scenes, themes, HUDs, menus, responsive layout, localization |
| `godot-technical-artist` | 1 | implementation | claude, opus, medium | shaders, particles, 2D and 3D scenes, lighting, asset import, audio |
| `godot-qa-engineer` | 1 | validation | claude, sonnet, high | headless parse checks, GUT and gdUnit4 tests, bug repros |
| `godot-code-reviewer` | 1 | validation | claude, opus, high | GDScript and C# review; never edits |
| `godot-animator` | 2 | implementation | claude, opus, medium | AnimationPlayer, AnimationTree, tweens, sprite animation |
| `godot-ai-programmer` | 2 | implementation | claude, opus, medium | navigation, state machines, LimboAI and Beehave behavior trees |
| `godot-network-engineer` | 2 | implementation | claude, opus, medium | MultiplayerAPI, RPCs, spawners, synchronizers, dedicated servers |
| `godot-world-builder` | 2 | implementation | claude, opus, medium | TileMap layers, GridMap, CSG, procedural levels, cameras |
| `godot-narrative-programmer` | 2 | implementation | claude, opus, medium | dialogue, Dialogue Manager, Popochiu, quests, localized text |
| `godot-performance-engineer` | 2 | implementation | claude, opus, high | profiling, frame time, threads, physics cost, asset size |
| `godot-release-engineer` | 2 | implementation | claude, sonnet, medium | export presets and builds, mobile and web targets; never uploads |
| `godot-csharp-engineer` | 3 | implementation | claude, opus, medium | GDScript-C# interop, `dotnet build`, source generators |
| `godot-tools-engineer` | 3 | implementation | claude, opus, medium | EditorPlugins, `@tool` scripts, inspector plugins, addon packaging |
| `godot-native-engineer` | 3 | implementation | claude, opus, medium | GDExtension in C++, `godot-cpp`, per-platform libraries |
| `godot-xr-developer` | 3 | implementation | claude, opus, medium | OpenXR, controllers, hand tracking, VR comfort, frame budget |
| `godot-porting-engineer` | 3 | implementation | claude, opus, medium | upgrade to 4.7, 2D/3D ports, single to multiplayer, desktop to mobile |

Every one runs on Claude. Only `godot-tech-lead` and `godot-code-reviewer`,
which read and plan but never build, fall back to `codex-sol`. The builders
and `godot-qa-engineer` have no fallback: a Godot import writes outside the
project, to the user data directory, and a Codex pane runs with the network
off and `workspace-write`. Nobody has tried that yet. Every implementation
teammate and `godot-qa-engineer` carry `godot-build-verify`, and every seat
that edits scenes carries `godot-scene-files`. `godot-code-reviewer` denies
`Edit`, `Write` and `NotebookEdit`, the same as `ue-code-reviewer`.
`godot-release-engineer` builds export artifacts and never uploads them (no
`butler push`, no store upload); like `app-release-preparer`, it ends at a
plan for the orchestrator. `available_skills:` names 3 related skills per
teammate.

C# is a language, not a domain. Every builder except `godot-csharp-engineer`
sets `skills_when: {"*.csproj": [godot-csharp-godot, godot-csharp-signals]}`,
so a C# project adds the 2 C# skills to its launch. `godot-csharp-engineer`
carries them always and owns interop, .NET builds and source-generator
problems.

Each persona carries the same six standing rules: read
`.agents/godot-project-context.md` first; treat `.tscn` and `.tres` files with
care, never touch `.godot/` or `.import` files, and commit the `.uid`
sidecars; check APIs against a `godot --doctool` dump of the project's engine;
run headless only, with one import at a time per working copy; a `DONE:` that
names the Godot version, the parse check and the tests with their result (and
`dotnet build` on C#); use an addon skill only when that addon is in
`addons/` at the pinned version. The `godot-*` skills target Godot 4.7, the
latest stable release on 2026-10-04 (4.7.2).

## The Blender teammate

| teammate | phase | agent, model, effort | use for |
|---|---|---|---|
| `blender-artist` | implementation | claude, opus, medium | Blender models, retopology, UVs, collision, LODs, rigs; FBX exports for the UE team |

`offer_when: ["*.blend", "*.uproject"]`. It drives Blender through the
official Blender Lab MCP server, pinned by commit, with no telemetry and no
asset downloads. Its headless tools need only a `blender` binary (on PATH,
or `BLENDER_PATH`); the live tools need Blender 5.1+ with the Blender Lab MCP
add-on. It carries `blender-ue-pipeline`, `blender-modeling`, `blender-rigging`
and `blender-baking`, and names `ue-editor-scripting`. It
exports to a source-art folder and never writes `.uasset`; `ue-technical-artist`
or `ue-tools-engineer` imports. No fallback: Codex has no MCP parity here.

## The Swift and Apple team

Seven specialists staff Swift and Apple-platform work. Each sets
`offer_when: ["*.xcodeproj", "*.xcworkspace", "Package.swift"]`, so the fleet
orchestrator is offered them only on an Apple project. All except
`codex-swift-reviewer` set `requires: [xcode]`, so `horch doctor` checks
`xcodebuild` and its first launch when the project is offered them.

| teammate | phase | agent, model, effort | fallback | use for |
|---|---|---|---|---|
| `swift-developer` | implementation | claude, opus, medium | none | SwiftUI features, SwiftData, Swift 6 concurrency |
| `apple-platform-developer` | implementation | claude, opus, medium | none | App Intents, widgets, Live Activities, background tasks, focus |
| `swift-reviewer` | validation | claude, opus, high | `codex-sol` | Swift review: concurrency, modern API, performance, Keychain; never edits |
| `swift-qa-engineer` | validation | claude, sonnet, high | none | Swift Testing, XCTest migration, simulator repros |
| `apple-accessibility-auditor` | validation | claude, sonnet, high | `codex-terra` | VoiceOver, Dynamic Type, Voice Control, UI copy; never edits |
| `app-release-preparer` | implementation | claude, sonnet, medium | none | archive, export, readiness audit, notes; writes the upload command for a human; never uploads or submits |
| `codex-swift-reviewer` | validation | codex, gpt-5.6-sol, high | `opus` | cross-vendor Swift review of Claude-built changes |

Every persona's first move is a toolchain check (`xcodebuild -version`,
`swift --version`, `xcrun simctl list devices available`). A builder or QA
pane reports `BLOCKED:` without it; a reviewer or auditor reviews the source
and says it had no build. The builders and QA have no fallback: a Codex pane
is untested with Xcode and the simulator, and a fallback takes the
fallback's MCP servers, so the build server would be gone.

The panes that build carry `mobilebuildmcp` (formerly XcodeBuildMCP),
pinned at 2.7.1, with the workflows each one needs switched on in the
server's `env`. The builders, the reviewers and the auditor carry the report's
house style for the points where the Swift skills disagree.

`app-release-preparer` gets its App Store Connect credential from a file the
operator owns: `ASC_CONFIG_PATH` (`~/.config/horch/asc/config.json`, with
one key entry and an absolute `private_key_path`) and `ASC_BYPASS_KEYCHAIN`.
Give it a key whose role cannot submit for review or change prices; that,
not its deny list, is the release safety. It never uploads: it writes the
exact upload command, and a human runs it. It ends every other App Store
Connect write at a `--dry-run` plan for the orchestrator, and it has no
fallback, because a fallback would drop the env, the sandbox and the deny
list.

`swift-developer` and `apple-platform-developer` set
`operator_skills: {dir: ~/.agents/skills, names: [swiftui-whats-new-27, test-modernizer]}`.
These are Apple's Xcode 27 skills, which cannot ship in this repo. Export
them on each Mac with Xcode 27 or later:

```sh
xcrun agent skills export --output-dir ~/.agents/skills
```

On a host without the export, `horch teammates --check` prints a `warning:`
line for each missing skill and still passes. The launch skips the skill,
and the worker's briefing says "Skipped: operator skill <name> is not
installed on this host". A skill that changes or disappears between
`horch spawn` and the worker's start fails the launch, because the worker
must run the skill digests that the ledger recorded.

## The effort-matrix personas

Eight more Claude specialists split the work by model tier and effort level
rather than by discipline. Sonnet does fast, cheap execution. Opus 5.5 brings
more baseline reasoning and handles complex abstraction. The effort level sets
how deep each one checks.

| effort | Sonnet | Opus |
|---|---|---|
| low | `sonnet-sketch`: drafts, brainstorms, boilerplate, one-file edits, scoping | `opus-architect`: architecture critique, API and schema design, multi-service bug triage |
| medium | `sonnet-feature`: endpoints, UI components, CRUD, test updates to a clear spec | `opus-domain`: intricate algorithms, domain rules, brownfield cross-cutting features |
| high | `sonnet-bugfix`: localized bugs with repro steps, edge-case validation, module refactors | `opus-hardening`: races, leaks, concurrency, adversarial security review, fuzz harnesses |
| max | `sonnet-sweep`: one refactor over many files, e2e boilerplate from a schema | `opus-verify`: Lean 4, hardware synthesis, compiler passes, sandboxed pentest |

Each persona states its boundary ("stop and report when...") in its body, and
the short form of that boundary in `brief_description`, so both the worker and
the orchestrator see it. The two `max` seats, `sonnet-sweep` and `opus-verify`,
have a `first_instruction` that makes the worker ask and wait on an
underspecified task. At max effort a wrong premise costs the most tokens.
Like the other Claude seats, each has a Codex `fallbacks:` entry for when the
Claude pool cannot serve a spawn: `codex-sol` for the Opus four, `codex-terra`
for the Sonnet four (`horch route <name>` shows the decision).

Token efficiency: for deep reasoning, `opus-hardening` (Opus at high) often
matches or beats Sonnet at max on fewer tokens. `sonnet-sweep` exists for
repetitive volume, not for depth. `opus-architect` is the fast, low-effort
counterpart of `staff-engineer` (deep plans, high) and `architect-reviewer`
(reviews a change, high).

## The generics

`sonnet`, `opus`, `codex-sol`, `codex-network`, `codex-terra`, `codex-luna`, `opencode-ultra`,
`opencode-pickle`, `opencode-lightning`, `pi` and `prime` are the fallbacks:
what the orchestrator spawns when no specialist's `brief_description` matches
the work. They are named after the tier or the harness on purpose, so
`horch spawn opus`, the ledger's `tier` field, and the roster in the
orchestrator briefing all keep meaning the same thing. There is no `fable` and
no `astra` generic: those tiers belong to the orchestrator.

`codex-network` is the only Codex worker whose sandbox reaches the network
(`-c sandbox_workspace_write.network_access=true`). Every other Codex
teammate keeps the network off, because it runs `auto` (`-a never`) and
nothing would stop it from sending code out. `codex-network` runs
`acceptEdits` (`-a on-request`) instead, and `horch teammates --check` fails
any Codex teammate that pairs network access with `auto` or
`bypassPermissions`. Claude workers need no switch: no Claude sandbox is
configured, so their Bash and WebFetch already reach the network.

They are not one ladder but three, and the orchestrator is choosing on cost and
confidentiality as much as on capability:

| | pays with | send it |
|---|---|---|
| `sonnet` `opus` `codex-*` `prime` | money | anything the project already trusts these providers with |
| `opencode-*` | **your prompts** | public and open-source work only |
| `antigravity` | **your prompts**, until the operator confirms the opt-out | public and open-source work only |
| `pi` | your own GPU | anything, including what must not leave the machine |

## Effort: set by role, stated in every file

Each teammate's `effort:` follows its role, not its model
([ImCesar/cezaar#40](https://github.com/ImCesar/cezaar/issues/40)).

| role | teammates | effort |
|---|---|---|
| orchestrator | `orchestrator`, `orchestrator-codex` | xhigh |
| reviewers and planners | `architect-reviewer`, `qa-engineer`, `codex-reviewer`, `staff-engineer`, `product-lead` | high |
| builders and research | `backend-developer`, `frontend-developer`, `designer`, `researcher`, `sonnet`, `opus`, `codex-sol`, `codex-network`, `prime` | medium |
| runners | `codex-terra`, `codex-luna`, `pi` | low |
| effort-matrix personas | `sonnet-*`, `opus-*` (effort is the role; see above) | low to max |
| no effort setting | `opencode-*` (their free models define no variants) | - |

Rules `horch teammates --check` enforces:
- the level must be one this agent's CLI takes;
- an offered codex teammate must state one;
- haiku and the free opencode models must not have one.

Change a level with the `tune-fleet` skill (`.claude/skills/tune-fleet/`),
which works from `horch cost` numbers, and write the reason next to the line.

## Free tiers and `trains_on_input`

The `opencode-*` teammates run on OpenCode's free models. Free means the
provider trains on what it is sent, so every one of them sets
`trains_on_input: true`. That appends the shared block from
`_base/fleet-worker.md` to the worker's briefing: treat the pane as public,
work only with public or open-source code, and report `BLOCKED` rather than
read anything proprietary into it.

Set the flag on any teammate whose provider trains on input, and say so in
`brief_description` too - the flag reaches the worker, the description reaches
the orchestrator deciding who gets the task, and the decision needs both ends.
Leave it off for paid, local and self-hosted models: a warning that appears
everywhere stops being read anywhere.

## Background calls switched off

Some CLIs make model calls that the worker did not ask for. Nobody reads
their output in a fleet pane, so these switches turn them off. Each switch
was checked on the installed CLI on 2026-10-06.

| harness | switch | where | what it stops |
|---|---|---|---|
| claude (64 files) | `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "false"` | `env:` | a fork of the full conversation after each turn, to suggest the next prompt (2.1.292) |
| claude (64 files) | `DISABLE_AUTOUPDATER: "1"` | `env:` | a mid-fleet update, so all panes in a fleet run 1 build |
| codex (8 files) | `-c tui.auto_recap=false` | `args:` | a hidden recap thread on the pane's model when the pane loses focus (0.160.0) |
| codex (7 workers) | `-c notify=[]` | `args:` | the operator's `notify` command after every worker turn. `orchestrator-codex` keeps it |
| opencode (3 files) | `{"agent":{"title":{"disable":true}}}` in `OPENCODE_CONFIG_CONTENT` | `env:` | a title call that sends the briefing to the operator's paid `small_model` (1.18.34) |
| opencode (3 files) | `OPENCODE_DISABLE_EXTERNAL_SKILLS: "1"` | `env:` | the operator's personal skills from `~/.claude/` and `~/.agents/` going to a provider that trains on input. The phase skills still load |
| prime | `RLM_MAX_DEPTH: "1"` | `env:` | recursive child sessions, each a fresh Opus context (default depth 2) |

No `_base/` file carries `env:` or `args:`, so each teammate file carries its
own lines. `_template.md` has the Claude lines, so `horch teammates --new`
copies them. Put the lines in every new teammate of these harnesses.

To turn a switch back on, delete its line from the teammate file, or set the
opposite value (`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "true"`,
`-c tui.auto_recap=true`). The launch oracles in
`crates/horch-core/tests/oracles/launch/` freeze the Codex args and the
OpenCode config: bless them after the change.

Not switched off:
- `CLAUDE_CODE_DISABLE_TERMINAL_TITLE` stops the Claude session-title call,
  but it also stops the terminal title. herdr reads that title to see if a
  Claude pane is working or idle. Do not set it.
- Prime auto-refine has no env var or flag. It needs `autoRefine.enabled:
  false` in a fleet-owned Prime agent dir; workstream W1 designs that dir.
- The Codex automatic thread title had no switch in 0.154.0 (checked
  2026-09-18, not re-checked on 0.160.0).

## Running `pi` on local models

`pi` needs an Ollama provider before `ollama/qwen3.8` resolves. Add
`~/.pi/agent/models.json`:

```json
{
  "providers": {
    "ollama": {
      "baseUrl": "http://localhost:11434/v1",
      "api": "openai-completions",
      "apiKey": "ollama",
      "models": [
        {
          "id": "qwen3.8",
          "name": "Qwen3.8 27B (local)",
          "reasoning": true,
          "input": ["text", "image"],
          "contextWindow": 262144,
          "maxTokens": 32000,
          "cost": { "input": 0, "output": 0, "cacheRead": 0, "cacheWrite": 0 }
        }
      ]
    }
  }
}
```

`pi --list-models | grep ollama` confirms it. The `id` must match the Ollama tag
(`ollama list`), and `pi.md`'s `model:` must match `ollama/<id>`.

`pi.md` runs at `effort: low`. On Qwen3.8 the long-context benchmark scores
`xhigh` below `low`, and a local model pays for every thinking token in
wall-clock time. Known Qwen3.8 issues (2026-09):
- **`--thinking off` does not turn thinking off** without a
  `thinkingLevelMap` in the model entry.
- **The OpenAI-compatible endpoint (`/v1`) can hang on tool calls** with
  `qwen3.8:27b`. If a pi pane stalls mid-tool-call:
  1. Point the provider at Ollama's native API instead.
  2. Or switch the tag to `qwen3.6:27b` (SWE-bench Verified 77.2; fits
     24 GB). Update both `id` here and `pi.md`'s `model:`.

## The two orchestrators

`orchestrator.md` (Claude Code) and `orchestrator-codex.md` (Codex) back the
four fleet flavors: `horch fleet opus` (the default) and `fable` launch the
first, `astra` and `sol` the second. The flavor passes the model as the pane's
`--model`, which wins over the file's own `model:`; the file's value is only
the fallback. They share one briefing — `_base/fleet-orchestrator.md` — and
each file is only the paragraph about writing for the workers below it. Change
how orchestration works in the base; change how one agent's orchestrator talks
in its file.

Both also carry `skills: [orchestrate]`. That is the fleet playbook — how to
decompose work, the plan-file template, how to choose a teammate, how to verify
a `DONE:`, how to wrap up. It is attached **by name, not by phase**: adding it
to the `plan` catalog in `crates/horch-core/src/skills.rs` would hand it to
`staff-engineer` and to anything else spawned with `--phase plan`, none of
which directs a fleet. A skill that arrives where it does not apply is context
spent to no effect, and an instruction a worker may act on. `orchestrate` is
own text; it was reconciled from the retired external `herdr-orchestrator`
and `herdr-worker` skills against the real CLI, and every command in it is one
`horch --help` prints.

The Codex one also needs `_base/codex-orchestrator-execpolicy.md`. Codex refuses
any command outside its sandbox that its rules file does not name, and the
worker rules allow `note` and `done` — nothing an orchestrator runs. Without the
orchestrator rules that pane comes up able to think and unable to spawn.
`horch teammates --check` fails a rule that no briefing asks for, so the
allowlist cannot quietly outgrow what the prompt actually uses.

Each set reaches only the pane that needs it: horch builds a private
`CODEX_HOME` per launch rather than appending to the operator's shared
`~/.codex/rules/default.rules`. See "Starting codex panes clean" in the root
README.

## Adding a specialist

Copy `_template.md`, rename it, drop `generic:`, and write a
`brief_description` that answers "when would I reach for this?" — the
orchestrator is choosing from that line alone.
