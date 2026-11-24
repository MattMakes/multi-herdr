# Tokens

## Layer model

| Layer | Holds | Example | Changes when |
|---|---|---|---|
| Primitive | Raw values, no meaning | `--color-blue-600: #2563eb` | The palette or scale changes (rarely) |
| Semantic | Purpose aliases | `--primary: var(--color-blue-600)` | The theme or mode changes |
| Component | Per-component knobs | `--button-bg: var(--primary)` | One component needs its own value |

References point downward only: component to semantic, semantic to primitive. A component stylesheet reads semantic or component tokens. Primitives appear only in the semantic layer.

## Naming

Pattern: `--{category}-{item}-{variant}-{state}`.

| Category | Primitive examples | Semantic examples |
|---|---|---|
| color | `--color-gray-100`, `--color-blue-600` | `--primary`, `--primary-foreground`, `--muted`, `--ring` |
| space | `--space-1` ... `--space-24` | `--space-component`, `--space-section` |
| font | `--font-size-sm`, `--font-weight-semibold` | `--font-heading`, `--font-body`, `--text-label` |
| radius | `--radius-sm` ... `--radius-full` | `--radius` (base), `--radius-control` |
| shadow | `--shadow-sm` ... `--shadow-xl` | `--elevation-card`, `--elevation-overlay` |
| motion | `--duration-150`, `--ease-out` | `--duration-fast`, `--duration-normal` |
| z | `--z-10` ... `--z-50` | `--z-dropdown`, `--z-modal`, `--z-toast` |

shadcn/ui uses bare semantic names (`--background`, `--primary`). Keep that convention in shadcn projects; use the `--color-*` prefix for primitives.

## Colour naming grammar

A project uses 1 naming system. A shadcn project keeps its names (`--primary`, `--primary-foreground`) and does not adopt the grammar below. A new non-shadcn project can use it. Never run both in 1 project.

| Form | Pattern | Example |
|---|---|---|
| Paint | `--color-<paint>-<role>`; paint is `text`, `icon`, `background`, `border`; role is an emphasis (`primary`, `secondary`, `disabled`, `emphasized`), an intent, a surface name, a hue name or `control` | `--color-text-secondary`, `--color-border-control` |
| Intent | `--color-<intent>`; intents are `accent`, `neutral`, `info`, `success`, `warning`, `error` | `--color-success` |
| Variant | A paint or intent name plus 1 suffix: `-muted` (pale), `-emphasized` (strong), `-hover`, `-pressed`, `-inverted` | `--color-accent-muted` |
| Content | `--color-on-<fill>`; fill is an intent, a variant, `dark` or `light`. A `-muted` fill pairs with `--color-on-<intent>-muted` | `--color-on-accent` |
| Domain | `--color-<domain>-<name>` for a separate layer such as charts or code syntax | `--color-chart-1` |

Role-less utility names allowed: `border`, `overlay` (with `-hover`, `-pressed`), `shadow`, `skeleton`, `track`.

- Do not add a group word (`status`), a second paint word (`foreground`) or a paint suffix (`-text`).
- A role that changes by mode is 1 token with a `[light, dark]` pair (CSS `light-dark()`). Never make `-light` and `-dark` names.
- A utility alias (Tailwind key) is a projection of a token. It never adds a CSS variable.
- A name is either portable or local to 1 theme, never both.

Map to the current names:

| Grammar | Current name |
|---|---|
| `--color-background-page` | `--background` |
| `--color-text-primary` | `--foreground` |
| `--color-text-secondary` | `--muted-foreground` |
| `--color-background-muted` | `--muted` |
| `--color-accent` / `--color-on-accent` | `--primary` / `--primary-foreground` |
| `--color-accent-muted` / `--color-on-accent-muted` | `--accent` / `--accent-foreground` |
| `--color-error` / `--color-on-error` | `--destructive` / `--destructive-foreground` |
| `--color-<intent>` / `--color-on-<intent>` | `--<intent>` / `--<intent>-foreground` |
| `--color-<intent>-muted` / `--color-on-<intent>-muted` | `--<intent>-muted` / `--<intent>-muted-foreground` |
| `--color-text-<intent>` | `--<intent>-text` |
| `--color-border-<intent>` | `--<intent>-border` |
| `--color-border-control` | `--input` |
| `--color-border-focus` | `--ring` |

## File layout

Either one file with ordered sections, or one file per layer:

```css
/* tokens.css */
/* === PRIMITIVES === */  :root { ... }
/* === SEMANTIC (light) === */ :root { ... }
/* === SEMANTIC (dark) === */  .dark { ... }
/* === COMPONENTS === */  :root { ... }
```

## Primitive scales

### Color ramps

Generate ramps with `python3 -I scripts/palette.py ramp` (see `palette-generation.md`). A ramp has 1 colour per tone stop: stops 0, 5, ... 100 (21 stops) or 0, 10, ... 100 (compact, 11 stops). A tone is CIE L*, so the same stop has the same lightness in every family. Stop 0 is `#000000` and stop 100 is `#ffffff`. Name a primitive `--color-<family>-<stop>`: `--color-accent-50`. Dark mode has its own ramp per family with the same lightness and 15 percent less chroma.

One neutral ramp per product; do not mix a cool gray and a warm gray. Tint it with `--gray-tint` (`none`, `warm`, `cool`, `from-seed`). Hand-made ramps are allowed as seeds. A project that keeps 50 to 950 names maps each name to 1 tone stop and still runs `check`.

```css
:root {
  --color-neutral-95: #f1f1f1;  --color-neutral-50: #777777;  --color-neutral-10: #1b1b1b;
  --color-accent-50: #366ee6;   --color-accent-90: #d4e3ff;   --color-accent-10: #001053;
}
```

Hex, HSL, and OKLCH all work (OKLCH example: `oklch(0.55 0.22 264)`). Pick one format per project; the script writes hex.

### Type scale

Pick one ratio: 1.125 (dense UI), 1.2 (product UI), 1.25 (general), 1.333 (marketing, editorial). Base 16 px.

| Token | rem | px | Weight | Line height | Use |
|---|---|---|---|---|---|
| `--font-size-xs` | 0.75 | 12 | 400-500 | 1.4 | Captions, badges. Never body. |
| `--font-size-sm` | 0.875 | 14 | 400-500 | 1.5 | Labels, secondary text, dense tables |
| `--font-size-base` | 1 | 16 | 400 | 1.5-1.75 | Body |
| `--font-size-lg` | 1.125 | 18 | 400 | 1.6 | Lead text |
| `--font-size-xl` | 1.25 | 20 | 600 | 1.4 | H5 |
| `--font-size-2xl` | 1.5 | 24 | 600 | 1.35 | H4 |
| `--font-size-3xl` | 1.875 | 30 | 600 | 1.3 | H3 |
| `--font-size-4xl` | 2.25 | 36 | 700 | 1.25 | H2 |
| `--font-size-5xl` | 3 | 48 | 700 | 1.15 | H1 |
| `--font-size-6xl` | 3.75 | 60 | 700 | 1.1 | Display |

Fluid display sizes: `clamp(2.25rem, 1.5rem + 3vw, 3.75rem)`. Use unitless line heights. Tracking: display -0.02em, body 0, all caps +0.05em.

### Spacing (4 px base)

| Token | Value | Token | Value |
|---|---|---|---|
| `--space-1` | 0.25rem (4) | `--space-8` | 2rem (32) |
| `--space-2` | 0.5rem (8) | `--space-10` | 2.5rem (40) |
| `--space-3` | 0.75rem (12) | `--space-12` | 3rem (48) |
| `--space-4` | 1rem (16) | `--space-16` | 4rem (64) |
| `--space-5` | 1.25rem (20) | `--space-20` | 5rem (80) |
| `--space-6` | 1.5rem (24) | `--space-24` | 6rem (96) |

Density presets for section rhythm: spacious 24 to 96 px (marketing), standard 16 to 64 px, dense 8 to 32 px (dashboards).

### Radius, elevation, motion, z-index

```css
:root {
  --radius-none: 0; --radius-sm: 0.25rem; --radius-md: 0.375rem; --radius-lg: 0.5rem;
  --radius-xl: 0.75rem; --radius-2xl: 1rem; --radius-full: 9999px;

  --shadow-sm: 0 1px 2px 0 rgb(0 0 0 / 0.05);
  --shadow-md: 0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1);
  --shadow-lg: 0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1);
  --shadow-xl: 0 20px 25px -5px rgb(0 0 0 / 0.1), 0 8px 10px -6px rgb(0 0 0 / 0.1);

  --duration-fast: 150ms; --duration-normal: 200ms; --duration-slow: 300ms;
  --ease-out: cubic-bezier(0.16, 1, 0.3, 1); --ease-in: cubic-bezier(0.7, 0, 0.84, 0);

  --z-dropdown: 10; --z-sticky: 20; --z-overlay: 30; --z-modal: 40; --z-toast: 50;
}
```

Elevation map: card `sm`, raised card or dropdown `md`, popover `lg`, dialog `xl`. Tint shadows toward the background hue on colored surfaces. Exit durations are about 60 to 70 percent of enter durations.

## Semantic set

Minimum set (shadcn-compatible names):

| Token | Role | Pair |
|---|---|---|
| `background` / `foreground` | Page surface and body text | 4.5:1 |
| `card` / `card-foreground` | Raised surface | 4.5:1 |
| `popover` / `popover-foreground` | Menus, popovers | 4.5:1 |
| `primary` / `primary-foreground` | Main action fill | 4.5:1 |
| `secondary` / `secondary-foreground` | Secondary action fill | 4.5:1 |
| `muted` / `muted-foreground` | Quiet surfaces, secondary text | 4.5:1 on `muted` and on `background` |
| `accent` / `accent-foreground` | Hover and selected surfaces | 4.5:1 |
| `destructive` / `destructive-foreground` | Dangerous actions, errors | 4.5:1 |
| `border`, `input` | Dividers, field outlines | `input` 3:1 against `background` |
| `ring` | Focus ring | 3:1 against adjacent colors |
| `success`, `warning`, `info` (+ `-foreground`) | Status | 4.5:1 for text |

Add `primary-hover` and `primary-active` only if the design cannot use an opacity or lightness step of `primary`.

```css
:root {
  --background: var(--color-gray-50);  --foreground: var(--color-gray-900);
  --primary: var(--color-blue-600);    --primary-foreground: #fff;
  --muted: var(--color-gray-100);      --muted-foreground: var(--color-gray-600);
  --destructive: var(--color-red-600); --destructive-foreground: #fff;
  --border: var(--color-gray-200);     --input: var(--color-gray-500);
  --ring: var(--color-blue-500);
}
.dark {
  --background: var(--color-gray-950); --foreground: var(--color-gray-50);
  --primary: var(--color-blue-400);    --primary-foreground: var(--color-gray-950);
  --muted: var(--color-gray-800);      --muted-foreground: var(--color-gray-400);
  --border: var(--color-gray-800);     --input: var(--color-gray-500);
  --ring: var(--color-blue-400);
}
```

Measured: `primary` on white 5.16:1; `muted-foreground` on `muted` 6.87:1 (light) and 5.78:1 (dark); `input` on `background` 4.63:1 (light) and 4.16:1 (dark); `ring` on `background` 3.52:1 (light). `gray-400` on `gray-50` is only 2.43:1: never use it for text or input borders.

## Role derivation

`palette.py ramp` applies these rules. Stops are `[light / dark]`. If a stop is not in the layout, use the nearest stop; on a tie use the one nearer 50.

| Role | Rule |
|---|---|
| `background`, `muted`, `secondary` | neutral `[95 / 10]` |
| `card`, `popover` | neutral `[100 / 10]` |
| surface (raised page area) | neutral `[100 / 15]` |
| `foreground` | neutral `[0 / 100]` |
| inverted surface (tooltip, toast) | neutral `[10 / 95]` |
| `border` | neutral 0 at alpha 20/255 (light), neutral 100 at alpha 26/255 (dark), composited to hex |
| hover and pressed overlay | neutral `[0 / 100]` at alpha 13/255 and 26/255 |
| `muted-foreground` | start neutral `[30 / 65]`, need 4.5:1 on every surface |
| `input` (control edge) | start neutral `[60 / 50]`, need 3:1 on every surface |
| `primary`, status fill | light: the seed. Dark: the seed if its content reaches 4.5:1 and it reaches 3:1 on every dark surface; else the stop nearest 70 that does. |
| `primary-foreground`, `<status>-foreground` | first choice neutral 100 (light) or 5 (dark); warning prefers 5; the other if the first fails 4.5:1; then the ramp end |
| `accent` (tint) | accent ramp `[85 / 30]` |
| `<status>-muted` (tint) | status ramp 85 (light); stop 75 at alpha 61/255 (dark), composited over the page |
| `<status>-text` | start at the seed stop (exact fill) or `[45 / 80]`, need 4.5:1 on every surface and on the tint |
| `<status>-muted-foreground` | start `[25 / 80]`, need 4.5:1 on the tint |
| `<status>-border` | start `[80 / 65]`, need 3:1 on every surface |
| `ring` | start at the seed stop, need 3:1 on every surface |

Stop search: order all stops by distance from the start; on a tie take the one that moves toward more contrast (darker in light, lighter in dark). Take the first stop whose lowest ratio on all grounds meets the need. If none does, take the best and report a failure.

"Every surface" is page, surface and card. A light fill under 3:1 on a light surface gets a warning with the first darker colour that passes (RGB scaled by `1 - i/100`). Never invent a hue for a gray seed: if its chroma is under 0.02, treat it as neutral.

If a theme sets only status fills, derive their content per mode and never create a fill. Content on a fill: the theme's light or dark content colour, whichever contrasts more; below 4.5:1, `#fff` or `#000`. Status text: keep the fill hue and chroma, solve for lightness to reach 4.58:1 (4.5 plus a 0.08 margin) on surface and card, 32 bisection steps, and lower chroma by 10 percent per step until it is in gamut. A status fill or text colour needs OKLCH chroma 0.03 or more and lightness 0.20 to 0.95; else use the base theme's hue. Derive a role again when the theme sets a colour the role is measured against. A value the theme sets always wins.

Secondary text on a tinted ground (accent tint, neutral track, coloured card): search the neutral ramp for 4.5:1 on that ground over every surface and under the hover overlay. Emit it as a per-component override, not a new token.

## Dark mode rules

- Override semantic tokens only. Primitives and component tokens stay the same.
- Do not invert. Use a dark gray (`#0a0a0a` to `#121212`, or a tinted near-black) rather than pure `#000` for large surfaces, unless the style is OLED black by design.
- Shift fills one or two steps lighter and less saturated (`blue-600` becomes `blue-400`).
- Show elevation with lighter surfaces (`card` lighter than `background`), not heavier shadows.
- Borders and dividers must stay visible: check them on their own.
- Measure every pair again. Light-mode results do not carry over.
- Respect `prefers-color-scheme` as the default; let the user override; store the choice.

## Contrast check

WCAG 2.2 ratio. Compute it; do not estimate it: `python3 -I scripts/palette.py contrast '#2563eb' '#ffffff'` prints `5.16`. A ratio is truncated to 2 decimals, never rounded up: 4.499 shows as 4.49 and fails 4.5.

| Pair | Need | WCAG |
|---|---|---|
| Primary and secondary text on every surface | 4.5:1 | 1.4.3 |
| Content on the accent fill and on each status fill | 4.5:1 | 1.4.3 |
| Content on each status tint, tint drawn over the surface | 4.5:1 | 1.4.3 |
| Role text on every surface and on its tint | 4.5:1 | 1.4.3 |
| Secondary text on each coloured tint | 4.5:1 | 1.4.3 |
| Strong fill against every surface (fill boundary) | 3:1 | 1.4.11 |
| Secondary icon, role icon, status border on every surface | 3:1 | 1.4.11 |
| Form-control edge against surface and page | 3:1 | 1.4.11 |
| Focus ring against every surface | 3:1 | 1.4.11, 2.4.13 |
| Large text (24 px, or 18.66 px bold) | 3:1 (AAA 4.5:1) | 1.4.3 |
| Disabled controls | exempt | - |

For a translucent colour, composite it over the real ground (`top * alpha + under * (1 - alpha)` per channel), round to 8-bit hex, then measure. Run `palette.py check` on the token file; it prints 1 line per pair.

## Scale formulas

- Type: size at step `s` (-5 to 6) is `round(base * ratio^s)` px, written as rem (`px / 16`, 4 decimals). Headings h1 to h6 use steps 3, 2, 1, 0, -1, -2. Body, label and code use 0, large text 1, supporting text -1, display sizes 4, 5, 6.
- Line height for size `f` px: target factor 1.5 when `f < 20`, 1.4 when `f < 32`, else 1.25. Snapped height is `max(4 * round(f * target / 4), 4 * ceil((f + 4) / 4))`. Leading is snapped height over `f`. Example: base 16, ratio 1.25, h1 is 31 px, 44 px high, leading 1.4194.
- Radius from `base` and `multiplier`: inner `round(base * mult)`, element `2x`, container `3x`, page `7x` px; none 0; full 9999px.
- Spacing is rem: step `N` is `N * 0.25rem`. A px step is allowed only as a documented exception.
- Motion: tempo is clamped to 0.75 to 1.5 and multiplies every duration. A character (`crisp`, `standard`, `expressive`) sets the easing curves. An explicit duration wins over tempo, an explicit token over both. Busy-indicator loops ignore tempo.
- Adaptation rules are ordered `{when, value}` pairs. `when` combines a width bound (sm 640, md 768, lg 1024, xl 1280, 2xl 1536 px), pointer (`coarse`, `fine`), contrast preference and reduced motion. A rule compiles to 1 media query. A rule cannot turn reduced motion off for the whole theme.

## Theme inheritance

- A child names 1 base theme. The base must be a fully built theme; anything else is an error, never a partial theme.
- The built child is complete and flat. Its stylesheet works without the base stylesheet.
- Precedence, lowest first: resolved base; scale-generated values (colour, type, radius, motion); palette role tokens; explicit tokens; component typography, then palette component overrides, then explicit component overrides; surface overrides; local tokens; icon registries. A later layer wins token by token, whatever the key order.
- Component overrides merge deeply: restating 1 property keeps the other inherited properties of the same component. Motion inherits field by field. Adaptation rules keep order: inherited first, then local.
- A role the base sets counts as set in the child. A role the base only derived is derived again from the child's colours.

## Token JSON (W3C DTCG)

Use when a tool needs JSON (Style Dictionary, Figma variables).

```json
{
  "color": { "blue": { "600": { "$type": "color", "$value": "#2563eb" } } },
  "semantic": { "primary": { "$type": "color", "$value": "{color.blue.600}" } },
  "space": { "4": { "$type": "dimension", "$value": "1rem" } }
}
```

## Migration from flat values

1. List every distinct raw value in the code (the grep in SKILL.md step 8).
2. Snap each value to the nearest scale step; record each snap that changes a pixel.
3. Create primitives, then semantic aliases, then replace usages file by file.
4. Rerun the grep and the contrast table.
