# Drawing parts

Use these parts when you draw the centre picture yourself. Copy a part, then change its positions, sizes and words to fit your design. The parts depend on the style sheet in `assets/starter.html`:

- Colour tokens: `--c-text`, `--c-sheet`, `--c-line`, `--c-hot` (the accent), `--c-soft`, `--c-fill-a`, `--c-fill-b`, `--c-fill-c`.
- SVG stroke classes: `.edge` (outlines) and `.pointer` (leader lines).
- HTML classes: `.note` (callout), `.plate`, `.plate-tag`, `.plate-line` (card), `.key` (legend), `.flavor`, `.mechanic`, `.tuning`, `.math` (text voices), `.far`, `.near`, `.fine` (text levels).

All words over a drawing are SVG text or HTML placed in inches. No word is ever part of an image.

## Rules for every part

- The drawing layer has 100 drawing units for each inch of the sheet. Its view box follows the sheet size.
- Draw from back to front. A later shape covers an earlier one.
- A leader line is straight or has one bend. It is never curved, and two leader lines never cross.
- All cards on one page use one layout.
- One set of marks for the page. The legend lists it.

## The drawing layer

Put one SVG directly inside the sheet, pinned to its four edges. Do not give it a `width` attribute. The view box is `0 0 (W x 100) (H x 100)` for a sheet of W by H inches:

| Sheet | View box |
|---|---|
| 17 x 11 in | `viewBox="0 0 1700 1100"` |
| 30 x 20 in | `viewBox="0 0 3000 2000"` |
| 24 x 18 in | `viewBox="0 0 2400 1800"` |

Change the view box each time you change the sheet size. The layer is decoration for screen readers. Shapes and leader lines go in the layer. Longer text goes in HTML on top of it.

```html
<svg aria-hidden="true" viewBox="0 0 1700 1100"
     style="inset:0; position:absolute; height:100%; width:100%">
  <!-- shapes first, leader lines last -->
</svg>
```

Font sizes in the layer are drawing units. 100 units is 1 in, which is 72 pt, so 6 pt is about 8.4 units. Write 9 units or more.

## Route with stops

A line in the text colour, with no fill, runs through the stops. A stop is a small white circle with an outline and a bold number in its centre. Number from 0 or from 1, and keep to one choice on the page.

```html
<g data-part="route">
  <path d="M260 840 L470 760 L690 805 L930 650 L1180 610" fill="none"
        stroke-linejoin="round" stroke-width="4" stroke="var(--c-text)"/>
  <g data-part="stops" text-anchor="middle" font-weight="700" font-size="16">
    <circle r="16" cy="840" cx="260" fill="#fff" class="edge"/>
    <text y="846" x="260">0</text>
    <circle r="16" cy="805" cx="690" fill="#fff" class="edge"/>
    <text y="811" x="690">1</text>
    <circle r="16" cy="610" cx="1180" fill="#fff" class="edge"/>
    <text y="616" x="1180">2</text>
  </g>
</g>
```

## Isometric tiles and blocks

A tile is a diamond that is two times as wide as it is tall: pale fill, dark outline. This tile is 160 units wide and 80 tall, with its top point at (800, 300):

```html
<polygon class="edge" fill="var(--c-fill-b)" points="800,300 880,340 800,380 720,340"/>
```

A block is that tile as its top face, plus a left face and a right face of one fixed block height (here 50). Each side face is a slightly darker neutral tone. Draw the side faces first, then the top:

```html
<g data-part="block">
  <polygon class="edge" fill="#bdb6aa" points="880,340 880,390 800,430 800,380"/>
  <polygon class="edge" fill="#d3cdc2" points="720,340 800,380 800,430 720,390"/>
  <polygon class="edge" fill="var(--c-fill-b)" points="800,300 880,340 800,380 720,340"/>
</g>
```

To stack blocks, move each layer up by one block height. Draw the lowest block first and the highest block last. For four blocks on one tile, use the block above four times, with `transform="translate(0,0)"`, then `translate(0,-50)`, then `translate(0,-100)`, then `translate(0,-150)`. Across the whole scene, draw the rows at the back before the rows at the front.

## Room seen from above

A rectangle with a pale fill and an outline. A small white rectangle with an outline sits on one wall as the door. A small label in the centre names the room.

```html
<g data-part="room">
  <rect class="edge" fill="var(--c-fill-a)" height="210" width="320" y="420" x="980"/>
  <rect class="edge" fill="#fff" height="14" width="48" y="623" x="1116"/>
  <text text-anchor="middle" font-size="15" y="530" x="1140">Chart room</text>
</g>
```

## Land and zones

Land is one closed, smooth, curved shape with a pale fill. A zone is a white circle that you can partly see through, with a small grey label in its centre.

```html
<path class="edge" fill="var(--c-fill-b)"
      d="M300 520 Q330 330 560 300 Q820 260 980 380 Q1120 500 1010 680
         Q900 860 620 840 Q360 820 310 680 Q290 600 300 520 Z"/>
<circle fill-opacity="0.5" fill="#fff" r="125" cy="560" cx="640"/>
<text fill="var(--c-soft)" font-size="13" text-anchor="middle" y="565" x="640">Salt flats</text>
```

## Time ruler and phase dividers

A horizontal line in the line colour near the bottom edge. Short ticks sit at equal steps along it, with small grey numbers below them. A caption below the numbers names the unit. A phase divider is a dashed vertical line in a muted cool grey that runs the full height of the drawing.

```html
<g data-part="ruler" stroke-width="2" stroke="var(--c-line)">
  <path d="M150 1010 H1550"/>
  <path d="M150 1010 v14 M430 1010 v14 M710 1010 v14 M990 1010 v14 M1270 1010 v14 M1550 1010 v14"/>
</g>
<g fill="var(--c-soft)" font-size="12" text-anchor="middle">
  <text y="1042" x="150">0</text><text y="1042" x="430">4</text><text y="1042" x="710">8</text>
  <text y="1042" x="990">12</text><text y="1042" x="1270">16</text><text y="1042" x="1550">20</text>
  <text y="1070" x="850">turns</text>
</g>
<path data-part="phase" d="M570 0 V1100" stroke-dasharray="14 6" stroke-width="2" stroke="#93a3b4"/>
```

## Marks

Choose a few marks for the page and use only those. Each mark below is a group whose `data-mark` attribute names it, so the legend and the drawing use the same words.

```html
<g data-mark="player"><text font-size="30" font-weight="700" text-anchor="middle" y="420" x="300">O</text></g>
<g data-mark="enemy"><text font-size="30" font-weight="700" text-anchor="middle" y="420" x="380">X</text></g>
<g data-mark="token"><circle stroke-width="2.5" stroke="var(--c-text)" fill="var(--c-hot)" r="10" cy="410" cx="460"/></g>
<g data-mark="figure" stroke-linecap="round" stroke-width="3" stroke="var(--c-text)" fill="none">
  <circle r="8" cy="392" cx="540"/>
  <path d="M540 400 V426 M526 410 H554 M540 426 L530 446 M540 426 L550 446"/>
</g>
<g data-mark="hit">
  <polygon stroke="var(--c-text)" fill="#f0a91e"
           points="640,380 646,398 664,392 654,408 670,418 652,422 656,440 642,428 630,442 628,424 610,420 626,408 618,392 634,398"/>
</g>
<g data-mark="cone"><path d="M720 410 L840 360 A130 130 0 0 1 840 460 Z" fill-opacity="0.35" fill="#79ade0"/></g>
```

## Leader line

A thin line in the line colour, with no fill, goes from the callout to its target. It is straight or it has one bend. It ends in a small dot on the target itself, not on a label or a panel. It is never curved. When two leader lines would cross, swap their callouts or move one callout to another margin.

```html
<g data-part="leader">
  <path class="pointer" d="M200 260 H460 L720 470"/>
  <circle fill="var(--c-line)" r="5" cy="470" cx="720"/>
</g>
```

Example: a callout in the left margin points at a well near the centre. The line leaves the callout level, bends once toward the well, and its dot sits on the well.

## Callout

An HTML block placed in inches on the sheet. It has a short heading, then one story line in italic, one rule line in upright text, and one small italic parameter line.

```html
<div class="note" style="top:3.1in; left:0.6in">
  <h3>2 · Signal fire</h3>
  <div class="flavor">Keepers light it when the fog rolls in.</div>
  <div class="mechanic">Ships within range see the dock on their map.</div>
  <div class="tuning">range 40 m · burns 3 turns · 1 log</div>
</div>
```

## Card

A box placed in inches. It has a bold heading with a number and a name, a coloured second line, and rows. Each row has a small icon, a bold name and a small italic note. Every card on the page uses this one layout.

```html
<div class="plate" style="top:1.2in; left:12.9in">
  <h4>4 · Reef Lantern</h4>
  <div class="plate-tag">Rank 3 · tide out</div>
  <div class="plate-line"><span>▲</span><b>Beam</b><i>shows hidden rocks for 2 turns</i></div>
  <div class="plate-line"><span>■</span><b>Oil</b><i>one flask lasts a whole tide</i></div>
</div>
```

## Matrix

An HTML table placed in inches, in fine print. It has a header row and a header column, thin cell borders in the line colour, a little padding, and an x in every empty cell. Fill it from the top.

```html
<table class="fine" style="border-collapse:collapse; top:6.4in; left:9.6in; position:absolute">
  <tr><th></th><th>Hide</th><th>Mail</th><th>Ward</th></tr>
  <tr><th>Spear</th><td>pierces</td><td>x</td><td>x</td></tr>
  <tr><th>Mace</th><td>x</td><td>cracks</td><td>x</td></tr>
  <tr><th>Wand</th><td>x</td><td>x</td><td>breaks</td></tr>
</table>
<!-- each th and td: padding:2pt 4pt; border:0.5pt solid var(--c-line) -->
```

## Trade-off triangle

A white triangle with an outline, cut from its centre into three regions. Each region has its own soft tint. A small label sits along each edge and turns to follow it.

```html
<g data-part="triangle" transform="translate(1300 640)">
  <path fill="var(--c-fill-a)" d="M0 0 L0 -230 L199 115 Z"/>
  <path fill="var(--c-fill-b)" d="M0 0 L199 115 L-199 115 Z"/>
  <path fill="var(--c-fill-c)" d="M0 0 L-199 115 L0 -230 Z"/>
  <path class="edge" fill="none" d="M0 -230 L199 115 L-199 115 Z"/>
  <text transform="translate(-118 -70) rotate(-60)" text-anchor="middle" font-size="13">hidden</text>
  <text transform="translate(118 -70) rotate(60)" text-anchor="middle" font-size="13">tough</text>
  <text transform="translate(0 140)" text-anchor="middle" font-size="13">quick</text>
</g>
```

## Storyboard

A row of frames side by side. All frames have one width, a thin border and a small gap between them. Each frame holds a small SVG drawn with the page's marks and a caption of one sentence in fine print.

```html
<div style="gap:0.12in; display:flex; bottom:0.9in; left:1.2in; position:absolute">
  <figure style="border:0.5pt solid var(--c-line); width:1.8in; margin:0">
    <svg viewBox="0 0 180 110">
      <text font-weight="700" font-size="30" y="70" x="30">O</text>
      <text font-weight="700" font-size="30" y="70" x="130">X</text>
    </svg>
    <figcaption class="fine">Turn 1: the player spots the guard.</figcaption>
  </figure>
  <!-- the next frames, all with the same width -->
</div>
```

## Legend

A white box with a border, placed in inches, usually near a bottom corner. It has one line for each mark: the mark, then what it means.

```html
<div class="key" style="bottom:0.6in; left:0.6in">
  <div><b>O</b> = player</div>
  <div><b>X</b> = guard</div>
  <div><span style="color:var(--c-hot)">●</span> = lantern oil</div>
</div>
```

## Step number

For a procedure, each step gets a bold number of about 10 pt inside a small circle with a border in the text colour. The circle sits in the line of text.

```html
<b style="font-size:10pt; border-radius:999px; border:1pt solid var(--c-text);
          block-size:1.5em; inline-size:1.5em; place-items:center;
          display:inline-grid; vertical-align:middle">5</b>
```
