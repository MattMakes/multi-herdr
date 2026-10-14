# Trailers and store media

Read this when you record trailers with Movie Maker, make share copies, or make store screenshots and store art.

## Trailers and store media

Script every clip, so a gameplay change costs 1 command and not a day of re-editing.

1. Test Movie Maker mode with a short clip first.
   Run `godot --write-movie <dir>/frame.png --fixed-fps 30 --quit-after <frames>` in a real
   window. It writes lossless PNG frames and a WAV file.
   Check the frame size, the frame count and the audio level.
   Movie Maker needs a real renderer. A headless run gives blank frames.
2. Write 1 shot script per clip from a scripted scene or a stress driver, with a fixed seed.
   - Each clip sets its own save state, so no clip depends on your save.
   - Each clip turns off tutorial hints. Some hints hide only after real input events, so send
     them.
   - Each clip turns its music off. The assembler adds 1 music bed over the whole cut.
3. Record each clip at the game view size. Upscale later.
   - Check that each clip took a normal time. A clip under 1 s or with too few game phases
     failed; record it again at a longer length.
   - Log per-frame stats (for example entity counts) and check them against caps.
4. Make contact sheets with time labels to pick cut points:
   `ffmpeg -i clip.mp4 -vf "fps=1/2,scale=320:-1,tile=6x4" -frames:v 1 sheet.png`.
   Add a `drawtext` time label per tile. The `tile` filter replaces ImageMagick `montage`.
5. Write 1 assembly script.
   - Read cut points from data.
   - Add title cards in the game font and palette, fades and the logo.
   - Upscale with nearest-neighbour (`scale=iw*3:ih*3:flags=neighbor`) for pixel art.
   - Give the script an output-folder flag, so a trial run does not overwrite the release.
6. Encode a master and a 2-pass share copy under the size limit of the target site.
   Compute the bitrate from the size limit and the length.
   Check each output with `ffprobe`: size, length, codec, resolution.
7. Check the music loop seam. Compare the sample values on both sides of the seam
   (`references/script-loop-seam.md`; copy the block into a file).
8. Extract stills at set times. Compare pairs of near stills for a seam or a frozen frame.
9. Pick store screenshots from the stills at the store size. Assert the size in the script.
   Choose varied gameplay scenes, not 8 views of 1 fight.
10. Fetch the current store asset list. Make each store asset (capsules, header, library art)
    with a script at the exact size. Remove every mockup tag in the script, not by hand.
    Look at every asset.
11. Wrap the chain in 1 command and a task-runner recipe.
    Run it in the background and watch for its step header lines, not for any word that
    ffmpeg prints.
12. Keep the raw frames until the assembly is final. A re-assembly without them needs a new
    recording. Delete temp frames on exit only.
    A single-trailer run must not delete the outputs of other trailers.
13. Keep large media out of git. Copy the deliverables to a release folder.
14. Update the store guide rows and the README times from the measured run.
    Disclose AI-made images where the store asks for it.
