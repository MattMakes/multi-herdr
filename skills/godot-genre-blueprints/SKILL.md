---
name: godot-genre-blueprints
description: "Use when starting or planning a Godot 4.7 game of a known genre, or when a task asks what systems, scene layout or pitfalls a genre needs. A router: pick 1 of 27 genres (action RPG, battle royale, card game, educational, fighting, horror, idle/clicker, metroidvania, MOBA, open world, party, platformer, puzzle, racing, rhythm, roguelike, romance, RTS, sandbox, third-person shooter, FPS, simulation/tycoon, sports, stealth, survival, tower defense, visual novel), then read that one reference for its core loop, required systems mapped to the godot-* skills that teach them, a 4.7 scene tree sketch, genre-only GDScript, and pitfalls. Also holds a project-template reference for the folder layout, autoload order, pause modes, feature tags and DLC packs of a new project."
---

# Godot genre blueprints

A genre decides which systems a game needs before any code exists. This
skill maps a genre to those systems and to the `godot-*` skill that teaches
each one. It does not teach the systems itself.

## How to use it

1. Read `.agents/godot-project-context.md` first. If it is missing, send
   `QUESTION:` to the orchestrator and ask for `godot-tech-lead`
   (`godot-project-context` writes the file).
2. Pick one genre from the table below. Read only its reference file.
   Do not read the whole `references/` folder.
3. For a hybrid, pick the genre that owns the core loop as the primary.
   Read at most one second reference, for the systems the primary lacks
   (for example `metroidvania.md` with `roguelike.md` for a roguelite
   metroidvania).
4. If the genre is not clear from the task or the project context, send one
   `QUESTION:` to the orchestrator with your proposed genre and why. Do not
   guess silently.
5. For each required system, open the named skill only when you build that
   system.
6. For a new project, also read `references/project-templates.md`.

Each reference has the same parts: core loop, required systems (a table
with the skill for each), a scene tree sketch for 4.7, one short GDScript
block for the one thing no other skill covers, and pitfalls.

## Pick a genre

| genre | reference | pick this when |
|---|---|---|
| Action RPG | `references/action-rpg.md` | real-time combat drives loot, stats and builds (Diablo, Souls-like) |
| Battle royale | `references/battle-royale.md` | many online players, one map, a shrinking zone, last one standing |
| Card game | `references/card-game.md` | the player draws and plays cards against a board (CCG, deckbuilder) |
| Educational | `references/educational.md` | the goal is that the player learns a real skill or facts |
| Fighting | `references/fighting.md` | 1v1 fights built on frame data, combos and inputs |
| Horror | `references/horror.md` | dread, scarcity and a stalking threat are the point |
| Idle / clicker | `references/idle-clicker.md` | numbers grow without input; prestige resets (incremental) |
| Metroidvania | `references/metroidvania.md` | one connected world opened by new abilities and backtracking |
| MOBA | `references/moba.md` | team heroes push lanes of minions against towers |
| Open world | `references/open-world.md` | one large streamed map with free travel and many points of interest |
| Party | `references/party.md` | 2 to 4 local players play short minigames for points |
| Platformer | `references/platformer.md` | the main verb is jumping with precise control (side view) |
| Puzzle | `references/puzzle.md` | the player solves logic or grid puzzles; undo is required |
| Racing | `references/racing.md` | vehicles race on tracks against rivals or the clock |
| Rhythm | `references/rhythm.md` | actions are judged against the beat of music |
| Roguelike / roguelite | `references/roguelike.md` | generated runs, permadeath, and optional meta unlocks |
| Romance / dating sim | `references/romance.md` | relationships with characters, schedules and routes drive the game |
| Real-time strategy | `references/rts.md` | the player commands many units, gathers and builds in real time |
| Sandbox | `references/sandbox.md` | the player edits the world (voxels, falling sand, physics toys) |
| Third-person shooter | `references/shooter.md` | over-the-shoulder gunplay, cover and aim assist |
| First-person shooter | `references/shooter-fps.md` | first-person gunplay, mouse look and a weapon view model |
| Simulation / tycoon | `references/simulation.md` | the player builds and manages a system (park, city, factory) |
| Sports | `references/sports.md` | a ball, teams, a referee and a match clock |
| Stealth | `references/stealth.md` | avoiding detection by vision, light and sound is the core |
| Survival / crafting | `references/survival.md` | needs, gathering, crafting and building keep the player alive |
| Tower defense | `references/tower-defense.md` | waves follow a path and the player places towers |
| Visual novel | `references/visual-novel.md` | text, portraits and choices that branch a story |
| (new project) | `references/project-templates.md` | laying out folders, autoloads and pause before genre work |

## Rules that hold in every genre

These come up in nearly every reference. The references do not repeat them
in full.

- **Data in Resources, state per instance.** Define enemies, items, cards,
  waves and moves as Resources (`godot-resource-pattern`). A Resource loaded
  from disk is shared: call `duplicate(true)` before one instance changes
  it, or keep runtime values in the instance.
- **The model is the truth; nodes and UI follow.** Inventory, grid, score
  and flags live in data. The HUD and the scene redraw from signals
  (`godot-event-bus`). Never read game state back from labels or node
  order.
- **Physics and timing in `_physics_process()`.** Hits, movement and fixed
  simulation steps run there. Input that needs exact timing is read in
  `_input()` or `_unhandled_input()`.
- **No heavy work on the main thread.** Load scenes with
  `ResourceLoader.load_threaded_request()`; run generation and big
  calculations in `WorkerThreadPool` tasks (`godot-multithreading`). Only
  the main thread changes the scene tree: apply results with
  `call_deferred()`.
- **Pool what spawns often.** Projectiles, damage numbers, notes, loot.
- **Seeded randomness.** Use your own `RandomNumberGenerator` with a seed
  for anything a test, a replay or a bug report must reproduce.
- **Integers for discrete values.** Money, cards, costs and counts are
  `int`. Never compare floats with `==`.
- **The server decides in online games.** Clients send intents; the server
  validates and applies (`godot-multiplayer-basics`,
  `godot-multiplayer-sync`).
- **`StringName` for hot identifiers.** State names, flags and action names
  that are compared often: `&"idle"`.

## Proving the work

A genre system is done when it runs, not when it parses. Follow
`godot-build-verify`: headless import, the project-wide parse check, the
tests, and a smoke run of the main scene. Edit scenes as text only by the
rules of `godot-scene-files`. Name the Godot version and the results in
`DONE:`.

## Version notes

Everything here targets Godot 4.7. Typed dictionaries
(`Dictionary[StringName, int]`), used in several code blocks, need 4.4 or
later. Use `TileMapLayer` nodes, not the old `TileMap`. If the project uses
an older 4.x, say in `DONE:` which advice may not apply
(`godot-version-migration`).
