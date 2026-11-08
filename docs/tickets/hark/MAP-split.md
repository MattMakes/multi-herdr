# Map: hark wave 2 split

Spec: `ai_docs/plans/hark/decisions.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 17a | [Build the 8 UI recipes](17a-impl-recipes-ui.md) | T-12 | 1 |
| 17b | [Build the 8 impact recipes](17b-impl-recipes-impacts.md) | T-12 | 1 |
| 18a | [Build the 7 motion recipes](18a-impl-recipes-motion.md) | T-12 | 1 |
| 18b | [Build the 8 riser, glitch and notify recipes](18b-impl-recipes-risers.md) | T-12 | 1 |
| 20a | [Build hark fit analysis models](20a-impl-fit-analysis.md) | T-12 | 1 |
| 20b | [Build hark fit pipeline: model choice, quality and code generation](20b-impl-fit-pipeline.md) | T-12 | 1 |
| 25b | [Integrate hark end to end and run every acceptance test](25b-integration.md) | T-13, T-14, T-15, T-16, T-19, T-21, 17a, 17b, 18a, 18b, 20a, 20b | 2 |
| 26b | [Clean-room similarity gate against the source](26b-similarity-gate.md) | T-22, T-23, T-24, 25b | 3 |

## Waves

- Wave 1: 17a, 17b, 18a, 18b, 20a, 20b
- Wave 2: 25b
- Wave 3: 26b

## Critical path

T-12 -> 20a, 20b -> 25b -> 26b

## Open questions

None.
