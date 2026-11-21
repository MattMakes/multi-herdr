# Staging and camera

Every block here is `proof: not run (needs Godot)`.

## Staging before polish

1. One idea per shot. Name it in 3 words: "guard blocks gate". If the 3 words need "and", split the shot.
2. Silhouette first. Turn every actor black on a mid-grey background. If the pose does not read, change the pose or the camera, not the animation.
3. Put the subject where the player already looks. The first shot starts on the point the gameplay camera held, then moves.
4. Lead the eye with 1 mover. While the subject moves, every other thing holds or drifts under 10 % of its speed.
5. Keep the screen direction. A character that walks left to right keeps doing so across a cut, unless a shot on the axis line resets it.
6. Stay on one side of the line between 2 characters who talk. Crossing it flips them on screen and reads as a mistake.
7. Leave space in the direction of a look or a move: about 1/3 of the frame.
8. Keep faces, hands and text out of the outer 5 % of the frame, and out of the subtitle band at the bottom 20 %.

## Shot grammar for a game

| Shot | Game use | Length |
|---|---|---|
| Establishing wide | A new place, a boss arena, the goal of the level | 2 to 4 s |
| Medium two-shot | Two characters trade lines | 2 to 6 s per line |
| Close on a face or object | The thing that changes: a key, a wound, a switch | 1 to 3 s |
| Over the shoulder (3D) | The player character listens; keeps the player in frame | 2 to 5 s |
| Reveal pan or crane | Show scale; end on the goal | 3 to 6 s |
| Insert | A hand on a lever, a light going out | 0.5 to 1.5 s |
| Return shot | Ends within a small blend of the gameplay camera | 1 to 2 s |

Cut on action: start a move in shot A and finish it in shot B, 2 to 4 frames after the move starts. Cut on a beat marker when music leads. A cut is a change of at least 30 degrees or a clear change of size; a smaller change reads as a jump.

A boss entrance in 5 shots: wide (the arena, empty) 2 s; insert (the thing that announces it) 1 s; reveal (the boss, low angle) 3 s; close (the player character reacts) 1 s; return shot with the name card 2 s. Total 9 s, skippable after the first play.

## Camera moves

- A move has a start, a constant middle and a soft stop. Use `TRANS_SINE` or `TRANS_CUBIC` with `EASE_IN_OUT`. Never `TRANS_BACK`, `TRANS_ELASTIC` or `TRANS_BOUNCE` on a camera.
- A move needs a reason: follow a subject, reveal a thing, or push in on a line. A camera that moves in every shot has no emphasis left.
- A slow push of 3 to 8 % over a whole line keeps a talking shot alive. Use bezier tracks for it so the speed never steps.
- 2D: move the `Camera2D` position and `zoom`. Turn `position_smoothing_enabled` off for the cutscene camera; smoothing adds a second clock. Call `reset_smoothing()` on the gameplay camera after the return.
- 3D: animate a rig node, not the `Camera3D` itself. The rig carries position and yaw, a child carries pitch, the camera carries `fov`. Roll stays 0 unless the story tilts the world.
- A path move uses `PathFollow2D` or `PathFollow3D` with `progress_ratio` on a track. The curve gives the shape; the track gives the timing.
- With the Phantom Camera addon in `addons/`, give each shot a camera with a priority and key the priority. The addon owns the blend. Follow **godot-phantom-camera** and its pinned version.

## Borrow and return the camera

```gdscript
extends Node
## Blends from the current Camera2D to a shot camera and back.

var _borrowed: Camera2D
var _tween: Tween


func take(shot: Camera2D, blend_sec: float = 0.6) -> void:
	_borrowed = get_viewport().get_camera_2d()
	if _borrowed == null or blend_sec <= 0.0:
		shot.make_current()
		return
	var target_position := shot.global_position
	var target_zoom := shot.zoom
	shot.global_position = _borrowed.get_screen_center_position()
	shot.zoom = _borrowed.zoom
	shot.make_current()
	_tween = create_tween().set_parallel(true)
	_tween.tween_property(shot, "global_position", target_position, blend_sec) \
		.set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN_OUT)
	_tween.tween_property(shot, "zoom", target_zoom, blend_sec) \
		.set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN_OUT)


func give_back() -> void:
	if _tween:
		_tween.kill()
	if is_instance_valid(_borrowed):
		_borrowed.make_current()
		_borrowed.reset_smoothing()
	_borrowed = null
```

On a skip, call `give_back()` from `_exit()` with no blend. A blend after a skip makes the player wait for a scene they refused.

## Letterbox and framing

- Letterbox bars tell the player that control is gone. Slide 2 `ColorRect` bars in over 0.3 to 0.5 s on a `CanvasLayer` above the game and below the subtitles. Bars of 10 to 12 % of the height each give about 2.39:1 on a 16:9 screen.
- Subtitles sit on the picture or in the lower bar, never half on each.
- Test at 16:9, 16:10, 21:9 and the handheld size the game ships on. Frame for the narrowest; anchor bars and text to the viewport, not to pixels.
- Hide the HUD with 1 call on the HUD root. Do not key the visibility of single widgets.

## Depth and focus

- 3D depth of field: `CameraAttributesPractical` on the cutscene camera, far blur on, and key `dof_blur_far_distance` to rack focus between 2 subjects over 0.4 to 0.8 s. Leave it off on the Compatibility renderer and on low settings.
- 2D depth: 3 to 5 layers with different scroll speeds. See the parallax recipe in `stylized-techniques.md`.
- A vignette and a 2 to 4 % desaturation of the background pull the eye to the subject at almost no cost.
