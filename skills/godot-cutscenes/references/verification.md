# Verification

Every block here is `proof: not run (needs Godot)`.

A cutscene has 2 kinds of faults. State faults (skip leaves a flag unset, the camera is not given back, a key is missing) show in a headless test. Look faults (a bad cut, an unreadable line, a pop) show only in real frames.

## Headless tests for state

Write them in the project's test framework (see **godot-testing**). The assertions below are plain GDScript; wrap them in the framework's test functions.

### 1. Skip gives the same end state as a full play

Record the facts the game keeps after a full play. Then, for several skip times, play to that time, skip, and compare.

```gdscript
extends Node
## Helper for cutscene tests. `snapshot` returns the facts the game keeps.

const SKIP_FRACTIONS: Array[float] = [0.0, 0.1, 0.5, 0.9, 0.999]


func full_play_state(make_cutscene: Callable, snapshot: Callable) -> Dictionary:
	var cutscene := make_cutscene.call() as Cutscene
	add_child(cutscene)
	cutscene.play()
	# Manual advance: no waiting for real time.
	cutscene.player.advance(cutscene.player.current_animation_length + 0.1)
	var state: Dictionary = snapshot.call()
	cutscene.free()
	return state


func skipped_states(make_cutscene: Callable, snapshot: Callable) -> Array[Dictionary]:
	var states: Array[Dictionary] = []
	for fraction in SKIP_FRACTIONS:
		var cutscene := make_cutscene.call() as Cutscene
		add_child(cutscene)
		cutscene.play()
		cutscene.player.advance(cutscene.player.current_animation_length * fraction)
		cutscene.skip()
		states.append(snapshot.call())
		cutscene.free()
	return states
```

The test passes when every dictionary in `skipped_states` equals `full_play_state`. `make_cutscene` must build a fresh level state each time. When the full play does not emit `finished` after the manual advance on the project's engine, drive the test with `await cutscene.finished` under a raised `Engine.time_scale` instead.

### 2. The lock is released

After `finished`, on both paths:

- the current camera is the camera that was current before `play()`;
- every node in group `gameplay` reports unlocked;
- `get_tree().paused` has its old value and `Engine.time_scale` is 1.0;
- the HUD root is visible;
- no cutscene audio player is playing.

### 3. Every method key has a target

```gdscript
extends Node


func missing_method_targets(player: AnimationPlayer, animation_name: StringName) -> PackedStringArray:
	var missing := PackedStringArray()
	var animation := player.get_animation(animation_name)
	var root := player.get_node(player.root_node)
	for track in animation.get_track_count():
		if animation.track_get_type(track) != Animation.TYPE_METHOD:
			continue
		var target := root.get_node_or_null(animation.track_get_path(track))
		for key in animation.track_get_key_count(track):
			var method := animation.method_track_get_name(track, key)
			if target == null or not target.has_method(method):
				missing.append("%s.%s" % [animation.track_get_path(track), method])
	return missing
```

The same loop over all tracks finds a path that no longer resolves after a scene edit: `root.get_node_or_null(...)` is null.

### 4. Subtitles

- Every cue key has a translation in every shipped locale: `tr(key) != key` after the locale is set.
- Cues are in time order and do not overlap.
- Every cue is at least 1.0 s long and under 17 characters per second in every locale.
- The last cue ends before the animation ends.

### 5. Play once

Run the trigger twice against a store that records the id. The second run starts nothing. Load a save made after the scene: the end state holds and the cutscene node does not exist.

## Real frames for the look

Headless Godot uses a dummy renderer. Use **godot-visual-verification** for windowed captures, or a Movie Maker recording (`prerendered-and-capture.md`). Look at:

| Frame | Question |
|---|---|
| First frame | Does it match the gameplay frame before it, or cut with intent? |
| 2 frames each side of every cut | Is there a flash frame, a pop, or a half-blended camera? |
| The middle of every shot | Does the silhouette read? Is the subject clear of the subtitle band? |
| Every frame with text | Can the longest locale fit and be read in the time given? |
| Last frame | Does it match the gameplay frame that follows? |
| The frame after a skip at 3 different times | Is it the same picture as the last frame of a full play? |

Make a contact sheet: 1 frame per shot plus the frames around each cut. Judge it at the size the player sees, not zoomed in.

Then watch it once at full speed with sound, and once with no sound. With no sound, the story must still read from picture and subtitles.

## Motion review

Play at half speed (`Engine.time_scale = 0.5` in a debug build) and check against `timing-and-feel.md`:

- Does anything start or stop as 1 block?
- Is any visible move linear, or shorter than 6 frames?
- Do 2 things compete for the eye?
- Does every hit have a hold, a shake and a sound on the same frame?
- Is the camera moving without a reason?

## Performance

Measure on the lowest target device with **godot-perf-measure**: frame time through the whole scene, the first frame after the trigger (load hitch), and the first use of each material (shader compile). A cutscene that drops frames reads worse than a simpler one that holds the rate.

## What to report

A `DONE:` for a cutscene names: the scene file, its length, the kind, the Godot version, the parse check, each test above with its result, the capture files you looked at, each binary asset a human must author, and each check you could not run with the reason.
