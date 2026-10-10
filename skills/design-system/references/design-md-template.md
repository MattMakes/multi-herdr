# DESIGN.md template

`DESIGN.md` is the design system in plain language plus exact values. A person or an agent reads it before building a screen and can then build that screen without asking. Keep it at the project root, next to the code it describes, and change it in the same commit as the tokens.

## Writing rules

- Name every color by role and appearance, then give the value and its job: "**Charcoal Ink** (`#18181B`): primary text". Never a bare hex, never a bare adjective.
- Give exact values in code format (`1rem`, `0 20px 40px -15px rgb(0 0 0 / 0.05)`), and the token name that holds each value.
- Describe intent in words that a non-specialist understands ("generously rounded corners, `1rem`"), not utility class names alone.
- State the bans. A rule that says what never to do is as useful as one that says what to do.
- Use one term for one thing in the whole file.
- Every value must match the code. When they differ, the code is wrong or the file is stale: fix one in the same change.

## Dials (optional)

Three 1 to 10 dials summarize the intent. Record them so that later screens stay in range.

| Dial | 1 to 3 | 4 to 7 | 8 to 10 |
|---|---|---|---|
| Density | Airy, gallery spacing (section spacing 64 to 128 px) | Balanced (32 to 64 px) | Dense, cockpit (8 to 32 px), monospace numbers |
| Variance | Symmetric, centered, predictable | Subtle offsets | Asymmetric, every section different |
| Motion | Static, state changes only | Hover and entrance cues | Choreographed sequences (needs a reduced-motion path) |

## Template

```markdown
# Design System: <Product>

> Version <x.y> - updated <YYYY-MM-DD> - tokens: `<path to token file>`

## 1. Atmosphere
<2 to 4 sentences: mood, audience, density, variance, motion. Example: "A calm, document-like
workspace for finance teams. Balanced density (5), low variance (3), restrained motion (3).
It should feel precise and quiet, like a well-kept ledger.">

Dials: Density <n> - Variance <n> - Motion <n>
Style: <primary style> with <one secondary trait>
Modes: <light | dark | both>, default <system>

## 2. Color
| Name | Token | Light | Dark | Role |
|---|---|---|---|---|
| **<Canvas>** | `--background` | `#...` | `#...` | Page surface |
| **<Ink>** | `--foreground` | `#...` | `#...` | Body text |
| **<Surface>** | `--card` | `#...` | `#...` | Cards, panels |
| **<Muted ink>** | `--muted-foreground` | `#...` | `#...` | Secondary text, metadata |
| **<Line>** | `--border` | `#...` | `#...` | Dividers, card borders |
| **<Brand>** | `--primary` | `#...` | `#...` | Primary actions, selection |
| **<Accent>** | `--accent-cta` | `#...` | `#...` | One accent: calls to action only |
| **<Danger>** | `--destructive` | `#...` | `#...` | Errors, destructive actions |
| **<Focus>** | `--ring` | `#...` | `#...` | Focus rings |

Contrast (measured): <pair: ratio light / ratio dark>, ...
Banned: <for example: pure #000000 text, a second accent, purple-to-blue gradients, warm and cool grays mixed>

## 3. Typography
- **Display / headings:** <Family>, <weights>, tracking <-0.02em>, line height <1.1-1.25>
- **Body:** <Family>, 400, <16px>/<1.6>, measure <65ch>
- **Mono:** <Family> for code, IDs, and <numbers in tables>
- **Scale:** <ratio>; <list: 12 / 14 / 16 / 18 / 20 / 24 / 30 / 36 / 48>
- **Banned:** <families or treatments, with the reason>

## 4. Space, shape, depth
- Spacing: 4 px base; section rhythm <values>; container <max width>, gutters <values per breakpoint>
- Radius: controls <value>, cards <value>, pills `9999px`
- Elevation: <level: shadow token, where used>; dark mode uses lighter surfaces
- Z-index: <dropdown / sticky / overlay / modal / toast values>

## 5. Components
For each: shape, color, depth, states, and the do/don't that matters.
- **Buttons:** <variants, sizes, press feedback, focus ring, loading>
- **Inputs:** <label above, helper text, error below, focus ring>
- **Cards:** <when to use; when to use dividers instead>
- **Navigation:** <pattern per breakpoint, active state>
- **Feedback:** <toasts, alerts, skeletons, empty and error states>
- **Data:** <tables, charts, number formatting>

## 6. Layout and responsive
- Grid: <columns, gaps>; layout primitives: <grid for 2D, flex for 1D>
- Breakpoints tested: 375, 768, 1024, 1440 px
- Below 768 px: <collapse rules, touch target size, nav pattern>
- Full-height sections: `min-height: 100dvh`

## 7. Motion
- Durations: fast <150ms>, normal <200ms>, slow <300ms>; easing <tokens>
- What moves: <list>; what never moves: <list>
- Reduced motion: <what changes>

## 8. Accessibility target
WCAG <2.2 AA>. Text 4.5:1, UI and large text 3:1, visible focus, keyboard complete,
targets <24 px web / 44 pt touch>, reduced motion honored.

## 9. Anti-patterns (never do)
- <banned pattern> - <why>
- ...

## 10. Changelog
| Version | Date | Change |
|---|---|---|
```

## Default anti-pattern seeds

Pick the ones that fit the product and the chosen style; delete the rest. For a deeper taste list, use the `ui-taste` skill if available.

- Raw hex, rgb, or pixel values in components.
- More than one accent color; status colors used as decoration.
- Pure `#000000` text on white for long reading (prefer an off-black).
- Gradient text on large headings; neon outer glows on buttons.
- Emoji as icons; mixed icon families or stroke widths.
- Placeholder used as a label; errors shown only at the top of a form.
- `height: 100vh` on mobile layouts; `h-screen` in Tailwind.
- Arbitrary `z-index: 9999`.
- Generic filler content: "John Doe", "Acme", "Lorem ipsum", invented round metrics such as "99.99%".
- Marketing cliché copy ("Elevate", "Seamless", "Unleash", "Next-gen").
- Infinite decorative animation without a reduced-motion path.
- Default library styling shipped unchanged when the brief asks for a distinct identity.

## Filled example (short)

```markdown
# Design System: Ledgerline

> Version 1.0 - updated 2026-10-03 - tokens: `src/styles/tokens.css`

## 1. Atmosphere
A calm, document-like workspace for small finance teams. Balanced density (6), low variance (3),
restrained motion (3). Precise and quiet; numbers are the hero.

Dials: Density 6 - Variance 3 - Motion 3
Style: Minimalism & Swiss Style with Data-Dense Dashboard tables
Modes: both, default system

## 2. Color
| Name | Token | Light | Dark | Role |
|---|---|---|---|---|
| **Paper** | `--background` | `#F8FAFC` | `#0B1120` | Page surface |
| **Ledger Ink** | `--foreground` | `#0F172A` | `#E2E8F0` | Body text |
| **Slate Note** | `--muted-foreground` | `#475569` | `#94A3B8` | Metadata, captions |
| **Rule Line** | `--border` | `#E2E8F0` | `#1E293B` | Table rules, dividers |
| **Ledger Blue** | `--primary` | `#1E40AF` | `#60A5FA` | Primary actions, selection |
| **Settled Green** | `--success` | `#15803D` | `#4ADE80` | Paid status, with a check icon |
| **Overdue Red** | `--destructive` | `#DC2626` | `#F87171` | Overdue status, delete |
| **Focus Blue** | `--ring` | `#2563EB` | `#93C5FD` | Focus rings |

Contrast (light / dark): foreground 17.1 / 15.3; muted 7.2 / 7.3; primary 8.3 / 7.4;
success 4.8 / 10.8; destructive 4.6 / 6.8; ring 4.9 / 10.4 (all on background).
Banned: a second accent; color-only status; gradients.

## 3. Typography
- **Headings:** IBM Plex Sans 600, tracking -0.01em, line height 1.25
- **Body:** IBM Plex Sans 400, 16px/1.6, measure 70ch
- **Mono:** IBM Plex Mono for amounts and IDs; all table numbers `tabular-nums`, right-aligned
- **Scale:** 1.2; 12 / 14 / 16 / 19 / 23 / 28 / 33 px
- **Banned:** display or script faces; weights under 400 below 16px

## 9. Anti-patterns (never do)
- Charts without a table alternative - finance users audit numbers.
- Truncated amounts - always show the full value or wrap.
- Toasts for errors that need action - use inline alerts.
```

Measure every pair in your own file; the example values above were measured for this example only.

## Check

- Every section is filled or explicitly marked "not applicable".
- Every token named in the file exists in the token file, with the same value.
- A new screen built from the file alone passes the review checklist in SKILL.md.
