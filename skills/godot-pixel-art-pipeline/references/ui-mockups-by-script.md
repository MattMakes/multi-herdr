# UI mockups by script

Use this when a feature design or a UI spec needs mockups before any game code exists.

1. Read the art direction, the palette and real screenshots of the current build.
2. Compute the contrast ratio of each text and focus colour pair.
3. Test the pixel font in Pillow at its cell sizes.
4. Write a mockup script that draws each screen at native resolution. Reuse the palette and
   the drawing helpers of an existing mockup script.
5. Generate the mockups and look at each one.
6. Review the design with a UI rubric. Add the fixes to the spec. A review can also find
   design faults, such as a reward that repeats for free.
7. Snap every dim and every blend to the nearest palette colour. A 60 % dim makes new colours.
   Check each PNG for off-palette pixels.

If an accent focus ring has low contrast on its panel (for example 1.6:1), let a dark ink
ring carry the 3:1 contrast and keep the accent inside it.
