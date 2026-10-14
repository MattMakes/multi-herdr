# Low-poly models and hitboxes

## Low-poly models from a parts table

Use this when an art unit hands low-poly 3D vehicles or props to a code unit as atlas cells
plus a parts table.

1. The art unit writes 1 parts table per model. It states the units (metres) and, per part,
   the name, shape, size, position, rotation and atlas cell. It also lists the moving parts.
2. The model unit treats each row as a contract.
3. Write a failing contract test for the first model. It checks the named mesh, size,
   position, rotation and atlas cell of each part.
4. Write a shared base class that builds meshes from a table. Write the first model script.
   Make the test pass.
5. Add 1 contract test and 1 model per remaining table. Each model script adds only its own
   animation.
6. Agree the public API (node name, attach function, pose or animate call) with the units
   that use the model, before they write code against it.
7. Add animation and the reduced-motion behaviour. Test the public API.
8. Add each new material to the shader warm-up list and run its guard test.
9. Get captures on each renderer that the game ships through `godot-visual-verification`. Compare
   them with the concept sheet.

When a face shows the wrong art, check the face-to-cell mapping first. In one case every face
used an empty cell and the art was correct.

## Hitboxes from the art

Use this when collision sizes must match the drawn sprites or models.

1. Find where the game reads the hitbox value. It is often a data row, not the scene.
2. Measure the drawn body of each model with a script: from the parts table for a model, or
   from the opaque pixel bounds for a sprite.
3. Write the values into the data source. Re-export the game data.
4. Update the tests that assert the old numbers in the same commit. Expect several.
5. Get a capture with the collision shapes visible through `godot-visual-verification`.
