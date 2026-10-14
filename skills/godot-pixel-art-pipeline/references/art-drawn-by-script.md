# Art drawn by script and pixel fonts

## Art drawn by script

Use this when no image model is in the loop, for a tiny cell, a glyph, a mockup or a logo
concept. Script art is acceptable for small, geometric cells. For characters and props,
prefer an image-model source, because review rejects finals drawn from simple shapes.

1. Read the palette, the art direction, the atlas layout and the colours of the old art.
2. Measure the target box in the scene and the on-screen height of peer sprites of the same
   class. A prop that is shorter than its neighbours does not show.
3. Write a Pillow script that draws from palette colours only. Give it a `--preview` option
   and a `--write` flag, so a dry run cannot overwrite the atlas.
4. Keep the script in a review folder that git tracks, so the art can be rebuilt. Do this
   also when the art source folder is ignored by git.
5. Preview at 4x to 8x with nearest-neighbour scale, at the real draw size and on each ground
   colour. Use fewer, bolder shapes until the art reads. Inner rings blur together at 16 px.
6. Run the checks: 0 off-palette pixels, old cells byte-identical, a 2x2 tile without seams.
7. Look for other users of the old file. If another screen reads it, add a new file.
8. Append manifest rows as text, so the other rows keep their format. Parse the manifest.
9. Re-import. Get an in-game capture through `godot-visual-verification`. Run the palette check on
   its 1x image.

If the real art comes from another unit, build the screen with flat placeholders first. A
placeholder script makes 1 flat cell per manifest row at the right size. Merge the real art
later and capture again.

## Pixel fonts

A font import with antialiasing gives soft, off-palette text. In one case 1 line of text held
61 off-palette colours. Set antialiasing, hinting and subpixel positioning off in the font
import settings. Prove the cause with a test state that has antialiasing off. Check every
glyph that the text uses. A tiny bitmap font in a build script can lack glyphs, and the build
crashes on the first missing letter.
