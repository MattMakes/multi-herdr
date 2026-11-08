# Map: hark

Spec: `ai_docs/plans/hark/decisions.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [Spec: core audio I/O, features, CLI shell, models and doctor](01-core-io.md) |  | 1 |
| 02 | [Spec: describe, match, rank, find, compare, realness, profile and the vocabulary](02-listen.md) |  | 1 |
| 03 | [Spec: timeline, sheet, video, critique and the launch profile](03-timeline.md) |  | 1 |
| 04 | [Spec: the forge engine (render, DSP, FX, tone, polish, knobs, schema, search, critic, refsets, viz)](04-forge-core.md) |  | 1 |
| 05 | [Spec: the impact and UI recipes](05-recipes-impacts.md) |  | 1 |
| 06 | [Spec: the motion, riser, glitch and notify recipes](06-recipes-motion.md) |  | 1 |
| 07 | [Spec: the sound bank, sfx render and film kits](07-bank-kit-sfx.md) |  | 1 |
| 08 | [Spec: fit, audio to recipe code](08-fit.md) |  | 1 |
| 09 | [Spec: gallery, MCP server, LLM critique, limits and integration](09-surfaces.md) |  | 1 |
| 10 | [Spec: the 2 agent skills and their workflows](10-skills-and-docs.md) |  | 1 |
| 11 | [Probe: can agy -p hear an audio file?](11-agy-audio-probe.md) |  | 1 |
| 12 | [Architecture: module map, contracts, venv and test plan of hark](12-architecture.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11 | 2 |
| 13 | [Build hark core: audio I/O, features, CLI, models, doctor and launcher](13-impl-core.md) | 12 | 3 |
| 14 | [Build hark listen: describe, match, rank, find, compare, realness, profile](14-impl-listen.md) | 12 | 3 |
| 15 | [Build hark timeline: timeline, sheet, video, critique](15-impl-timeline.md) | 12 | 3 |
| 16 | [Build the hark forge engine](16-impl-forge-core.md) | 12 | 3 |
| 17 | [Build the 16 impact and UI recipes](17-impl-recipes-impacts.md) | 12 | 3 |
| 18 | [Build the 15 motion, riser, glitch and notify recipes](18-impl-recipes-motion.md) | 12 | 3 |
| 19 | [Build the hark bank, sfx render and kits](19-impl-bank.md) | 12 | 3 |
| 20 | [Build hark fit: audio to recipe code](20-impl-fit.md) | 12 | 3 |
| 21 | [Build hark surfaces: gallery, MCP server, agy critique](21-impl-surfaces.md) | 12 | 3 |
| 22 | [Write the hark and sound-sourcing skills](22-skills.md) | 12 | 3 |
| 23 | [Add the sound-engineer teammate and wire the skills into the catalog](23-sound-engineer.md) | 22 | 4 |
| 24 | [Add the audio-reviewer teammate on agy](24-audio-reviewer.md) | 11, 22 | 4 |
| 25 | [Integrate hark end to end and run every acceptance test](25-integration.md) | 13, 14, 15, 16, 17, 18, 19, 20, 21 | 4 |
| 26 | [Clean-room similarity gate against the source](26-similarity-gate.md) | 22, 23, 24, 25 | 5 |

## Waves

- Wave 1: 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11
- Wave 2: 12
- Wave 3: 13, 14, 15, 16, 17, 18, 19, 20, 21, 22
- Wave 4: 23, 24, 25
- Wave 5: 26

## Critical path

01-10 -> 12 -> 13-21 -> 25 -> 26

## Open questions

None.
