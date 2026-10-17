# Pitfalls: the full table

| Symptom | Cause | Fix |
| --- | --- | --- |
| Review rejects the finals as programmer art. | The worker drew finals by script from simple shapes. | Keep the generated sources. Run every crop through the pixelize script. |
| A frame shows the wrong pose or asset. | Generated file names do not keep the request order. | Map each file by its content. Copy each output at once with a fixed name. |
| A small icon has many mid colours. | Automatic reduction to 16 px keeps too much detail. | Hand-simplify to match the existing strip. |
| A prompt for a small decal returns a whole vehicle. | A batch prompt has a weak silhouette rule. | Regenerate only that frame with a stricter silhouette rule. |
| A preview PNG shows up in the game files. | The preview sat next to the asset and got its own `.import` file. | Write previews to a temp or review folder. Delete strays before the final check. |
| Preloads fail on the first import. | The import scanned scripts before the new PNG files imported. | Run the import a second time. Judge the second run. |
| Godot shows the old texture. | The changed PNG did not import. | Run the headless import. |
| Old atlas cells changed. | An alpha composite changed hidden pixels. | Paste the old bytes. Compare with the base copy cell by cell. |
| The palette check finds 1 bright colour everywhere. | The strip canvas used the opaque key colour, not transparency. | Remove the key in the assembly. Keep the palette check in the loop. |
| A code path shows only some cells of an atlas. | The code sliced the atlas by a fixed cell count. | Read the cell count from data or the texture width. Test it. |
| A test fails on the atlas width after an append. | The test pinned the old cell count. | Ask the owner for that 1 edit. Better, read the count from data. |
| The plan names a check tool that does not exist. | The plan assumed a tool from another project. | Write a small check script in the review folder, modelled on an earlier checker. |
| Text has soft, off-palette pixels. | The font imported with antialiasing. | Set antialiasing, hinting and subpixel positioning off. |
| A UI spec and the manifest give different sizes. | 2 documents own the same fact. | Keep the manifest size. Apply the spec shape and contrast inside it. Report the conflict. |
| An art-only unit reports blocked on test failures. | It ran the full suite in a sandbox. | Run palette, size, manifest and import checks only. |
| Art files appear outside the game folder. | The art script resolved the repository or worktree root. | Check the resolved root before any write. |
| A shell loop breaks "name path" pairs wrongly. | zsh does not split an unquoted variable into words. | Use arrays, or `${=var}` in zsh. |
| The manifest check fails on valid data. | The check read the wrong JSON root. | Fix the check, not the data. |
| An older asset has colours outside the palette. | The asset predates the palette rule. | Snap to the nearest palette colour. Or add the colour to the palette with approval. |
| A billboard sprite ignores its node scale. | The shader builds the quad from its own size. | Resize the quad, not the node. |
| One atlas cell looks cropped and stretched. | The cell art crosses the cell window. | Redraw that cell only. Add a pixel-boundary test that fails on the old cell. |
| A cutout keeps a dark glow or box. | The edit pass left near-background shades. | Run 1 extraction pass. Remove the rest of the shades with a script. |
| A silhouette has grey tones. | The model output antialiases edges. | Threshold to 2 colours. Check with a histogram. |
| A thin icon reads as a dark ring or the wrong tool. | Shapes are too fine for the cell. | Use fewer, bolder shapes. Give a blade a constant-width body and clear tips. |
| A mid-tone icon vanishes over the ground. | Its fill is close to the ground and it has no outline. | Use a lighter fill with palette highlights. Keep the shape and cell index. |
| Plan example colours fail the contrast gate. | Nobody measured them against each background. | Measure first. Then ask for a rule with the numbers. |
| Regular bright stripes read as gold lines in play. | A repeating highlight forms a pattern at speed. | Put bright stripes on the lit side only. |
| A dark hole shows between thin stems. | No backing fills the gaps. | Add a dense backing behind the stems. |
| A prop reads as a flat slab or does not show. | The shape is weak or shorter than its neighbours. | Give it a solid body and clear masses. Match the height of the related prop. |
| A sprite stands off its seam or ground line. | The offset has the wrong sign. | Test the other sign in a capture. |
| `python3 -I` cannot import Pillow. | Isolated mode skips the user site packages. | Use `python3 -P` from a separate folder, or a venv in a temp folder. |
| Pillow warns that `getdata` is deprecated. | The Pillow version is new. | Use `get_flattened_data`. |
| A colour shows only as 1 small shade pixel. | The palette snap kept a rare colour. | Accept it if it is in the palette. Record it in the notes. |
| A model face uses an empty atlas cell. | The face-to-cell mapping is wrong. | Fix the mapping. The art is often correct. |
| A tool script fails on class names. | The tool compiled before autoloads registered. | Remove static project-class references. Load them at run time. |
| Tests fail after a hitbox change. | The tests pinned the old numbers. | Update them in the same commit. |
| Large concept images bloat the repository. | Sources went into git. | Keep large concepts in an ignored source folder. Track the prompts and scripts. |
| A shared manifest conflicts on merge. | 2 units appended rows. | Keep both sides. Check for duplicate paths. |
