# Shader warm-up

Use this when the game stalls the first time a material draws, or the first frames of a run hitch.
The warm-up must stay complete as the game grows, so it needs a guard test.

1. Scan the code for every `.gdshader`, every script that builds a material in code, and every
   MultiMesh or instanced draw (each is a separate shader variant). List the materials of each scene:
   meshes, ground, lights, masks, post-process, transitions.
2. Write the warm-up as one list of material sources in named groups, one group per shader variant.
   Draw one heavy group per frame, at load only, never at a key moment.
3. Hold a reference to each warmed material. The engine frees an unheld shader and the compile
   comes back.
4. Write a guard test. It fails when a file that builds a material or a MultiMesh is not drawn by a
   group and not listed as skipped.
5. Prove the guard. Plant a new shader file and check that the test fails. Run it against the old
   warm-up list and check that it fails.
6. Pause title-screen animations until the warm-up ends.
7. Measure the boot frame and the run start in A/B pairs on each driver. Count frames over 60 ms
   and the worst frame. Use a control run that skips the warm-up, because the engine prints no line
   per compile.
8. When you add a material, add one row to its group. Remove rows for deleted placeholders.
9. Measure the first render frame on its own. Part of its cost is not shader compile.

Newer engine versions (for example Godot 4.4 and later) reduce compile stalls with ubershaders.
Measure before you build a warm-up on those versions.
