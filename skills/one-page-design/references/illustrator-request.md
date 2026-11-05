# Illustrator request

Fill in this form when someone else will make the centre picture. That can be a person who draws, a model that makes images, or another agent that has image tools. Fill in every field; none is optional. Send the whole form, including the fixed parts, unchanged.

## Form

```text
PICTURE REQUEST FOR A ONE-PAGE DESIGN

Page title: <the point sentence of the page, so the picture supports it>

Subject: <one sentence naming the one thing to show, including the point of view>

View type: <choose one: top-down (camera looks straight down) | high oblique (camera above, looking down at a slant) | eye level (camera level with the subject) | equal-axis (three axes drawn at equal angles) | single frame (one still from a series of frames)>

Must be visible:
- <part 1>
- <part 2>
- <...>
(Labels and callouts will point at each part, so draw each one separate and
easy to locate.)

Must be absent:
- <anything that would mislead: extra figures, interface elements, effects
  the design does not have>

Output file: <absolute path of the PNG to write>

Style (fixed): a flat illustration with pale fills and dark outlines. A
transparent background, so the paper colour shows through. One accent colour
only: <name the colour>. No gradients, no photorealism, no camera effects
(no depth of field, lens flare or motion blur).

Requirements (fixed):
- The picture contains NO TEXT of any kind: no words, letters, numbers,
  captions, signs or logos. The page adds every label itself.
- The largest size available, at least 2048 pixels on the longer side.
- PNG with a transparent background.
```

## Filled example

```text
PICTURE REQUEST FOR A ONE-PAGE DESIGN

Page title: Every gear in the workshop is a puzzle the player can reach from the bench

Subject: A clockmaker's workshop seen from a raised three-quarter angle, looking
across the workbench toward the back wall of gears.

View type: high oblique

Must be visible:
- the gears on the back wall, each one separate
- the workbench in the foreground
- the pendulum hanging to the right of the bench

Must be absent:
- people or other figures
- interface elements, health bars or cursors
- sparks, glows or other effects

Output file: /Users/me/pages/clock-workshop/centre.png

Style (fixed): a flat illustration with pale fills and dark outlines. A
transparent background, so the paper colour shows through. One accent colour
only: orange. No gradients, no photorealism, no camera effects
(no depth of field, lens flare or motion blur).

Requirements (fixed):
- The picture contains NO TEXT of any kind: no words, letters, numbers,
  captions, signs or logos. The page adds every label itself.
- The largest size available, at least 2048 pixels on the longer side.
- PNG with a transparent background.
```

## When the picture comes back

1. Look at it. If it has any text at all (a sign on a door that says OPEN, a number on a dial, a scribble that reads as letters), refuse it and send the same form again. Never edit or paint out the text yourself.
2. Check that every must-be-visible part is there and easy to locate, and nothing on the must-be-absent list is. If not, send the form again with that part made clearer.
3. Once it passes, turn the PNG into base64 text and write that text into the `src` of the page's `<img>` as a `data:image/png;base64,...` URI. With the picture inside the HTML, the page needs no separate image file.
4. Put the filled form into the page as an HTML comment directly beside the `<img>`, so anyone can make the picture again in one step.

```html
<!-- PICTURE REQUEST FOR A ONE-PAGE DESIGN
     Page title: ...
     (the whole filled form) -->
<img alt="Clockmaker's workshop" src="data:image/png;base64,iVBORw0KG..."
     style="position:absolute; left:3in; top:1.5in; width:11in">
```

Labels, numbers and leader lines then go in an SVG layer or in HTML on top of the image, never in it.
