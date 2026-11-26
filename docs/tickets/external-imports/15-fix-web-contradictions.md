# 15: Correct the 3 web rules that contradict modern platform guidance

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-feature`, fallback `codex-terra` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core --test skills_catalog --test baseline_oracles && cargo test -p horch --test baseline_cli --test skills_cli`
**Own:** `skills/ui-taste/SKILL.md`, `skills/ui-taste/references/states-and-interaction.md`, `skills/design-system/references/ux-guidelines.md`, `skills/motion-gsap/SKILL.md`, `skills/landing-page/SKILL.md`, `crates/horch/tests/oracles/`, `crates/horch-core/tests/oracles/`

## Goal

The 3 fleet web rules that contradict modern web platform practice are corrected, and `ui-taste` states which rule wins.

## Context

- You are a CLEAN worker. Do not open `GoogleChrome/modern-web-guidance`, its website or its npm package.
- Operator decision: the fleet web rules follow the direction of modern platform guidance.
- The research report `ai_docs/reports/external-imports/modern-web-guidance.md` has a "Comparison table". Read rows C1, C6 and C7 only.
- The import of the full guide set is a later run. Do not name a guide file path in a skill.

## Steps

1. C1, `skills/ui-taste/references/states-and-interaction.md` near line 39: a modal uses the native `<dialog>` element with `showModal()`. The browser makes the rest of the page inert, traps focus and closes on Esc. Remove the instruction to add `inert` to the page. State: add no manual focus trap and no Esc handler for a native modal dialog.
2. C6, `skills/design-system/references/ux-guidelines.md` near line 128: replace "never animate height" with this rule: animate open and close to intrinsic size with `interpolate-size: allow-keywords` and a transition on `block-size`, as a progressive enhancement; where the browser does not support it, the element opens with no animation. Do not animate `height` with a fixed `max-height` value. Keep each other performance rule of that section.
3. C7, `skills/motion-gsap/SKILL.md` step 1: for a scroll-linked effect that is decoration (reveal, fade, parallax, progress bar), the first choice is a native CSS scroll-driven animation (`animation-timeline: scroll()` or `view()`) with a `prefers-reduced-motion` guard. GSAP ScrollTrigger is for what CSS cannot do: pinning, scrubbed multi-step timelines, sequencing across elements, and callbacks.
4. Add 1 paragraph "Precedence" to `skills/ui-taste/SKILL.md`: modern platform guidance wins on platform API use and browser support. `ui-taste` and the direction contract win on look. For a scroll effect, native CSS comes before GSAP. Add 1 line that points to that paragraph in `skills/motion-gsap/SKILL.md` and `skills/landing-page/SKILL.md`.
5. Run the Check. A frozen oracle can fail only because the byte size of a skill file you changed is different. Then update the oracle with `HORCH_BLESS=1` for the failing test. Check: `git diff` of the oracle folders shows only size lines of the skills you changed. If another line changes, send `horch tell orchestrator "QUESTION: ..."` and end your turn.

## Constraints

- Do not write the word `check` alone in backticks in a skill file. The skill catalog reads it as the skill named check.
- Keep each `SKILL.md` at most 12 KB.
- Add no file. Do not edit a teammate file.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes.
- `grep -n "inert" skills/ui-taste/references/states-and-interaction.md` shows no instruction to add `inert` for a native dialog.
- `grep -n "interpolate-size" skills/design-system/references/ux-guidelines.md` prints 1 line or more.
- `grep -n "animation-timeline" skills/motion-gsap/SKILL.md` prints 1 line or more.

## Report

Run `horch ledger finish`, then `horch ledger done`. The summary gives the old rule and the new rule for C1, C6 and C7.
