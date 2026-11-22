---
name: godot-cutscenes
description: Use when building or fixing a cutscene in a Godot 4 game - an intro, a story beat, a boss entrance, a level-end sequence, a logo sting or a pre-rendered clip. Covers the one-clock rule with AnimationPlayer as the sequencer, skip that lands on the same end state, the gameplay lock and camera handoff, shot staging, animator timing and easing mapped to Tween and bezier tracks, music and beat sync, localized subtitles from cue lists, particles and post effects that scrub, painterly parallax, doodle boil and motion-comic styles, VideoStreamPlayer clips, Movie Maker capture, and headless tests that prove skip, scrub and end state.
---

# Cutscenes in Godot 4

Target engine: **Godot 4.7**. Nothing in this skill ran on an engine in the session that wrote it: every GDScript block is `proof: not run (needs Godot)` until the gate parses it on 4.7.2. Check each call against `godot --doctool` of the project's engine before you rely on it.

A film cutscene is a file that plays. A game cutscene is a piece of the game that takes control away from the player and must give it back in a known state. That difference sets every rule here.

> **Related skills:** **godot-animation-system** for tracks and libraries, **godot-tween-animation** for tweens, **godot-camera-system** for rigs and blends, **godot-dialogue-system** for lines and choices, **godot-particles-vfx** and **godot-shader-basics** for effects, **godot-audio-system** for buses, **godot-visual-verification** for real frames, **godot-library** shelves `animation-advanced`, `particles-and-vfx` and `design-iteration-toolkit`.

## Pick the kind first

| Kind | Use it when | Clock | Skip cost |
|---|---|---|---|
| In-engine sequence | The scene uses the game's own actors, level and camera | 1 `AnimationPlayer` | low |
| Scripted beat | 1 to 5 s, no camera cut: a door opens, a boss lands | 1 `Tween` | low |
| Dialogue scene | The player reads and chooses | the dialogue runner | per line |
| Motion comic | 2D panels, pans and type; no rigs | 1 `AnimationPlayer` | low |
| Pre-rendered clip | The picture cannot be drawn in real time on the target hardware | `VideoStreamPlayer` | free |

Choose the cheapest kind that tells the beat. A pre-rendered clip costs download size, cannot show the player's own gear or choices, and breaks at other aspect ratios. Use it for a logo sting or a film-quality intro, not for story beats that happen in a level.

## The 6 rules of a game cutscene

1. **One clock.** One `AnimationPlayer` (or 1 `Tween`) owns time. Every other thing reads that time: subtitles, particles, the camera, shader parameters. Nothing runs a second timer. Then scrub, pause, slow motion and skip all work with no extra code.
2. **End state is code, not the last keyframe.** A function `_apply_end_state()` sets every fact the game keeps: quest flags, actor positions, doors, inventory, the active camera, the music state. The normal ending calls it. Skip calls it. A load of a save made after the cutscene reaches the same state without playing anything. Method-call tracks do cosmetic work only (a sound, a burst, a shake), because a seek does not fire the keys it jumps over.
3. **Skip is a seek.** Skip seeks the clock to the end with update on, stops it, and calls the same finish path as the normal ending. Hold-to-skip (0.6 to 1.0 s) with a visible ring stops accidental skips. The first play of a story-critical scene may refuse skip; a replay never does.
4. **Lock gameplay, then give it back.** On entry: stop player input, stop enemy AI and timers that can kill the player, hide the HUD, and store what you changed. On exit: restore exactly that. The lock survives the pause menu and a lost window focus.
5. **The camera is borrowed.** Store the gameplay camera, blend to the cutscene camera, and blend back to the stored one. The last shot ends near the gameplay framing so the return does not jump. Reset smoothing after a cut.
6. **Every player can follow it.** Subtitles come from translation keys and are on by default. Text stays on screen for at least 1 s plus 0.3 s per word. A reduced-motion setting removes shake and fast zooms and keeps cuts. No flash is faster than 3 per second.

`references/sequencer.md` holds the `Cutscene` base script, the 2 lock patterns, hold-to-skip, the trigger that plays a scene once, and the save rule.

## Workflow

1. Read the brief and name the beat in 1 sentence: who wants what, and what changes. Write the shot list as a table: shot, seconds, camera, action, line, sound. A cutscene with more than 1 change of fact is 2 cutscenes.
2. Set the length. A gameplay interruption runs 3 to 8 s. A story scene runs 15 to 45 s. Past 60 s, add a player choice, a playable moment, or cut. An intro before the first input stays under 20 s or is skippable from frame 1.
3. Pick the kind from the table. Make 1 scene file per cutscene, `cutscenes/<name>.tscn`, with the script that extends the base in `references/sequencer.md`.
4. Block it: place cameras and actors at their key poses with stepped keys and no easing. Play it. Fix the staging before any polish (`references/staging-and-camera.md`).
5. Time it: set easing, holds, overlap and anticipation from `references/timing-and-feel.md`. Lock picture times to the music markers and the voice lines (`references/audio-and-subtitles.md`).
6. Add effects that read the clock (`references/stylized-techniques.md`): particles with a fixed seed, post effects, parallax plates, boil, type.
7. Write `_apply_end_state()` and the lock. Then write the tests in `references/verification.md`: skip at 5 times gives the end state of a full play; every method key names a method that exists; every subtitle key has a translation.
8. Look at real frames. Headless Godot draws nothing. Capture frames in a window with **godot-visual-verification**, or record the scene with Movie Maker (`references/prerendered-and-capture.md`). Judge the first frame, each cut and the last frame.

## Timing at a glance

A cutscene uses film timing for the camera and game timing for feedback.

| Move | Duration | Tween | Note |
|---|---|---|---|
| UI or caption in | 0.20 to 0.35 s | `TRANS_CUBIC`, `EASE_OUT` | exit in 2/3 of the enter time, `EASE_IN` |
| Prop or actor accent | 0.25 to 0.50 s | `TRANS_BACK`, `EASE_OUT` | overshoot 5 to 10 %; never on a camera |
| Camera pan or dolly | 1.5 to 4.0 s | `TRANS_SINE` or `TRANS_CUBIC`, `EASE_IN_OUT` | no overshoot, no bounce |
| Camera push on a line | 3 to 8 s | `TRANS_SINE`, `EASE_IN_OUT`, 3 to 8 % zoom | slow enough that nobody names it |
| Impact shake | 0.15 to 0.35 s | decay with `TRANS_EXPO`, `EASE_OUT` | off under reduced motion |
| Hold after a reveal | 0.6 to 1.2 s | no motion | the player needs time to read the picture |
| Fade to or from black | 0.4 to 0.8 s | `TRANS_SINE`, `EASE_IN_OUT` | a hard cut is often better |

Linear motion is for constant things only: a conveyor, a scrolling sky, a progress ring. One thing moves at a time; the rest holds or drifts.

## Budget

- A cutscene runs at the game's frame budget on the lowest target device. Measure it there with **godot-perf-measure** when it adds lights, particles or a second viewport.
- Load the cutscene scene before the trigger (`ResourceLoader.load_threaded_request`), or the first frame hitches.
- Shaders compile on first use. Show every cutscene material once behind the loading screen, or the first shot stutters on the Compatibility and Mobile renderers.
- Free the cutscene scene when it ends. Keep nothing in an autoload except the list of played scenes.

## Hand-offs

- Art that must be authored by hand (keyframes on imported rigs, painted plates, voice files) goes in the `DONE:` list.
- Movement rules, damage and AI belong to the gameplay roles. A cutscene calls their public methods; it does not reach into their state.
- A film of the game (a trailer, a devblog cut) belongs to the `video-*` team. This skill gives them the plate: `references/prerendered-and-capture.md`.

## Common mistakes

| Mistake | Result | Fix |
|---|---|---|
| A quest flag set on a method-call track | Skip leaves the flag unset | Set it in `_apply_end_state()` |
| `await get_tree().create_timer(2.0).timeout` chains | Cannot scrub, cannot skip cleanly, drifts under pause | Put the beats on 1 `AnimationPlayer` |
| `get_tree().paused = true` with actors that must move | The actors freeze | Use the group lock, or set those actors to `PROCESS_MODE_ALWAYS` |
| Subtitle text typed into a track | No translation, no font fallback | Keys and a cue list |
| Camera tween with `TRANS_BACK` or `TRANS_ELASTIC` | The world lurches | `TRANS_SINE` or `TRANS_CUBIC`, in-out |
| Particles with a random seed | Every play differs, the capture cannot be repeated | `use_fixed_seed` |
| Music started by code and picture by the animation | The picture drifts off the beat after about 20 s | Audio-master sync in `references/audio-and-subtitles.md` |
| The cutscene starts again after a load | The trigger kept no record | Save the played list; check it in the trigger |
