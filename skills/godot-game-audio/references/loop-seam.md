# Measure a loop seam

`godot-game-audio` owns loop-seam measurement. Other skills (for example `godot-release-and-store`
for a trailer music bed) call this file instead of a second procedure. A seam is the join where
the last sample of a loop plays next to the first sample.

1. **Measure the file that ships.** If the game or the video plays an encoded file (`.ogg`,
   `.mp3`, a QOA import), decode it to integer PCM first, at its own rate:
   `ffmpeg -v error -i loop_wind.ogg -c:a pcm_s16le /tmp/seam/loop_wind.wav`.
   Measure the source WAV too, so you can tell a recipe fault from a codec fault.
2. **Run the report.** Copy the block in `script-wav_report.md` into a file `wav_report.py`.
   Run `python3 -I wav_report.py /tmp/seam/*.wav`. Read 2 fields:
   - `loop_seam_ratio`: the step from the last sample to the first sample, divided by the
     99th-percentile step inside the file;
   - `edge_step_ratio`: the largest step across the last N and the first N samples joined
     (N is 64 by default; set it with `--edge`), divided by the same 99th-percentile step.
3. **Apply the rules.**
   - `loop_seam_ratio` at most 1.0. The seam step is then no larger than a normal step inside
     the file, so the ear does not hear a click.
   - `edge_step_ratio` under a limit that you set and write in the audio contract (for example
     1.5, a sample value). This catches a codec ramp or a short glitch near the edges.
4. **Fix and measure again.** For a scripted loop, render it as a circular buffer so the end
   flows into the start. For an edited track, move the loop start or cut to a zero crossing.
   For an encoded file that fails while its source passes, encode again (for example at a
   higher quality) and decode again. Never mark a seam as passed from the source WAV only.
5. **Record the result.** Write 1 row per loop: file, decoded or source, both ratios, PASS or
   FAIL. Check the rows with a script, not from memory.

A video music bed that loops (a trailer or a store clip) uses the same steps. Measure the
seam on the audio that the final encode holds. Decode the audio track of the video to PCM.
Cut exactly 1 loop period that starts at the loop start, and run the report on that cut.
