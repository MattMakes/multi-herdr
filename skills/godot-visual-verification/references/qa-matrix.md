# Visual QA matrix

Read this when an effect must read in every area, shape, level and setting, or a plan assumes a visual cause.

Use this when an effect must read in every area, shape, level and setting, or a plan assumes a
visual cause, and nobody can check every combination by eye.

1. Build a matrix of cells: 1 per area, pattern, renderer and setting. Write a route library that
   drives each pattern, and a cell tool that plays 1 cell.
2. Find pass, headless: play each cell and record the tick of each event in 1 JSON file per cell.
3. Shoot pass, windowed: replay to each tick and save frames on each renderer. Add a burst pass
   that saves every frame for 1 s after the event, for flicker checks.
4. Write an analyzer with 1 check per defect. Test each check on synthetic frames first.
5. Run the matrix. Look at a sample of failed cells by eye. Recalibrate each check that fails
   cells that look correct.
6. Make the run resumable and keep finished cells. Pin the game clock, so a run cannot end at
   dusk before the effect completes.
7. Hand the matrix to a separate QA worker on a named commit. The QA worker reports pass or fail
   per cell with evidence. The builder groups failed cells by cause, fixes each group with a test,
   and asks for a rerun of the failed cells, then the full matrix.
8. Before you build a fix for an assumed visual cause, capture every area at rest and in a short
   driven-play mode that measures the real play numbers. The data can reject the assumed cause.
9. Commit contact sheets, not raw shots.
