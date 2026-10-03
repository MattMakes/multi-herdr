# Tailwind and shadcn/ui wiring

Detect the version before you write config: Tailwind v4 has `@import "tailwindcss"` in CSS and usually no `tailwind.config.*`; v3 has `@tailwind base;` and a config file. shadcn/ui projects have `components.json`. Follow what the project already uses; do not migrate versions as a side effect.

## Tailwind v4 (CSS-first)

Semantic variables live in `:root` and `.dark`; `@theme inline` maps them to utilities. This is the current shadcn/ui layout.

```css
@import "tailwindcss";
@custom-variant dark (&:is(.dark *));

:root {
  --radius: 0.625rem;
  --background: oklch(1 0 0);            --foreground: oklch(0.145 0 0);
  --card: oklch(1 0 0);                  --card-foreground: oklch(0.145 0 0);
  --popover: oklch(1 0 0);               --popover-foreground: oklch(0.145 0 0);
  --primary: oklch(0.205 0 0);           --primary-foreground: oklch(0.985 0 0);
  --secondary: oklch(0.97 0 0);          --secondary-foreground: oklch(0.205 0 0);
  --muted: oklch(0.97 0 0);              --muted-foreground: oklch(0.556 0 0);
  --accent: oklch(0.97 0 0);             --accent-foreground: oklch(0.205 0 0);
  --destructive: oklch(0.577 0.245 27.325);
  --border: oklch(0.922 0 0);            --input: oklch(0.922 0 0);
  --ring: oklch(0.708 0 0);
}
.dark {
  --background: oklch(0.145 0 0);        --foreground: oklch(0.985 0 0);
  --card: oklch(0.205 0 0);              --card-foreground: oklch(0.985 0 0);
  --primary: oklch(0.922 0 0);           --primary-foreground: oklch(0.205 0 0);
  --muted: oklch(0.269 0 0);             --muted-foreground: oklch(0.708 0 0);
  --border: oklch(1 0 0 / 10%);          --input: oklch(1 0 0 / 15%);
  /* ...every token from :root */
}

@theme inline {
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  /* one line per semantic token */
  --radius-sm: calc(var(--radius) - 4px);
  --radius-md: calc(var(--radius) - 2px);
  --radius-lg: var(--radius);
  --radius-xl: calc(var(--radius) + 4px);
  --font-sans: var(--font-body), ui-sans-serif, system-ui, sans-serif;
  --font-heading: var(--font-display), ui-sans-serif, system-ui, sans-serif;
}

@layer base {
  * { @apply border-border outline-ring/50; }
  body { @apply bg-background text-foreground; }
}
```

- `@theme inline` keeps the `var()` reference, so `.dark` overrides reach the utilities.
- Plain `@theme` (no `inline`) creates new primitives: `--color-brand-500: oklch(...)` gives `bg-brand-500`. Use it for primitive ramps and custom scales (`--spacing-section: 6rem`, `--breakpoint-3xl: 120rem`, `--shadow-card: ...`).
- Measure the defaults too. In the neutral set above, `muted-foreground` on `background` is 4.74:1 (pass) but on `muted` it is 4.35:1 (fails for normal text); `input` against `background` is 1.26:1 and `ring` is 2.58:1 (both below 3:1). Darken `muted-foreground` to about `oklch(0.5 0 0)`, and darken `input` and `ring` when the field boundary or the focus ring must meet 3:1. Measure again after every change.
- Custom utilities: `@utility name { ... }`. Custom variants: `@custom-variant theme-x (&:where([data-theme="x"] *));`.

## Tailwind v3 (config file)

Older shadcn projects store channels without a function: `--primary: 222.2 47.4% 11.2%;`. Map them with `hsl(var(--x))` so opacity modifiers work (`bg-primary/50`).

```ts
// tailwind.config.ts
export default {
  darkMode: ["class"],
  content: ["./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        background: "hsl(var(--background))",
        foreground: "hsl(var(--foreground))",
        primary: { DEFAULT: "hsl(var(--primary))", foreground: "hsl(var(--primary-foreground))" },
        // secondary, muted, accent, destructive, card, popover: same shape
        border: "hsl(var(--border))", input: "hsl(var(--input))", ring: "hsl(var(--ring))",
      },
      borderRadius: { lg: "var(--radius)", md: "calc(var(--radius) - 2px)", sm: "calc(var(--radius) - 4px)" },
      fontFamily: { sans: ["var(--font-body)", "system-ui", "sans-serif"] },
    },
  },
  plugins: [require("tailwindcss-animate")],
}
```

Do not mix channel values and full color values in one project.

## shadcn/ui conventions

- Initialize with `npx shadcn@latest init`; add with `npx shadcn@latest add button card dialog`. Preview changes to existing files with `--dry-run`. Components are copied into the repo (`@/components/ui`) and are yours to edit.
- Theme through variables first; add a variant with `cva` second; edit component source last.
- Extend variants in the component, not with ad-hoc classes at call sites:

```tsx
const buttonVariants = cva("inline-flex items-center justify-center gap-2 rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50", {
  variants: {
    variant: {
      default: "bg-primary text-primary-foreground hover:bg-primary/90",
      secondary: "bg-secondary text-secondary-foreground hover:bg-secondary/80",
      outline: "border border-input bg-background hover:bg-accent hover:text-accent-foreground",
      ghost: "hover:bg-accent hover:text-accent-foreground",
      destructive: "bg-destructive text-white hover:bg-destructive/90",
      link: "text-primary underline-offset-4 hover:underline",
    },
    size: { sm: "h-8 px-3", default: "h-10 px-4", lg: "h-12 px-6 text-base", icon: "size-10" },
  },
  defaultVariants: { variant: "default", size: "default" },
});
```

- Use `className` with `cn()` for one-off layout (margin, width), not for color or radius overrides.
- Compound components (`Card`, `CardHeader`, `CardContent`) over one component with many props.
- Pick the right primitive: `Dialog` for modal content, `AlertDialog` for destructive confirmation, `Sheet` for side panels, `Popover` for contextual content, `DropdownMenu` for action lists, `Tooltip` for hints (one `TooltipProvider` at the app root), `Command` for search palettes, `Toaster` (Sonner) once in the root layout.
- Forms: the current pattern is `Field`, `FieldLabel`, `FieldDescription`, `FieldError` with React Hook Form `Controller` or TanStack Form; older projects use `Form`, `FormField`, `FormItem`, `FormMessage`. Follow the one the repo has.
- Component base: shadcn supports Radix primitives and other bases (Base UI, React Aria). `asChild` exists only on Radix components; Base UI uses `render`. Check `components.json` before you compose.
- Install `next-themes` (Next.js) or a small script for class-based dark mode.

## Radix behavior you get for free (do not rebuild)

| Component | Keyboard and focus |
|---|---|
| Dialog, AlertDialog, Sheet | Focus trap, Esc closes, focus returns to trigger, `aria-modal` |
| DropdownMenu, Select, Menubar | Arrow keys move, Enter/Space select, typeahead, Esc closes |
| Tabs | Arrow keys switch tabs, roving tabindex, `aria-selected` |
| Accordion | Enter/Space toggle, `aria-expanded`, `aria-controls` |
| Popover, Tooltip | Esc closes, tooltip shows on focus as well as hover |

Do not override generated ARIA attributes. Do add names: an icon-only `SelectTrigger` or button still needs `aria-label`.

## Dark mode toggle

```tsx
// Next.js: <ThemeProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange>
// Put suppressHydrationWarning on <html>.
```

```js
// Without a library: run before first paint to avoid a flash
const t = localStorage.theme;
const dark = t === "dark" || (!t && matchMedia("(prefers-color-scheme: dark)").matches);
document.documentElement.classList.toggle("dark", dark);
```

Multiple brand themes: scope semantic overrides with `[data-theme="x"] { --primary: ...; }`.

## Responsive and layout defaults

- Mobile first: base classes for small screens, then `sm` 640, `md` 768, `lg` 1024, `xl` 1280, `2xl` 1536. Use 2 or 3 breakpoints per element.
- Container: `mx-auto max-w-7xl px-4 sm:px-6 lg:px-8`. Text measure: `max-w-prose` (65ch).
- Use container queries (`@container`, `@md:`) for components that live in variable-width slots.
- Full-height sections: `min-h-dvh`, not `h-screen`.
- Use grid for two-dimensional layout, flex for one row or column. Do not compute columns with `calc()` percentages.
- `motion-safe:` and `motion-reduce:` variants gate animation. Combine with the global reduced-motion rule:

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after { animation-duration: 0.01ms !important; animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important; scroll-behavior: auto !important; }
}
```

## Fonts

- Next.js: `next/font` with `variable: "--font-body"`, then reference the variable in the theme.
- Elsewhere: self-host or load with `display=swap`, preload only the 1 or 2 critical files, and set a metric-matched fallback to limit layout shift.

## Checks

- `grep -rn "dark:" src` in a token-driven project should find few hits: semantic tokens should make most `dark:` classes unnecessary.
- `grep -rnE "bg-(red|blue|green|gray|slate|zinc)-[0-9]+" src/components` finds palette utilities that bypass semantic tokens.
- `grep -rn "focus:outline-none" src | grep -v "focus-visible:ring"` finds removed focus rings.
