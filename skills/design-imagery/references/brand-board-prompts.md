# Brand board and logo prompts

Use this file to generate a brand board (one image that shows a whole identity system) or a logo exploration (several marks to choose from). Brand strategy, voice, and token files belong to the brand-identity skill, if it is available. This file covers the images.

## 1. Strategy before symbols

Write 1 line for each before you prompt: category, audience, product action, emotional promise, core metaphor, trust level, what the brand must avoid. Every symbol must come from this list. Do not pick symbols at random.

| Category | Core ideas | Symbol logic |
|---|---|---|
| Developer tool | build, speed, precision | cursor, frame, scaffold, grid |
| AI assistant | delegation, clarity | spark, orbit, signal, path, node |
| Security | protection, vigilance | shield, eye, seal, protected core |
| Games, chance | reward, tension | dice, gem, card, trophy |
| Voice, audio | sound, rhythm | waveform, mic, orb, speech path |
| Compliance, legal | trust, order | seal, badge, document, stamp |
| Robotics, drones | flight, control, vision | wing, crosshair, path, zone |
| Luxury, editorial | taste, ritual, restraint | monogram, emboss, vessel |
| Productivity | focus, momentum | path, check, block, light |

## 2. Logo concept methods

Use one method, or combine two at most.

1. Monogram with meaning: an initial plus a metaphor, built with a cut, fold, or negative space (K + frame, S + sound path).
2. Product action: the main verb as an abstract shape (build = scaffold, protect = boundary, automate = loop).
3. Metaphor fusion: two ideas reduced to one readable mark (shield + mountain, moon + waveform).
4. Negative space: a hidden arrow, a protected center, a cutout initial. Edges must be crisp.
5. Construction geometry: circles, diagonals, grids, modular blocks, orbital paths, measured lines.

Logo types, for the prompt and for the report:

| Type | Use when |
|---|---|
| Wordmark | the name is short and distinctive |
| Lettermark | the name is long; 2 to 4 initials |
| Pictorial mark | one clear object carries the idea |
| Abstract mark | the idea is a quality, not an object |
| Combination mark | a new brand needs the name and the symbol together |
| Emblem | a heritage or institutional tone; check small sizes |
| Mascot | family, food, or sports brands with a character |

Style modifiers (add one to the prompt): minimalist (simple geometry, one color); geometric (precise, symmetric); line art (single continuous stroke); negative space (hidden second image); lettermark (typographic monogram); wordmark (custom lettering); emblem (enclosed badge); vintage (badge, muted earth tones); luxury (thin lines, serif, restrained metallic); organic (flowing lines, earth tones); playful (rounded shapes, bright but limited palette).

Reject: generic lightning bolts without a reason, random animals, fake crests, copies of famous marks, clip art, meaningless sparkles, more than 3 colors.

## 3. Logo exploration prompt

Generate 4 to 6 directions, one image each, each from a different method. Generate the symbol without text first; image models often misspell. Set the wordmark later in a real font.

```
Logo design reference for "<brand>", a <category> for <audience>.
Idea: <method>: <the two concepts and how they combine>.
Type: <logo type>. Style: <one style modifier>.
Colors: <1 to 2 hex values>, also readable in solid black.
Render: one centered mark on a plain white background, square 1:1, flat vector style, crisp edges, high contrast, clear silhouette, generous margin.
No text unless specified. Not photorealistic, no 3D render, no texture, no gradient mesh, no mockup, no multiple logos, no watermark.
```

Variation prompt, after the operator picks a direction:

```
Alternative version of the "<brand>" mark. Keep: <elements>. Change: <elements>. Style direction: <modifier>. Same render rules as before.
```

Logo checks (record pass or fail per direction):

- The silhouette reads at 16 x 16 px. Downscale the image and look, or ask for a favicon-size render.
- It works in one color, and reversed on a dark background.
- No detail disappears at business-card size.
- It connects to the strategy line, in 1 sentence you can write in the report.

## 4. Brand board layouts

Default: one overview image, 4:3 or 16:10, on a clean presentation grid with even gutters. The canvas may be dark or light; pick what fits the brand. Text is sparse: the brand name, 1 tagline, 2 to 5 labels, short UI chips.

Default 3 x 3 panels:

1. Logo cover: mark and wordmark, large negative space.
2. Construction: the grid or geometry that explains the mark.
3. Digital application: browser bar, app header, terminal, or app icon.
4. Brand essence: one short tagline in large type.
5. Color system: swatches with the hex values.
6. Typography: a large specimen, primary and secondary pairing.
7. Physical application: card, badge, label, packaging, or folder.
8. Image direction: one art-directed scene in the palette.
9. System detail: icon row, chips, input bar, or pattern.

Other layouts: 2 x 3 deck (logo, product surface, functional panel, atmosphere image, symbol or seal, tagline); 2 x 2 compact; 1 x 3 strip; 4 x 2 contact sheet. Give panels a rhythm (quiet, functional, emotional, technical, atmospheric, detailed). Do not make every panel equally loud.

Visual modes (choose 1):

| Mode | For | Cues |
|---|---|---|
| Dark builder | dev tools, infra, AI builders | near-black, monospace accents, terminal, subtle grid, one cyan, coral, or lime accent |
| Dark operator | growth, sales, automation | black with amber or red, UI chips, progress motifs |
| Dark calm | strategy, travel, wellness SaaS | deep green, lime accent, misty landscapes, soft overlays |
| Dark security | security, monitoring | black or navy, shield forms, radar lines, alert chips |
| Light editorial | legal, privacy, documents | ivory, paper texture, seals, deep blue, red, gold |
| Luxury | beauty, fashion, hospitality | ivory, stone, espresso, serif wordmark, emboss, soft shadow |
| Voice | voice AI, chat, audio | dark indigo, lilac, waveform, mic motif |
| Cultural | music, events, creative tools | halftone, CRT or print texture, one bold accent, poster panels |

Taglines: short and specific ("Nothing random.", "On guard.", "Build better."). No buzzword soup.

## 5. Brand board prompt

```
Brand identity board for "<brand>", one image, <3x3 | 2x3 | custom> grid, <4:3 | 16:10>, <dark | light> presentation canvas, even gutters, generous negative space.
Strategy: category <x>; audience <x>; personality <3 traits>; metaphor <x>; mark <method and idea>.
Mode: <visual mode>. Palette: base <#hex>, primary accent <#hex>, secondary accent <#hex>, neutrals <#hex>; accents repeat across panels.
Panels: <list in order with 1 line each>.
Type: <pairing>, large and readable, no small fake text.
The same mark appears consistently on every panel that shows it.
Exclude: copied real logos, stock people, office photos, robot clichés, rainbow colors, cheap neon, full fake dashboards, lorem ipsum.
```

## 6. Identity mockups (optional)

For single deliverable mockups (business card, signage, apparel), describe: deliverable, surface and angle, materials and finish, lighting (studio, natural, dramatic, warm), context (marble desk, wood table, office, flat lay), and the logo placement. If you have the chosen logo as an image and the tool accepts image input, pass it; otherwise expect the model to reinterpret the mark, and label the mockup "concept only". Exclude distorted or misspelled text.

## 7. When the operator gives references

Take from them: grid style, gutter and spacing, type scale, density, logo placement, amount of text, image treatment, accent logic. Do not take: the logo, the name, the slogan, the exact composition, or any unique asset.
