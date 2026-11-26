# 06: Align the fleet web skills with the imported guides

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog`
**Own:** `skills/ui-taste/`, `skills/motion-gsap/`, `skills/landing-page/`, `skills/ui-redesign/`, `skills/design-system/references/ux-guidelines.md`, `skills/design-system/references/components.md`, `skills/design-system/references/shadcn-tailwind.md`, `skills/design-system/references/styles.md`

Spec: `docs/specs/external-imports.md` (not yet committed; read it at this absolute path)

## Goal

No fleet web skill contradicts the imported web-platform guides, and each fleet web skill sends the worker to those guides for platform API use.

## Context

- You are a CLEAN worker. Do not open `GoogleChrome/modern-web-guidance`, its website or its npm package. Your source of truth is the skill that ticket 05 installed in this repository. Find it: run `git log --name-status -5 -- skills/copied.json` and read the newest skill folder it names.
- The research report `ai_docs/reports/external-imports/modern-web-guidance.md` names 3 known contradictions, C1, C6 and C7 (dialog `inert`, no height animation, GSAP before CSS scroll timelines), with file paths. Read its "Comparison table" section.
- Operator decision D4: the fleet rules follow the direction of the modern guidance. Precedence after this ticket: the imported guides win on platform API use and on browser support. `ui-taste` and the direction contract win on look. For a scroll effect, a native CSS scroll timeline is the first choice, and GSAP is for what CSS cannot do.

## Steps

1. Fix C1, C6 and C7 in the files the report names. Check: each fixed rule names the guide file that supports it.
2. Audit each `.md` file under Own against the imported guides. Write the result to `ai_docs/plans/external-imports/web-alignment-audit.md`: 1 row per rule that a guide contradicts or makes obsolete, with file, line, the guide file and the new rule text.
3. Apply each row. Rewrite the rule in place. Do not copy a guide passage into another skill: link the guide file by its path relative to `skills/`.
4. Add 1 precedence paragraph (the text of Context above) to `skills/ui-taste/SKILL.md`, and 1 line that points to it in `skills/motion-gsap/SKILL.md`, `skills/landing-page/SKILL.md` and `skills/ui-redesign/SKILL.md`.
5. Run the Check line.

## Constraints

- Do not edit `skills/design-system/SKILL.md`, `tokens.md`, `palettes.md` or the imported skill.
- Do not edit a teammate file.
- Keep each `SKILL.md` at most 12 KB.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes.
- `ai_docs/plans/external-imports/web-alignment-audit.md` exists, and each row is marked "applied".
- The DONE summary gives the count of rules changed per skill.

## Report

Run `horch ledger finish`, then `horch ledger done`.
