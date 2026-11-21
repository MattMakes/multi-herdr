# Pre-rendered clips and capture

Every block here is `proof: not run (needs Godot)`. Run every Godot command through the wrapper of **godot-build-verify**.

## A clip inside the game

`VideoStreamPlayer` plays a clip. The engine itself decodes Ogg Theora (`.ogv`) only. Other formats need an addon that the project already pins.

Where a clip comes from: the film pipeline's `render/master.mp4`, a Blender or Manim render, or a Movie Maker recording of the game itself (below).

Encode it for the game:

```sh
ffmpeg -i master.mp4 -c:v libtheora -q:v 7 -g 30 -c:a libvorbis -q:a 5 -ar 48000 intro.ogv
```

- `-q:v` 6 to 8 is the usual range; check file size against the download budget. Theora needs about 2 times the bitrate of H.264 for the same look.
- Keep the clip's frame rate at the game's rate or half of it.
- The clip has 1 language of baked audio. Prefer a clip with music and effects only, and play voice and subtitles from the game, so that they translate.
- Do not bake text into the picture, for the same reason.

```gdscript
extends Control
## Plays a clip as a cutscene: skippable, with the same finish path.

signal finished(skipped: bool)

@export var video: VideoStreamPlayer

var _running: bool = false


func play() -> void:
	_running = true
	video.finished.connect(_on_video_finished)
	video.play()


func skip() -> void:
	if _running:
		video.stop()
		_finish(true)


func position_sec() -> float:
	return video.stream_position if _running else 0.0


func _on_video_finished() -> void:
	_finish(false)


func _finish(skipped: bool) -> void:
	if not _running:
		return
	_running = false
	video.finished.disconnect(_on_video_finished)
	finished.emit(skipped)
```

- Set the player's `expand` on and anchor it to the full rect. Letterbox or pillarbox with a black `ColorRect` behind it; do not stretch the picture.
- A clip does not pause with the tree unless its `process_mode` says so. Set `paused` on the player when the game pauses.
- Seeking a Theora stream is limited on older 4.x versions. Check `stream_position` in the class reference of the project's engine before you design a scrub or a chapter jump.
- Subtitles for a clip use the cue list of `audio-and-subtitles.md` with `position_sec()` as the clock.

## Record the game with Movie Maker

Movie Maker mode renders every frame at a fixed step, however slow the machine. Audio is mixed offline and stays in sync. Use it for:

- a plate for a trailer or a devblog film (the `video-*` team's capture input);
- a clip to play back in the game on hardware that cannot draw the scene live;
- review frames of a cutscene, and a repeatable visual check.

It needs a real window and a renderer: headless Godot draws nothing. On a machine without a display, report `BLOCKED:`.

1. Make a capture scene that holds the level, the cutscene and nothing interactive. Its script plays the cutscene and quits when `finished` fires.
2. Fix every source of chance: `use_fixed_seed` on particles, a seeded `RandomNumberGenerator`, no reads of the wall clock.
3. Run it with `--write-movie <file>`, `--fixed-fps <n>` and `--resolution <w>x<h>`.
4. Convert the result with `ffmpeg`.

```gdscript
extends Node
## Capture harness: plays one cutscene and quits.

@export var cutscene: Cutscene


func _ready() -> void:
	if OS.has_feature("movie"):
		# Recording does not run in real time, so quality costs nothing.
		get_viewport().msaa_3d = Viewport.MSAA_4X
	cutscene.finished.connect(_on_finished)
	cutscene.play()


func _on_finished(_skipped: bool) -> void:
	get_tree().quit()
```

Output formats and what they are for:

| Output | Use |
|---|---|
| `.png` sequence plus a WAV file | The film pipeline. Lossless frames; encode once at the end. |
| `.avi` (MJPEG) | A quick review file. Large; convert before you share it. |
| `.ogv` | A clip to ship in the game, on engine versions that write it. |

A PNG sequence to a plate the film pipeline accepts (constant frame rate, H.264, `yuv420p`):

```sh
ffmpeg -framerate 60 -i capture/frame%08d.png -i capture/audio.wav \
  -c:v libx264 -crf 14 -preset slow -pix_fmt yuv420p -r 60 -c:a aac -b:a 192k -ar 48000 plate.mp4
```

Check the frame name pattern that the engine wrote before you run this; it differs by version.

Quality while recording:

- Record at 2 times the target size and scale down in `ffmpeg` for clean edges, when the renderer has no good anti-aliasing for the art.
- For motion blur, record at 4 times the frame rate and blend 4 frames into 1 with the `tmix` filter of `ffmpeg`. Effects that settle over several frames also look better that way.
- Hide debug overlays and the mouse pointer. Turn the HUD off with the same call the cutscene uses.
- Record 0.5 s more than the scene on each end. An editor needs handles for a cut.

## Hand a plate to the film team

The `video-*` team works from `timeline.json` and files in a shared media root. Give them:

- the plate file, its frame rate, its size and its exact frame count (`ffprobe -count_frames`);
- the times of the cuts and hits inside it, in seconds, so that the audio role can place sounds;
- the commit of the game that the plate shows.

Do not commit the plate. Working media stays out of git; the orchestrator delivers finished films.
