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

Verbatim copies of MIT-licensed upstream skills, each with its upstream `LICENSE`. They attach by name to the Swift teammates and belong to no phase.

| Skill | Upstream source |
| --- | --- |
| [app-intents](app-intents/SKILL.md) | `n0an/App-Intents-Agent-Skill`: `app-intents/` (verbatim) |
| [background-execution](background-execution/SKILL.md) | `n0an/Background-Execution-Agent-Skill`: `background-execution/` (verbatim) |
| [swift-focusengine-pro](swift-focusengine-pro/SKILL.md) | `mhaviv/Swift-FocusEngine-Agent-Skill`: repository root (verbatim) |
| [widgets](widgets/SKILL.md) | `n0an/Widgets-Agent-Skill`: `widgets/` (verbatim) |
These skills come from MIT-licensed Swift skill repositories. Each keeps its upstream `LICENSE` next to `SKILL.md`. "Vendored" means a verbatim copy; "adapted" means a curated copy with the edits listed in `provenance.json`. They attach by name to the Swift teammates and belong to no phase.

| Skill | Kind | Upstream source |
| --- | --- | --- |
| [observability](observability/SKILL.md) | vendored | `n0an/Observability-Agent-Skill`: `observability/` |
| [swift-concurrency-pro](swift-concurrency-pro/SKILL.md) | adapted | `twostraws/Swift-Concurrency-Agent-Skill`: `swift-concurrency-pro/`; `AvdLee/Swift-Concurrency-Agent-Skill`: `skills/swift-concurrency/SKILL.md` (build-settings table) |
| [swift-format-style](swift-format-style/SKILL.md) | adapted | `n0an/Swift-FormatStyle-Agent-Skill`: `swift-format-style/` |
| [swift-testing-pro](swift-testing-pro/SKILL.md) | adapted, light | `twostraws/Swift-Testing-Agent-Skill`: `swift-testing-pro/` |
| [swiftdata-pro](swiftdata-pro/SKILL.md) | adapted | `twostraws/SwiftData-Agent-Skill`: `swiftdata-pro/`; `vanab/swiftdata-agent-skill`: `swiftdata-expert-skill/references/migrations-and-history.md`, `core-data-adoption.md` |
| [swiftui-liquid-glass](swiftui-liquid-glass/SKILL.md) | adapted | `Dimillian/Skills`: `swiftui-liquid-glass/` |
| [swiftui-pro](swiftui-pro/SKILL.md) | adapted | `twostraws/SwiftUI-Agent-Skill`: `swiftui-pro/` (top-level copy) |
## Unreal Engine skills

31 bundles vendored from `quodsoler/unreal-engine-skills` (MIT, revision `f3742d7b688690810df369802b90430324e380b9`). 30 are verbatim copies of the upstream skill directory, minus dotfiles, plus the upstream `LICENSE` next to `SKILL.md`. `ue-project-context` is adapted: its interview becomes `[unknown]` markers and one `QUESTION:` to the orchestrator, and its `provenance.json` entry lists the edits. Each entry has `"vendored": true`. These skills belong to no phase; a Unreal Engine teammate attaches them by name.

| Skill | Upstream source | Copy |
| --- | --- | --- |
| [ue-actor-component-architecture](ue-actor-component-architecture/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-actor-component-architecture/` | verbatim |
| [ue-ai-navigation](ue-ai-navigation/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-ai-navigation/` | verbatim |
| [ue-animation-system](ue-animation-system/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-animation-system/` | verbatim |
| [ue-async-threading](ue-async-threading/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-async-threading/` | verbatim |
| [ue-audio-system](ue-audio-system/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-audio-system/` | verbatim |
| [ue-blueprint-cpp-interop](ue-blueprint-cpp-interop/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-blueprint-cpp-interop/` | verbatim |
| [ue-character-movement](ue-character-movement/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-character-movement/` | verbatim |
| [ue-cpp-foundations](ue-cpp-foundations/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-cpp-foundations/` | verbatim |
| [ue-data-assets-tables](ue-data-assets-tables/SKILL.md) | `quodsoler/unreal-engine-skills`: `skills/ue-data-assets-tables/` | verbatim |
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
