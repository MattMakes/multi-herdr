# Preservation: what must not move

A redesign changes the visual and interaction layer. Users, search engines, analytics, and other code depend on everything else. Write the do-not-change list in step 3 and check it again at the end.

## Never change without explicit approval

- URL structure, route files, page slugs, anchor IDs.
- Primary navigation labels and order.
- Form field `name` attributes, field order, and input types (autofill and analytics depend on them).
- Element IDs, `data-*` attributes, and class names that analytics, tests, or scripts select.
- The logo, the wordmark, and brand colors the brand defines.
- Legal, consent, cookie, and pricing text.
- App logic: state management, data fetching, auth, integrations, business rules.
- SEO elements: `<title>`, meta description, canonical URL, structured data, Open Graph tags, heading text that ranks.

To find selectors in use, search the code for each ID and class you plan to rename (tests, analytics setup, scripts). If any match, keep the name.

## Non-destructive editing

- Edit the named files in place, or add new components and tokens wired through the existing routes.
- Before editing, list the files you will modify, create, and delete. Any deletion needs approval of that list.
- If the change would remove several components, replace a route tree, or collapse pages into one, stop and ask: `horch tell orchestrator "[<role>] QUESTION: ..."` with the file list and your recommendation.
- A global stylesheet is append-only: keep framework directives (`@tailwind`, `@import "tailwindcss"`) where they are, put new tokens below them, and keep any new `@import` at the top. Reuse the project's existing token names (`--background`, `--foreground`, a Tailwind `@theme`) instead of a parallel set.
- Briefs, PDFs, and docs are source material, not page copy, unless the user asks for verbatim text.

## Modes

| Mode | Keep | Change |
| --- | --- | --- |
| Preserve | brand colors and type (refined, not replaced), information architecture, copy voice, signature elements | spacing, scale, states, generic components, contrast, motion |
| Overhaul | content, routes, information architecture, behavior | the whole visual language: type, color, layout families, component voice |

In preserve mode, measure the current density and character of the site and stay near it; increase motion or variety by one notch at most.

## Multi-page products

- Pages of one product share one system: tokens, type pairing, accent and its placement, button shape and radius, section-heading pattern.
- Pages may differ in layout within a family (marketing, app, content), not in theme.
- Change the system once (tokens, base styles, shared components), then adapt pages. Do not restyle each page on its own.
- If the project has a design-system file (`design.md`, `DESIGN.md`, a token file), read it first and treat it as the rule. If a page genuinely needs something the system lacks, amend the system file with a stated variant; do not override locally.
- A page that drifts from the system is a defect, even if it looks good alone.

## Increment discipline

- One concern per increment (for example "type scale and pairing", "button states", "hero recomposition").
- After each increment: build, run the project's tests and linters, render the affected pages at the fixed widths, and compare with the baseline.
- Check the do-not-change list: run the search for every preserved ID, field name, and route.
- If anything regressed, revert the increment and re-plan. Do not fix forward on top of a broken step.
- Commit each increment separately when you are allowed to commit, with a subject that names the concern.
