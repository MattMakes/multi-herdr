# Typography

Type carries the design. Wrong type cannot be saved by anything else on the page.

## Rules

- **A pairing, not one font.** One display face and one body face. A third face (an "outlier") is allowed for one role only, in at most two places: the wordmark and a hero figure, or code. Same family at different weights is one family. Mono counts as a family when it appears outside code. A single-family page is allowed only when the single face is the idea (a true terminal or poster aesthetic).
- **Commit to contrast.** Heading weight differs from body weight by 300 or more (400 body with 700 headings, or 400 with 200). 400 next to 600 reads as a default.
- **A fixed scale.** Choose one ratio (1.2, 1.25, 1.333, or 1.5) from a 16 px body and use only its steps. At most 6 sizes on a page. Need more hierarchy? Use weight or color, not another size.
- **Measure.** Prose 45 to 75 characters; default `max-width: 65ch`.
- **Leading.** Body 1.5 to 1.65. Display 1.0 to 1.15. All-caps display never below 1.0 (cap tops collide on wrap).
- **Tracking.** Display `-0.02em` to `-0.04em`. Small uppercase labels `+0.08em` to `+0.14em`. Body never above `0.05em`.
- **Roman headings.** No italic headings and no italic emphasis word inside a heading. Italic is for emphasis in body paragraphs.
- **Real punctuation.** `“ ” ‘ ’ …`, en dash for ranges, non-breaking space before units (`10 kg`).
- **Numbers.** `font-variant-numeric: tabular-nums` on tables, prices, stats, and timers.
- **Loading.** `font-display: swap`; load only the weights you use; never synthesized bold or italic. Use the framework font loader (`next/font`, `@fontsource/*`) or self-hosted `@font-face` in production. Match fallback metrics (`size-adjust`, `ascent-override`) to limit layout shift.
- **Floors.** Body 16 px. Nothing below 12 px. No all-caps paragraphs. No justified text without hyphenation.
- **Semantic order.** `h1` then `h2` then `h3`; style freely, but do not skip levels.
- **Wrapping.** `text-wrap: balance` on headings and `text-wrap: pretty` on short paragraphs to avoid one-word last lines.

## Choosing faces

Choose by the tone in the design read, not by habit. Three tests:

1. **Not the default.** Inter, Roboto, Open Sans, Lato, Poppins, Montserrat, Raleway, Work Sans, DM Sans, Nunito, Arial, Helvetica, and the bare system stack are what every generated page uses. Use one only with a written reason: the brand specifies it, the project already uses it, or the brief is public-sector or accessibility-first.
2. **Not the generated "premium" default either.** Fraunces and Instrument Serif as display, and Playfair Display as body, are the faces generated pages reach for when told "editorial" or "premium". Use them only with a stated fit. A serif display needs a reason (editorial, publishing, luxury, heritage); "it feels creative" is not one.
3. **Not your last project.** Do not reuse the display face of the previous build for an unrelated brief.

Free baseline faces, by role. Prefer variable fonts. Name a paid foundry face in code only when the user confirms the license; otherwise the page falls back to a system font.

| Role | Faces (Google Fonts unless marked; FS = Fontshare) |
| --- | --- |
| Editorial serif display | Newsreader, EB Garamond, Cormorant Garamond, Bodoni Moda, DM Serif Display, Source Serif 4, Cardo |
| Grotesque or geometric display | Geist, Bricolage Grotesque, Space Grotesk, Cabinet Grotesk (FS), General Sans (FS), Satoshi (FS), Switzer (FS) |
| Condensed or heavy display | Anton, Big Shoulders Display, Tanker (FS), Clash Display (FS), Tomorrow |
| Sans body | Geist, IBM Plex Sans, Switzer (FS), General Sans (FS), Satoshi (FS) |
| Serif body | Newsreader, Source Serif 4, EB Garamond, Crimson Pro, Spectral |
| Mono | Geist Mono, JetBrains Mono, IBM Plex Mono, Commit Mono, Space Mono |

Pairings by tone (display + body):

| Tone | Free pairing examples |
| --- | --- |
| Technical | Geist 700 + Geist; JetBrains Mono headings + IBM Plex Sans |
| Editorial | Newsreader + Switzer; Cabinet Grotesk + Source Serif 4 |
| Brutalist | Anton or Bricolage Grotesque 800 + Geist; mono numerals |
| Soft / friendly | Bricolage Grotesque 500 + Geist; Satoshi + Newsreader |
| Luxury | Cormorant Garamond or Bodoni Moda + EB Garamond or Crimson Pro |
| Austere | Switzer 400 + Switzer; Geist 400 + Geist Mono |

For dashboards and dense product UI, use a sans body and mono numerals; keep serif display out.

## Scale

A 1.25 scale from 16 px, as tokens:

```css
:root {
  --text-xs: 0.8rem;      /* 12.8px  labels, captions */
  --text-sm: 0.875rem;    /* 14px    secondary UI */
  --text-base: 1rem;      /* 16px    body */
  --text-lg: 1.25rem;     /* 20px    lead */
  --text-xl: 1.5625rem;   /* 25px    h3 */
  --text-2xl: 1.953rem;   /* 31px    h2 */
  --text-3xl: 2.441rem;   /* 39px    h1 in app UI */
  --text-display: clamp(2.75rem, 5vw + 1rem, 5.25rem);
}
```

Keep the display maximum at or below 5.5rem (88 px), 6rem for heavy poster themes. Only a single word or short number of 12 characters or less goes larger.

## Hero headline sizing

Count the characters of the hero `h1`.

| Length | Size |
| --- | --- |
| up to 20 | full display size |
| 21 to 50 | display size; step down one rung if it wraps past 2 lines at 414 px |
| 51 to 90 | one rung below display; consider a shorter headline plus a subline |
| over 90 | rewrite shorter, or cap at `--text-3xl` |

When you write the headline, aim for 7 words and 50 characters or fewer. The headline is at most 3 lines at 1280 px. Widen the container before you shrink the type; a narrow `max-width` on the `h1` is the usual cause of a 5-line headline.

## Wordmark

On a brand-led page the wordmark may use the display or outlier face. On a page where display and body are the same family, a contrasting wordmark face adds the one register that says "this is a brand". Keep it small and typeset, not decorated.
