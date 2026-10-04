# Action RPG

Real-time combat feeds loot, loot feeds a build, and the build lets the
player beat harder areas. Diablo, Path of Exile and the Souls games are the
reference points.

## Core loop

Fight → collect drops → equip and level up → enter a harder area → fight.
The loop works only when each pass makes the character visibly stronger and
the next area pushes back.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Melee and projectile hits | hitbox/hurtbox areas, i-frames, stagger | `godot-combat-system`, `godot-component-system` |
| Stats and modifiers | base stats, flat and percent modifiers, level curve | `godot-ability-system` |
| Skills with cooldown and mana | hotbar, cost, area of effect | `godot-ability-system` |
| Loot and equipment | item Resources, affixes, gear slots | `godot-inventory-system`, `godot-resource-pattern` |
| Enemy and boss AI | chase, telegraphed attacks, phases | `godot-state-machine`, `godot-ai-navigation` |
| Gold and vendors | sinks and sources | `godot-economy-system` |
| Quests | kill and fetch goals | `godot-quest-system` |
| Persistence | character, stash, unlocked areas | `godot-save-load` |
| Damage numbers and juice | pooled labels, hit flash, shake | `godot-tween-animation`, `godot-camera-system` |

## Scene tree (4.7)

```text
World (Node2D or Node3D)
├── Level (TileMapLayer layers, or a GridMap in 3D)
├── Player (CharacterBody2D)
│   ├── Visuals (AnimatedSprite2D) + AnimationTree
│   ├── Hurtbox (Area2D)
│   ├── WeaponPivot (Node2D) → Hitbox (Area2D, monitoring off between swings)
│   ├── Stats (Node; holds a duplicated CharacterStats Resource)
│   └── AbilityRunner (Node)
├── Enemies (Node2D; each enemy reuses Hurtbox, Hitbox, Stats)
├── Drops (Node2D; pooled loot pickups)
└── HUD (CanvasLayer; listens to Stats signals only)
```

## Genre code

Two formulas carry the build fantasy: armor with diminishing returns, and a
loot roll where rarity decides the number of affixes. Keep both in a pure
helper so tests can run them with a fixed seed.

```gdscript
extends RefCounted

const RARITY_WEIGHTS := {&"common": 70, &"magic": 22, &"rare": 7, &"unique": 1}
const AFFIX_COUNT := {&"common": 0, &"magic": 2, &"rare": 4, &"unique": 0}

# Fraction of damage removed. Approaches 1.0 but never reaches it.
static func armor_reduction(armor: float, attacker_level: int) -> float:
	var k := 50.0 + 10.0 * attacker_level
	return armor / (armor + k)

static func roll_rarity(rng: RandomNumberGenerator) -> StringName:
	var total := 0
	for w in RARITY_WEIGHTS.values():
		total += w
	var pick := rng.randi_range(1, total)
	for key in RARITY_WEIGHTS:
		pick -= RARITY_WEIGHTS[key]
		if pick <= 0:
			return key
	return &"common"
```

Scale enemy health and damage by area level with a power curve (for
example `base * pow(1.12, area_level)`). A linear curve makes late gear
feel worthless.

## Pitfalls

- A stat Resource shared by every goblin: one hit damages all of them.
  Call `duplicate(true)` when the enemy spawns, or mark the sub-resource
  `resource_local_to_scene`.
- Armor that adds up linearly reaches 100% and makes the player immune.
  Use the diminishing formula above.
- Hits resolved in `_process()` depend on frame rate. Turn hitboxes on and
  off from the animation and resolve them in `_physics_process()`.
- No stagger: hits with no reaction feel weightless. Give large hits a
  0.2 to 0.5 s hit-recovery state.
- Loot that looks the same at every rarity. Give each rarity its own colour
  and sound.
- Damage numbers created and freed each hit cause hitches in large fights.
  Pool them.
- The HUD reads the inventory from its own child nodes. The inventory
  Resource is the truth; the HUD redraws on its signals.
- Stats recomputed every frame. Recompute on equip, level-up and buff change.
