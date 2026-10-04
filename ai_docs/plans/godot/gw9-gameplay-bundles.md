# GW9 godot-gameplay-bundles: loops, combat, economy, quests

Follow `ai_docs/plans/godot/00-godot-conventions.md`. Own text only (the
sources are gd-agentic, LGPL).

## GOAL

Four bundles, as `ai_docs/reports/godot-wave.md` "gd-agentic only" says:

- `godot-gameplay-loops`: a router and 6 references, from game-loop-collection,
  game-loop-harvest, game-loop-time-trial, game-loop-waves, mechanic-revival,
  mechanic-secrets.
- `godot-combat-system`: real-time and turn-based, from combat-system and
  turn-system. Hitbox/hurtbox points to `godot-component-system`; do not
  repeat it.
- `godot-economy-system`, from economy-system.
- `godot-quest-system`, from quest-system.

## Method

Per bundle: read the gd-agentic skill and scripts; write SKILL.md (≤ 12 KB)
and references in your own words with your own typed 4.7 GDScript; point at
the combined `godot-*` skills for what they cover (inventory, save-load,
state machine, resource pattern, event bus) instead of repeating it; run
`api_check.py` and `gdscript_blocks_check.py`. Prove one non-trivial system
per bundle in a scratch project (`.worktrees/_scratch/godot-<role>/`): it
loads and a short headless script exercises it. Commit per bundle with the
lock. Provenance: `sources: []`, `vendored: false`, "own text, consulted
thedivergentai/gd-agentic-skills@4c4d0ff: <dirs>".

## FILES

own: the 4 skill directories, lock-held edits of the 2 index files,
`ai_docs/reports/godot/gw9-gameplay-bundles.md`.
