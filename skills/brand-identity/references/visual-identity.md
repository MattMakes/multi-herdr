# Visual identity

## Brand idea first

Answer these before any visual choice:

1. What does the brand stand for, in one sentence?
2. What is the core metaphor?
3. How does the mark express that metaphor?
4. How does the system scale across UI, print, imagery, and small details?
5. What makes it ownable (a competitor could not use it)?

Infer from: category, audience, product function, emotional promise, trust level, cultural position, and what the brand must avoid.

### Symbol logic by category

| Category | Core ideas | Symbol directions |
|---|---|---|
| Developer tool | Building, speed, precision, control | Cursor, frame, scaffold, grid, bracket |
| AI assistant | Delegation, clarity, intelligence | Spark, orbit, signal, path, node |
| Security | Protection, vigilance, boundary | Shield, eye, seal, protected core |
| Compliance, legal | Trust, order, rules | Seal, stamp, document, monogram |
| Finance | Stability, growth, precision | Ledger line, column, upward path, seal |
| Voice, audio | Sound, rhythm, flow | Waveform, orb, speech path, pulse ring |
| Productivity | Focus, momentum, clarity | Path, check, block, light |
| Robotics, drones | Flight, control, vision | Wing, crosshair, path, zone |
| Games, chance | Reward, tension, speed | Die, gem, card, trophy |
| Luxury, editorial | Taste, material, ritual, restraint | Monogram, seal, emboss, vessel |
| Wellness, nature | Calm, growth, balance | Leaf, horizon, moon, circle |

Do not pick a symbol at random. Each symbol must trace back to the brand idea.

## Logo types

| Type | What it is | Best for | Risk |
|---|---|---|---|
| Wordmark | The name in custom type | Short, distinctive names | Weak with long or generic names |
| Lettermark | Initials | Long names, firms | Low recognition without equity |
| Pictorial mark | A recognizable symbol | Brands with recognition | Literal symbols date fast |
| Abstract mark | A non-representational shape | Tech, differentiation | Needs meaning to be memorable |
| Combination mark | Symbol plus name in a lockup | New brands (most flexible) | Two parts to keep consistent |
| Emblem | Name inside a badge or seal | Heritage, institutions, craft | Fails at small sizes |
| Mascot | A character | Consumer, food, kids, sports | Hard to scale down; can feel dated |

New brands: start with a combination mark, so the symbol and the name can separate later.

## Concept methods (use 1, combine at most 2)

1. **Monogram plus meaning:** the initial shaped by a metaphor (K plus kite, S plus sound wave). Use cuts, folds, or negative space; never a plain letter in a circle.
2. **Product action:** turn the main verb into a shape (build to frame, protect to boundary, convert to switch, automate to loop). Abstract, not literal.
3. **Metaphor fusion:** two ideas in one reduced mark (owl plus drone vision, shield plus mountain). Subtle and readable.
4. **Negative space:** a hidden arrow, a protected center, an internal path. Keep the space crisp at small sizes.
5. **Construction geometry:** a mark built from a visible system (circles, a grid, diagonal cuts, modular blocks). Document the construction.

### Mark standard

The mark is simple, memorable, symbolic, scalable, balanced, and usable as icon, wordmark lockup, badge, app icon, and pattern.

Avoid: generic lightning bolts or sparkles without a reason, random animals, fake crests, clip-art icons, gradients that carry the shape, more than 2 colors, details that vanish at 16 px, anything close to a famous mark.

Checks:

- Recognizable at 16 x 16 px (favicon) and at 32 px in UI.
- Works in one color, in black on white, and in white on black.
- Silhouette test: filled solid, the shape still reads.
- Squint test: the main form survives blur.
- Search test: a reverse image search or a trademark search finds no near match (flag for legal review; do not claim clearance).

If the brief asks for a drawn mark, a simple SVG concept in code is in scope. Rendered logo images, mockups, and brand boards belong to the `design-imagery` skill if available.

## Logo system

### Variants

| Variant | Use |
|---|---|
| Primary (horizontal lockup) | Headers, documents, most uses |
| Stacked | Square spaces, social avatars |
| Symbol only | Favicon, app icon, small UI |
| Wordmark only | Where the symbol already appears nearby |

### Color versions

| Version | Background |
|---|---|
| Full color | White and light neutrals |
| Reversed (white) | Brand primary, dark backgrounds |
| One-color dark | Light backgrounds where color is not possible |
| One-color light | Dark backgrounds where color is not possible |
| On photography | Use a solid version on a calm area; add a scrim if needed; measure contrast |

### Clear space and minimum size

- Clear space on all sides: at least the height of the symbol (or the cap height of the wordmark for a wordmark).
- Minimum digital width: full lockup 120 px (80 px absolute floor), symbol 24 px, symbol in UI 32 px.
- Minimum print width: full lockup 25 to 35 mm, symbol 10 mm.

### Misuse (never)

Stretch or squash, rotate, recolor outside the palette, add gradients, shadows, glows, or outlines, change transparency, crop, rearrange parts, add elements, place on a busy background without a scrim, or set the wordmark in another font.

### Co-branding

Equal visual weight (match cap height or optical size), clear space applies to both, separate with space or a thin divider, each logo in its own approved colors.

### Files and placements

| Use | Format |
|---|---|
| Web, UI | SVG (primary), PNG with transparency (fallback) |
| Print | PDF or EPS (vector) |
| Favicon | SVG favicon plus 32 x 32 PNG; Apple touch icon 180 x 180 |
| Social avatar | Symbol only, square, centered with padding (most platforms crop to a circle) |

Name files `logo-<variant>-<color>.<ext>` (for example `logo-horizontal-reversed.svg`). Keep one source folder.

## Color roles

| Role | Count | Rule |
|---|---|---|
| Primary | 1 (plus tints) | The brand hue. Logo, key UI, headings. |
| Accent | 0 or 1 | Calls to action and highlights, 5 to 10% of a layout. |
| Neutrals | 3 to 5 | Background, surface, text, muted text, border. One family. |
| Status | 4 | Success, warning, error, info. Never brand decoration. |

Document each color with name, role, hex, RGB (and CMYK or Pantone only when print needs it), and the contrast it achieves with its usual partners. Compute contrast with the WCAG formula (`(L1 + 0.05) / (L2 + 0.05)`); text needs 4.5:1, large text and UI 3:1.

Accent discipline: an accent repeats across touchpoints; one accent can carry a whole system. No rainbow unless the brief asks for it. A purple-to-blue "AI" gradient is a cliché: use it only with a reason tied to the brand idea.

### Industry cues

A starting point, not a rule. Break a cue on purpose when differentiation matters more than convention.

| Industry | Common logo types | Common hues | Type tone | Avoid |
|---|---|---|---|---|
| Technology, SaaS | Abstract, minimal, geometric | Indigo, sky, emerald | Geometric sans | Clip art, dated fonts |
| Healthcare, pharmacy | Minimal, line, professional | Blue, teal, green | Clean sans | Red (blood), aggressive forms |
| Finance, insurance, accounting | Wordmark, lettermark, emblem | Navy, deep blue, teal | Serif or sober sans | Playful effects |
| Legal | Wordmark, emblem, lettermark | Navy, near-black, brown | Traditional serif | Casual fonts, bright colors |
| Food, restaurant, bakery | Badge, mascot, combination | Red, orange, gold, brown | Friendly script or bold sans | Cold blues |
| Fashion, jewelry, luxury | Wordmark, monogram | Black, white, gold | Elegant serif or thin sans | Trendy effects |
| Beauty, spa, wellness | Script, organic, minimal | Blush, teal, soft gold | Thin script or light sans | Harsh, loud forms |
| Education | Wordmark, emblem | Indigo, violet, green | Clear readable sans | Childish unless for kids |
| Sports, fitness | Dynamic, bold abstract | Red, orange, black | Bold condensed sans | Weak, delicate forms |
| Entertainment, music, gaming | Bold abstract, mascot | Violet, pink, amber, cyan | Display, experimental | Conservative, static |
| Eco, agriculture, energy | Organic, hand-drawn, abstract | Greens, earth browns | Organic or clean sans | Industrial, polluting cues |
| Crypto, web3 | Geometric, gradient, abstract | Violet, cyan, orange | Geometric, futuristic | Dated traditional forms |
| Trades (plumbing, electrical, moving) | Badge, combination, bold | Blue, orange, slate | Strong bold sans | Weak, delicate forms |
| Hospitality, travel | Elegant wordmark, combination | Gold, teal, navy, sky | Elegant serif or friendly sans | Cold, industrial cues |

### Identity modes

| Mode | Fits | Base and accents | Type | Mood |
|---|---|---|---|---|
| Dark builder | Developer tools, infra, AI builders | Near-black; cyan, coral, or lime | Sans plus mono accents | Precise, sharp |
| Dark operator | Sales, growth, automation | Black, dark red, amber | Bold sans | Fast, tactical |
| Dark calm | Strategy, travel, climate | Deep green, lime accent, fog gray | Quiet sans | Calm, focused |
| Security | Security, monitoring, compliance | Black or navy; red and blue alert chips | Technical sans | Serious, vigilant |
| Light editorial | Legal, privacy, documents | Warm ivory; deep blue, red, gold | Serif labels, clean sans | Refined, institutional |
| Luxury | Beauty, fashion, hospitality | Ivory, stone, espresso | Serif wordmark, monogram | Tasteful, adult |
| Voice | Voice AI, chat, audio | Dark indigo, lilac | Rounded sans | Fluid, intimate |
| Cultural | Music, events, creative tools | Bold accent on neutral, print texture | Custom wordmark, display | Memorable, controlled |

## Type roles

| Role | Typical choice | Rule |
|---|---|---|
| Display / headings | Brand character face | Short text only; 600 to 700 weight |
| Body | Highly legible text face | 16 px minimum on screen, line height 1.5 to 1.6 |
| UI labels | Body family at 500 | 14 px minimum |
| Mono | Data, code, IDs | Tabular figures |

At most 2 families plus a mono. Define a scale (sizes, weights, line heights for H1 to caption, desktop and mobile), tracking (display slightly tight, all caps +0.05em), and the license for each family. Use the `design-system` skill, if available, to turn the scale into tokens.

## Imagery

State one direction and keep it:

- **Photography:** lighting (natural, soft, dramatic), subjects (real people at work, products in use, places), composition (centered, rule of thirds, close crops), color treatment (warm, muted, high contrast, brand-tinted).
- **Illustration:** style (flat, line, textured), stroke weight, corner radius, palette (brand colors only), complexity level.
- **Texture and graphic devices:** grain, halftone, grid lines, a pattern built from the mark. Use sparingly and consistently.
- **Avoid:** generic stock (handshakes, people pointing at screens), clichéd robot imagery, busy scenes, images that fight the palette.

## Icons

One family, one style (outline, filled, or duotone), one stroke width (1.5 or 2 px), a 24 px base grid, consistent corner radius. Icons that carry meaning need a text label or an accessible name; never use emoji as UI icons.

## Taglines

Short, specific, ownable. Good: "Nothing random.", "Every mission under control.", "Clarity builds confidence." Bad: generic slogans, buzzword strings, long marketing sentences. Test: would it be false for a competitor? If not, rewrite it.
