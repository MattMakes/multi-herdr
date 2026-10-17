# Cutouts and silhouettes from existing art

Use this when a logo or a sprite needs a transparent background or a flat teaser silhouette.

1. Get the original source file. If it is missing, report that you are blocked. Do not guess
   the design from a screenshot.
2. Run the image model in edit mode with the original as reference. Ask it to remove only
   the background and keep the design.
3. Check the alpha channel. If a glow or a box stays, run 1 more targeted extraction pass.
4. Remove the leftover near-background shades with a script. The model output keeps them.
   Rebuild a uniform 1-pixel ink outline. Keep a transparent margin.
5. For a silhouette, threshold to exactly 2 colours. Count the colours with a histogram.
   Expect 2.
6. Build previews over a checkerboard and over the real game background.
