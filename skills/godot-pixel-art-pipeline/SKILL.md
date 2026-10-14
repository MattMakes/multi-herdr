---
name: godot-pixel-art-pipeline
description: >-
  Make palette-locked pixel art for a Godot game, from image-model source art or from a
  drawing script. Each asset gets an exact cell size and a fixed palette, and a check script
  proves size, palette, frame grid, outline and unchanged old atlas cells. The skill also
  covers the contrast gate against every real background, UI mockups drawn by script,
  low-poly models built from a parts table, and hitboxes measured from the art. Use it
  whenever someone wants new sprites, animation strips, tiles, icons, atlas cells, glyphs, a
  logo cutout, a silhouette or a UI mockup in a fixed palette. Also use it when someone asks
  why a sprite does not read, why the art has off-palette or soft pixels, why old atlas cells
  changed, why Godot shows a stale texture, or how to hand 3D props from an art unit to a
  code unit. Use it even if nobody says "pixel art pipeline".
---

# Pixel Art Pipeline

Target engine: **Godot 4.7**. The Python blocks use the standard library only.

Agents that make pixel art fail in 2 ways. Raw image-model output has too many colours, soft
edges and the wrong size. Art drawn in code from rings and blocks reads as programmer art and
fails review. This method keeps the image model for the shapes and gives the pixel rules to a
deterministic script: the script crops, resizes, keys out the background, snaps every pixel
to the palette and adds the outline. A check script then proves the asset contract, and a
human-scale look at native size proves that the art reads. The split works because each part
does the job it is good at, and a script result is the same on every run.

## When to use

- New sprites, strips, tiles, icons, glyphs or atlas cells must match a palette and a cell size.
- An existing atlas gets new cells, and the old cells must stay byte-identical.
- A logo or a sprite needs a clean transparent cutout or a 2-colour silhouette.
- A sprite, bullet or icon must read over every ground colour and UI panel.
- A UI design needs mockup images before any game code exists.
- An art unit hands low-poly models to a code unit, or hitboxes must match the drawn art.

## When not to use

- The game uses high-resolution or painted art with no palette rule.
- A human artist delivers final files. Use only the check script and the import steps.
- You must judge the art in the running game window. Hand that capture to
  `godot-visual-verification`.

## Method

The steps assume an image model for the source art. With no image model, draw the source in
a script (see `references/art-drawn-by-script.md`) and start at step 4.

1. **Read the art contract.** The contract is the manifest row (path, cell size, frame count,
   frame order, anchor, owner), the palette file, the outline rule and the art direction.
   Treat it as complete. Check: you can state the exact output size and frame count. If a UI
   spec and the manifest give different sizes, keep the manifest size and report the conflict.
2. **Write 1 prompt per asset or per frame.** Use a shared prefix: style, camera angle, flat
   key-colour background (for example magenta), silhouette rule. Generate 1 source per frame,
   not 1 sheet for all frames. Keep the same camera, ground line and silhouette in 1
   animation. Generate in small batches. A model does not keep cells aligned in a sheet.
3. **Map and copy each output at once.** Map each generated file to its request by its
   content, because the image tool does not keep the request order. Copy each file into a
   source folder in the project with a fixed name. The image tool writes to its own folder,
   and later files hide which output is which. Check: list the folder before the next step.
4. **Pixelize each source with a script.** The script crops, resizes to the exact cell size,
   keys out the background, snaps each pixel to the nearest palette colour and adds a
   1-pixel outline. Try 2 key thresholds on a hard source and compare them. Check: the output
   has the cell size and hard alpha.
5. **Look at the result.** View it at native size and at 4x to 8x with nearest-neighbour
   scale, next to existing art of the same kind. Hand-simplify cells with too many mid colours
   or a weak silhouette. Add 1-pixel details after the pixelize step, not before. Redraw any
   icon that does not read at native size. Automatic reduction to 16 px keeps too much
   detail, so small icons almost always need a hand pass.
6. **Assemble the strip or the atlas.** Join frames into a horizontal strip, or append cells
   to the atlas. Keep old cells at the same index. Paste the old bytes of each preserved cell.
   Do not alpha-composite over them, because a composite changes hidden pixels.
7. **Run the check script.** It tests exact size, every pixel with alpha above 0 in the
   palette, hard alpha, the frame grid and count against the manifest, a closed outline, and
   old cells byte-identical to the base copy. Get the base copy with
   `git show <base>:<atlas path>`. Tile a ground texture 3x3 and look for seams. Check:
   0 failed checks. `references/script-check_art.md` does all of this except the seam look.
   Copy its block into a file.
8. **Update the manifest row.** Write the frame count, the size and the description. Validate
   the JSON. If a test asserts the atlas width or the cell count, change it with the owner's
   approval. Better, make the code and the test read the cell count from the manifest or the
   texture width, so the next append does not break them.
9. **Record the prompts and build a contact sheet.** The prompt record file holds each prompt,
   the source file name and any divergence. Build the contact sheet at 1x and 4x in a temp or
   review folder, never in the asset folder.
10. **Import twice.** Run the headless Godot import. Judge the second run: the first run can
    scan scripts before the new PNG files import, so preloads fail once. Delete stray preview
    PNG files and `.import` files that are not deliverables. A changed PNG needs an import, or
    Godot shows the stale texture.
11. **See it in the game.** Hand the in-game capture to `godot-visual-verification`. It stages
    the state, captures at native size and compares before and after. Add a test that each new
    kind draws its own cell and that 2 old kinds keep theirs.

The checks of an art-only unit are palette, size, manifest and import. Leave the full test
suite to the unit that lands the work. An art unit that runs the full suite in a sandbox can
stop on failures that have nothing to do with the art.

## Cutouts and silhouettes from existing art

For a transparent logo or sprite, or a flat 2-colour teaser silhouette, read
`references/cutouts-and-silhouettes.md`.

## Art drawn by script and pixel fonts

For a tiny cell, a glyph, a mockup or a logo concept with no image model, or for soft,
off-palette text and missing glyphs, read `references/art-drawn-by-script.md`.

## Readability and contrast gate

When a sprite, bullet, icon or glyph must read at 1x over every background, read
`references/readability-contrast-gate.md`.

## UI mockups by script

When a feature design or a UI spec needs mockups before any game code exists, read
`references/ui-mockups-by-script.md`.

## Low-poly models from a parts table

When an art unit hands low-poly 3D vehicles or props to a code unit, read
`references/low-poly-models-and-hitboxes.md`.

## Hitboxes from the art

When collision sizes must match the drawn sprites or models, read the "Hitboxes from the art"
section of `references/low-poly-models-and-hitboxes.md`.

## Pitfalls

The 8 main rows. The full table of 38 rows is in `references/pitfalls.md`.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Review rejects the finals as programmer art. | The worker drew finals by script from simple shapes. | Keep the generated sources. Run every crop through the pixelize script. |
| A frame shows the wrong pose or asset. | Generated file names do not keep the request order. | Map each file by its content. Copy each output at once with a fixed name. |
| A small icon has many mid colours. | Automatic reduction to 16 px keeps too much detail. | Hand-simplify to match the existing strip. |
| Preloads fail on the first import. | The import scanned scripts before the new PNG files imported. | Run the import a second time. Judge the second run. |
| Godot shows the old texture. | The changed PNG did not import. | Run the headless import. |
| Old atlas cells changed. | An alpha composite changed hidden pixels. | Paste the old bytes. Compare with the base copy cell by cell. |
| A test fails on the atlas width after an append. | The test pinned the old cell count. | Ask the owner for that 1 edit. Better, read the count from data. |
| Text has soft, off-palette pixels. | The font imported with antialiasing. | Set antialiasing, hinting and subpixel positioning off. |

## Artifacts to build

- **Palette file**: 1 hex colour per line. It is the single source for every check and script.
- **Art manifest**: JSON with 1 row per file: path, cell size, frame count, frame order,
  anchor, owner, description. Code and tests read cell counts from it.
- **Pixelize script**: source image in, exact-size palette-locked cell out. Options for the
  size, the key colour, the key threshold, the outline colour, the anchor and a preview.
- **Prompt record**: 1 Markdown file per asset group, with the shared prefix, each prompt,
  the source file name and the divergences.
- **Check script**: size, palette, hard alpha, grid, outline and preserved cells, with PASS or
  FAIL lines and a non-zero exit. `references/script-check_art.md` holds a standard-library
  version.
- **Contact sheet script**: new art at 1x and 4x next to existing art of the same kind, with
  3x3 tiles for ground textures. It writes to a temp or review folder.
- **Contrast table**: fills and outline against each background, with a PASS or FAIL per row.
  `references/script-contrast_table.md` holds a standard-library version.
- **Placeholder script**: 1 flat cell per manifest row, so code can land before the art.
- **Cutout script**: removes near-background shades, rebuilds the outline, makes previews.
- **Parts table**: per model, the units, and per part the shape, size, position, rotation and
  atlas cell, plus the moving parts. A contract test reads it.
- **Hitbox measure script**: reads the parts table or the opaque bounds and prints the sizes.

## References

The 2 script references hold standard-library Python. Copy the block of each into a file.
Run the file with `python3 -I`.

- `references/cutouts-and-silhouettes.md`: read when a logo or sprite needs a cutout or a
  2-colour silhouette.
- `references/art-drawn-by-script.md`: read when you draw art in a Pillow script, or when a
  pixel font gives soft text or lacks glyphs.
- `references/readability-contrast-gate.md`: read when a sprite must read over every
  background colour at 1x.
- `references/ui-mockups-by-script.md`: read when a UI spec needs mockups before game code.
- `references/low-poly-models-and-hitboxes.md`: read when you build low-poly models from a
  parts table, or measure hitboxes from the art.
- `references/pitfalls.md`: read when a symptom is not in the short Pitfalls table.
- `references/script-check_art.md`: read when you check size, palette, grid, outline and
  preserved cells of an asset.
- `references/script-contrast_table.md`: read when you compute the contrast table.

## Related skills

- `godot-visual-verification`: owns the rendered in-game review: window captures at native
  1x size, before and after pairs and pixel measures. This skill owns the source assets and
  their checks (size, palette, grid, outline, preserved cells, contrast against background
  colours).
- `godot-headless-runs`: the import flags, the `.import` hygiene and tool scripts that must
  not name project classes.
- `godot-test-loop`: the tests that each kind draws its own cell and the cell-boundary tests.
- `godot-perf-measure`: the shader warm-up list for new materials, batching and the pixel-art
  camera.
- `godot-balance-model-data`: when hitbox values live in a data model, re-export through it.
- `godot-fleet-game-dev`: the contract-first split when an art unit and a code unit work in
  parallel.
