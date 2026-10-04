Adds `AnimationTree` layering beyond the state machine and blend spaces in SKILL.md: one-shot overlays, upper-body blends with bone filters, time scale, the Transition node, nested state machines, advance conditions and expressions, and root motion applied to a `CharacterBody3D`. Read it when a character must attack while running, play a hit reaction over any state, or move by its animation.

> ← Back to [SKILL.md](../SKILL.md)

# AnimationTree: Layers, Requests and Root Motion

## The rule: the tree owns the player

When an `AnimationTree` is `active`, it drives the `AnimationPlayer` it points at. Do not call `play()` on that player: the two fight, and the pose jitters. Everything goes through tree **parameters**.

Parameter paths start with `parameters/`, then the node names inside the tree, as shown in the Inspector under the tree's Parameters section. Copy them from there; a typo silently does nothing.

## A layered blend tree

A common shape: locomotion (a state machine) as the base, an upper-body action blended over it, a one-shot hit reaction on top, and a time scale for slow motion.

```
BlendTree (tree_root)
├─ Locomotion   AnimationNodeStateMachine   (idle / walk / run / jump)
├─ UpperAction  AnimationNodeAnimation      (e.g. "aim")
├─ UpperBlend   AnimationNodeBlend2         filter: spine and arm bones only
├─ HitShot      AnimationNodeOneShot        shot input: "hit_react"
├─ Speed        AnimationNodeTimeScale
└─ output
Locomotion → UpperBlend.in, UpperAction → UpperBlend.blend
UpperBlend → HitShot.in, HitReact → HitShot.shot → Speed → output
```

```gdscript
# character_animator.gd
class_name CharacterAnimator
extends Node

@export var tree: AnimationTree

const AIM_BLEND := &"parameters/UpperBlend/blend_amount"
const HIT_REQUEST := &"parameters/HitShot/request"
const HIT_ACTIVE := &"parameters/HitShot/active"
const SPEED := &"parameters/Speed/scale"
const LOCOMOTION := &"parameters/Locomotion/playback"

var _aim: float = 0.0


func set_aiming(aiming: bool, delta: float) -> void:
    var target := 1.0 if aiming else 0.0
    var next := move_toward(_aim, target, delta * 6.0)
    # Write only on change: fewer property sets, and the Inspector stays readable.
    if not is_equal_approx(next, _aim):
        _aim = next
        tree.set(AIM_BLEND, _aim)


func play_hit() -> void:
    tree.set(HIT_REQUEST, AnimationNodeOneShot.ONE_SHOT_REQUEST_FIRE)


func is_hit_playing() -> bool:
    return bool(tree.get(HIT_ACTIVE))


func set_slow_motion(factor: float) -> void:
    tree.set(SPEED, factor)


func go_to(state: StringName) -> void:
    var playback := tree.get(LOCOMOTION) as AnimationNodeStateMachinePlayback
    playback.travel(state)
```

### One-shot overlays

- `ONE_SHOT_REQUEST_FIRE` starts the shot; `ONE_SHOT_REQUEST_FADE_OUT` ends it with its fade-out; `ONE_SHOT_REQUEST_ABORT` cuts it at once. Read `parameters/<name>/active` to know whether it still plays.
- `fadein_time` and `fadeout_time` on the node smooth the overlay. Without them the shot snaps in.
- A one-shot holds one animation. For several reactions (hit from front, back, left), feed the shot input from a `Transition` node or a small state machine, and pick the reaction before you fire.

### Bone filters for upper-body layers

A `Blend2` (or `Add2` for additive poses) with **filters** blends only the listed tracks. Enable "Filter" on the node in the tree editor and tick the spine, neck and arm bones. In code: `blend_node.filter_enabled = true` and `blend_node.set_filter_path(NodePath("Skeleton3D:Spine"), true)` per bone. Without a filter, the aim pose also overwrites the legs, and the character slides in an aim stance.

### Transition node

`AnimationNodeTransition` is a switch with cross-fade, simpler than a state machine when there are no rules between states (weapon type: unarmed, pistol, rifle). Set `parameters/<name>/transition_request` to the input name; read `parameters/<name>/current_state`.

## State machine rules

- **Nested machines.** A state can itself be a state machine ("Combat" inside "Locomotion"). Its playback has its own path: `parameters/Locomotion/Combat/playback`. `travel()` on the outer playback reaches the inner machine's start state.
- **Advance conditions.** A transition with `advance_condition = "is_dead"` fires when the condition parameter is `true`. The path is under the state machine that owns the transition: `parameters/conditions/is_dead` when the machine is the `tree_root`, and `parameters/Locomotion/conditions/is_dead` for a machine named `Locomotion` inside a blend tree (tested on 4.7.2). Set it with `tree.set(path, true)`. Conditions are booleans only.
- **Advance expressions.** A transition's `advance_expression` (for example `velocity.length() > 0.1`) is evaluated against the node set as the tree's `advance_expression_base_node`. That node needs the named property; the tree reads it every tick, so the gameplay script does not push values.
- **Auto advance.** `advance_mode = ADVANCE_MODE_AUTO` with no condition moves on as soon as the current animation can end (combo chains, death to corpse). Do not use it for transitions that the player's input should decide.
- **`travel()` needs a path.** If no chain of transitions leads from the current state to the target, `travel()` does nothing (see the pitfalls table in SKILL.md). `start(state)` jumps without a path.

## Root motion

Root motion moves the character by the animation's root bone instead of by code, so feet do not slide on a walk cycle. The tree extracts the root bone's motion each tick and removes it from the pose. Your script applies it to the body.

1. Set the mixer's `root_motion_track` to the root bone's track, for example `NodePath("Armature/Skeleton3D:Root")` (relative to the mixer's `root_node`).
2. Run the tree on the physics tick: `callback_mode_process = AnimationMixer.ANIMATION_CALLBACK_MODE_PROCESS_PHYSICS`, because `move_and_slide()` runs there.
3. Each physics frame, turn the body by the root rotation, and move it by the root position as a velocity.

```gdscript
# root_motion_body.gd
extends CharacterBody3D

@export var tree: AnimationTree

var gravity: float = ProjectSettings.get_setting("physics/3d/default_gravity")


func _ready() -> void:
    tree.callback_mode_process = AnimationMixer.ANIMATION_CALLBACK_MODE_PROCESS_PHYSICS


func _physics_process(delta: float) -> void:
    # Rotation first: the position delta is expressed in the rotated frame.
    quaternion = quaternion * tree.get_root_motion_rotation()
    var motion := tree.get_root_motion_position()
    # The accumulator undoes the rotation already baked into this frame's motion.
    var world_motion := (tree.get_root_motion_rotation_accumulator().inverse() * quaternion) * motion
    var vertical := velocity.y
    velocity = world_motion / delta
    if not is_on_floor():
        velocity.y = vertical - gravity * delta
    move_and_slide()
```

- `get_root_motion_position()` is the motion **of this tick**, not a position. Divide by `delta` for a velocity, and let `move_and_slide()` handle walls and slopes. Writing `global_position += motion` walks through walls.
- Keep gravity in code. Most clips have no vertical root motion; the code above keeps the body's own vertical velocity off the floor.
- Clips that should not move the character (an idle with sway) must have a still root bone, or the character drifts. Check the import: root motion often needs "Root Motion" / the root bone set in the import dock so the root track exists.
- Mixing code movement and root motion: blend between `world_motion / delta` and the input velocity with a weight, rather than switching, or the speed jumps at the switch.
