# 04: Seeded palettes, derived roles and a theme record in design-system

<!-- horch-ticket: 1 -->
**Persona:** primary `design-system-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `python3 -I skills/design-system/scripts/test_palette.py && cargo test -p horch-core --test skills_catalog`
**Own:** `skills/design-system/SKILL.md`, `skills/design-system/references/tokens.md`, `skills/design-system/references/palettes.md`, `skills/design-system/references/palette-generation.md`, `skills/design-system/references/theme-spec-template.md`, `skills/design-system/references/design-md-template.md`, `skills/design-system/scripts/`
**Do not touch:** `skills/design-system/references/ux-guidelines.md`, `skills/copied.json`

Spec: `docs/specs/external-imports.md` (not yet committed; read it at this absolute path)

## Goal

`skills/design-system/` teaches a worker to generate a palette from seed colours with a script, derive contrast-safe roles, and record the theme decisions, for every web stack.

## Context

- You are a CLEAN worker. Do not open https://github.com/meta/astryx, its website or its npm package. Do not read `ai_docs/reports/external-imports/astryx.md`.
- Your only input for the method is `ai_docs/plans/external-imports/astryx-method-spec.md` (requirements `AM-<nn>`, written by ticket 03). If that file is missing, or a requirement is not clear, send `horch tell orchestrator "QUESTION: ..."` and end your turn.
- The operator forbids third-party tools (D1). The script uses python3 3.10 or later with the standard library only. Other skills keep scripts in `skills/<skill>/scripts/` (example: `skills/skill-intake/scripts/`).
- The skill uses shadcn token names now. Keep 1 naming system: keep the current names, and map each new role to them.

## Steps

1. Write the test `skills/design-system/scripts/test_palette.py` first, from the test vectors of the method spec. It uses `unittest` and exits non-zero on a failure. Check: it fails before step 2.
2. Write `skills/design-system/scripts/palette.py`. Commands: `ramp` (seed colours in, JSON and CSS custom properties out, light and dark), `contrast` (2 colours in, WCAG ratio out), `check` (a token JSON file in, 1 line per role pair with its ratio and pass or fail, exit 1 on a fail). Check: the test passes.
3. Write `references/palette-generation.md`: when to run each command, the inputs, and how to read the output. Maximum 120 lines.
4. Update `references/tokens.md`: the colour naming grammar, the status roles and the derivation rules of the method spec, mapped to the current names.
5. Write `references/theme-spec-template.md`: the theme record with its fields. Link it from `references/design-md-template.md`. The record goes beside `DESIGN.md`; it does not replace it.
6. Update `SKILL.md`: the steps that pick colours now run `palette.py ramp`, and the step that verifies contrast runs `palette.py check`. Replace the manual contrast table when the script covers it. Keep `SKILL.md` at most 12 KB.
7. Remove from `references/palettes.md` each rule that the new method contradicts. Keep the hand-made palettes as seed examples.
8. Run the Check line.

## Constraints

- Add no code comments. The repository lint refuses them.
- Do not name Astryx or Meta in a skill file.
- No network call, no package install, no dependency outside the standard library.
- Do not edit a file that Own does not name.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes.
- `python3 -I skills/design-system/scripts/palette.py ramp --help` exits 0.
- Each `AM-<nn>` requirement appears in the DONE summary as "done: <file>" or "not done: <reason>".

## Report

Run `horch ledger finish`, then `horch ledger done`.
