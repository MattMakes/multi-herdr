# Integrated worker skills

These repo-owned bundles are curated adaptations from 2 upstream families:

- Process skills (planning, implementation, review, handoff) adapt the local public-skills collection, upstream [MattMakes/skill-marketplace](https://github.com/MattMakes/skill-marketplace) at revision `d47670328c59a3311a9b4149bc5f8f33f0a92754`. Each one adapts one upstream SKILL.md.
- Design skills combine and rewrite files from 7 MIT-licensed design repositories: `akseolabs-seo/cinematic-ui`, `greensock/gsap-skills`, `nutlope/hallmark`, `stevembarclay/pencilplaybook`, `leonxlnx/taste-skill`, `felix-huber/ui-landingpage-generator-skill` and `nextlevelbuilder/ui-ux-pro-max-skill`. One design skill can draw on several repositories. The upstream LICENSE files are not copied.

Two bundles are outside both families: `orchestrate` is original to this repository, and `skill-creator` is a verbatim copy from `anthropics/claude-plugins-official`. All bundles are deliberately maintained copies, not links to a developer machine and not byte-for-byte upstream mirrors (except `skill-creator`). [provenance.json](provenance.json) records, for each adapted file, the repository, the pinned revision, the path, the SHA-256 of the original and the license. A repo-original bundle has no sources.

Each folder contains a self-contained Agent Skills entrypoint with a name, a targeted discovery description, and the complete adapted workflow. Only the selected phase's fleet-owned catalog is materialized; harnesses may also discover ambient skills. Workers load relevant bodies on demand. None requires runtime downloads, another skill, an upstream script, a particular model, or a particular harness tool API.

## Deliberate adaptations

- Preserve research evidence, behavioral preservation, dependency/wiring checks, verified implementation, regression testing, and evidence-based review.
- Replace repeated human approval gates and UI questions with focused orchestrator messages. Existing task authorization remains authoritative; only actions dependent on an unanswered decision are blocked.
- Remove mandatory nested agent fan-out, provider-specific commands, machine paths, bundled-script assumptions, repetitive persuasion, and fixed response rituals. Workers use the tools actually available in their harness.
- `execute` adapts `execute-plan`; `check` adapts `check-implementation`; `code-review` adapts `review-git-changes`; `handoff` adapts `whats-next`; `document` adapts `document-project`.
- `code-analysis` adapts `complexity-sweep` by using project-native analyzers. The upstream JavaScript engine, its 14-metric implementation, config generator, plugins, and dependency installation are intentionally not bundled. Available measurements are reported honestly; an unavailable analyzer gets an explicitly qualitative review, not fabricated scores.
- `security-review` adapts `security-sweep` into a direct audit workflow with trust-boundary mapping, deduplication, false-positive verification, and evidence reports. It does not claim to ship the upstream scanner scripts, language catalogs, report assembler, or hierarchical agent pipeline.
- `orchestrate` has no upstream. It is original to this repository, reconciled from the retired external `herdr-orchestrator` and `herdr-worker` skills against the real `horch` CLI: every command, flag and lifecycle claim in those two files was checked against `horch --help` and either carried, corrected or dropped. The reconciliation table is in [ai_docs/reports/bake-in-orchestration-inventory.md](../ai_docs/reports/bake-in-orchestration-inventory.md). It is attached to `orchestrator` and `orchestrator-codex` by name and belongs to no phase.
- `skill-creator` is not an adaptation. It is a verbatim copy of the whole skill directory from [anthropics/claude-plugins-official](https://github.com/anthropics/claude-plugins-official/tree/main/plugins/skill-creator) at the revision in `provenance.json`, with its Apache-2.0 `LICENSE.txt`. It is attached to the Claude `orchestrator` by name and belongs to no phase. `horch teammates --check` fails any other teammate that names it, and every Claude pane switches off the official plugin and the claude.ai-synced copy. The skill tells its user to spawn subagents and to run `claude -p`; the orchestrator's persona tells it to take the skill's no-subagent path instead.
- `tdd` retains red/green/refactor for meaningful behavior changes and removes unrelated skill-authoring machinery and destructive instructions to delete existing work. Structural checks are appropriate for low-impact prose/config changes.

## Source mapping

A source path with no repository name is a path in `MattMakes/skill-marketplace`. The [Design skills](#design-skills) table lists more design bundles.

| Bundle | Upstream source |
| --- | --- |
| [brainstorm](brainstorm/SKILL.md) | `plugins/dev/skills/brainstorm/SKILL.md` |
| [research-codebase](research-codebase/SKILL.md) | `plugins/dev/skills/research-codebase/SKILL.md` |
| [trace](trace/SKILL.md) | `plugins/dev/skills/trace/SKILL.md` |
| [create-plan](create-plan/SKILL.md) | `plugins/dev/skills/create-plan/SKILL.md` |
| [pre-flight](pre-flight/SKILL.md) | `plugins/tools/skills/pre-flight/SKILL.md` |
| [execute](execute/SKILL.md) | `plugins/dev/skills/execute-plan/SKILL.md` |
| [tdd](tdd/SKILL.md) | `plugins/dev/skills/tdd/SKILL.md` |
| [debug](debug/SKILL.md) | `plugins/dev/skills/debug/SKILL.md` |
| [check](check/SKILL.md) | `plugins/dev/skills/check-implementation/SKILL.md` |
| [code-analysis](code-analysis/SKILL.md) | `plugins/code/skills/complexity-sweep/SKILL.md` |
| [code-review](code-review/SKILL.md) | `plugins/dev/skills/review-git-changes/SKILL.md` |
| [security-review](security-review/SKILL.md) | `plugins/code/skills/security-sweep/SKILL.md` |
| [handoff](handoff/SKILL.md) | `plugins/dev/skills/whats-next/SKILL.md` |
| [document](document/SKILL.md) | `plugins/dev/skills/document-project/SKILL.md` |
| [orchestrate](orchestrate/SKILL.md) | *(none - repo-original)* |
| [skill-creator](skill-creator/SKILL.md) | `anthropics/claude-plugins-official`: `plugins/skill-creator/skills/skill-creator/` (verbatim) |

## Multi-source skills

A skill may combine several upstream files, from several repositories. Its entry in [provenance.json](provenance.json) then lists `sources`, one item per adapted file: `{repository, revision, path, sha256, license}`. A skill written in this repository has `"sources": []` and an `adaptation` text. The older single-source fields (`source_path`, `source_sha256`, optional `source_repository` and `source_revision`) still parse as a 1-item list. Every bundled skill needs an entry. A new skill keeps `SKILL.md` at or under 12 KB and its whole directory at or under 160 KB, in `.md` and `.txt` files only (plus a `LICENSE` file); `skill-creator` and every vendored skill are exempt from the size budget (see [Vendored skills](#vendored-skills)). `horch skills show <id>` prints each source.

## Vendored skills

A skill is either vendored or adapted. A vendored skill is a verbatim copy of an upstream skill directory (`SKILL.md` and its references), without dotfiles. An adapted skill is a curated copy with a listed set of edits, plus the fleet rules: no subagents, no human approval gate (a message to the orchestrator instead), no machine paths and no `${CLAUDE_*}` variables. Both kinds keep the upstream `LICENSE` file next to `SKILL.md`, because the MIT licence requires the notice. A file named exactly `LICENSE` is the only file without a `.md` or `.txt` extension that a skill directory may hold. The provenance entry lists `sources` (a `https://github.com/<owner>/<repo>` repository, the full 40-hex revision, the path, the sha256 of each copied file and the licence) and `adaptation` (`"verbatim"` or the list of edits). A vendored skill also sets `"vendored": true`. That flag exempts it from the 12 KB and 160 KB budget, not from the text-only rule. `horch skills show <id>` prints `vendored: true` for it. The build never bundles a file or directory whose name starts with `.`.

To re-vendor a skill at a new upstream revision:

1. Bump the pin: update the upstream checkout and its `PINS.txt`, and write the new full revision into every `sources` item of the skill in `provenance.json`.
2. Re-copy: replace the skill directory with the upstream files at that revision, keep `LICENSE`, and re-apply the listed edits for an adapted skill.
3. Re-hash: write the new sha256 of each copied upstream file into its `sources` item.
4. Re-run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.

## Design skills

| Skill | Upstream sources |
| --- | --- |
| [art-direction](art-direction/SKILL.md) | `akseolabs-seo/cinematic-ui`, `leonxlnx/taste-skill` (`minimalist`, `brutalist`, `soft`), `stevembarclay/pencilplaybook` (combined and rewritten; files in `provenance.json`) |
| [brand-identity](brand-identity/SKILL.md) | `nextlevelbuilder/ui-ux-pro-max-skill`: `.claude/skills/brand/` (SKILL.md, references, starter template), `.claude/skills/design/` (logo style guide, logo color psychology, logo industries data); `leonxlnx/taste-skill`: `skills/brandkit/SKILL.md` (non-image parts) |
| [design-imagery](design-imagery/SKILL.md) | `leonxlnx/taste-skill`: `skills/imagegen-frontend-web/`, `skills/imagegen-frontend-mobile/`, `skills/image-to-code-skill/`, `skills/brandkit/`; `nextlevelbuilder/ui-ux-pro-max-skill`: `.claude/skills/design/` logo and CIP prompt references, logo prompt template |
| [design-system](design-system/SKILL.md) | `nextlevelbuilder/ui-ux-pro-max-skill`: `.claude/skills/design-system/` (SKILL.md, references, token validator), `.claude/skills/ui-styling/` (SKILL.md, shadcn and Tailwind references), `.claude/skills/ui-ux-pro-max/` (SKILL.md, references, styles, colors, typography, UX guidelines and shadcn data); `leonxlnx/taste-skill`: `skills/stitch-skill/` (SKILL.md, DESIGN.md) |
| [landing-page](landing-page/SKILL.md) | `felix-huber/ui-landingpage-generator-skill`: `SKILL.md`, `design_prompt.txt`, `generate_landing.sh`; `nextlevelbuilder/ui-ux-pro-max-skill`: `.claude/skills/banner-design/`, `.claude/skills/ui-ux-pro-max/data/landing.csv`, `data/products.csv`, `references/quick-reference.md`; `leonxlnx/taste-skill`: `skills/taste-skill/` (landing parts) |
| [motion-gsap](motion-gsap/SKILL.md) | `greensock/gsap-skills`: `skills/gsap-*/SKILL.md` (8 skills) and `examples/`; `akseolabs-seo/cinematic-ui`: motion parts of `SKILL.md`, `references/implementation-guardrails.md`, `references/data/camera-shots-50.md`, `references/data/interaction-effects-50.md` |
| [ui-redesign](ui-redesign/SKILL.md) | `nutlope/hallmark`: `skills/hallmark/SKILL.md` and its `verbs/redesign.md`, `verbs/audit.md`, `study.md` references; `leonxlnx/taste-skill`: `skills/redesign-skill/`, `skills/taste-skill/`, `skills/output-skill/` |
| [ui-taste](ui-taste/SKILL.md) | `nutlope/hallmark`: `skills/hallmark/SKILL.md` and 17 rule references; `leonxlnx/taste-skill`: `skills/taste-skill/`, `skills/taste-skill-v1/`, `skills/gpt-tasteskill/`, `skills/output-skill/`, `skills/redesign-skill/` |

## Swift and Apple skills

These skills come from MIT-licensed Swift skill repositories. Each keeps its upstream `LICENSE` next to `SKILL.md`. They attach by name to the Swift teammates and belong to no phase. In the Kind column, "verbatim" and "vendored" mean the references are verbatim copies, and a vendored skill is exempt from the size budget. "adapted" means a curated copy with the edits listed in `provenance.json`. The App Store skills never change App Store Connect: each live write becomes a dry-run sent to the orchestrator. Pins, hashes and every edit are in `provenance.json`.

| Skill | Upstream source | Kind |
| --- | --- | --- |
| [app-intents](app-intents/SKILL.md) | `n0an/App-Intents-Agent-Skill`: `app-intents/` (verbatim) | verbatim |
| [app-store-changelog](app-store-changelog/SKILL.md) | `Dimillian/Skills`: `app-store-changelog/` (SKILL.md, references; the `git log` script is inlined) | adapted |
| [appkit-accessibility-auditor](appkit-accessibility-auditor/SKILL.md) | `rgmez/apple-accessibility-skills`: `skills/appkit-accessibility-auditor/` | Adapt, path fix only |
| [appstore-review](appstore-review/SKILL.md) | `3paws-ai/mobile-ai-skills`: `skills/appstore-review/` (SKILL.md, `references/appstore-review-ref.md`) | adapted, vendored |
| [asc-cli-usage](asc-cli-usage/SKILL.md) | `rudrankriyam/app-store-connect-cli-skills`: `skills/asc-cli-usage/SKILL.md` | adapted |
| [asc-crash-triage](asc-crash-triage/SKILL.md) | `rudrankriyam/app-store-connect-cli-skills`: `skills/asc-crash-triage/SKILL.md` | verbatim |
| [asc-id-resolver](asc-id-resolver/SKILL.md) | `rudrankriyam/app-store-connect-cli-skills`: `skills/asc-id-resolver/SKILL.md` | verbatim |
| [asc-metadata-sync](asc-metadata-sync/SKILL.md) | `rudrankriyam/app-store-connect-cli-skills`: `skills/asc-metadata-sync/SKILL.md` | adapted, dry-run only |
| [asc-submission-health](asc-submission-health/SKILL.md) | `rudrankriyam/app-store-connect-cli-skills`: `skills/asc-submission-health/` (SKILL.md, `references/readiness-repairs.md`) | adapted, diagnosis only |
| [asc-xcode-build](asc-xcode-build/SKILL.md) | `rudrankriyam/app-store-connect-cli-skills`: `skills/asc-xcode-build/SKILL.md` | adapted |
| [background-execution](background-execution/SKILL.md) | `n0an/Background-Execution-Agent-Skill`: `background-execution/` (verbatim) | verbatim |
| [ios-simulator-run](ios-simulator-run/SKILL.md) | `Dimillian/Skills`: `ios-debugger-agent/` | Adapt, rewritten as a playbook |
| [observability](observability/SKILL.md) | `n0an/Observability-Agent-Skill`: `observability/` | vendored |
| [swift-code-audit](swift-code-audit/SKILL.md) | `jazzychad/ios-code-audit`: `SKILL.md`, `references/` | Adapt, heavily |
| [swift-concurrency-pro](swift-concurrency-pro/SKILL.md) | `twostraws/Swift-Concurrency-Agent-Skill`: `swift-concurrency-pro/`; `AvdLee/Swift-Concurrency-Agent-Skill`: `skills/swift-concurrency/SKILL.md` (build-settings table) | adapted |
| [swift-focusengine-pro](swift-focusengine-pro/SKILL.md) | `mhaviv/Swift-FocusEngine-Agent-Skill`: repository root (verbatim) | verbatim |
| [swift-format-style](swift-format-style/SKILL.md) | `n0an/Swift-FormatStyle-Agent-Skill`: `swift-format-style/` | adapted |
| [swift-security-expert](swift-security-expert/SKILL.md) | `ivan-magda/swift-security-skill`: `swift-security-expert/` | Adapt SKILL.md, vendored references |
| [swift-testing-pro](swift-testing-pro/SKILL.md) | `twostraws/Swift-Testing-Agent-Skill`: `swift-testing-pro/` | adapted, light |
| [swiftdata-pro](swiftdata-pro/SKILL.md) | `twostraws/SwiftData-Agent-Skill`: `swiftdata-pro/`; `vanab/swiftdata-agent-skill`: `swiftdata-expert-skill/references/migrations-and-history.md`, `core-data-adoption.md`, `concurrency-and-actors.md` | adapted |
| [swiftdata-testing](swiftdata-testing/SKILL.md) | `akshaypimprikar/ios-swiftdata-testing-agent-skill`: `swiftdata-testing/` | Adapt |
| [swiftui-accessibility-auditor](swiftui-accessibility-auditor/SKILL.md) | `rgmez/apple-accessibility-skills`: `skills/swiftui-accessibility-auditor/` | Adapt, path fix only |
| [swiftui-liquid-glass](swiftui-liquid-glass/SKILL.md) | `Dimillian/Skills`: `swiftui-liquid-glass/` | adapted |
| [swiftui-performance-audit](swiftui-performance-audit/SKILL.md) | `Dimillian/Skills`: `swiftui-performance-audit/` | Adapt |
| [swiftui-pro](swiftui-pro/SKILL.md) | `twostraws/SwiftUI-Agent-Skill`: `swiftui-pro/` (top-level copy) | adapted |
| [uikit-accessibility-auditor](uikit-accessibility-auditor/SKILL.md) | `rgmez/apple-accessibility-skills`: `skills/uikit-accessibility-auditor/` | Adapt, path fix only |
| [widgets](widgets/SKILL.md) | `n0an/Widgets-Agent-Skill`: `widgets/` (verbatim) | verbatim |
| [writing-for-interfaces](writing-for-interfaces/SKILL.md) | `andrewgleave/skills`: `writing-for-interfaces/` | Adapt SKILL.md, vendored references |

## Unreal Engine skills

31 bundles vendored from `quodsoler/unreal-engine-skills` (MIT, revision `f3742d7b688690810df369802b90430324e380b9`). 30 are verbatim copies of the upstream skill directory, minus dotfiles, plus the upstream `LICENSE` next to `SKILL.md`. `ue-project-context` is adapted: its interview becomes `[unknown]` markers and one `QUESTION:` to the orchestrator, and its `provenance.json` entry lists the edits. Each entry has `"vendored": true`. These skills belong to no phase; a Unreal Engine teammate attaches them by name.

The 3 `own text` skills are written in this repository and have no upstream copy. Their rows name the upstream skills or documents they consulted. `blender-ue-pipeline` is for the Blender teammate that hands assets to the Unreal teammates.

| Skill | Upstream source | Copy |
| --- | --- | --- |
| [blender-ue-pipeline](blender-ue-pipeline/SKILL.md) | *(none - repo-original; checked against the Blender 5.2 API and Epic's 5.8 FBX pipeline docs)* | own text |
| [ue-actor-component-architecture](ue-actor-component-architecture/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-actor-component-architecture/` | verbatim |
| [ue-ai-navigation](ue-ai-navigation/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-ai-navigation/` | verbatim |
| [ue-animation-system](ue-animation-system/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-animation-system/` | verbatim |
| [ue-async-threading](ue-async-threading/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-async-threading/` | verbatim |
| [ue-audio-system](ue-audio-system/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-audio-system/` | verbatim |
| [ue-blueprint-cpp-interop](ue-blueprint-cpp-interop/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-blueprint-cpp-interop/` | verbatim |
| [ue-build-verify](ue-build-verify/SKILL.md) | *(none - repo-original; consulted `quodsoler/unreal-engine-skills` `ue-testing-debugging` and `ue-module-build-system`)* | own text |
| [ue-character-movement](ue-character-movement/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-character-movement/` | verbatim |
| [ue-cpp-foundations](ue-cpp-foundations/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-cpp-foundations/` | verbatim |
| [ue-data-assets-tables](ue-data-assets-tables/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-data-assets-tables/` | verbatim |
| [ue-editor-scripting](ue-editor-scripting/SKILL.md) | *(none - repo-original; consulted `quodsoler/unreal-engine-skills` `ue-testing-debugging`)* | own text |
| [ue-editor-tools](ue-editor-tools/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-editor-tools/` | verbatim |
| [ue-game-features](ue-game-features/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-game-features/` | verbatim |
| [ue-gameplay-abilities](ue-gameplay-abilities/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-gameplay-abilities/` | verbatim |
| [ue-gameplay-cameras](ue-gameplay-cameras/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-gameplay-cameras/` | verbatim |
| [ue-gameplay-framework](ue-gameplay-framework/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-gameplay-framework/` | verbatim |
| [ue-gameplay-tags-messaging](ue-gameplay-tags-messaging/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-gameplay-tags-messaging/` | verbatim |
| [ue-input-system](ue-input-system/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-input-system/` | verbatim |
| [ue-mass-entity](ue-mass-entity/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-mass-entity/` | verbatim |
| [ue-materials-rendering](ue-materials-rendering/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-materials-rendering/` | verbatim |
| [ue-module-build-system](ue-module-build-system/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-module-build-system/` | verbatim |
| [ue-mover](ue-mover/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-mover/` | verbatim |
| [ue-networking-replication](ue-networking-replication/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-networking-replication/` | verbatim |
| [ue-niagara-effects](ue-niagara-effects/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-niagara-effects/` | verbatim |
| [ue-physics-collision](ue-physics-collision/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-physics-collision/` | verbatim |
| [ue-procedural-generation](ue-procedural-generation/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-procedural-generation/` | verbatim |
| [ue-project-context](ue-project-context/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-project-context/` | adapted |
| [ue-sequencer-cinematics](ue-sequencer-cinematics/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-sequencer-cinematics/` | verbatim |
| [ue-serialization-savegames](ue-serialization-savegames/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-serialization-savegames/` | verbatim |
| [ue-state-trees](ue-state-trees/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-state-trees/` | verbatim |
| [ue-testing-debugging](ue-testing-debugging/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-testing-debugging/` | verbatim |
| [ue-ui-umg-slate](ue-ui-umg-slate/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-ui-umg-slate/` | verbatim |
| [ue-world-level-streaming](ue-world-level-streaming/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-world-level-streaming/` | verbatim |

## Godot skills

Skills for Godot 4.7 teammates. They belong to no phase; a Godot teammate attaches them by name.

18 skills are vendored from `jame581/GodotPrompter` (MIT, revision `3e8d0f005f9604e1dbdad3de693e39555384c5af`) and have no `gd-agentic-skills` twin. 16 are copies of the upstream skill directory, minus dotfiles, with the `godot-` prefix on the name and on every cross-reference, plus the upstream `LICENSE` next to `SKILL.md`. `godot-grill` and `godot-brainstorming` are adapted: they ask the orchestrator one `QUESTION:` instead of asking a human, and their `provenance.json` entries list the edits. Each entry has `"vendored": true`. The 5 addon skills name their pinned addon version in the text.

| Skill | Upstream source | Copy |
| --- | --- | --- |
| [godot-addon-development](godot-addon-development/SKILL.md) | `jame581/GodotPrompter`: `skills/addon-development/` | renamed |
| [godot-assets-pipeline](godot-assets-pipeline/SKILL.md) | `jame581/GodotPrompter`: `skills/assets-pipeline/` | renamed |
| [godot-beehave](godot-beehave/SKILL.md) | `jame581/GodotPrompter`: `skills/beehave/` | renamed |
| [godot-brainstorming](godot-brainstorming/SKILL.md) | `jame581/GodotPrompter`: `skills/brainstorming/` | adapted |
| [godot-build-verify](godot-build-verify/SKILL.md) | *(none - repo-original; consulted gd-agentic-skills `godot-builder` for facts; run on Godot 4.7.2)* | own text |
| [godot-camera-system](godot-camera-system/SKILL.md) | `jame581/GodotPrompter`: `skills/camera-system/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-camera-systems/`) | combined |
| [godot-code-review](godot-code-review/SKILL.md) | `jame581/GodotPrompter`: `skills/godot-code-review/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-auditor/`, `godot-analyst/`) | combined |
| [godot-combat-system](godot-combat-system/SKILL.md) | *(none - repo-original; consulted `thedivergentai/gd-agentic-skills`: `godot-combat-system/`, `godot-turn-system/`; code run on Godot 4.7.2)* | own text |
| [godot-csharp-godot](godot-csharp-godot/SKILL.md) | `jame581/GodotPrompter`: `skills/csharp-godot/` | renamed |
| [godot-csharp-signals](godot-csharp-signals/SKILL.md) | `jame581/GodotPrompter`: `skills/csharp-signals/` | renamed |
| [godot-dedicated-server](godot-dedicated-server/SKILL.md) | `jame581/GodotPrompter`: `skills/dedicated-server/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-server-architecture/`, `godot-multiplayer-networking/`) | combined |
| [godot-dialogue-manager](godot-dialogue-manager/SKILL.md) | `jame581/GodotPrompter`: `skills/dialogue-manager/` | renamed |
| [godot-dimension-port](godot-dimension-port/SKILL.md) | *(none - repo-original; consulted `thedivergentai/gd-agentic-skills` `godot-adapt-2d-to-3d` and `godot-adapt-3d-to-2d`; checked against Godot 4.7.2)* | own text |
| [godot-economy-system](godot-economy-system/SKILL.md) | *(none - repo-original; consulted `thedivergentai/gd-agentic-skills`: `godot-economy-system/`; code run on Godot 4.7.2)* | own text |
| [godot-export-pipeline](godot-export-pipeline/SKILL.md) | `jame581/GodotPrompter`: `skills/export-pipeline/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-export-builds/`, `godot-platform-desktop/`, `godot-platform-web/`, `godot-platform-console/`, `godot-adapt-mobile-to-desktop/`) | combined |
| [godot-gameplay-loops](godot-gameplay-loops/SKILL.md) | *(none - repo-original; consulted `thedivergentai/gd-agentic-skills`: `godot-game-loop-collection/`, `godot-game-loop-harvest/`, `godot-game-loop-time-trial/`, `godot-game-loop-waves/`, `godot-mechanic-revival/`, `godot-mechanic-secrets/`; code run on Godot 4.7.2)* | own text |
| [godot-gdextension](godot-gdextension/SKILL.md) | `jame581/GodotPrompter`: `skills/gdextension/` | renamed |
| [godot-gdscript-patterns](godot-gdscript-patterns/SKILL.md) | `jame581/GodotPrompter`: `skills/gdscript-patterns/` | renamed |
| [godot-genre-blueprints](godot-genre-blueprints/SKILL.md) | *(none - repo-original; consulted `thedivergentai/gd-agentic-skills` @ `4c4d0ff`: the 27 `godot-genre-*` skills and `godot-project-templates`)* | own text |
| [godot-grill](godot-grill/SKILL.md) | `jame581/GodotPrompter`: `skills/grill/` | adapted |
| [godot-hud-system](godot-hud-system/SKILL.md) | `jame581/GodotPrompter`: `skills/hud-system/` | renamed |
| [godot-input-handling](godot-input-handling/SKILL.md) | `jame581/GodotPrompter`: `skills/input-handling/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-input-handling/`) | combined |
| [godot-inventory-system](godot-inventory-system/SKILL.md) | `jame581/GodotPrompter`: `skills/inventory-system/`; own references consulted `thedivergentai/gd-agentic-skills`: `godot-inventory-system` | combined |
| [godot-limboai](godot-limboai/SKILL.md) | `jame581/GodotPrompter`: `skills/limboai/` | renamed |
| [godot-localization](godot-localization/SKILL.md) | `jame581/GodotPrompter`: `skills/localization/` | renamed |
| [godot-math-essentials](godot-math-essentials/SKILL.md) | `jame581/GodotPrompter`: `skills/math-essentials/` | renamed |
| [godot-mobile-development](godot-mobile-development/SKILL.md) | `jame581/GodotPrompter`: `skills/mobile-development/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-platform-mobile/`, `godot-adapt-desktop-to-mobile/`) | combined |
| [godot-multiplayer-basics](godot-multiplayer-basics/SKILL.md) | `jame581/GodotPrompter`: `skills/multiplayer-basics/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-adapt-single-to-multiplayer/`, `godot-multiplayer-networking/`) | combined |
| [godot-multiplayer-sync](godot-multiplayer-sync/SKILL.md) | `jame581/GodotPrompter`: `skills/multiplayer-sync/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-multiplayer-networking/`, `godot-adapt-single-to-multiplayer/`) | combined |
| [godot-multithreading](godot-multithreading/SKILL.md) | `jame581/GodotPrompter`: `skills/multithreading/` | renamed |
| [godot-phantom-camera](godot-phantom-camera/SKILL.md) | `jame581/GodotPrompter`: `skills/phantom-camera/` | renamed |
| [godot-physics-system](godot-physics-system/SKILL.md) | `jame581/GodotPrompter`: `skills/physics-system/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-2d-physics/`, `godot-physics-3d/`, `godot-raycasting-queries/`) | combined |
| [godot-player-controller](godot-player-controller/SKILL.md) | `jame581/GodotPrompter`: `skills/player-controller/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-characterbody-2d/`) | combined |
| [godot-popochiu](godot-popochiu/SKILL.md) | `jame581/GodotPrompter`: `skills/popochiu/` | renamed |
| [godot-project-context](godot-project-context/SKILL.md) | *(none - repo-original; shape of `ue-project-context`; run on Godot 4.7.2)* | own text |
| [godot-quest-system](godot-quest-system/SKILL.md) | *(none - repo-original; consulted `thedivergentai/gd-agentic-skills`: `godot-quest-system/`; code run on Godot 4.7.2)* | own text |
| [godot-responsive-ui](godot-responsive-ui/SKILL.md) | `jame581/GodotPrompter`: `skills/responsive-ui/` | renamed |
| [godot-save-load](godot-save-load/SKILL.md) | `jame581/GodotPrompter`: `skills/save-load/`; own references consulted `thedivergentai/gd-agentic-skills`: `godot-save-load-systems` | combined |
| [godot-scene-files](godot-scene-files/SKILL.md) | *(none - repo-original; consulted gd-agentic-skills `godot-builder` for facts; run on Godot 4.7.2)* | own text |
| [godot-shader-basics](godot-shader-basics/SKILL.md) | `jame581/GodotPrompter`: `skills/shader-basics/`; own-text references, consulted gd-agentic-skills `godot-shaders-basics` | combined |
| [godot-state-machine](godot-state-machine/SKILL.md) | `jame581/GodotPrompter`: `skills/state-machine/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-state-machine-advanced/`) | combined |
| [godot-testing](godot-testing/SKILL.md) | `jame581/GodotPrompter`: `skills/godot-testing/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-testing-patterns/`) | combined |
| [godot-tween-animation](godot-tween-animation/SKILL.md) | `jame581/GodotPrompter`: `skills/tween-animation/`; own-text references, consulted gd-agentic-skills `godot-tweening` | combined |
| [godot-ui](godot-ui/SKILL.md) | `jame581/GodotPrompter`: `skills/godot-ui/`; own-text references, consulted gd-agentic-skills `godot-ui-theming`, `godot-ui-rich-text`, `godot-ui-containers` | combined |
| [godot-version-migration](godot-version-migration/SKILL.md) | *(none - repo-original; consulted `thedivergentai/gd-agentic-skills` `godot-version-migration`; checked against the Godot 4.7 upgrade guides and the 4.7.2 doctool dump)* | own text |
| [godot-xr-development](godot-xr-development/SKILL.md) | `jame581/GodotPrompter`: `skills/xr-development/` + own references (consulted `thedivergentai/gd-agentic-skills`: `godot-platform-vr/`) | combined |
