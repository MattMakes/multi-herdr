# Readability and contrast gate

Use this when a sprite, a bullet, an icon or a glyph must read at 1x over every background.

1. List every background colour where the sprite shows. Include the ground bands (lit, mid,
   shadow), each area tint and each UI panel.
2. Compute the WCAG contrast ratio of each fill and of the outline against each background,
   before you edit the art. `script-contrast_table.md` prints a PASS or FAIL table. Copy its block into a file.
3. Count the fails. If no palette colour passes, send a question with the numbers per
   background. Do not edit first, because the fix is a rule decision, not an art decision.
4. Apply the approved rule. A rule that works: per background, the higher of fill contrast
   and outline contrast is at least 3:1, with a 1-pixel dark outline on all 4 sides.
5. Keep distinct shapes when 2 kinds share a fill colour.
6. Redraw the failing cells, or snap the off-palette pixels to the nearest palette colour.
   Diff the old and new sheets. Confirm that only the target cells changed.
7. Run the table again. Expect 0 fails.
8. Build previews of the key frames (idle, attack, exposed pose) over each background at 1x
   and 4x. Confirm that each key pose differs at 1x. For the in-game check at 1x and 2x, use
   `godot-visual-verification`. It owns the window captures and the before and after comparison.
9. Record both contrast numbers per background in the notes.

A palette rule can also keep enemy colours out of the colour range of the ground and the props.
Without it, an enemy hides in the scenery.
