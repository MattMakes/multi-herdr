# Timing and feel

Every block here is `proof: not run (needs Godot)`.

Timing is the number of frames a move takes. Spacing is how the move spreads over those frames. An animator sets both on purpose. A default tween sets neither.

## Durations

Set a duration by what moves and how far, then check it at the game's frame rate.

| What moves | Distance | Duration |
|---|---|---|
| Small UI: caption, icon, name card | under 10 % of the screen | 0.15 to 0.30 s |
| Panel, portrait, letterbox bar | 10 to 40 % | 0.30 to 0.50 s |
| Full-screen wipe or title | over 40 % | 0.45 to 0.80 s |
| Light prop or small actor accent | short | 0.20 to 0.40 s |
| Heavy thing: gate, boss, vehicle | any | 0.60 to 1.50 s, slow start |
| Camera pan, dolly or crane | any | 1.5 to 4.0 s |
| Slow push on a line | 3 to 8 % zoom | the length of the line |

Rules that set the rest:

- An exit takes 2/3 of its enter. The player already knows the thing.
- Bigger and heavier is slower. Double the size, add about 30 % time.
- Nothing visible is shorter than 0.10 s (6 frames at 60 fps). Below that it reads as a pop.
- A hold is a move of 0. After a reveal, hold 0.6 to 1.2 s. After a line of text, hold 1 s plus 0.3 s per word.
- At 30 fps, round every time to a whole frame (multiples of 0.0333 s).

## Easing: pick by role

| Role | Transition and ease | Why |
|---|---|---|
| A thing enters and stops | `TRANS_CUBIC` or `TRANS_QUART`, `EASE_OUT` | arrives fast, settles |
| A thing leaves | same curve, `EASE_IN` | starts soft, is gone |
| A thing moves from A to B on screen | `TRANS_CUBIC`, `EASE_IN_OUT` | both ends are visible |
| Camera | `TRANS_SINE`, `EASE_IN_OUT` | the gentlest curve; no lurch |
| Playful accent | `TRANS_BACK`, `EASE_OUT` | one small overshoot |
| Physical drop | `TRANS_BOUNCE`, `EASE_OUT`, used once | gravity |
| Wobble that dies | `TRANS_SPRING` or `TRANS_ELASTIC`, `EASE_OUT`, small | a sign, a jelly, a UI badge |
| Constant motion | `TRANS_LINEAR` | scroll, conveyor, progress ring only |

`EASE_OUT_IN` is almost never right: it stops in the middle.

When no stock curve fits, key a bezier track in the `AnimationPlayer`, or sample a `Curve` resource:

```gdscript
extends Node2D

@export var curve: Curve
@export var target: Node2D
@export var from_position: Vector2
@export var to_position: Vector2


func move(duration: float) -> Tween:
	var tween := create_tween()
	tween.tween_method(_apply, 0.0, 1.0, duration)
	return tween


func _apply(progress: float) -> void:
	# The Curve may leave the 0..1 range to overshoot.
	target.position = from_position.lerp(to_position, curve.sample_baked(progress))
```

A `Curve` saved as a `.tres` file is a motion token: the door, the name card and the HUD return can share it, and a designer tunes 1 file.

## The principles that pay in a cutscene

1. **Anticipation.** Before a big move, move 5 to 10 % the other way for 0.08 to 0.15 s. The door dips before it rises. The boss crouches before the leap. It tells the player where to look.
2. **Follow-through and overlap.** Parts stop at different times. The body stops; the cape, the antenna and the dust stop 0.06 to 0.12 s later. Offset the keys of child parts by 2 to 6 frames. Nothing with parts stops as 1 block.
3. **Slow in and slow out.** Most frames sit near the poses. That is what the easing table gives.
4. **Arcs.** Living things move on curves. A hand, a head turn, a jump: add a second key on the other axis, or move along a `Path2D`. Straight lines are for machines.
5. **Squash and stretch.** Scale along the move by 5 to 15 % and across it by the inverse, so the area stays the same. Return to 1.0 within 0.10 to 0.15 s. More reads as cartoon; use the amount the game's art allows.
6. **Secondary action.** One small thing supports the main one: a blink on a look, dust on a landing, a flicker when the gate hits its stop. It never competes.
7. **Staging.** One mover at a time. Dim or still the rest.
8. **Exaggeration.** A game camera is far away. Push poses and reactions 20 to 30 % past what looks right up close.

## Choreography

- Stagger a group by 0.03 to 0.08 s per item, and at most 5 items. A longer list enters as groups.
- Order by importance, or by distance from the cause. A shock wave reaches near things first.
- Give the whole scene 1 rhythm. Mark the beats, and put each key on a beat or on a half beat. Uneven gaps with no reason read as a mistake.
- Contrast makes emphasis. A fast move reads fast only after a hold. Put the longest hold before the biggest move.
- Two things that are related move with the same curve and duration. Two things that are not, do not.

## What cheap motion looks like

Find these in a play-through and fix each one.

| Tell | Fix |
|---|---|
| Every move has the same duration and curve | Set each by role from the 2 tables |
| Everything fades in from opacity 0 with a small slide up | Give each element an entrance that fits what it is: a card slides from its source, a light switches on, a title is written |
| Things start and stop together | Overlap and stagger |
| Linear moves on visible objects | Ease them |
| Bounce or elastic on everything | Keep 1 playful accent per scene |
| The camera never rests | Lock off most shots; move for a reason |
| A move with no cause | Tie it to a sound, a hit, a line or another move |
| Idle things are frozen | Add breath, sway, flicker or drift at a small amplitude, read from the cutscene clock |
| Text appears faster than it can be read | Apply the hold rule |
| Effects sit on top with no light on the scene | Let a flash light the actors: modulate, a light, or a glow |

## Game feel inside a cutscene

- An impact has 3 parts within 0.25 s: 2 to 4 frames of hold on the contact pose, a shake that decays, and a burst of particles from the contact point. The sound starts on the contact frame.
- Slow motion: key `Engine.time_scale` only from the cutscene and restore it in `_exit()`. The `AnimationPlayer` slows with it; that is what you want. Audio pitch does not follow by itself.
- Controller rumble follows the same keys as the shake, and is off under the accessibility setting.
- The **game-juice** skill, when the worker carries it, lists the feedback layers and their strengths.
