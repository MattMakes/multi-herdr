# An Unreal Engine wave for the fleet

Which skills and teammates to add so a horch fleet can staff Unreal Engine
work. The source is
unreal-engine-skills
at `f3742d7b688690810df369802b90430324e380b9` (2026-09-28, "Updated skills for
Unreal Engine 5.8"). This is a recommendation. Nothing in this report has been
added to `skills/` or `teammates/` yet.

## Summary

- **Vendor all 31 upstream skills** into `skills/` as `ue-*` bundles. Copy 30
  of them verbatim. Adapt `ue-project-context` only, because it interviews a
  human. Every skill already passes the bundled-skill
  checks in `skills.rs`.
- **Add 11 `ue-*` teammates in two waves.** Wave 1 has six teammates and
  covers almost all UE C++ work: lead, gameplay, network, technical art, QA
  and review. Wave 2 adds five system specialists. Between them, the 11
  teammates cover all 31 skills.
- **Run every UE teammate on `agent: claude`.** Most UE studios work on
  Windows, and on native Windows `ensure_supported` refuses Codex teammates
  that have skills.
- **Write two skills of our own.** Upstream has no skill that builds the
  project or runs tests headless, and none for the editor work an agent cannot
  do by editing text (`.uasset`). A fleet cannot verify a `DONE:` without the
  first one.
- **Change three small things in the code.** Fix the catalog test, add a
  domain list beside `ORCHESTRATOR_ONLY_SKILLS`, and optionally add an
  `available_skills:` field. Details are in [Code changes](#code-changes).

## What upstream ships

| fact | value |
|---|---|
| skills | 31, each a `SKILL.md` (408-499 lines) plus 0-3 `references/*.md` |
| engine | UE 5.8. Upstream says it checked every API against the 5.8 headers and compiled every example (about 945 blocks) against 5.8 |
| size | 2.07 MB in total: about 30 KB per `SKILL.md` and about 35 KB of references per skill |
| description length | 546-715 characters. The catalog limit is 1024 |
| names | `ue-<area>`, lowercase with hyphens. All pass `catalog()` validation |
| shared contract | `ue-project-context` writes `.agents/ue-project-context.md`. The other 30 skills read it first and continue if it is missing |
| harness assumptions | None found. There are no subagent instructions, no `${CLAUDE_PLUGIN_ROOT}`, no `/Users/` paths and no links outside a skill's own folder |
| interactive | Only `ue-project-context`, which works "scan first, then interview" |

For comparison, the current `skills/` folder is 416 KB. Vendoring the UE
skills adds about 2 MB to the `horch` binary (`include_bytes!`). A launch
writes only the skills its teammate selects, so a non-UE pane pays nothing.

## Skills to add

### Vendor verbatim (30)

Copy `skills/<name>/` with its `references/` folder. Do not copy upstream's
top-level `.DS_Store`: `build.rs` bundles every regular file it finds.
Add one `provenance.json` entry per skill with `source_repository`,
`source_revision` and `source_sha256`. Add a row to the source mapping table
in `skills/README.md`.

Do not rewrite these skills. Their value is the 5.8 API checking, including
the "Deprecated — do not use" tables, and a paraphrase would lose it. If the
descriptions cost too much in briefings, solve it on our side (see
`available_skills:` below), not by editing upstream text.

### Adapt one: `ue-project-context`

Steps 2 and 3, and the fallback questionnaire, ask "the user" to correct the
draft. No one answers in a fleet pane. Adapt it the way `skills/README.md`
already adapts upstream approval gates:

- Keep step 1 (scan and draft) and the document template exactly as they are.
- Replace the interview with this: write the draft with `[unknown]` markers,
  then send **one** `QUESTION:` to the orchestrator. List the
  highest-value unknowns, in the skill's own order: conventions, framework
  class names, networking model, then the rest.
- Keep the skill's rule against guessing. An `[unknown]` field is a valid
  final answer.
- Record the adaptation in `provenance.json`.

### Write two of our own

Upstream gives the commands for running headless tests
(`UnrealEditor-Cmd ... -ExecCmds="Automation RunTests ..." -unattended
-nopause -testexit=...`, in `ue-testing-debugging`). It has no skill for the
loop a fleet worker needs, which is: change code, build, test, report.

1. **`ue-build-verify`**: the step every implementer must pass before it
   sends `DONE:`.
   - Find the engine from `EngineAssociation` and the project context.
   - Build only the changed target and configuration (`Build.bat` /
     `Build.sh` / `RunUBT`).
   - Run the automation filter that matches the change.
   - Read the UBT, UHT and automation logs, and point to
     `ue-module-build-system`'s `common-build-errors.md` for each error.
   - Fleet-specific rules:
     - A full engine or editor build is longer than one foreground shell call
       allows (Claude Code's Bash limit is 10 minutes). Run long builds in
       the background and wait for them to finish.
     - If **Live Coding is active**, UBT will not build. Report `BLOCKED`.
       Do not close the operator's editor.
     - **Two panes building the same working copy conflict**, because UBT
       allows only one instance. The orchestrator must run builds one at a
       time (see the persona rules below).
     - A read-only source file in a Perforce workspace needs `p4 edit`.
       Report it. Do not `chmod` it.
   - Check the exact flags against the 5.8 tree before shipping this skill.
2. **`ue-editor-scripting`**: changes to assets and Blueprints, through the
   Python Editor Script Plugin and a headless commandlet
   (`-run=pythonscript`; check this against 5.8). Without it, an agent can
   expose C++ to Blueprint but cannot connect the result in an asset. Until
   this skill exists, every persona hands asset work back to a human in its
   `DONE:`.

Optional later: `ue-packaging` (BuildCookRun, cook errors). Upstream covers
cook rules only for data assets.

## Teammates to add

All of them use `base: fleet-worker`, `agent: claude`, `inherit_plugins:
false`, `mcp_servers: {}`, `disallowed_tools: [Agent]` and
`disabled_skills: [herdr-orchestrator, herdr-worker]`, the same as the
existing specialists. `permission_mode: auto`. Effort follows the role table
in `teammates/README.md`. The model follows `model-guide-2026-09.md`: Opus for
building and reviewing, Sonnet for QA, the same as `qa-engineer`.

"Briefing" below is the number of description bytes that `Bundle::briefing`
adds for the teammate's expected skills. Each expected skill is printed with
its full description.

### Wave 1: ship first

| teammate | `brief_description` (≤120) | phase | model / effort | expected `ue-*` skills | briefing |
|---|---|---|---|---|---|
| `ue-tech-lead` | Unreal Engine lead. Scans the project into .agents/ue-project-context.md; plans modules, plugins and features. | plan | opus / high | project-context, module-build-system, game-features, cpp-foundations | 2.6 KB |
| `ue-gameplay-engineer` | Unreal Engine gameplay C++: actors, components, GameMode/Pawn/Controller, GAS, tags, data assets, traces. | implementation | opus / medium | cpp-foundations, actor-component-architecture, gameplay-framework, gameplay-abilities, gameplay-tags-messaging, blueprint-cpp-interop, data-assets-tables, physics-collision | 5.4 KB |
| `ue-network-engineer` | Unreal Engine multiplayer: replication, RPCs, GAS prediction, CMC/Mover net movement, dedicated servers. | implementation | opus / medium | networking-replication, gameplay-framework, gameplay-abilities, character-movement, mover | 3.3 KB |
| `ue-technical-artist` | Unreal Engine rendering, VFX and audio C++: materials, render targets, Niagara, MetaSounds, Sequencer. | implementation | opus / medium | materials-rendering, niagara-effects, audio-system, sequencer-cinematics, procedural-generation | 3.3 KB |
| `ue-qa-engineer` | Unreal Engine QA. Builds headless, writes Automation/CQTest tests, reproduces crashes, profiles with Insights. | validation | sonnet / high | testing-debugging, module-build-system (+ `check`, `debug`, `tdd`) | 1.3 KB |
| `ue-code-reviewer` | Unreal Engine C++ review: GC and UPROPERTY, lifecycle, net authority, thread safety, 5.8 deprecations. Never edits. | validation | opus / high | cpp-foundations, actor-component-architecture, networking-replication, async-threading (+ `code-review`) | 2.6 KB |

`ue-code-reviewer` copies `architect-reviewer`'s read-only setup:
`disallowed_tools: [Agent, Edit, Write, NotebookEdit]`. Its four skills cover
the UE bugs that compile without error and fail at runtime: missing
`UPROPERTY` (objects lost to GC), constructor-versus-`BeginPlay` work, missing
authority checks, and touching UObjects off the game thread. The deprecation
tables in the skills supply the rest of the checklist.

### Wave 2: add when the project uses the system

| teammate | `brief_description` (≤120) | phase | model / effort | expected `ue-*` skills | briefing |
|---|---|---|---|---|---|
| `ue-character-engineer` | Unreal Engine characters: CharacterMovement or Mover modes, AnimInstance, montages, cameras, Enhanced Input. | implementation | opus / medium | character-movement, mover, animation-system, gameplay-cameras, input-system | 3.3 KB |
| `ue-ai-engineer` | Unreal Engine AI: AIController, behavior trees, EQS, navmesh, State Tree, Smart Objects, Mass crowds. | implementation | opus / medium | ai-navigation, state-trees, mass-entity, async-threading | 2.6 KB |
| `ue-ui-engineer` | Unreal Engine UI: UMG, Slate, Common UI screen stacks, MVVM view models, gamepad focus and input routing. | implementation | opus / medium | ui-umg-slate, input-system, blueprint-cpp-interop | 2.0 KB |
| `ue-tools-engineer` | Unreal Editor tooling C++: detail customizations, editor utility widgets, menus, asset definitions, validators. | implementation | opus / medium | editor-tools, ui-umg-slate, module-build-system, data-assets-tables, blueprint-cpp-interop | 3.3 KB |
| `ue-world-engineer` | Unreal Engine worlds: World Partition, data layers, level streaming, PCG, collision and physics, save games. | implementation | opus / medium | world-level-streaming, procedural-generation, physics-collision, serialization-savegames | 2.7 KB |

When `ue-build-verify` exists, add it to every implementation teammate and to
`ue-qa-engineer`. When `ue-editor-scripting` exists, add it to `ue-tools-engineer`
and `ue-technical-artist`.

### Coverage: every skill has at least one owner

| skill | teammates |
|---|---|
| project-context | tech-lead |
| cpp-foundations | tech-lead, gameplay, code-reviewer |
| actor-component-architecture | gameplay, code-reviewer |
| blueprint-cpp-interop | gameplay, ui, tools |
| module-build-system | tech-lead, qa, tools |
| async-threading | code-reviewer, ai |
| gameplay-framework | gameplay, network |
| gameplay-abilities | gameplay, network |
| gameplay-tags-messaging | gameplay |
| character-movement | network, character |
| mover | network, character |
| animation-system | character |
| game-features | tech-lead |
| networking-replication | network, code-reviewer |
| materials-rendering | technical-artist |
| niagara-effects | technical-artist |
| audio-system | technical-artist |
| sequencer-cinematics | technical-artist |
| gameplay-cameras | character |
| world-level-streaming | world |
| procedural-generation | technical-artist, world |
| physics-collision | gameplay, world |
| serialization-savegames | world |
| data-assets-tables | gameplay, tools |
| ai-navigation | ai |
| state-trees | ai |
| mass-entity | ai |
| ui-umg-slate | ui, tools |
| input-system | character, ui |
| editor-tools | tools |
| testing-debugging | qa |

With wave 1 only, the uncovered skills are: animation, cameras, input,
streaming, save games, AI, State Tree, Mass, UI and editor tooling. For a
project that uses one of these, add the wave 2 teammate before the first task
in that area.

### What the persona text must say

These are role constraints, not protocol, so they belong in the persona body
and not in `_base/fleet-worker.md`. Every UE persona needs:

1. **Read `.agents/ue-project-context.md` first.** If it is missing, send
   `QUESTION:` asking for `ue-tech-lead` to run first. Do not write the file
   from a guess.
2. **Never edit `.uasset` or `.umap` bytes.** They are binary and cannot be
   merged. Expose the C++ hook, then list the asset change a human must make
   in `DONE:`. Only `ue-editor-scripting` may remove this rule, once it exists.
3. **Check APIs in the engine headers, not from memory.** If the project
   context gives an engine path, grep `Engine/Source` and `Engine/Plugins`.
   That is the same check upstream used. If the project is not on 5.8, say
   which skill advice may not apply.
4. **Only one build at a time per working copy.** Build only after the
   orchestrator assigns the build. Report Live Coding and Perforce read-only
   files as `BLOCKED`. Do not work around them.
5. **"Compiles" is not done.** A `DONE:` names the target and configuration
   that was built, and the automation filter that was run with its result.

## Code changes

1. **`skills.rs` test `skills_catalog_is_portable_and_every_phase_resolves`.**
   It asserts `catalog.len() == 16`, and that every skill outside a phase is
   in `ORCHESTRATOR_ONLY_SKILLS`. The 31 `ue-*` skills (33 with our two) break
   both assertions. Add a `DOMAIN_SKILLS` set (or match the `ue-` prefix) and
   assert that named-only skills = orchestrator-only ∪ domain. Do **not** add
   UE skills to a `Phase` catalog. That would give them to `backend-developer`
   and every other implementation teammate, which is the mistake the test
   comment warns about for `orchestrate`.
2. **Optional: an `available_skills:` field.** Today every skill in `skills:`
   is expected and printed with its full description. The only skills offered
   by name only are the phase catalog. With `available_skills:`, a persona
   could list a related skill, for example `ue-async-threading` for the
   gameplay engineer, at the cost of one name in the briefing. `selected()`
   would include the field, and `briefing()` already puts every non-expected
   name under "Also available". Note that the harness still loads the
   descriptions of all materialized skills into its own skill list. Keep the
   list to the skills each one names under "Related Skills". Do not list all
   31.
3. **Optional: offer UE teammates only on UE projects.** Overlay layers can
   add or replace a teammate but cannot remove one. Teammates in `teammates/`
   are therefore offered on every project: 11 lines, about 1.2 KB in the
   orchestrator's briefing. Each line starts with "Unreal Engine", so this
   cost is acceptable for now. If it grows, a filter such as
   `offer_when: "*.uproject"` in the roster renderer would offer them only
   when the project needs them.
4. **`horch teammates --check`** needs no change. All the proposed
   teammates are Claude teammates on phases that exist, and every
   `brief_description` is 101-115 characters.

## Harness notes

- **Claude, not Codex, for UE teammates.** Native Windows refuses Codex
  teammates with skills (`ensure_supported`), and most UE work happens on
  Windows. Codex also runs `auto` with network off and `workspace-write`. A UE
  editor or commandlet can write outside the project, for example to a shared
  Derived Data Cache under the user profile, and the sandbox can block that.
  Test this before you add a Codex UE teammate. On macOS and Linux,
  `codex-reviewer` is still a good second opinion. Give it a UE-specific brief
  in the task, not a new teammate.
- **context7 is not needed.** The upstream skills were checked against 5.8.
  The engine headers are the ground truth for anything the skills do not
  cover.

## Suggested order

1. Vendor the 31 skills, with provenance and the test fix, and
   adapt `ue-project-context`. Check with `cargo test` and `horch skills`.
2. Add the six wave-1 teammates. Run `horch teammates --check`.
3. Write `ue-build-verify` and attach it. Try it on a sample project
   (Lyra, or a blank C++ template on 5.8) with one `ue-gameplay-engineer`
   task and one `ue-qa-engineer` task.
4. After a few runs, check model and effort with `tune-fleet`, using
   `horch cost` numbers.
5. Add wave 2 teammates when projects need them. Write `ue-editor-scripting`
   when asset work becomes the bottleneck.

## Open questions for the operator

- Which engine version and platform will the first project use? Upstream
  targets 5.8 only. On 5.5-5.7, some of the "use instead" APIs in its
  deprecation tables may not exist yet.
- Perforce or Git LFS? This changes the file-lock wording in the persona text
  and in `ue-build-verify`.
- Should the vendored copy follow upstream (re-vendor at a pinned revision
  when 5.9 ships) or be frozen as of this pin?
