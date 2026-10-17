# Pixel diff with a noise floor

Read this when a change must look the same as the base, or must change 1 area only.

Use this when a change must look the same (a refactor, a new render path) or change 1 area only.

1. Find each shader input that uses time. Set its strength to 0 for the capture. Hold every
   time-driven effect still. Use a fixed fps.
2. Capture the base 2 times at the same frame. The changed-pixel count between the 2 base
   captures is the noise floor.
3. Render the change, or path A and path B, with the same camera, scene and seed.
4. Write A, B and a diff image. Print the changed-pixel count against each base capture. Judge
   only pixels that differ from both base captures.
5. Trust a diff of 0 only after you check that the frames show the subject. 2 empty frames match.
6. Zoom on each changed pixel cluster and explain it. Write a failing test for each real
   difference, then fix it on both paths.
7. For looks that must stay apart (modes, teams, states), measure mean luminance in fixed boxes
   and check a minimum gap. Use a box over the full block, not 1 row. Check palette membership.
8. When pixel counts cannot judge the goal (a blob shadow against a speckled shadow), say so and
   compare by eye with a zoom sheet.
9. Flip the default to the new path in its own commit. Put the counts and the noise floor in the
   notes.

`script-pixdiff.md` holds `pixdiff.py`; copy the block into a file. It counts changed pixels between 2 PNG files, with a tolerance and an optional
box, and can write a diff mask. It uses the Python standard library only.
