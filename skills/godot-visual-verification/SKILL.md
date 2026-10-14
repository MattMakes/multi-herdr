---
name: godot-visual-verification
description: >-
  Prove that a Godot game looks right with real rendered frames, not headless test logs. The
  skill stages exact game states in a capture tool, takes before and after images in a real
  window and at native 1x size, reads every image, measures pixels against a noise floor, runs an
  automatic windowed visual gate on every renderer, and turns each visual fault into a headless
  layout test. Use it whenever someone changes a shader, material, sprite, palette, HUD, menu,
  tween, particle effect or camera, wants screenshots or a contact sheet, asks "does this look
  right", "capture the screen", "check the UI for overlap", "why is this window shot blank",
  "compare before and after", "add a visual regression gate", "check reduced motion" or "does
  this text fit", or works in a sandbox that cannot open a window. Use it even if nobody says
  "visual verification": headless Godot does not draw, so a passing test suite says nothing
  about the look.
---

# Visual verification

Target engine: **Godot 4.7**. The GDScript block parses on 4.7.2. The Python scripts use the
standard library only.

Headless Godot uses a dummy renderer. It draws nothing, a viewport texture is null, and a test
suite can pass while the screen shows a broken look. The method is to stage each exact state in
a small capture tool, take real frames in a window under a lock, and judge every image by eye and
by number against a base capture. It works because the evidence is the real pixels that a player
sees, and because a base-against-base control separates a real change from render noise.

## When to use

- A change touches a shader, material, renderer setting, palette, sprite, VFX, HUD or menu.
- A change has motion (a tween, flash, shake, particle) and the game has a reduced-motion setting.
- A refactor or a new render path must look the same as the old one.
- A reviewer reports overlap, clipping, off-screen focus or text that does not fit.
- The owner must pick between 2 or 3 looks before final assets.
- A worker runs in a sandbox or headless and still needs real frames.

## When not to use

- The change is pure logic with no visible output. Use `godot-test-loop` instead.
- You need frame cost, not the look. Use `godot-perf-measure`.
- You need art from scratch. Use `godot-pixel-art-pipeline`, then come back here to check it in game.

## Method

The anchor procedure: one reviewed set of before and after captures for one change.

1. **Find or copy a capture tool.** Copy the closest earlier capture tool into the review folder
   of this change. Do not name it like a test file, so the test runner does not pick it up. As an
   alternative, add optional arguments to a demo scene (area, level, wallet, shot name). Check:
   with no arguments the demo behaves exactly as before, so the visual gate does not move.
2. **Stage the exact state.** Drive the real scene with the real input actions. Load a scratch
   save or a preset save, never the player's real save. Place the player or the camera beside the
   target. Drain queued toasts and popups. Focus the node under review. Set the time of day.
   Mirror every setup step that the real game scene does (for example a placeholder that the
   real scene hides). Check: the tool prints the state it built, and a first shot shows it.
3. **Fix the clock.** Run at a fixed fps, so game time is exact per frame. Turn vsync and the
   frame cap off, so the run is short. Stop the game director or wave clock during a shot. Hold
   each step for a few frames before a window shot, so the window shows the same state as the
   viewport. Give the tool its own quit timer, because some platforms have no `timeout` command.
4. **Take the window.** Take a window lock or send the job through the capture queue (see
   `references/capture-queue.md`). After start, set the window size explicitly, set it always
   on top, and set it to take no focus. Wait about 60 frames before the first shot. Force a draw
   when the window reports it cannot draw. Check: 2 shots a few frames apart differ when the scene moves, so the window is
   not frozen.
5. **Save 2 images per shot.** Save a true 1x image at the content size from your own viewport.
   When the root viewport follows the window size, downscale with nearest-neighbour filter. Save a
   window image at each target size. On macOS, grab your own window by its window id, crop the
   title bar and allow for the Retina scale. Use absolute output paths, because the working
   directory of Godot is the project folder. Check: the pixel size of each file is what you asked.
6. **Take "before" first.** Capture the base on a detached worktree, or set the changed files
   aside and restore them from the base commit. Use the same tool, shot names, frames and camera
   spots for "before" and "after". Check: a before and after pair differs only where you expect.
7. **Start with 1 shot.** Run 1 shot, look at it, fix the tool, then run the full set. Capture
   each renderer the game ships. Add a reduced-motion set when the change has motion. Capture
   each target window size. Check: the log shows the renderer you asked for.
8. **Look at every image.** Open each image at native size. Write 1 line per image: what reads
   and what does not. Zoom 4x crops for small details. Build a labelled contact sheet when there
   are many shots. Judge pixel art at 1x, because a scaled view blends dither into grey.
9. **Measure where you can.** Count changed pixels, mean luminance in a box, a hue share, a
   palette colour count or a width in pixels. Take a base-against-base control first to find the
   noise floor (see `references/pixel-diff.md`).
10. **Fix and capture again.** Fix what does not read: low alpha, a misleading shape, a colour
    that matches the ground or reads as a hazard. Capture again with the same names. Turn each
    visual fault into a headless test where you can (see `references/ui-review.md`).
11. **Prune and record.** Delete stale, duplicate and frozen shots. Compare files with `cmp` or
    a checksum and drop byte-identical copies. Check the folder size. Commit the tool, the chosen
    shots or contact sheets, and 1 line per image. Remove any claim in the notes that no image
    confirms.
12. **No window at all.** When no window is possible, build a reference image from the shipped
    sprites and write "visual check not verified" in the report. Do not report a look as checked
    from headless output.

## More procedures

8 more procedures moved to reference files. `## References` names each file and its use.

## Pitfalls

The full table has about 90 rows. Read `references/pitfalls.md` when a capture is blank, frozen,
the wrong size, the wrong renderer, or noisy, or when a gate flakes or a layout test is unstable.
The rows below are the ones that cost the most time.

| Symptom | Cause | Fix |
| --- | --- | --- |
| Tests pass, but the real window shows a broken look | Headless Godot does not draw | Capture in a real window; read every image |
| Window shot is 1 frozen frame, or the gate hangs | Another window covers the capture window; the OS stops its redraw | Always on top, no focus, force a draw, run limit, window lock |
| First shot after start or resize is blank | The window did not draw yet | Wait about 60 frames; add a readiness check on a real render value |
| `get_texture().get_image()` is null; a render wait hangs | Headless dummy renderer | Capture only windowed; sync tests on frame counts |
| `--resolution` has no effect; shot is fullscreen | Project settings apply a window scale or mode | Set a fixed windowed size after start |
| A "1x" file is the window size | The root viewport follows the window | Nearest-neighbour downscale; save your own viewport |
| 2 captures of 1 build differ by many pixels | Shaders and wind use wall-clock time | Zero the time inputs; take a base-against-base control |
| Output files land inside the project or nowhere | Relative paths resolve from the project folder | Absolute output paths |

## Artifacts to build

- **Capture tool**: a `SceneTree` script with `--scene`, `--state`, `--seed`, `--frames`, `--out`
  (absolute) and a quit watchdog. It saves a 1x viewport image and a window image per named shot.
- **Demo arguments**: optional demo-scene arguments that stage a state. No arguments means no
  change.
- **Window id helper**: a tiny native helper that prints the window id of your own process for
  the OS screen grab (on macOS, for `screencapture -l <id>`).
- **Window lock**: an atomic directory lock (`mkdir`) with an owner file. Release it only inside
  the branch that took it, and only if you own it.
- **Visual gate script**: runs gate scenes windowed on each renderer, prints 1 `PASS`, `FAIL` or
  `HANG` line per case with values, and has a per-run time limit.
- **Capture queue**: a queue folder, a 3-line request file, and 1 runner outside the sandbox
  (see `references/capture-queue.md`).
- **Pixel diff**: `pixdiff.py` from `references/script-pixdiff.md`, changed-pixel counts with tolerance, box and diff mask.
- **Contact sheet maker**: tiles labelled images into 1 sheet (ImageMagick `montage` or a short
  script).
- **UI harness**: builds named UI states from real-data fixtures for capture and for tests.
- **Rect assert helpers and a settle helper**: test helpers for "no intersect", "inside parent",
  "fits width" and "inside view".
- **Word counter**: a headless tool that counts visible words per screen state.
- **Text fit check**: `text_fit.py` from `references/script-text-fit.md`, and a JSON file of line kinds, limits and lines.
- **Reduced-motion checklist**: 1 line per effect with its still form, kept in the effect plan.
- **QA matrix**: a route library, a cell tool, find and shoot passes, and an analyzer with 1 check
  per defect.
- **Review note**: 1 line per committed image: what it shows, what reads, what does not.

## Related skills

- `godot-test-loop`: the headless test loop. This skill hands each visual fault to it as a test.
- `godot-headless-runs`: rules for `-s` tool scripts and headless runs that capture tools follow.
- `godot-perf-measure`: frame cost. Use it when the question is speed, not the look.
- `godot-pixel-art-pipeline`: makes the art, contrast rules and mockups that this skill checks in game.
- `godot-safe-refactor`: behaviour identity proofs; this skill gives the pixel identity proof.
- `godot-feature-seams-and-saves`: flags and prototype branches for alternate looks; scratch saves.
- `godot-upgrade-tree-design`: the owner consult rule for mechanics; owner visual pick is the same rule
  for looks.
- `godot-release-and-store`: trailers and store media. It calls `references/fixed-seed-capture.md`, then owns clip selection, assembly and store deliverables.
- `godot-fleet-game-dev`: locks, sandboxed workers and landing rules when many agents share 1 machine.
- `godot-testing`: GUT and gdUnit4 basics for the layout and reduced-motion tests.

## References

- `references/pitfalls.md`: the full pitfalls table. Read it when a capture, a gate or a UI test
  does something you did not expect.
- `references/visual-gate.md`: windowed visual gate and gate deflake. Read it when you build,
  run or deflake the gate.
- `references/pixel-diff.md`: pixel diff with a noise floor. Read it when a change must look the same or change 1 area only.
- `references/script-pixdiff.md`: the pixel diff script. Copy the block into a file.
- `references/fixed-seed-capture.md`: fixed-seed capture, frame extraction and contact sheets.
- `references/capture-queue.md`: capture queue for windowless workers. Read it when a worker cannot open a window.
- `references/ui-review.md`: UI state capture review, UI layout invariant tests, text fit check
  and UI function first.
- `references/script-text-fit.md`: the text fit script and an example input. Copy the block into
  a file.
- `references/reduced-motion.md`: reduced-motion coverage. Read it when a change adds or touches motion or a colour cue.
- `references/qa-matrix.md`: visual QA matrix. Read it when an effect must read in every combination of settings.
- `references/owner-visual-pick.md`: owner visual pick. Read it when the owner must choose between looks.
