# Fixed-seed capture, frame extraction and contact sheets

Read this when you need repeatable captures, frames from motion, or a contact sheet.

This skill owns the capture mechanics. Other skills call this file, for example
`godot-release-and-store` before it picks clips for a trailer. That skill owns clip selection,
assembly and store deliverables.

1. **Fix the seed.** Give the capture tool a `--seed` argument and seed every random number
   generator from it before the scene loads. Print the seed in the log. Check: 2 runs with the
   same seed give byte-identical 1x images (`cmp`), after the time inputs are zero (`pixel-diff.md`
   step 1).
2. **Own the state.** Each capture run loads its own scratch or preset save, turns off tutorial
   hints and turns off music. A run must not depend on the save on your machine.
3. **Extract still frames.** For single states, save PNG files from the capture tool (SKILL.md
   method step 5). For motion, use Movie Maker mode in a real window, for example
   `godot --write-movie /abs/out/frame.png --fixed-fps 30 --quit-after 300 -- --seed=7`.
   It writes 1 lossless PNG per frame and a WAV file. Movie Maker needs a real renderer; a
   headless run gives blank frames. To pull frames from a video file, use
   `ffmpeg -i clip.mp4 -vf fps=2 /abs/out/f_%04d.png`.
4. **Check the run.** Count the frames and check the frame size. A run that is much shorter
   than its frame limit, or that shows too few game phases, failed. Run it again.
5. **Build a contact sheet.** Tile labelled frames into 1 sheet, for example
   `ffmpeg -i clip.mp4 -vf "fps=1/2,scale=320:-1,tile=6x4" -frames:v 1 sheet.png`, or
   `montage /abs/out/f_*.png -tile 6x -geometry +2+2 -label '%f' sheet.png` (ImageMagick).
   Add a time or shot label to each tile, with the ffmpeg `drawtext` filter or `-label`.
6. **Review the sheet.** Look at each tile (SKILL.md method step 8). Write 1 line per tile or per row:
   what reads and what does not. Open a single frame at native size for each doubtful tile.
7. **Hand off.** Give the caller the seed, the tool command, the frame folder and the sheet
   path. The caller picks the cut points or shots from the sheet.
