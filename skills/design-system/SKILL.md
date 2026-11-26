---
name: design-system
description: Use when creating, extending, or auditing a design system in code - design tokens, CSS variables, a Tailwind or shadcn/ui theme, type, spacing and color scales, dark mode, component state specs, or a DESIGN.md that agents and people build from.
---

# Design System

Build one source of truth for visual decisions, wire it into the code, and prove it holds: every color pair passes contrast, every component state exists, and no component carries a raw value.

## When to use

- A project needs tokens, a theme, or a `DESIGN.md` before UI work starts.
- An existing UI has drifted: raw hex values, random spacing, missing states, a broken dark mode.
- You add a component and must spec its variants, sizes, and states.
- You move a theme between Tailwind v3, Tailwind v4, and shadcn/ui.

Not for: brand voice, positioning, or logo rules (use the `brand-identity` skill if available); page-level taste and layout critique (use `ui-taste` if available); animation choreography (use `motion-gsap` if available).

## Inputs

- The repository: framework, CSS entry points, `tailwind.config.*`, `components.json`, existing tokens, existing `DESIGN.md`.
- Brand inputs if any: brand guidelines, logo colors, fonts. Brand values override the reference palettes here.
- Product type, audience, density (marketing page vs dashboard), and the required modes (light, dark, or both).
- The accessibility target. Default: WCAG 2.2 AA.

If product type, required modes, or the accessibility target is missing and changes the result, ask with `horch tell orchestrator "[<role>] QUESTION: ..."`, state your default, and continue with the default for independent steps.

## Workflow

1. **Audit what exists.** Search for token sources: `:root` custom properties, `@theme` blocks, `tailwind.config.*` theme keys, `components.json`, CSS-in-JS theme objects, `DESIGN.md`. Count raw values in components (see the grep in step 8).
   Check: a written list of every token source with its path, and a decision "extend X" or "create new". Never create a second token system beside an existing one.

2. **Set the direction.** Choose a style, seed colors, and a font pairing that fit the product. Start from brand inputs; otherwise use `references/styles.md`, `references/palettes.md` (seed rows), and `references/font-pairings.md` as starting points, not as final answers.
   Check: one paragraph that names the style, the seed colors (1 primary and at most 1 accent, plus a seed per status), the heading and body fonts, the density, and the modes. Every choice has a one-line reason tied to the product.

3. **Generate the palette and define primitives** (layer 1). Run `python3 -I skills/design-system/scripts/palette.py ramp --seed accent=<hex> --seed danger=<hex> --seed success=<hex> --seed warning=<hex> --seed info=<hex> --json palette.json --css palette.css`. Read warnings and diagnostics (`references/palette-generation.md`). Then define font families, a type scale, a 4 px spacing scale, radii, elevation, motion, and a z-index scale. Load `references/tokens.md`.
   Check: the script exits 0; every ramp is monotonic; the 2 closest families differ by `closest_delta_e` of about 15 or more; all raw values live in one primitives block or file; no primitive name carries meaning (`accent-50`, not `brand-blue`).

4. **Define semantic tokens** (layer 2). Take the derived roles from `palette.json` (`background`, `foreground`, a `-foreground` partner for every fill, `border`, `input`, `ring`, status roles). They keep the shadcn names. Edit a role only for a reason you can write down, such as a brand value. Write the light set and the dark set.
   Check: `python3 -I skills/design-system/scripts/palette.py check palette.json` exits 0. It prints every text pair (4.5:1) and UI boundary (3:1) in each mode. After a hand edit, run it again on the edited file.

5. **Spec components** (layer 3): for each component, list variants, sizes, states (default, hover, active, focus-visible, disabled, loading, error, selected where it applies), and anatomy. Load `references/components.md`.
   Check: a state matrix per component with no empty cell. Component tokens point at semantic tokens only.

6. **Wire it into the code.** Emit CSS variables, map them into the Tailwind theme (v4 `@theme inline` or the v3 config), and align shadcn/ui variables. Load `references/shadcn-tailwind.md`.
   Check: the project builds; a utility such as `bg-primary text-primary-foreground` renders the token value; theme switching changes only semantic variables.

7. **Dark mode.** Override semantic tokens only. Use lighter, less saturated tints on dark surfaces, not an inversion. Raise elevation with lighter surfaces, not darker shadows.
   Check: `palette.py check` passes for the dark set; screenshots of the same screen in both modes, if a browser tool is available.

8. **Verify the code obeys the system.** Run, adapting paths:
   `grep -rnE '#[0-9a-fA-F]{3,8}\b|rgba?\(|hsla?\(' src --include='*.tsx' --include='*.jsx' --include='*.vue' --include='*.svelte'`
   `grep -rnE '\b(p|m|gap|w|h|text|rounded)-\[[0-9.]+(px|rem)\]' src`
   Check: zero hits outside token files, or each remaining hit has a written reason. Tab through one screen: every interactive element shows a visible focus ring. With `prefers-reduced-motion: reduce`, no non-essential motion runs. At 375, 768, 1024, and 1440 px there is no horizontal scroll.

9. **Write `DESIGN.md`** at the project root (or update it) from `references/design-md-template.md`. It records the decisions, the exact values, the component rules, and the banned patterns. Write the theme record beside it from `references/theme-spec-template.md`: it stores the `receipt` of step 3, the output of `palette.py check` as contrast evidence, and the decision log.
   Check: every value in `DESIGN.md` matches the code; a reader can build a new screen from `DESIGN.md` alone; the record names a test that can fail for each contract.

## Rules

- One naming system per project: keep the current names (shadcn names in a shadcn project). The colour grammar in `references/tokens.md` maps to them.
- Three layers: primitive, semantic, component. Components read semantic or component tokens, never primitives and never raw values.
- Pair every fill with its foreground token. Name tokens by purpose (`destructive`), not by look (`red`).
- One primary and at most one accent per product. Status colors (success, warning, error, info) are not accents.
- Color never carries meaning alone. Add an icon, a label, or a pattern.
- Spacing comes from a 4 px base (4, 8, 12, 16, 24, 32, 48, 64, 96). Body text is at least 16 px with line height 1.5 to 1.75 and a measure of 60 to 75 characters.
- One type scale with a fixed ratio. Headings 600 to 700, body 400, labels 500. Use tabular figures for numbers in tables, prices, and timers.
- One elevation scale and one radius scale. Do not invent a shadow or radius in a component.
- Use `:focus-visible` with a ring at least 2 px wide and 3:1 against its background. Never remove an outline without a replacement.
- Disabled uses the native `disabled` attribute or `aria-disabled="true"`, reduced emphasis, and no action. Read-only looks different from disabled.
- Touch targets are at least 44 x 44 px on touch screens; web pointer targets are at least 24 x 24 CSS px.
- Motion tokens: animate `transform` and `opacity` only, exit faster than enter, honor `prefers-reduced-motion`.
- Edit an existing theme in place. Do not overwrite customized shadcn components; preview with `npx shadcn@latest add <name> --dry-run`, if the CLI is available.
- `DESIGN.md` and the token files change together in the same commit.

## Review checklist

- [ ] One token source; no parallel system.
- [ ] Primitive, semantic, and component layers exist and reference downward only.
- [ ] `palette.py check` exits 0 in every mode (text 4.5:1, large text and UI 3:1).
- [ ] Every component has all applicable states, including focus-visible, disabled, loading, and error.
- [ ] Raw-value grep returns zero unexplained hits.
- [ ] Dark mode overrides semantic tokens only and was checked on its own.
- [ ] Reduced motion, keyboard focus, and the 4 breakpoints were checked.
- [ ] `DESIGN.md` exists, is complete, and matches the code; the theme record exists and holds the palette receipt.

## References

- `references/palette-generation.md`: load at steps 3 and 4. The `palette.py` commands, inputs, output, warnings.
- `references/tokens.md`: load at steps 3 and 4. Layer model, naming and the colour grammar, primitive scales and formulas, semantic set, role derivation, theme inheritance, dark mode, contrast, DTCG JSON.
- `references/components.md`: load at step 5. Variant, size, and state specs for button, input, card, badge, alert, dialog, table; state priority; ARIA states.
- `references/shadcn-tailwind.md`: load at step 6. Tailwind v4 and v3 theme wiring, shadcn/ui variables and conventions, Radix behavior, dark-mode toggling.
- `references/styles.md`: load at step 2. Style catalog with fit, misfit, token implications, and accessibility risk.
- `references/palettes.md`: load at step 2. Seed colors by product type and color-role rules.
- `references/font-pairings.md`: load at step 2. Heading and body pairings by mood and product.
- `references/ux-guidelines.md`: load at step 8 and for any UI review. Priority-ordered UX and accessibility rules with checks.
- `references/design-md-template.md`: load at step 9. The `DESIGN.md` structure and a filled example.
- `references/theme-spec-template.md`: load at step 9. The theme record that sits beside `DESIGN.md`.

## Worker coordination

Follow the assigned brief and repository instructions. For a material ambiguity or a missing prerequisite, use `horch tell orchestrator` with a concise question, evidence, and the affected step. Continue independent work and block only the dependent action. Do not wait on an interactive human prompt or add approval checkpoints when the work is already authorized.
