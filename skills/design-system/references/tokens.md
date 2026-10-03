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

Generate 11 steps per hue (50, 100 ... 900, 950). Prefer OKLCH: keep hue fixed, step lightness evenly, reduce chroma at both ends. One neutral ramp per product; do not mix a cool gray and a warm gray.

```css
:root {
  --color-gray-50: #f9fafb;  --color-gray-100: #f3f4f6; --color-gray-200: #e5e7eb;
  --color-gray-300: #d1d5db; --color-gray-400: #9ca3af; --color-gray-500: #6b7280;
  --color-gray-600: #4b5563; --color-gray-700: #374151; --color-gray-800: #1f2937;
  --color-gray-900: #111827; --color-gray-950: #030712;
  --color-blue-400: #60a5fa; --color-blue-500: #3b82f6; --color-blue-600: #2563eb;
  --color-blue-700: #1d4ed8;
  --color-red-600: #dc2626;  --color-green-600: #16a34a; --color-amber-500: #f59e0b;
}
```

Hex, HSL, and OKLCH all work (OKLCH example: `oklch(0.55 0.22 264)`). Pick one format per project.

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

Measured: `primary` on white 5.17:1; `muted-foreground` on `muted` 6.87:1 (light) and 5.78:1 (dark); `input` on `background` 4.63:1 (light) and 4.16:1 (dark); `ring` on `background` 3.52:1 (light). `gray-400` on `gray-50` is only 2.43:1: never use it for text or input borders.

## Dark mode rules

- Override semantic tokens only. Primitives and component tokens stay the same.
- Do not invert. Use a dark gray (`#0a0a0a` to `#121212`, or a tinted near-black) rather than pure `#000` for large surfaces, unless the style is OLED black by design.
- Shift fills one or two steps lighter and less saturated (`blue-600` becomes `blue-400`).
- Show elevation with lighter surfaces (`card` lighter than `background`), not heavier shadows.
- Borders and dividers must stay visible: check them on their own.
- Measure every pair again. Light-mode results do not carry over.
- Respect `prefers-color-scheme` as the default; let the user override; store the choice.

## Contrast check

WCAG 2.x ratio. Compute it; do not estimate it.

```js
// node -e, or a browser console
const lin = c => { c /= 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
const L = hex => { const n = parseInt(hex.slice(1), 16);
  return 0.2126 * lin(n >> 16) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255); };
const ratio = (a, b) => { const [x, y] = [L(a), L(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
console.log(ratio('#2563eb', '#ffffff').toFixed(2)); // 5.17
```

| Element | AA | AAA |
|---|---|---|
| Normal text | 4.5:1 | 7:1 |
| Large text (24 px, or 18.66 px bold) | 3:1 | 4.5:1 |
| UI boundaries, icons that carry meaning, focus rings | 3:1 | - |
| Disabled controls | exempt | - |

For a translucent color, composite it over the real background first, then measure. Record results in a table: pair, mode, ratio, pass or fail.

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
