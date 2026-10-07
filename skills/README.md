# Integrated worker skills

These bundles are maintained copies in this repository. Each folder contains a self-contained Agent Skills entrypoint with a name, a targeted discovery description, and the complete workflow. Only the selected phase's fleet-owned catalog is materialized; harnesses may also discover ambient skills. Workers load relevant bodies on demand. None requires runtime downloads, another skill, an external script, a particular model, or a particular harness tool API.

A bundle is one of these kinds:

- **verbatim**: an unchanged copy of a skill directory, minus dotfiles. Its `copied.json` entry has `"verbatim": true`. No other kind has it.
- **renamed**: a copy with the `godot-` prefix on the name and on every cross-reference. It is not verbatim.
- **adapted**: a curated copy with a set of edits, plus the fleet rules: no subagents, no human approval gate (a message to the orchestrator instead), no machine paths and no `${CLAUDE_*}` variables.
- **combined**: a copied base skill plus references written in this repository.
- **combined and rewritten**: files from several skills merged and rewritten into one skill.
- **own text**: written in this repository.

## Deliberate adaptations

- Preserve research evidence, behavioral preservation, dependency/wiring checks, verified implementation, regression testing, and evidence-based review.
- Replace repeated human approval gates and UI questions with focused orchestrator messages. Existing task authorization remains authoritative; only actions dependent on an unanswered decision are blocked.
- Remove mandatory nested agent fan-out, provider-specific commands, machine paths, bundled-script assumptions, repetitive persuasion, and fixed response rituals. Workers use the tools actually available in their harness.
- `code-analysis` uses project-native analyzers. It bundles no JavaScript engine, metric implementation, config generator, plugins, or dependency installation. Available measurements are reported honestly; an unavailable analyzer gets an explicitly qualitative review, not fabricated scores.
- `security-review` is a direct audit workflow with trust-boundary mapping, deduplication, false-positive verification, and evidence reports. It ships no scanner scripts, language catalogs, report assembler, or hierarchical agent pipeline.
- `orchestrate` is own text, reconciled from the retired external `herdr-orchestrator` and `herdr-worker` skills against the real `horch` CLI: every command, flag and lifecycle claim in those two files was checked against `horch --help` and either carried, corrected or dropped. It is attached to `orchestrator` and `orchestrator-codex` by name and belongs to no phase.
- `skill-creator` is a verbatim copy of a whole skill directory. It is attached to the Claude `orchestrator` by name and belongs to no phase. `horch teammates --check` fails any other teammate that names it, and every Claude pane switches off the official plugin and the claude.ai-synced copy. The skill tells its user to spawn subagents and to run `claude -p`; the orchestrator's persona tells it to take the skill's no-subagent path instead.
- `tdd` retains red/green/refactor for meaningful behavior changes and removes unrelated skill-authoring machinery and destructive instructions to delete existing work. Structural checks are appropriate for low-impact prose/config changes.

## Copied files

[copied.json](copied.json) has one entry per bundled skill: `{"name", "copied_files", "verbatim"}` and, when needed, `"budget_exempt"`, sorted by name. `copied_files` lists the files, relative to the skill directory, that were copied into this repository; an own-text skill has `"copied_files": []`. The Godot checks (`scripts/godot/*_check.py --strict-own skills/copied.json`) report a problem in a copied file but do not fail on it; a problem in any other file fails. `"verbatim": true` marks an unchanged copy: `copied_files` lists every file in the directory, and nobody edited them. A test fails a verbatim skill that holds a file outside `copied_files`. A verbatim skill is exempt from the bundled size budget. A renamed, adapted or combined skill over the budget is not verbatim: its entry gives the reason in `"budget_exempt": "<reason>"` instead. Set `budget_exempt` only on a skill over the budget; a test fails a stale one. Every bundled skill needs an entry. A new skill keeps `SKILL.md` at or under 12 KB and its whole directory at or under 160 KB, in `.md` and `.txt` files only; every verbatim skill and every skill with a `budget_exempt` reason is exempt from the size budget, and only `skill-creator` is exempt from the text-only rule, plus 1 file: `godot-build-verify/scripts/godot-run.sh`, the wrapper for every Godot call (SKL-11 keeps its execute bit). No skill directory holds a licence or notice file. `horch skills show <id>` prints the copied files. The build never bundles a file or directory whose name starts with `.`.

To refresh a copied skill:

1. Replace the copied files with the new copy, minus dotfiles, and re-apply the edits for an adapted skill.
2. Update the skill's `copied_files` in `copied.json` when the file list changes. Add or drop its `budget_exempt` reason when the copy crosses the size budget.
3. Re-run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.

## Process skills

| Skill | Kind |
| --- | --- |
| [brainstorm](brainstorm/SKILL.md) | adapted |
| [check](check/SKILL.md) | adapted |
| [code-analysis](code-analysis/SKILL.md) | adapted |
| [code-review](code-review/SKILL.md) | adapted |
| [create-plan](create-plan/SKILL.md) | adapted |
| [debug](debug/SKILL.md) | adapted |
| [document](document/SKILL.md) | adapted |
| [execute](execute/SKILL.md) | adapted |
| [handoff](handoff/SKILL.md) | adapted |
| [orchestrate](orchestrate/SKILL.md) | own text |
| [pre-flight](pre-flight/SKILL.md) | adapted |
| [research-codebase](research-codebase/SKILL.md) | adapted |
| [security-review](security-review/SKILL.md) | adapted |
| [skill-creator](skill-creator/SKILL.md) | verbatim |
| [tdd](tdd/SKILL.md) | adapted |
| [trace](trace/SKILL.md) | adapted |

## Design skills

| Skill | Kind |
| --- | --- |
| [art-direction](art-direction/SKILL.md) | combined and rewritten |
| [brand-identity](brand-identity/SKILL.md) | combined and rewritten |
| [design-imagery](design-imagery/SKILL.md) | combined and rewritten |
| [design-system](design-system/SKILL.md) | combined and rewritten |
| [landing-page](landing-page/SKILL.md) | combined and rewritten |
| [motion-gsap](motion-gsap/SKILL.md) | combined and rewritten |
| [ui-redesign](ui-redesign/SKILL.md) | combined and rewritten |
| [ui-taste](ui-taste/SKILL.md) | combined and rewritten |

## Swift and Apple skills

These skills attach by name to the Swift teammates and belong to no phase. The App Store skills never change App Store Connect: each live write becomes a dry-run sent to the orchestrator.

| Skill | Kind |
| --- | --- |
| [app-intents](app-intents/SKILL.md) | verbatim |
| [app-store-changelog](app-store-changelog/SKILL.md) | adapted |
| [appkit-accessibility-auditor](appkit-accessibility-auditor/SKILL.md) | adapted, path fix only |
| [appstore-review](appstore-review/SKILL.md) | adapted |
| [asc-cli-usage](asc-cli-usage/SKILL.md) | adapted |
| [asc-crash-triage](asc-crash-triage/SKILL.md) | verbatim |
| [asc-id-resolver](asc-id-resolver/SKILL.md) | verbatim |
| [asc-metadata-sync](asc-metadata-sync/SKILL.md) | adapted, dry-run only |
| [asc-submission-health](asc-submission-health/SKILL.md) | adapted, diagnosis only |
| [asc-xcode-build](asc-xcode-build/SKILL.md) | adapted |
| [background-execution](background-execution/SKILL.md) | verbatim |
| [ios-simulator-run](ios-simulator-run/SKILL.md) | adapted, rewritten as a playbook |
| [observability](observability/SKILL.md) | verbatim |
| [swift-code-audit](swift-code-audit/SKILL.md) | adapted |
| [swift-concurrency-pro](swift-concurrency-pro/SKILL.md) | adapted |
| [swift-focusengine-pro](swift-focusengine-pro/SKILL.md) | verbatim |
| [swift-format-style](swift-format-style/SKILL.md) | adapted |
| [swift-security-expert](swift-security-expert/SKILL.md) | adapted SKILL.md, verbatim references |
| [swift-testing-pro](swift-testing-pro/SKILL.md) | adapted |
| [swiftdata-pro](swiftdata-pro/SKILL.md) | adapted |
| [swiftdata-testing](swiftdata-testing/SKILL.md) | adapted |
| [swiftui-accessibility-auditor](swiftui-accessibility-auditor/SKILL.md) | adapted, path fix only |
| [swiftui-liquid-glass](swiftui-liquid-glass/SKILL.md) | adapted |
| [swiftui-performance-audit](swiftui-performance-audit/SKILL.md) | adapted |
| [swiftui-pro](swiftui-pro/SKILL.md) | adapted |
| [uikit-accessibility-auditor](uikit-accessibility-auditor/SKILL.md) | adapted, path fix only |
| [widgets](widgets/SKILL.md) | verbatim |
| [writing-for-interfaces](writing-for-interfaces/SKILL.md) | adapted SKILL.md, verbatim references |

## Engineering and product skills

These skills are own text. They belong to no phase; each attaches by name to the specialist whose work it covers. `product-requirements` is for `product-lead`, `system-design` for `staff-engineer`, `architecture-review` for `architect-reviewer`, and `api-contracts` and `data-migrations` for `backend-developer` (`staff-engineer` names both under `available_skills`). They are language- and framework-neutral: each tells the worker to follow the repository's own tools and to check library behaviour with a docs tool.

| Skill | Kind |
| --- | --- |
| [api-contracts](api-contracts/SKILL.md) | own text |
| [architecture-review](architecture-review/SKILL.md) | own text |
| [data-migrations](data-migrations/SKILL.md) | own text |
| [product-requirements](product-requirements/SKILL.md) | own text |
| [system-design](system-design/SKILL.md) | own text |

## Unreal Engine skills

These skills belong to no phase; an Unreal Engine teammate attaches them by name. `ue-project-context` is adapted: its interview becomes `[unknown]` markers and one `QUESTION:` to the orchestrator. The 3 own-text skills are written in this repository. `blender-ue-pipeline` is for the Blender teammate that hands assets to the Unreal teammates; it is checked against the Blender 5.2 API and the Unreal Engine 5.8 FBX pipeline docs. `blender-modeling`, `blender-rigging` and `blender-baking` are own text for the same teammate: they cover building the asset that `blender-ue-pipeline` exports, and every helper function in their references was run against the `bpy` 5.0.1 module.

| Skill | Kind |
| --- | --- |
| [blender-baking](blender-baking/SKILL.md) | own text |
| [blender-modeling](blender-modeling/SKILL.md) | own text |
| [blender-rigging](blender-rigging/SKILL.md) | own text |
| [blender-ue-pipeline](blender-ue-pipeline/SKILL.md) | own text |
| [ue-actor-component-architecture](ue-actor-component-architecture/SKILL.md) | verbatim |
| [ue-ai-navigation](ue-ai-navigation/SKILL.md) | verbatim |
| [ue-animation-system](ue-animation-system/SKILL.md) | verbatim |
| [ue-async-threading](ue-async-threading/SKILL.md) | verbatim |
| [ue-audio-system](ue-audio-system/SKILL.md) | verbatim |
| [ue-blueprint-cpp-interop](ue-blueprint-cpp-interop/SKILL.md) | verbatim |
| [ue-build-verify](ue-build-verify/SKILL.md) | own text |
| [ue-character-movement](ue-character-movement/SKILL.md) | verbatim |
| [ue-cpp-foundations](ue-cpp-foundations/SKILL.md) | verbatim |
| [ue-data-assets-tables](ue-data-assets-tables/SKILL.md) | verbatim |
| [ue-editor-scripting](ue-editor-scripting/SKILL.md) | own text |
| [ue-editor-tools](ue-editor-tools/SKILL.md) | verbatim |
| [ue-game-features](ue-game-features/SKILL.md) | verbatim |
| [ue-gameplay-abilities](ue-gameplay-abilities/SKILL.md) | verbatim |
| [ue-gameplay-cameras](ue-gameplay-cameras/SKILL.md) | verbatim |
| [ue-gameplay-framework](ue-gameplay-framework/SKILL.md) | verbatim |
| [ue-gameplay-tags-messaging](ue-gameplay-tags-messaging/SKILL.md) | verbatim |
| [ue-input-system](ue-input-system/SKILL.md) | verbatim |
| [ue-mass-entity](ue-mass-entity/SKILL.md) | verbatim |
| [ue-materials-rendering](ue-materials-rendering/SKILL.md) | verbatim |
| [ue-module-build-system](ue-module-build-system/SKILL.md) | verbatim |
| [ue-mover](ue-mover/SKILL.md) | verbatim |
| [ue-networking-replication](ue-networking-replication/SKILL.md) | verbatim |
| [ue-niagara-effects](ue-niagara-effects/SKILL.md) | verbatim |
| [ue-physics-collision](ue-physics-collision/SKILL.md) | verbatim |
| [ue-procedural-generation](ue-procedural-generation/SKILL.md) | verbatim |
| [ue-project-context](ue-project-context/SKILL.md) | adapted |
| [ue-sequencer-cinematics](ue-sequencer-cinematics/SKILL.md) | verbatim |
| [ue-serialization-savegames](ue-serialization-savegames/SKILL.md) | verbatim |
| [ue-state-trees](ue-state-trees/SKILL.md) | verbatim |
| [ue-testing-debugging](ue-testing-debugging/SKILL.md) | verbatim |
| [ue-ui-umg-slate](ue-ui-umg-slate/SKILL.md) | verbatim |
| [ue-world-level-streaming](ue-world-level-streaming/SKILL.md) | verbatim |

## Godot skills

Skills for the 19 `godot-*` teammates (see [`teammates/README.md`](../teammates/README.md)). They belong to no phase; a Godot teammate attaches them by name. The 65 skills are of 4 kinds, named in the Kind column: 18 **renamed** or **adapted** skills; 36 **combined** skills, each a renamed skill with an extended description plus own-text `references/*.md`; and **own text**, which is 11 skills written in this repository (genres, gameplay loops, combat, economy, quests, dimension ports, version migration, `godot-project-context`, `godot-build-verify`, `godot-scene-files`, `godot-language-choice`). Own-text references and code blocks are written in this repository's own words. `godot-grill` and `godot-brainstorming` are adapted: they ask the orchestrator one `QUESTION:` instead of asking a human. The 5 addon skills name their pinned addon version in the text. The skills target Godot 4.7 only (4.7.2 is the latest stable release): `scripts/godot/api_check.py` checks the engine calls in a skill against a `--doctool` dump of 4.7.2, and `scripts/godot/gdscript_blocks_check.py` parses its GDScript blocks on 4.7.2. Each command in the own-text skills was run on Godot 4.7.2. Failures in copied text do not fail these checks; they ratchet against `scripts/godot/copied-baseline.json` (a count can only go down). `scripts/godot/gameplay_scenarios.py` runs the gameplay, combat, economy and quest bundles headless in the gate.

**Proof levels.** A note `proof: <level>` next to a claim says how far this repository proved it on Godot 4.7.2:

- `proof: headless-run`: a headless Godot run executed the code and checked the result.
- `proof: parse-checked only`: the code parses (GDScript) or compiles (C#, with `dotnet` and `Godot.NET.Sdk`), and nothing ran it. Every GDScript or C# block without a note is at this level. On 2026-10-07 Godot .NET 4.7.2 ran a C# scene and a C# signal headless (`docs/live-checks/godot-csharp.md`); a C# block without a note stays compile-checked only. Copied and renamed skills with verbatim text (for example `godot-csharp-godot` and `godot-csharp-signals`) carry no per-claim notes, and their C# examples are parse-checked only. On a host with Godot .NET 4.7, the live check `scripts/godot/live_csharp.sh` runs a C# scene and a C# signal headless.
- `proof: not run (needs <hardware>)`: nothing here ran it, because it needs something a headless run on the test host does not have: a GPU renderer (shaders, GPU particles, MSAA), an XR headset, a store SDK or console kit, a router (UPnP), certificates or a service (TLS, a matchmaker), a release export template, Linux, or a human play test (game feel). No shader block is compiled: headless Godot compiles no shader.

The notes are in own text. A copied file keeps its upstream text and has a note only where the fleet already edited it; another claim in a copied file has no note and stays at the level its source gives it.

| Skill | Kind |
| --- | --- |
| [godot-2d-essentials](godot-2d-essentials/SKILL.md) | combined |
| [godot-3d-essentials](godot-3d-essentials/SKILL.md) | combined |
| [godot-ability-system](godot-ability-system/SKILL.md) | combined |
| [godot-addon-development](godot-addon-development/SKILL.md) | renamed |
| [godot-ai-navigation](godot-ai-navigation/SKILL.md) | combined |
| [godot-animation-system](godot-animation-system/SKILL.md) | combined |
| [godot-assets-pipeline](godot-assets-pipeline/SKILL.md) | renamed |
| [godot-audio-system](godot-audio-system/SKILL.md) | combined |
| [godot-beehave](godot-beehave/SKILL.md) | renamed |
| [godot-brainstorming](godot-brainstorming/SKILL.md) | adapted |
| [godot-build-verify](godot-build-verify/SKILL.md) | own text |
| [godot-camera-system](godot-camera-system/SKILL.md) | combined |
| [godot-code-review](godot-code-review/SKILL.md) | combined |
| [godot-combat-system](godot-combat-system/SKILL.md) | own text |
| [godot-component-system](godot-component-system/SKILL.md) | combined |
| [godot-csharp-godot](godot-csharp-godot/SKILL.md) | renamed |
| [godot-csharp-signals](godot-csharp-signals/SKILL.md) | renamed |
| [godot-debugging](godot-debugging/SKILL.md) | combined |
| [godot-dedicated-server](godot-dedicated-server/SKILL.md) | combined |
| [godot-dependency-injection](godot-dependency-injection/SKILL.md) | combined |
| [godot-dialogue-manager](godot-dialogue-manager/SKILL.md) | renamed |
| [godot-dialogue-system](godot-dialogue-system/SKILL.md) | combined |
| [godot-dimension-port](godot-dimension-port/SKILL.md) | own text |
| [godot-economy-system](godot-economy-system/SKILL.md) | own text |
| [godot-event-bus](godot-event-bus/SKILL.md) | combined |
| [godot-export-pipeline](godot-export-pipeline/SKILL.md) | combined |
| [godot-gameplay-loops](godot-gameplay-loops/SKILL.md) | own text |
| [godot-gdextension](godot-gdextension/SKILL.md) | renamed |
| [godot-gdscript-advanced](godot-gdscript-advanced/SKILL.md) | combined |
| [godot-gdscript-patterns](godot-gdscript-patterns/SKILL.md) | renamed |
| [godot-genre-blueprints](godot-genre-blueprints/SKILL.md) | own text |
| [godot-grill](godot-grill/SKILL.md) | adapted |
| [godot-hud-system](godot-hud-system/SKILL.md) | renamed |
| [godot-input-handling](godot-input-handling/SKILL.md) | combined |
| [godot-inventory-system](godot-inventory-system/SKILL.md) | combined |
| [godot-language-choice](godot-language-choice/SKILL.md) | own text |
| [godot-limboai](godot-limboai/SKILL.md) | renamed |
| [godot-localization](godot-localization/SKILL.md) | renamed |
| [godot-math-essentials](godot-math-essentials/SKILL.md) | renamed |
| [godot-mobile-development](godot-mobile-development/SKILL.md) | combined |
| [godot-multiplayer-basics](godot-multiplayer-basics/SKILL.md) | combined |
| [godot-multiplayer-sync](godot-multiplayer-sync/SKILL.md) | combined |
| [godot-multithreading](godot-multithreading/SKILL.md) | renamed |
| [godot-optimization](godot-optimization/SKILL.md) | combined |
| [godot-particles-vfx](godot-particles-vfx/SKILL.md) | combined |
| [godot-phantom-camera](godot-phantom-camera/SKILL.md) | renamed |
| [godot-physics-system](godot-physics-system/SKILL.md) | combined |
| [godot-player-controller](godot-player-controller/SKILL.md) | combined |
| [godot-popochiu](godot-popochiu/SKILL.md) | renamed |
| [godot-procedural-generation](godot-procedural-generation/SKILL.md) | combined |
| [godot-project-context](godot-project-context/SKILL.md) | own text |
| [godot-project-setup](godot-project-setup/SKILL.md) | combined |
| [godot-quest-system](godot-quest-system/SKILL.md) | own text |
| [godot-resource-pattern](godot-resource-pattern/SKILL.md) | combined |
| [godot-responsive-ui](godot-responsive-ui/SKILL.md) | renamed |
| [godot-save-load](godot-save-load/SKILL.md) | combined |
| [godot-scene-files](godot-scene-files/SKILL.md) | own text |
| [godot-scene-organization](godot-scene-organization/SKILL.md) | combined |
| [godot-shader-basics](godot-shader-basics/SKILL.md) | combined |
| [godot-state-machine](godot-state-machine/SKILL.md) | combined |
| [godot-testing](godot-testing/SKILL.md) | combined |
| [godot-tween-animation](godot-tween-animation/SKILL.md) | combined |
| [godot-ui](godot-ui/SKILL.md) | combined |
| [godot-version-migration](godot-version-migration/SKILL.md) | own text |
| [godot-xr-development](godot-xr-development/SKILL.md) | combined |
