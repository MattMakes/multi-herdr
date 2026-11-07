# Desktop app and engine capture

Use these rules when CDP cannot capture the product. Record each clip to
`plates/<clip>.mp4`. The timeline scene can name that path in `plate`.

## Build and isolation

- Capture the current build when it looks better than an old build.
- Show only the features that belong to the episode.
- Run the product at a fixed frame rate. Disable delta smoothing.
- Use 1 fixed seed for the product and the staging driver.
- Use an isolated user-data folder. Never read or change the operator's saves.
- On macOS, keep the capture window on top. A covered window can produce frozen frames.
- When 2 capture workers use 1 screen, acquire 1 window lock before either worker moves or covers a window.
  Release the lock after capture. Do not record without the lock.

## Staging driver

Write a driver script for the shot. The script starts the isolated build and
sets all shot state before recording. It sets these items explicitly:

- Camera distance.
- Subject path.
- Positions of threats, pickups, or other proof objects.
- Menu progress.

Stage menu values that show progress. Do not show `0/50`, `+0`, or rows of
locked items. Keep the driver with the episode source so another worker can
reproduce the shot with the same build, frame rate, and seed.

## Clip sidecar

Write `plates/<clip>.json` next to every `plates/<clip>.mp4`. Use this exact
shape:

```json
{
  "clip": "plates/<clip>.mp4",
  "fps": 60,
  "frame_count": 0,
  "seed": 0,
  "build": "<id>",
  "key_actions_sec": {"<name>": 1.67},
  "pixel_facts": {"subject_width_px": 120, "nearest_distance_px": 40},
  "max_frozen_sec": 0.017
}
```

Times start at frame 0 of the clip. Measure `fps` and `frame_count` from the
written MP4. Record the current build identifier and the driver seed. Measure
the subject width and nearest distance in pixels from the real plate. The
director syncs beats to `key_actions_sec`.

## Visual and automated checks

Run the plate gate from the video project root:

```sh
python3 "<skill-dir>/scripts/check_plate.py" <scene-id>
```

The gate reads the timeline scene's `plate`. It fails when decoded consecutive
frames are identical for longer than 1 frame. At 60 fps, the maximum passing
run is about 0.017 s. It reports the start and length of the longest frozen
run. When the clip has a sidecar, the gate validates its shape and compares
its `fps` and `frame_count` with the MP4.

Also inspect 3 or more frames at the key action times. Inspect 1 frame before,
1 frame at, and 1 frame after each important action. Check the staged state,
subject size, nearest distance, and visible action. Count the subject pixels
by hand when a measurement helper fails.
