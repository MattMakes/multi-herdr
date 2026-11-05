---
name: pixel-vfx-library
description: "Use when a game needs a pixel-art effect and a ready-made one could serve or start it: fire, explosions, water, ice, lightning, sci-fi, melee slashes and smashes, impacts, projectiles, spawns, arcane, holy, shadow, status effects on a character, nature, weather, UI pops, pickups, screen transitions, critters, and acted moves (gather, throw, hit). Holds 44 original effects as code pages with a catalog. Search them, extract one with its parts, recolour or resize it with options, study it as a technique reference, and compare your own draft against the best one in its category as the quality bar. Needs the pixel-vfx skill's tool."
---

# Pixel VFX library

Original pixel-art effects, each a complete effect module that passes the `pixel-vfx` tool's
`check` (and `act` when it has an act). One page per category in `references/`; every effect
has an id `<category>/<name>`, a one-line description and its code. `references/catalog.md`
lists them all with kind, duration, size and extra options.

This skill needs the `pixel-vfx` skill: copy its `references/script-pxfx.md` and
`references/script-pxact.md` blocks into `pxfx.mjs` and `pxact.mjs` first. Its contract,
quality bar and workflow apply to everything here.

## Find, get, reuse

```sh
node pxfx.mjs lib <this skill>/references list               # every effect
node pxfx.mjs lib <this skill>/references list ice           # all words must match id, tags or description
node pxfx.mjs lib <this skill>/references get ice/frost-nova --to fx
node pxfx.mjs check fx/ice-frost-nova.mjs && node pxfx.mjs act fx/ice-frost-nova.mjs
node pxfx.mjs render fx/ice-frost-nova.mjs --opts '{"palette":"poison","seed":9}' --gif
```

- `get` writes `<to>/<category>-<name>.mjs` (default `fx/`) and points its `./pxfx.mjs` and
  `./pxact.mjs` imports at the tool (`--tool <path>`, default the running `pxfx.mjs`).
- Moves import their parts (`./explosions-small-explosion.mjs`, ...); `get` brings the parts
  along, recursively. Get one id or several in one call.
- Reuse before you write: options first (`palette` takes a ramp name or a hex list; `seed`
  gives a new variant; `flip`, `angle`, `x`, `y`, `light` place it), then copy and edit.
  Change `meta.size`/`origin` only together with the code that draws to the edges.
- Kinds in `list`: `loop` or `once`, `+act` (acted: character, target, hits; timing comes
  from the act), `+body` (`meta.actor`: works on the character's body), `+move` (a chain).

## Options the writers added

- `melee/sword-slash`: `arc: [cx, cy, rx, ry, a0, a1, tilt]` reshapes the crescent (a negative
  `ry` gives a rising backhand, `rx = ry` an overhead). Exports `SWING` and `HIT` for moves.
- `melee/hammer-smash`: `smear` (default true) draws the swing arc; set `false` when a move
  uses only the floor impact. Exports `HIT` and `IX` (the head meets the floor at `(IX, 0)`).
- `impacts/hit-spark`, `impacts/crit-hit`: local (0, 0) is the floor under the target; the
  burst sits `h` px up (default 22).
- `ice/ice-shatter`: `w`, `h`, the block size. `explosions/small-explosion`: `ground`, the
  floor's y below the origin where sparks bounce. `arcane/spell-charge` exports `RELEASE`.
- `transitions/iris`: `cx`, `cy`, where the circle closes. `transitions/pixel-dissolve`:
  `cell` (px per cell) and `sweep` (how much the dissolve leans along a diagonal).
- `ui/damage-number`: `text`, the string to pop (digits, A-Z and `+ - ! ? . , : / % x`).

## Technique reference and quality benchmark

Read the effect closest to your task before drafting: the pages show how flames scroll, how
rings split into back and front halves, how acts key a wind-up and a snapped release, how
on-body effects recolour `o.actor`, and how moves time their cues from exported constants.
Borrow techniques, not designs.

The library is also the bar. Before you call your own effect done, put it next to the best
effect of the same category at the same size:

```sh
node pxfx.mjs compare fx/<yours>.mjs fx/<category>-<best>.mjs
```

If the library one has more detail, light or life, yours is not done.

## Categories

- `arcane`: portal, spell-charge
- `critters`: bat, slime
- `explosions`: small-explosion, smoke-bomb
- `fire`: torch, flame-pillar, fireball
- `holy`: heal, holy-pillar
- `ice`: frost-nova, ice-shatter
- `impacts`: hit-spark, crit-hit
- `lightning`: lightning-strike, zap-hit
- `melee`: sword-slash, hammer-smash
- `moves`: fireball-throw, ice-lance-throw, sword-combo
- `nature`: poison-cloud, earth-spike
- `pickups`: coin-spin, collect-burst
- `projectiles`: magic-missile, muzzle-flash
- `scifi`: warp-in, force-field
- `shadow`: void-orb, shadow-tendrils
- `spawn`: poof, dissolve-burn
- `status`: frozen, poisoned
- `transitions`: iris, pixel-dissolve
- `ui`: damage-number, level-up
- `water`: splash, bubbles
- `weather`: rain, snow

Full list with kinds, durations, sizes and options: `references/catalog.md`.
