# Compositing plates

A composite puts layers together: a 3D shot over a painted ground, a keyed recording over a scene, a grade over everything. The editor's master assembly joins scenes in time. This page is about layers inside 1 scene, before that.

Use `ffmpeg` first. It is on every host of the pipeline. Use EffectCraft only when its command line is installed and the task needs what `ffmpeg` does badly: many layers with keyframes, track mattes, a real keyer, or 32-bit float.

## With ffmpeg

Work on frame-exact inputs: every layer has the same frame rate and frame count. All commands write a lossless intermediate; encode the plate at the end.

Over, with alpha (a PNG sequence with straight alpha over a ground):

```sh
ffmpeg -y -i ground.mkv -framerate 60 -i fg/f%05d.png \
  -filter_complex "[0:v][1:v]overlay=format=auto:shortest=1" \
  -c:v ffv1 -level 3 -an comp.mkv
```

Blend modes (screen for light, multiply for shadow and paper):

```sh
ffmpeg -y -i base.mkv -i light.mkv \
  -filter_complex "[0:v]format=gbrp[a];[1:v]format=gbrp[b];[a][b]blend=all_mode=screen:all_opacity=0.8" \
  -c:v ffv1 -level 3 -an comp.mkv
```

A key on a flat green or blue ground, with spill removal:

```sh
ffmpeg -y -i take.mkv -i ground.mkv \
  -filter_complex "[0:v]chromakey=0x00d000:0.12:0.06,despill=type=green[k];[1:v][k]overlay=shortest=1" \
  -c:v ffv1 -level 3 -an comp.mkv
```

A grade and a finish:

| Need | Filter |
|---|---|
| Exposure, contrast, saturation | `eq=brightness=0.02:contrast=1.06:saturation=1.05` |
| Curves | `curves=preset=medium_contrast`, or points per channel |
| A look file | `lut3d=file=look.cube` |
| Vignette | `vignette=PI/5` |
| Grain | `noise=alls=6:allf=t+u` (`allf=t` changes per frame; give `all_seed` a fixed value) |
| Soft glow | split, blur 1 copy with `gblur=sigma=12`, then `blend=all_mode=screen:all_opacity=0.25` |
| Shutter blur from oversampled frames | `tmix=frames=4,select='not(mod(n\,4))'` on a plate rendered at 4 times the frame rate |
| Scale | `scale=1920:1080:flags=lanczos` |

Rules:

- The local `ffmpeg` of the pipeline has no text filters. All type is rendered in Chrome as a transparent layer by the editor.
- Convert to planar RGB (`format=gbrp`) before a blend, and back to `yuv420p` only at the final encode. A blend in limited-range YUV shifts colour.
- Give every noise filter a fixed seed. Check with 2 runs that the output frames match.
- Keep `shortest=1` on overlays, then count the frames of the result.
- 1 look per film. The grade values go in `MOTION.md` as tokens, so that every plate gets the same finish.

## With the EffectCraft command line

EffectCraft is a layer compositor with a command line and a project file. It is young software: its own notes call it alpha and not yet fit for client work, and its Linux and Windows builds are less tested than the macOS one. Treat a result from it like any other plate: it must pass the 5 checks.

Preflight: `effectcraft-cli info`. Missing: report `BLOCKED:` and name the release page of the project; do not build it from source unasked, and do not register its MCP server from a checked-out repository (that would compile and run code the operator did not review).

It renders on the CPU by default, so it needs no display and no GPU. Add its GPU option only when the task says so, and then for the whole plate.

Build a project with commands, not by hand-written JSON. List what the installed version can do first:

```sh
effectcraft-cli exec --list --schemas --json
effectcraft-cli exec comp.new --params '{"name":"Main","width":1920,"height":1080}' --empty --save-as comp.ecproj
effectcraft-cli exec file.import '{"paths":["plates/shot/f00001.png"],"sequence":true,"frameRate":60}' comp.ecproj --save
effectcraft-cli set Main '#1' transform/position '[960,540]' --time 0 comp.ecproj --save
```

- Set the composition's size, frame rate and length from `timeline.json` before you add a layer.
- Tell it how each footage item stores alpha (straight or premultiplied) when it guesses wrong: edges with a dark or light fringe are the sign.
- Keyframe times are in layer time; reads and frame renders are in composition time. When a layer does not start at 0, pass the time base that the command's schema names.
- Several steps go in 1 batch file and run in 1 call, so a failure leaves no half-built project.

Look before you render the range, then render frames:

```sh
effectcraft-cli render-frame comp.ecproj --time 1 --max-side 640 --out check.png --json
effectcraft-cli render comp.ecproj --comp Main --out 'plates/<clip>/f[#####].png' --progress
```

Render a PNG or EXR sequence and encode the plate with the `ffmpeg` command of the plate contract. That keeps 1 encoder and 1 set of colour tags for the whole film.

An error comes back as JSON with an `error` field and exit status 1; a usage error exits 2. Put the error text in your report.

## Which layer order

Ground, far layers, subject, near layers, light (screen or add), shadow and paper (multiply), grade, grain, then type on top from the editor. Grain goes over everything except type, so that all layers share 1 surface.
