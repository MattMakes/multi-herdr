# Blender shots as plates

The `blender-rendering`, `blender-lighting-lookdev`, `blender-animation` and `blender-scene-setup` skills own the Blender detail. This page adds what a film shot needs on top: the timeline fit, the camera, and the hand-off. Check every property name against the installed Blender; names moved between 4.x and 5.x.

## Fit the timeline

1. Read the scene row in `timeline.json`: `duration_sec`, and `meta.fps`, `meta.width`, `meta.height`.
2. Set the scene frame rate to `meta.fps` exactly. A 24 fps look in a 60 fps film is a creative choice that the director writes in `DIRECTION.md`; then animate on a held grid, and still render every timeline frame.
3. Set the frame range to `1` through `round(duration_sec * fps)` plus the handles of the plan.
4. Set the resolution to the timeline size at 100 %. For a scene with a zoom, render at the plan's `required_capture_scale`.
5. Save the `.blend` file and the driver script next to the episode source. The `.blend` file is a binary asset: it goes through git LFS, and the orchestrator decides when.

## Camera for film

The game-asset teammates never needed a camera that moves. A film shot does.

- Animate a rig, not the camera object. An empty carries position and heading; the camera is its child and carries tilt and focal length.
- A move starts and stops softly: bezier keys with automatic clamped handles. No overshoot on a camera.
- A dolly along a curve: a Follow Path constraint on the rig, with the offset factor keyed. A Track To constraint on the camera aims at a target empty. Animate the target for a look that leads the move.
- Focal lengths: 24 to 35 mm for a wide room, 50 mm for a natural view, 85 to 135 mm for a product or a face. Change the lens between shots, not during one, unless the shot is a zoom.
- Depth of field at real scale: f/2.8 to f/5.6, with the focus on an empty that you can key for a focus pull over 0.4 to 0.8 s.
- Keep the subject out of the bottom 20 % when captions will sit there. The director's `critical_regions` say where.
- A turntable for a product: 85 mm, 1 full turn in 4 to 8 s, linear rotation (constant motion is the 1 place for a linear key).

## Motion blur and samples

- Turn motion blur on with a shutter of 0.5 (a 180 degree shutter) for a photographic look. Keep it off for a stepped or graphic look.
- When the film gets shutter blur at the editor (`--subframes`), do not also blur in Blender. 1 blur grammar per film; `MOTION.md` names it.
- Use adaptive sampling with denoising for the final. Raising the sample count alone is the slow way to remove noise.
- Keep data loaded between frames for an animation (persistent data), and render on 1 device type for the whole shot.

## Render frames, then encode

Render to a frame folder, never straight to a video file:

```sh
blender -b shot.blend -E CYCLES -o //frames/f##### -F PNG -s 1 -e 360 -a
```

- A frame that failed is rendered again alone with `-f <n>`.
- For a shot that the compositor will grade or key, write multilayer EXR with the passes it needs, and convert a graded PNG sequence for the plate.
- Colour: render with the view transform the look-dev chose, and write display-referred PNG. Do not apply a second view transform when you encode.
- A transparent background needs film transparency on, and a PNG sequence with alpha as the plate.

Then encode with the `ffmpeg` command of the plate contract, write the sidecar, and run the 5 checks.

## Time per frame

Measure 3 frames before you start the full range and write the estimate in your first progress note: seconds per frame times the frame count. A 6 s shot at 60 fps is 360 frames. When the estimate is over the time the task allows, report the options to the orchestrator: fewer samples, a lower render scale with the editor's upscale limit in mind, a 30 fps render in a 60 fps film (each frame held twice, written in `MOTION.md`), or EEVEE for a look that allows it.

## Hand-off

- `plates/<clip>.mp4` or the PNG folder, with the sidecar.
- `key_actions_sec` for every impact, landing and cut point, read from the keyframes: `(frame - 1) / fps`.
- In `DONE:`: Blender version, engine, device, samples, seconds per frame, and each plate check with its result.
