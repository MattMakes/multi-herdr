---
name: brand-identity
description: Use when defining or auditing a brand - positioning, messaging, voice and tone, logo system and usage rules, color and type roles, imagery style - or when writing brand guidelines that designers, writers, and agents must follow.
---

# Brand Identity

Turn a product into a brand that people recognize and trust: one idea, said in one voice, shown with one consistent visual system, and written down so that others apply it without guessing.

## When to use

- A new product, company, or feature needs a name-level identity: positioning, voice, logo direction, colors, type.
- Brand material exists but is scattered or inconsistent and needs one guidelines document.
- Copy or design must be checked against the brand (a consistency audit).
- A design system needs brand inputs (colors, fonts, tone for UI text).

Not for: generating images, logo renders, or brand boards (use the `design-imagery` skill if available); building tokens and components in code (use `design-system` if available); page-level visual direction (use `art-direction` if available).

## Inputs

- The product: what it does, for whom, the problem, the category, the price position.
- Audience segments and the context where they meet the brand.
- Competitors or alternatives (3 to 5).
- Existing assets: name, logo files, colors, fonts, copy, past guidelines, legal constraints (trademarks, regulated claims).
- The output the requester needs: a full guidelines document, one part (voice only, logo rules only), or an audit.

If the audience, the category, or the required output is unknown and changes the result, ask with `horch tell orchestrator "[<role>] QUESTION: ..."`, state your assumption, and continue with it on independent steps. Never invent facts about the company (customers, numbers, awards); mark them as placeholders.

## Workflow

1. **Audit what exists.** Collect every current brand asset and every place the brand appears (site, app, docs, social, email). Note conflicts: two logos, five blues, three tones.
   Check: an inventory table (asset, location, status: keep, fix, retire).

2. **Write the strategy.** Fill the positioning statement, value proposition, mission, and 3 to 5 supporting messages with proof points. Load `references/voice.md` (messaging section).
   Check: the positioning passes 5 tests: clear (no jargon), different (a named competitor could not sign it), credible (each claim has proof), relevant (the audience cares), consistent (fits the product as it is).

3. **Find the brand idea.** Name one core metaphor that connects what the product does to how the brand looks and sounds (for example "a well-kept ledger", "a lighthouse", "a workbench"). Load `references/visual-identity.md` (brand idea section).
   Check: one sentence. Every later choice (symbol, color, type, voice trait) can point back to it.

4. **Define voice and tone.** Choose 3 to 5 traits written as "X, not Y". Place the brand on the 4 voice spectrums. Write tone shifts per context, a vocabulary list (use, avoid), and rewrite 3 real strings from the product. Load `references/voice.md`.
   Check: the 3 rewrites follow the traits; a reader can tell them apart from a competitor's copy; every banned term has a reason.

5. **Define the visual identity.** Logo system (type, concept, variants, clear space, minimum sizes, color versions, misuse), color roles, type roles, imagery, iconography. Load `references/visual-identity.md`.
   Check: the mark reads at 16 px and in one color; every text color pair has a measured contrast ratio (4.5:1 for text, 3:1 for large text and UI); the palette has 1 primary, at most 1 accent, neutrals, and status colors.

6. **Write the guidelines.** Fill `references/guidelines-template.md`. Save to the path the brief names, otherwise `docs/brand-guidelines.md`.
   Check: the quick reference fits on one screen; every rule has a do and a don't or a value; placeholders are marked `TODO(owner)`.

7. **Hand off to code.** Map brand colors and fonts to design tokens (primitive and semantic names) and UI voice to microcopy rules (errors, empty states, buttons).
   Check: a mapping table brand name to token name to value. If the project has a `DESIGN.md`, it and the guidelines agree.

8. **Audit (when asked to review).** Check each surface against the guidelines with the consistency checklist in `references/guidelines-template.md`.
   Check: a findings table (surface, rule, issue, fix, severity), most severe first.

## Rules

- Strategy before visuals. Do not choose a color, a font, or a symbol before step 3 is done.
- One brand idea. Every symbol, color, and voice trait serves it; drop what does not.
- Specific beats grand. Taglines are short and concrete; ban generic claims ("revolutionary", "best-in-class", "seamless", "next-gen", "unleash", "elevate", "synergy") unless the brief demands them.
- Voice stays constant; tone changes with context (support is calmer than launch copy).
- Logo: simple, ownable, built from a reason (initial, product action, metaphor fusion, negative space, or construction geometry), readable at 16 px, works in one color, never stretched, recolored, rotated, outlined, or given effects.
- Color: 1 primary, at most 1 accent, one neutral family, status colors separate from brand colors. Measure contrast; never rely on color alone.
- Type: at most 2 families (plus a mono when needed), defined roles, a body size of at least 16 px on screens.
- Imagery: one direction (lighting, subject, treatment) that matches the palette and the idea; no generic stock scenes.
- Use official assets only; never redraw or approximate a partner's or a third party's logo.
- Write every rule so a person who never met the team can apply it.

## Review checklist

- [ ] Positioning passes the 5 tests and names the audience and the alternative.
- [ ] One brand idea sentence exists and later choices reference it.
- [ ] Voice: 3 to 5 "X, not Y" traits, tone per context, vocabulary, 3 rewrites.
- [ ] Logo: concept reason, variants, clear space, minimum sizes, color versions, misuse list.
- [ ] Colors have roles, values in hex (and RGB for print if needed), and measured contrast.
- [ ] Type roles, sizes, weights, and line heights are defined.
- [ ] Imagery and icon direction are stated with do and don't.
- [ ] Guidelines saved; placeholders marked; token mapping done.

## References

- `references/voice.md`: load at steps 2 and 4. Messaging framework, positioning templates, voice spectrums, voice chart, tone by context, vocabulary, rewrite method, tests.
- `references/visual-identity.md`: load at steps 3 and 5. Brand idea and symbol logic by category, logo types and concept methods, logo system rules, color and type roles, industry cues, identity modes, imagery and icon direction.
- `references/guidelines-template.md`: load at steps 6 and 8. Full brand guidelines document template, asset naming, and the consistency and approval checklists.

## Worker coordination

Follow the assigned brief and repository instructions. For a material ambiguity or a missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected step. Continue independent work and block only the dependent action. Do not wait on an interactive human prompt or add approval checkpoints when the work is already authorized.
