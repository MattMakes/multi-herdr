Adds 2D skeletal (cutout) animation with `Skeleton2D`, `Bone2D` and a weighted `Polygon2D`, plus frame-exact events and seamless animation swaps on `AnimatedSprite2D`. Read it when a 2D character is made of parts instead of frames, or when gameplay must react to one sprite frame (a footstep, the impact frame of a swing).

> ← Back to [SKILL.md](../SKILL.md)

# 2D Cutout Rigs and Sprite Frame Events

## Choose the technique

| Technique | Art cost | Good for |
|---|---|---|
| Frame animation (`AnimatedSprite2D`, see [sprite-animation.md](sprite-animation.md)) | one drawing per frame | pixel art, small sprites, hand-drawn style |
| Cutout: parts as separate `Sprite2D` nodes under bones | parts drawn once | puppets, paper style, many characters sharing animations |
| Cutout with a deformed `Polygon2D` mesh | parts drawn once, plus weight painting | smooth bends at elbows and knees, cloth, tails |

## A cutout rig

```
Character (Node2D)
├─ Skeleton2D
│   └─ Hip (Bone2D)
│       ├─ Torso (Bone2D)
│       │   └─ UpperArm (Bone2D) ─ Forearm (Bone2D)
│       └─ Thigh (Bone2D) ─ Shin (Bone2D)
├─ Body (Polygon2D)          skeleton = ../Skeleton2D, weights painted per bone
└─ AnimationPlayer           tracks key Bone2D rotation (and Hip position)
```

1. Build the bone chain in the rest pose. Select the skeleton and use **Skeleton2D > Make Rest Pose** so each bone's `rest` transform matches.
2. For simple parts, parent each `Sprite2D` to its bone. No weights are needed; the part follows the bone.
3. For a bending mesh, draw the `Polygon2D` UV and polygon in its editor, add internal vertices at the joints, set `skeleton`, then use **Sync Bones** and paint weights per bone. Each vertex's weights should sum to 1.
4. Animate bone **rotation** with value tracks; animate position only on the root bone. Keyed bone positions drift the joints apart.

Rules:

- Keep a RESET animation with the rest pose (see [player-tracks-and-libraries.md](player-tracks-and-libraries.md)). Re-entering the editor otherwise saves the last previewed pose.
- `Bone2D.apply_rest()` snaps a bone back to its rest transform in code, for example before you switch rigs.
- Draw order between parts is node order and `z_index`, not bone order. A forearm that must pass behind the torso needs its own `z_index` key on the track.
- Flip the whole character by setting `scale.x = -1` on the root `Node2D`, not by flipping each part.
- 2D IK (`SkeletonModification2DTwoBoneIK`, `SkeletonModification2DFABRIK` and the stack in `Skeleton2D.set_modification_stack()`) exists, but the class reference marks these classes **Experimental** in 4.7: they may change or go away. For a shipping game, prefer keyed animation, or a small script that sets two bone angles from the law of cosines.

## Frame-exact events on `AnimatedSprite2D`

`AnimatedSprite2D` has no method tracks. Use its `frame_changed` signal and act on a frame number, kept in data, not in code:

```gdscript
# sprite_frame_events.gd
class_name SpriteFrameEvents
extends Node

signal frame_event(event_name: StringName)

@export var sprite: AnimatedSprite2D
## animation name -> (frame index -> event name), e.g. {"run": {1: "footstep", 5: "footstep"}}.
@export var events: Dictionary[StringName, Dictionary] = {}


func _ready() -> void:
    sprite.frame_changed.connect(_on_frame_changed)


func _on_frame_changed() -> void:
    var per_frame: Dictionary = events.get(sprite.animation, {})
    if per_frame.has(sprite.frame):
        frame_event.emit(StringName(str(per_frame[sprite.frame])))
```

- `frame_changed` fires when the shown frame changes, including when you set `frame` or `animation` from code. If an event must fire only during play, check `sprite.is_playing()` in the handler.
- `animation_looped` fires at each loop of a looping animation, and `animation_finished` at the end of a non-looping one (or the start, when played backwards); playback then pauses.

## Swap animations without a hitch

Switching from "run" to "run_shoot" at the same point of the cycle keeps the legs in step. Setting `frame` resets `frame_progress` to 0, which shortens the current frame. Copy both:

```gdscript
# sprite_swap.gd
class_name SpriteSwap
extends RefCounted


## Switches `sprite` to `next` at the same frame and progress, when both have the same length.
static func swap_in_step(sprite: AnimatedSprite2D, next: StringName) -> void:
    var frame := sprite.frame
    var progress := sprite.frame_progress
    sprite.play(next)
    sprite.set_frame_and_progress(frame, progress)
```

## `AnimatedTexture` is deprecated

`AnimatedTexture` (a texture that cycles frames by itself) is marked **Deprecated** in the 4.7 class reference: it does not work properly in current versions and may be removed. Use `AnimatedSprite2D`, an `AnimationPlayer` on a `Sprite2D`'s `frame`, or a flipbook shader instead.
