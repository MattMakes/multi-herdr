# Spec: external imports (plannotator, modern-web-guidance, Astryx)

## Operator decisions (2026-11-24)

- D1: No third-party app, binary, CLI or npm package runs in the fleet or on the operator machine. Never.
- D2: When a tool is necessary, the fleet builds it in this repository.
- D3: A source that has an owner comes in by clean-room rewrite. No source text and no source code is copied.
- D4: The 3 contradictions between fleet web rules and the modern web guidance are corrected. The fleet web rules follow the direction of the modern guidance.
- D5: All 3 projects come in.

## Clean-room rule for this run

- A contaminated worker reads the source and writes a behavior-only spec.
- A clean worker reads only the behavior-only spec and this repository. A clean worker never opens the source repository, its website or its package.
- Each ticket says if its worker is contaminated or clean.

## Units

1. plannotator: a spec gate with operator notes in `skills/orchestrate/SKILL.md` and the orchestrator briefing, and a revision mode for the 2 writers (ticket 01). A spec for a horch-native annotation tool (ticket 02). The build of that tool is a later run.
2. Astryx: a behavior-only method spec (ticket 03), then the method and a palette script in `skills/design-system/` (ticket 04).
3. modern-web-guidance: a clean-room import of the guide corpus by the `skill-importer` (ticket 05), then the alignment of the fleet web skills (ticket 06).

## Evidence

The research reports are in `ai_docs/reports/external-imports/` (gitignored, in the main checkout).
