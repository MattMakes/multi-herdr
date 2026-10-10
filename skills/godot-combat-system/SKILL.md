---
name: godot-combat-system
description: "Use when building combat in Godot 4.7 with GDScript, real-time or turn-based. Real-time: a typed damage payload with damage-type flags, a resolver for defense, resistances and critical hits, active attack frames, one hit per target per swing, hit-stop, knockback, death cleanup and server-side damage. Turn-based: round and initiative queues with deterministic ties, action points, phases, ATB gauges, CTB timelines with a turn-order preview, grid tactics on AStarGrid2D, and turn timeouts online. Hitbox, hurtbox and health nodes are in godot-component-system; abilities and cooldowns are in godot-ability-system."
---

# Godot combat system

Target engine: **Godot 4.7**. Every code block is typed GDScript that parses
on 4.7.2. Every engine API it names is in the 4.7.2 `--doctool` dump.

## The damage pipeline

Real-time and turn-based combat share one pipeline. Keep each step in its
own place:

| Step | Owner | Skill |
| --- | --- | --- |
| 1. Choose the attack (input, AI, menu) | the actor's state machine | `godot-state-machine`, `godot-input-handling` |
| 2. Pay its cost, start its cooldown | the ability | `godot-ability-system` |
| 3. Find the targets (overlap, ray, grid cell, menu pick) | hitbox, or the turn system | `godot-component-system`, this skill |
| 4. Build the hit: a `DamageInfo` | the attacker | this skill |
| 5. Resolve it against the target: defense, resistance, critical hit | a resolver function | this skill |
| 6. Apply the final number, with i-frames | hurtbox and health | `godot-component-system` |
| 7. React: hit-stop, knockback, flash, numbers, sound | listeners of the health signals | this skill, `godot-particles-vfx`, `godot-audio-system` |

Rules for the whole pipeline:

- **Never write `target.health -= n`.** All damage goes through steps 4 to 6,
  so armor, resistances and i-frames always apply.
- **A hit is a typed object, not a number.** `DamageInfo` carries the amount,
  the damage-type flags, the source, the knockback and the critical-hit flag.
  A bare `int` loses all of them.
- **Damage types are flags, not strings.** An `enum` with power-of-two values
  and `@export_flags` gives a checked, combinable type. A string such as
  `"fire"` fails silently on a typo.
- **Stats are resources, duplicated per actor.** A shared `.tres` gives every
  enemy one health pool. Call `duplicate_deep()` (4.5 and later) when an actor
  spawns (`godot-resource-pattern`).
- **UI listens.** Health bars and damage numbers connect to signals. Combat
  code never calls the HUD.
- **No exceptions in GDScript.** Check a target with `is` or
  `has_method(&"...")` before you call it.

## Real-time combat

Read [references/real-time.md](references/real-time.md) for the code. The
rules:

- **Active frames.** A hitbox monitors only while the swing can hit. Key its
  `monitoring` property (or the shape's `disabled`) in the attack animation
  with an `AnimationPlayer` property track (`godot-animation-system`). A
  hitbox that is always on hits on the wind-up and the recovery.
- **One hit per target per swing.** Overlap signals can fire again for the
  same target in one swing. Keep a set of targets hit in this swing, and
  clear it when the swing starts.
- **Collision layers filter hits.** Put player hurtboxes and enemy hurtboxes
  on different layers. The player's hitbox mask sees only the enemy hurtbox
  layer. Groups are labels, not hit filters.
- **Hit-stop.** Freeze the game for 40 to 100 ms on a heavy hit:
  lower `Engine.time_scale` and restore it with a timer that ignores the time
  scale. Never `OS.delay_msec()`: it blocks the main thread, input and audio.
- **Knockback in the physics step.** Add it to `velocity` on a
  `CharacterBody2D` or `CharacterBody3D` and let it decay in
  `_physics_process`. For a `RigidBody`, use `apply_central_impulse` in the
  physics step.
- **Death.** On `died`, turn off the hurtbox and the body shape with
  `set_deferred(&"disabled", true)` in the same frame, then play the death
  animation, then free. A dead body with live shapes blocks attacks and
  paths.
- **Damage numbers.** Pool the labels (`godot-optimization`). One new
  `Label` per hit fragments memory in a busy fight.

## Turn-based combat

Read [references/turn-based.md](references/turn-based.md) for the code.
Pick the turn model first:

| The game has... | Model |
| --- | --- |
| rounds; each actor acts once per round in speed order (classic JRPG, tactics) | round queue |
| a visible order where fast actors act more often (CTB, FFX-style) | timeline |
| gauges that fill in real time; an actor acts when its gauge is full (ATB) | gauge |
| a fixed order and a pool of points per turn (XCOM, card games) | action points with phases |

The rules:

- **Deterministic order.** Sort by speed, and break a tie with a fixed key
  (a stat, then a stable actor id). A random tie-break breaks replays and
  tests. Use a seeded `RandomNumberGenerator` for any roll.
- **Sort when the order can change**, not before every action: at the round
  start, and when a speed stat changes.
- **Never change the queue while you loop over it.** A death during a round
  marks the actor dead; the queue skips dead actors.
- **Check before you spend.** `can_afford(cost)` before you take action
  points.
- **Clean up, then signal.** Tick status effects and reset action points
  before `turn_ended`; a listener that starts the next turn sees the final
  state.
- **Wait for decisions with signals and `await`.** Never a `while` loop.
  Discrete menu input uses `is_action_just_pressed`, or `_unhandled_input`
  with `event.is_action_pressed`.
- **Phases are an `enum` with `match`**, or states (`godot-state-machine`).
- **Grids use `AStarGrid2D`.** Call `update()` after you change `region`,
  `cell_size` or other grid properties (`is_dirty()` is then `true`). That
  `update()` rebuilds the grid and clears every solid point and weight, so
  set walls after it. `set_point_solid()` needs no `update()`. (Measured on
  4.7.2.)
- **Online: the server owns the turn clock.** It starts a timer on each turn
  and takes a default action on timeout. Clients only draw the countdown.

## Other skills own these parts

| Need | Skill |
| --- | --- |
| `HitboxComponent`, `HurtboxComponent`, `HealthComponent`, i-frames | `godot-component-system` |
| Abilities, costs, cooldowns, buffs, stat modifiers | `godot-ability-system` |
| Actor states (idle, attack, stagger, dead), turn phases | `godot-state-machine` |
| Input buffers for combos and dodges | `godot-input-handling` |
| Stat and attack data as resources | `godot-resource-pattern` |
| Overlap queries, rays and layers | `godot-physics-system` |
| Attack animations and their tracks | `godot-animation-system` |
| Health bars, damage numbers, turn-order bars | `godot-hud-system` |
| Server-side damage and lag compensation | `godot-multiplayer-sync` |
| Enemy decisions | `godot-ai-navigation`, `godot-limboai`, `godot-beehave` |
| Waves of enemies | `godot-gameplay-loops` |
| Gold and loot from a kill | `godot-economy-system` |

## Prove it

> proof: headless-run: `scripts/godot/gameplay_scenarios.py` runs the checks of both references in the gate. Feel: proof: not run (needs a human play test).

Combat math is plain code: test it headless. Build the resolver, the queue
or the timeline in a `SceneTree` script with fixed stats and a seeded
`RandomNumberGenerator`, check exact numbers, and `quit(1)` on a mismatch.
`godot-build-verify` runs the script and the project parse check. Feel
(hit-stop length, knockback) needs a human play test: list it as not
checked in `DONE:`.
