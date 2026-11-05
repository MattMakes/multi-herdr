---
name: one-page-design
description: Use when someone asks for a one-page design, or wants a design (game, level, system, feature, building, play) turned into a single sheet - a one-sheet summary, a poster or table-mat version, something to pin on the wall or hand out at a meeting - or says a design document is too long or nobody reads it. Produces one self-contained, printable HTML page in the Stone Librande style - a picture of the designed thing in the centre, short linked text around it. Not for reports, slide decks or dashboards.
---

# One-page design

Turn a design into one printable sheet that a team reads, pins up and marks up. The method comes from Stone Librande: draw the thing itself in the middle of a single surface, then put short text around it, tied to the picture. A page shows the actual level, island, building or team on the field, not a box diagram about it. Words come only after the picture exists.

The deliverable is exactly one self-contained HTML file. Its sheet size fits the design, not a standard paper size. Every word is visible on the one surface.

## Scope

Use it for designs that people will read: game systems, levels, missions, worlds, mechanics, product features, spaces, procedures.

Do not use it for status reports, API reference documents, slide decks or dashboards. Those have their own formats.

## Supporting files

| Need | File |
|---|---|
| Which parts a page can have, and which picture form fits which design | `references/page-parts.md` |
| Copyable SVG and HTML for the drawing layer, tiles, rooms, routes, rulers, marks, leader lines, callouts, cards, matrix, triangle, storyboard, legend, step numbers | `references/drawing-parts.md` |
| Asking an image source for the centre picture | `references/illustrator-request.md` |
| Starting point for every page, with the print setup | `assets/starter.html` |
| The checklist a finished page must pass | `references/page-contract.md` |
| Which page approach suits which kind of design | `references/principles.md` |
| The static checker and preview | `python3 scripts/check_page.py page.html --preview page.png` |

Read `references/principles.md` before you design a page of a given kind.

## Workflow

### 1. Write the point

Write one sentence that says what the page must get across. That sentence is the page title.

If you cannot write it, for example the request is a loose list of unrelated ideas with no common claim, do not make a page. Tell the person there is not yet a page to make, say why, and ask them for the one point.

### 2. Choose the picture form

Ask: what would a photo of the finished design show? Plan to draw that.

Pick from the forms in `references/page-parts.md`: a space, an object, a journey, a procedure, a group of similar items, two attributes crossed, items that affect each other, action frame by frame, or screen flow. Most pages mix two forms. A flowchart is allowed only for screen or menu flow.

If no form fits on one surface, tell the person the design is probably too complex for a single page, and suggest how to split it.

### 3. Make the centre picture, before any prose

There are two ways.

- **Commissioned.** Fill in the form in `references/illustrator-request.md` and give it to whatever can make images: an image model, another agent with drawing tools, or a human artist. Embed the returned PNG as a `data:` URI so the page stays one file. Store the filled form next to the image as an HTML comment. If the returned image has any text in it, refuse it and request it again. Never edit the text out. This way suits scenes and objects.
- **Drawn.** Write inline SVG with `references/drawing-parts.md`. Use this way when no image source is available, or when the picture must be precise and editable: placed maps, matrices, card grids.

On both ways, every label, number, leader line, card and legend sits in SVG or HTML layered over the picture. None of it is drawn into an image.

### 4. Add the text, in three levels

- **Level 1**, legible from across a room: the title and the big labels on the picture (28 pt in the starter).
- **Level 2**, legible at reading distance: callouts linked to the picture by leader lines or matching numbers, and cards that all share one layout (11 pt or less).
- **Level 3**, the smallest: short italic parameter lists, and formulas in monospace inside a box (7 pt in the starter).

Inside a callout or card, story text is italic, rule text is upright, and parameters are one small italic line. In the drawing layer, SVG font sizes are in drawing units: at 100 units per inch the floor is about 8.4 units (6 pt). Anything that would need a fourth level is cut.

Draw time and difficulty: rulers, phase dividers, shaded zones, levels stacked down the page. Do not describe them in sentences.

### 5. Size, check and finish

1. Copy `assets/starter.html`. Set `--sheet-w` and `--sheet-h` in inches on `:root`, once. Nothing else sets the size.
2. Start at the smallest size where the centre picture is legible. Grow the sheet only until all text levels fit. If a side would pass 44 in (a typical plotter roll), trim content or split the design into more pages. Never make fine print smaller than 6 pt to gain space.
3. Put a date, with an optional status word, in the bottom-right corner.
4. Run `python3 scripts/check_page.py page.html --preview page.png`. Fix every finding. Exit 0 means no findings, 1 means findings, 2 means the file or the arguments are bad.
5. Look at the preview image. The checker reads text only and cannot see clipping or overlap, and the sheet clips at its edges when printed. Confirm nothing is cut off at an edge and nothing overlaps.
6. Check the page against `references/page-contract.md`.
7. Deliver the HTML file.

Each page gets its own layout, shaped by its design. Do not reuse one layout across pages. No content goes behind links, tabs, collapsed sections or hover.

## Printing

Tell the person: print to PDF at the declared size (the page sets it, with no margin), then send that PDF to a wide-format plotter. Without a plotter, open the PDF in a viewer that has a poster or tiling option and print it across several normal sheets.

## Common errors

| Error | Fix |
|---|---|
| No date on the page | Copies cannot be told apart. Add the date, bottom right. |
| Type made smaller to fit | Never go below 6 pt. Cut content or enlarge the sheet. |
| A standard paper size chosen up front | Size the sheet to the design. |
| Words inside a generated image | Request the image again without text. Put labels over it. |
| Text styles defined but not used | Use the story, rule and parameter styles in every callout and card. |
| Margins hold more than the picture | Keep the picture share rule: the picture region covers at least half the sheet. |
| Boxes and arrows for everything | Flowcharts only for screen flow. Draw the loop inside the world. |
| A document of headings and bullet lists | Go back to step 2 and choose a picture form. |
| A sequence written as ordered sections | Draw the place and mark the order as numbered circles on it. |
| The point repeated in a prose box, sizes in pixels, no date | The title states the point and the picture shows it. The date and the size in inches are required. |
