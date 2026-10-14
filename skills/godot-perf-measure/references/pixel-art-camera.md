# Pixel-art camera and view edge

Use this when a 3D game renders pixel art at a low internal resolution and the camera scroll
judders, or the camera can show past the edge of a finite play field.

1. Write a windowed drive tool that records the scroll per frame in window pixels, in 8 directions.
   Clear camera shake in the tool, because shake hides the follow cadence. Save a "before" frame
   series per direction and window size.
2. Snap the camera to the texel grid. Shift the upscaled image by the remainder in window pixels.
3. Prove even steps from the pixels with a still scene. Check that HUD pixels are identical at 2x,
   that 1x keeps the old path, and that every world pixel stays on the palette.
4. For the view edge, compute the view extent past each edge for every field, aspect ratio and
   camera pitch. Make it a failing headless test first.
5. Pick a margin above the largest extent. Fill it with cheap chunks built from a few shared
   templates.
6. Make every mask read outside the field return the untouched state. A clamp in the read streaks
   the edge state across the margin.
7. Capture all fields on each renderer. Check for seams and visible template repeats.
8. Measure frame cost and memory A/B against a base build under the perf lock.
