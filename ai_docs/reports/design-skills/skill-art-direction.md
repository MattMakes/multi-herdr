# D02 skill-art-direction: report

Unit `skill-art-direction`, branch `ds/skill-art-direction`, worker `opus-24`.
Result: one new bundled skill, `skills/art-direction/` (SKILL.md 10755 bytes, directory 92 KB, 18 text files).

## Source inventory

Sources at `_sources/design-skills/`, pinned commits from `PINS.txt`.

| Repository | Files read | Kind |
|---|---|---|
| cinematic-ui `24a66c1` | `SKILL.md`, `directors-library.md`, `references/{anti-convergence,anti-garbage,premium-calibration,reference-protocol,implementation-guardrails,library-index,output-templates}.md`, `references/data/{README,visual-styles,section-functions,narrative-beats,image-direction}.md` in full; `references/data/{camera-shots-50,compositions,color-grades,font-moods,typography-cinema,hero-archetypes,section-archetypes,textures,background-techniques,visual-elements,interaction-effects-50,directors-200}.md` by structure plus the entries for the chosen directors; `dna-index.tsv` header and format; `agents/openai.yaml`, `AGENTS.md`, `skill.json` | Director-led workflow (decisions, storyboard, compiled spec, build), 200+ director table, large CSS libraries |
| cinematic-ui (not read in depth) | `references/data/design-dna-db.txt` (2.3 MB), `docs/demo.mp4`, `docs/banner.svg`, `CLAUDE.md`, `CODEX.md`, `GEMINI.md`, `.cursor/`, `.windsurf/`, `.github/`, READMEs in 3 languages | Data dump, media, per-harness install shims |
| taste-skill `ce26fc2` | `skills/minimalist-skill/SKILL.md`, `skills/brutalist-skill/SKILL.md`, `skills/soft-skill/SKILL.md` | 3 style protocols with exact tokens |
| pencilplaybook `b732429` | `SKILL.md`, `setup.md`, `onboarding.md`, `README.md`, `CHANGELOG.md`, `presets/*.json` (7), `references/tool-reference.md`, `.claude/skills/best-in-world-research/SKILL.md`, `.claude/skills/best-in-world-strategy/{SKILL.md,references/question-bank.md}` | Pencil MCP workflows, 9 scaffolds, perceptual defaults tables, 7 token presets |

## Source-to-skill map

| Skill file | Built from |
|---|---|
| `SKILL.md` workflow | cinematic-ui `SKILL.md` phases 1 to 3 (decisions, storyboard, spec order: grammar, scene, signature composition, shared system last), its demo-uniqueness protocol (shell-ban list), `anti-convergence.md` (3 film-selection questions, hash tie-break), `premium-calibration.md` (one big idea, restraint statement, grid fallback test). |
| `SKILL.md` rules and review checklist | cinematic-ui `anti-garbage.md` (interaction budget, entrance variety, process-language ban, final review questions), `implementation-guardrails.md` (screening checks), taste-skill `soft-skill` performance guardrails (transform/opacity only, blur on fixed layers), pencilplaybook floors. |
| `references/storyboard.md` | cinematic-ui `output-templates.md` (storyboard fields), `narrative-beats.md` (25 beats, compressed to 24 rows), `section-functions.md`, `compositions.md` (families), `camera-shots-50.md` (entrance vocabulary), `reference-protocol.md` (reference classification and roles), `image-direction.md` (image notes per shot), `anti-convergence.md` required checks. |
| `references/perception.md` | pencilplaybook `SKILL.md` "Perceptual Design Defaults" (typography, color, motion, spacing, icons tables, with their WCAG and HIG citations). The hierarchy, attention and grouping sections are written for this skill: the source only states the plan's three topics through its tables, so the principles (levers of hierarchy, scan paths, isolation effect, grouping cues) are ours, each with a concrete check. |
| `references/direction-contract-template.md` | cinematic-ui `output-templates.md` (decisions and compiled spec fields), pencilplaybook token map (role names), soft and minimalist token values as the pattern. New: the change log and the checkable "Do not" rule. |
| `references/pencil.md` | pencilplaybook `SKILL.md` (session startup, canvas archaeology, token propagation, spatial management, naming, edge cases) and `references/tool-reference.md` (25-op limit, operation forms, guideline topics). |
| `directions/editorial-minimal.md` | taste-skill `minimalist-skill` (tokens, pastel tag pairs, components, motion), soft-skill "Editorial Luxury" archetype, pencilplaybook `grove` and `minimal` presets (neutral variant). |
| `directions/swiss-industrial.md` | taste-skill `brutalist-skill` "Swiss Industrial Print" mode, cinematic-ui `visual-styles.md` #25 Swiss. |
| `directions/tactical-terminal.md` | taste-skill `brutalist-skill` "Tactical Telemetry" mode, pencilplaybook `ember` preset, cinematic-ui `visual-styles.md` #38 Terminal. |
| `directions/soft-premium.md` | taste-skill `soft-skill` (double bezel, island nav, button-in-button, motion), "Soft Structuralism" archetype. |
| `directions/midnight-glass.md` | taste-skill `soft-skill` "Ethereal Glass" archetype, pencilplaybook `midnight` preset. |
| `directions/neo-brutalist.md` | pencilplaybook `volt` preset, cinematic-ui `visual-styles.md` #34 Neubrutalism. |
| `directions/bloom-playful.md` | pencilplaybook `bloom` preset. |
| `directions/cinematic-monumental.md` | cinematic-ui directors table (Villeneuve, Cuaron, Melville), Villeneuve arcs from `narrative-beats.md`, `color-grades.md` #49 Villeneuve Dust, compositions #1, #73, #74. |
| `directions/cinematic-mood.md` | Wong Kar-wai and Sofia Coppola rows and arcs, `color-grades.md` #19, compositions #51, camera #21 crossfade. |
| `directions/cinematic-symmetry.md` | Wes Anderson row and arcs, compositions #3, #39, #52, #76, camera #36 crab shot. |
| `directions/cinematic-evidence.md` | David Fincher and Errol Morris rows, Fincher arcs, camera #23 morph cut. |
| `directions/cinematic-luminous.md` | Makoto Shinkai and Hayao Miyazaki rows and arcs, camera #33 parallax depth. |
| `directions/cinematic-noir.md` | Film Noir rows (Wilder, Rodriguez, Johnnie To, Melville), `visual-styles.md` #3, camera #11 venetian blind. |

13 directions in total (the plan asked for 8 to 14).

## What was dropped and why

- **pencilplaybook `.claude/skills/best-in-world-research` and `best-in-world-strategy`: dropped.** They are generic research and decision frameworks (any domain: security, finance, org design), not design practice. They add nothing to choosing or holding a visual direction that the reference-decomposition step and the three film-selection questions do not already do. Their core move, "name the specific elite practitioner", invites invented attributions in a fleet with no guaranteed web access. The fleet already bundles `brainstorm` and `research-codebase` for decisions and research.
- pencilplaybook `skill-creator` (the skill map already excludes it), `setup.md` wizard and `onboarding.md` (interactive setup that edits the skill file itself; the contract replaces the configured token map), the 9 `batch_design` scaffold scripts (Pencil-only, app-shell layouts that are not art direction; the optional `pencil.md` keeps the canvas rules), the `material` preset (a framework default; pencilplaybook's own changelog says framework defaults cause averaged output), README marketing.
- cinematic-ui: the start questionnaire and per-phase user approvals (replaced with orchestrator questions for blocking decisions only), the delegation model (sub-agents are not allowed in the fleet), the `compiled-spec.md` phase with full CSS/JS per section (that is build work; it belongs to the builder, and `motion-gsap` covers the motion side), the source-id citation requirement against its own 50-entry libraries (we do not carry those libraries), `design-dna-db.txt` and `dna-index.tsv` (2.5 MB of scraped site data, CSV-like, outside the size budget), the External Library Decision block and the CDN library list (implementation, and partly `motion-gsap` scope), the 200-director table (13 curated directions replace it; a builder who wants another film can still follow the cinematic research steps), the per-harness files and install paths, Chinese-language duplicates.
- taste-skill: persona and "$150k agency" framing, the "silently roll the dice" ritual, `picsum.photos` placeholders (network dependency; image work belongs to `design-imagery`), the blanket font bans (moved into per-direction choices; see conflicts).

## Conflicts between sources and resolutions

| Topic | Sources disagree | Resolution |
|---|---|---|
| Motion duration | pencilplaybook: 400 ms ceiling, never above 500 ms. cinematic-ui: 0.8 s and up, fades of 2 s, slow directors 3 s. soft-skill: 800 ms and up. | Split by purpose. UI feedback (hover, press, menus): 100 to 400 ms, a floor. Scene entrances: per direction, at most 1200 ms, never blocking input. The 3 s figures are dropped as hostile to use. |
| Entrance variety | minimalist and soft: one fade-up on every block. cinematic-ui: fade-up at most 2 per page, at least 4 entrance types, no adjacent repeats. | Cinematic rule is the default; a direction may declare a single quiet entrance (only `editorial-minimal` does), because restraint is that direction's character. |
| Card radius | pencilplaybook: 8 px maximum, larger "reads consumer". soft-skill: 2 rem squircles. brutalist: 0. | Radius is a direction token. The 8 px value stays in `perception.md` as the default for product UI, not a floor. |
| Borders and shadows | minimalist: every card `1px solid #EAEAEA`. soft-skill: bans 1 px gray borders and requires the double bezel. | Direction-specific; each direction file lists its own as a checkable "Do not". |
| Font bans | minimalist bans Inter and Roboto; soft-skill bans Inter, Roboto, Arial, Helvetica; brutalist recommends Inter Black and Neue Haas; pencilplaybook `midnight` uses Inter. | No global ban in this skill. Each direction names its own stacks and avoids the defaults it does not need (`midnight-glass` uses Geist rather than Inter). Anti-default font policy is `ui-taste` scope. |
| Pure white text on dark | cinematic and brutalist allow it; pencilplaybook: halation, use off-white for body. | Off-white for body text everywhere; headlines may be white. |
| Light and dark in one site | brutalist: never mix substrates. Others silent. | Kept as a per-direction rule where it matters (swiss/terminal split into two directions). |
| Interactive approval | cinematic-ui requires a questionnaire and user approval between phases; pencilplaybook onboarding asks one question. | Removed. The worker records assumptions in the contract and asks the orchestrator only when blocked. |
| Film research | cinematic-ui: web research of the film is required. | Required when a web tool exists; otherwise the contract says "inferred" and work continues. |

## Contrast verification

Every color pair with a stated ratio in the direction files was computed with the WCAG relative-luminance formula. 18 stated ratios in my first draft were wrong by 0.1 to 0.5 and now show the computed value. 3 accent tokens fail 4.5:1 as body text: `cinematic-symmetry` accent was darkened from `#B8473C` (4.2:1) to `#A23B31` (5.3:1); `cinematic-mood` red (3.6:1) and `cinematic-noir` red (3.2:1) are marked "large text and fills only".

## Example trigger lines

- "Pick a visual direction for the Halden Robotics launch site and write the direction contract before the builders start."
- "Our three landing pages all look like the same Framer template; restate an art direction and storyboard the home page."
- "The client wants the new hotel site to feel like a film, not like their last site; plan it shot by shot before any code."

## Gotchas

- The skill is useful only if builders read `DESIGN-DIRECTION.md`. Personas that use `art-direction` (D07) should tell builders to read the contract first.
- `references/pencil.md` names Pencil tools exactly, by design (the plan asks for Pencil specifics in one optional file). The main workflow never needs them.
- The token-drift and process-language checks use `rg` with `src/` as the example path; builders adapt the path.

- Flaky test (not caused by this unit): `horch-e2e --test dataset` failed in 2 of 3 gate runs under machine load (load average about 7). The failures were `cmp_04_n_worktrees_same_base_modify_same_file` (candidate C frozen with an empty numstat) and once a preflight refusal. `cmp_04` then passed 3 of 3 runs alone, and the third full gate passed. This unit changes only Markdown and JSON under `skills/`.

## Follow-ups

- Provenance: 1 entry in `skills/provenance.json` with 26 sources (cinematic-ui 15 files, taste-skill 3, pencilplaybook 8), each with the `PINS.txt` revision, sha256. The entry is first in the list, in alphabetical order by skill id.
- README: 1 row in the `skills/README.md` source mapping table.
- Tests: no test or oracle changed. The orchestrator's fix on `design-skills` limits the `horch skills` oracles to the 16 A0 skills.
- Not done (outside my scope): `art-direction` belongs to no phase yet. D07 attaches it to personas by name.
