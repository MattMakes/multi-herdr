# Drawing parts

Small inline SVG and HTML parts to copy and adapt when you draw the centre picture yourself. They use the classes and colour tokens of `assets/starter.html` (`--ink`, `--paper`, `--rule`, `--accent`, `--wash-1..3`, `.lead`, `.outline`, `.callout`, `.card`, `.legend`, `.story`, `.rule`, `.param`). Change sizes, positions and content freely; keep the rules.

Text over a drawing is HTML positioned in inches, or SVG text. It is never part of an image.

## Drawing layer

One SVG covers the whole sheet, pinned to all four edges, at 100 drawing units per inch. For a sheet of W x H in the view box is `0 0 (100 x W) (100 x H)`:

- 17 x 11 in: `viewBox="0 0 1700 1100"`
- 30 x 20 in: `viewBox="0 0 3000 2000"`

When the sheet size changes, change the view box with it. Leave out the `width` attribute. Leader lines and drawings go in this layer; longer text goes in absolutely positioned HTML on top of it. Screen readers skip the layer.

```html
<svg viewBox="0 0 1700 1100" aria-hidden="true"
     style="position:absolute; inset:0; width:100%; height:100%">
  <!-- drawings, then leader lines -->
</svg>
```

SVG `font-size` is in drawing units: 100 units = 1 in = 72 pt, so 8.4 units is about 6 pt, the floor. Use 9 units or more.

## Isometric tiles and blocks

A tile is a diamond twice as wide as it is tall, pale fill, dark outline. Here the tile is 120 wide and 60 tall, centred on `(cx, cy)`.

```html
<polygon class="outline" fill="var(--wash-2)"
         points="600,400 660,430 600,460 540,430"/>
```

A block is a tile used as the top face plus two side faces of one fixed block height (here 40), each side a slightly darker neutral tone:

```html
<g class="block">
  <polygon class="outline" fill="#cfcac0" points="540,430 600,460 600,500 540,470"/>
  <polygon class="outline" fill="#b9b3a7" points="600,460 660,430 660,470 600,500"/>
  <polygon class="outline" fill="var(--wash-2)" points="600,400 660,430 600,460 540,430"/>
</g>
```

Stacking: each layer moves up by one block height. Draw from the back of the scene to the front, and inside a stack from the lowest block to the highest, so nearer faces cover farther ones. Four blocks stacked on one tile: draw the block at offset 0, then `transform="translate(0,-40)"`, then `-80`, then `-120`.

## Room in plan view

A rectangle with a pale fill and an outline, a small white outlined rectangle on one wall for the door, and a small centred label.

```html
<rect class="outline" fill="var(--wash-1)" x="300" y="300" width="260" height="180"/>
<rect class="outline" fill="#fff" x="410" y="472" width="40" height="16"/>
<text x="430" y="395" text-anchor="middle" font-size="14">Workshop</text>
```

## Land and zones

Land is a closed, smooth curved path with a pale fill. Zones are semi-transparent white circles, each with a small grey centred label.

```html
<path class="outline" fill="var(--wash-2)"
      d="M420,300 C560,180 860,200 980,330 C1100,460 1010,690 820,730
         C620,770 380,700 330,560 C290,450 330,370 420,300 Z"/>
<circle cx="700" cy="460" r="110" fill="#fff" fill-opacity="0.55"/>
<text x="700" y="464" text-anchor="middle" font-size="12" fill="var(--rule)">Marsh</text>
```

## Route with stops

An unfilled line in the text colour through the stops. Each stop is a small white outlined circle with a bold centred number. Start the numbering at 0 or at 1.

```html
<polyline fill="none" stroke="var(--ink)" stroke-width="3"
          points="380,620 520,540 700,560 880,430"/>
<g font-size="14" font-weight="bold" text-anchor="middle">
  <circle class="outline" fill="#fff" cx="380" cy="620" r="14"/><text x="380" y="625">1</text>
  <circle class="outline" fill="#fff" cx="520" cy="540" r="14"/><text x="520" y="545">2</text>
</g>
```

## Time ruler and phase dividers

A horizontal line in the line colour near the bottom, short ticks at equal spacing, small grey numbers under the ticks, and a centred caption under those that names the unit. Phase dividers are dashed vertical lines in a muted cool grey, the full height of the drawing.

```html
<g stroke="var(--rule)" stroke-width="2">
  <line x1="200" y1="1000" x2="1500" y2="1000"/>
  <line x1="200" y1="1000" x2="200" y2="1012"/>
  <line x1="460" y1="1000" x2="460" y2="1012"/>
  <!-- one tick every 260 units -->
</g>
<g font-size="11" fill="var(--rule)" text-anchor="middle">
  <text x="200" y="1030">0</text><text x="460" y="1030">5</text>
</g>
<text x="850" y="1055" text-anchor="middle" font-size="12">minutes</text>
<line x1="720" y1="0" x2="720" y2="1100" stroke="#8fa0b0" stroke-width="2" stroke-dasharray="10 8"/>
```

## Marks

Pick a small set and keep to it; list it in the legend.

```html
<circle cx="500" cy="500" r="9" fill="var(--accent)" stroke="var(--ink)" stroke-width="2"/>  <!-- token -->
<g stroke="var(--ink)" stroke-width="3" fill="none">                                         <!-- figure -->
  <circle cx="600" cy="470" r="9"/><path d="M600,479 V510 M584,492 H616 M600,510 L588,532 M600,510 L612,532"/>
</g>
<text x="700" y="510" font-size="28" font-weight="bold" text-anchor="middle">O</text>          <!-- player -->
<text x="760" y="510" font-size="28" font-weight="bold" text-anchor="middle">X</text>          <!-- enemy -->
<polygon fill="#f2a516" stroke="var(--ink)" stroke-width="1"
  points="850,470 857,488 876,484 864,499 878,512 859,513 856,532 846,516 829,524 836,506 820,496 840,492"/>  <!-- hit -->
<path d="M950,500 L1060,450 A120,120 0 0 1 1060,550 Z" fill="#7fb2e5" fill-opacity="0.4"/>   <!-- cone area -->
```

## Leader line

A thin unfilled line in the line colour from the callout to the target, straight or with one bend only, ending in a small dot on the target. Never curved. Leader lines never cross each other: when two would cross, swap their callouts or move one to another margin.

```html
<polyline class="lead" points="230,310 420,310 610,520"/>
<circle cx="610" cy="520" r="5" fill="var(--rule)"/>
```

Example: a callout in the left margin points at a well near the centre. Run the line level out of the callout, bend it once toward the well, and end it with the dot on the well itself, not on a label next to it.

## Callout

An absolutely positioned HTML block placed in inches: a short heading, a story line in italic, a rule line upright, a small italic parameter line.

```html
<div class="callout" style="left:0.5in; top:2.6in">
  <h3>1 · The well</h3>
  <div class="story">Villagers trade gossip here at dawn.</div>
  <div class="rule">Drinking restores 2 stamina once per day.</div>
  <div class="param">radius 3 m · cooldown 1 day</div>
</div>
```

## Card

An absolutely positioned box: a bold heading (number and name), a coloured second line, then rows that each have a small icon, a bold name and a small italic note. All cards on one page use the same layout.

```html
<div class="card" style="left:13.2in; top:1.4in">
  <h4>3 · Ember Moth</h4>
  <p class="sub">Tier 2 · night only</p>
  <div class="row"><span>◆</span><b>Swarm</b><i>splits into 3 on hit</i></div>
  <div class="row"><span>●</span><b>Glow</b><i>lights a 2 m circle</i></div>
</div>
```

## Matrix

An absolutely positioned HTML table in fine print: a header row, a header column, thin cell borders in the line colour, small padding, and an x in each empty cell. Fill it from the top.

```html
<table style="position:absolute; left:10in; top:6in; border-collapse:collapse; font-size:7pt">
  <tr><th></th><th>Cloth</th><th>Plate</th></tr>
  <tr><th>Blade</th><td>bleeds</td><td>x</td></tr>
  <tr><th>Hammer</th><td>x</td><td>dents</td></tr>
</table>
<!-- give th and td: border:0.5pt solid var(--rule); padding:2pt 4pt -->
```

## Trade-off triangle

A white outlined triangle divided from its centre into three regions, each a different soft tint, with small axis labels along each edge, rotated to follow it.

```html
<g transform="translate(1200,600)">
  <path d="M0,-200 L173,100 L0,0 Z" fill="var(--wash-1)"/>
  <path d="M173,100 L-173,100 L0,0 Z" fill="var(--wash-2)"/>
  <path d="M-173,100 L0,-200 L0,0 Z" fill="var(--wash-3)"/>
  <polygon class="outline" fill="none" points="0,-200 173,100 -173,100"/>
  <text font-size="12" text-anchor="middle" transform="translate(100,-60) rotate(60)">fast</text>
  <text font-size="12" text-anchor="middle" transform="translate(0,122)">armoured</text>
  <text font-size="12" text-anchor="middle" transform="translate(-100,-60) rotate(-60)">long range</text>
</g>
```

## Storyboard

A horizontal row of equal-width frames with thin borders and small gaps. Each frame holds a small SVG drawn with the page's marks and a one-sentence caption in fine print.

```html
<div style="position:absolute; left:1in; bottom:0.8in; display:flex; gap:0.15in">
  <figure style="margin:0; width:1.6in; border:0.5pt solid var(--rule)">
    <svg viewBox="0 0 160 100"><text x="40" y="60" font-size="28" font-weight="bold">O</text></svg>
    <figcaption class="t3">0.0 s: the player raises the shield.</figcaption>
  </figure>
  <!-- more frames of the same width -->
</div>
```

## Legend

An absolutely positioned white box with a border, usually near a bottom corner. One line per mark: the mark, then its meaning.

```html
<div class="legend" style="left:0.5in; bottom:0.5in">
  <div><b>O</b> player</div>
  <div><b>X</b> enemy</div>
  <div><span style="color:var(--accent)">●</span> key</div>
</div>
```

## Step number

For a procedure: a bold number of about 10 pt inside a small inline circle with a border in the text colour.

```html
<span style="display:inline-block; width:16pt; height:16pt; border:1pt solid var(--ink);
             border-radius:50%; text-align:center; line-height:16pt; font-size:10pt;
             font-weight:bold">3</span>
```
