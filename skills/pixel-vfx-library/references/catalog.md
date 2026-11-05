# Catalog

Generated from the category pages. Kind: `loop` or `once`; `act` has an act (character, target, hits); `body` works on the character (`meta.actor`); `move` chains other effects. Options lists the options beyond `palette` and `seed`.

## Arcane (`references/arcane.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `arcane/portal` | loop | 2 | 72x80 |  | An upright oval portal: a dark eye with spiral arms turning once per loop, a rim lit from the upper left with a running glint, motes spiralling in. Pure loop. |
| `arcane/spell-charge` | once | 1.1 | 64x64 |  | Energy streaks converge on the hand while three orbit arcs tighten and spin up, then a white pop, a ring and sparks; exports `RELEASE` for moves. One-shot. |

## Critters (`references/critters.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `critters/bat` | loop | 0.6667 | 48x36 | `eye` | A small bat flapping in place at 12 fps: four wing poses with a lit leading edge and finger bones, glowing eyes, and a one-pixel bob with the downstroke. |
| `critters/slime` | loop | 1.2 | 48x48 |  | A green slime hopping in place: anticipation squash, stretch on launch, a parabolic hop over a shrinking shadow, a splat landing with droplets and a wobble, a white shine and a translucent core. |

## Explosions (`references/explosions.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `explosions/small-explosion` | once | 1.3 | 80x88 | `smoke` `ground` | A quick fiery pop: white flash, a noisy orange ball with an air ring, bouncing sparks, dark smoke rising behind it and thinning. |
| `explosions/smoke-bomb` | once | 3.2 | 144x84 |  | A white pop on the floor, then a grey cloud bursts out low and wide, rolls along the floor, hides the spot and clears slowly. |

## Fire (`references/fire.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `fire/torch` | loop | 1.6 | 56x76 | `wood` `metal` `smoke` | A wall torch loop: a tall flickering flame on a short wrapped stick, embers drifting up, a faint smoke wisp and a warm light pool. |
| `fire/flame-pillar` | once | 3 | 96x128 |  | A floor glyph draws itself and brightens, a column of fire bursts up with a white flash, roars, then collapses into smoke and embers. |
| `fire/fireball` | loop | 0.4 | 52x30 | `smoke` | A looping fireball flying right: a ringed hot head, noisy flame tongues streaming back, shed embers and a thin smoke trail. |

## Holy (`references/holy.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `holy/heal` | once+body | 1.6 | 64x64 | `gold` | On-body heal: green-gold motes rise around the body, a two-frame white flash, a gold rim and halo, the body tinted warm then fading back. One-shot, `meta.actor`. |
| `holy/holy-pillar` | once | 2.6 | 96x132 |  | A beam drops from the top edge onto the anchor, flashes white, widens with falling light streaks, throws three floor rings, then narrows away while feathers sink and motes drift. One-shot. |

## Ice (`references/ice.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `ice/frost-nova` | once+act | 2.2 | 128x62 |  | Acted: the caster rises and stamps; ice spikes erupt ring by ring along the floor with a white flash, shards spin off, frost lines stay, then it melts. |
| `ice/ice-shatter` | once | 2 | 120x76 | `w` `h` | A translucent ice block cracks with a white line, then bursts into tumbling, bouncing shards and a puff of cold mist (options w, h). |

## Impacts (`references/impacts.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `impacts/hit-spark` | once+act | 0.45 | 56x60 | `h` | Light hit: one white star frame, then a shrinking four-point star with dashes that fly off the rays, a few sparks that fall and bounce; acted as a quick jab. |
| `impacts/crit-hit` | once+act | 0.9 | 112x92 | `h` | Heavy hit: two full white frames (a jagged star and long rays), then a red-orange star that collapses slowly inside a fading ring, radiating speed lines, big bouncing sparks and a smoky afterglow; acted as a sword blow with a 0.2 s freeze. |

## Lightning (`references/lightning.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `lightning/lightning-strike` | once | 1.4 | 96x132 | `scorch` | A leader feels down from the top, a white forked bolt strikes and restrikes, cools and dithers out, leaving a scorch and crackling sparks. |
| `lightning/zap-hit` | once+act | 0.6 | 72x60 | `arc` | Acted: a palm thrust at the target; a white contact frame, a ring, arcs crawling over the body and grounding to the floor, sparks. |

## Melee (`references/melee.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `melee/sword-slash` | once+act | 0.6 | 104x64 | `spark` `arc` | Acted sword stroke: a wide elliptical crescent sweeps from over the back shoulder (passing behind the head) to the front, a white leading edge and a trail that shortens and thins; the dummy is hit at the end of the arc with sparks and a flare. Option `arc` [cx, cy, rx, ry, a0, a1, tilt] reshapes it; a negative ry gives a rising backhand, rx = ry an overhead. |
| `melee/hammer-smash` | once+act | 1.6 | 128x72 | `rock` `dust` `smear` | Acted overhead hammer blow into the floor: a smear arc and two white frames on the release, a long freeze and heavy shake, a floor shockwave in two halves, glowing cracks and a crater behind, tumbling rocks that kick dust where they land, sparks and rolling dust. Option `smear: false` drops the hammer smear for reuse under other weapons. |

## Moves (`references/moves.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `moves/fireball-throw` | once+act+move | 2 | 136x96 |  | Acted move: fire gathers in the drawn-back hand, a snapped throw with a white release frame, a fireball flies, small-explosion bursts on the target and leaves it burnt. |
| `moves/ice-lance-throw` | once+act+move | 2.6 | 168x104 |  | Acted move: a frost lance forms in the raised hand, is thrown like a javelin, encases the target in ice and the block shatters with ice-shatter shards. |
| `moves/sword-combo` | once+act+move | 3.2 | 136x84 |  | Acted three-stroke combo stepping forward: a high slash, a rising backhand and a heavy overhead into the floor, chained from melee/sword-slash (reshaped with `arc`) and the floor impact of melee/hammer-smash, with three freezes that grow heavier. |

## Nature (`references/nature.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `nature/poison-cloud` | once | 4 | 112x80 |  | A white-green pop on the floor, then a lit sickly cloud billows out and up from a venting floor, bubbles rise and pop inside it, and it thins away over four seconds. One-shot. |
| `nature/earth-spike` | once+act | 2 | 96x80 | `dust` | Acted: the caster raises an arm and slams the floor; under the target the floor cracks and a faceted rock spike bursts up with a white flash, rocks and rolling dust, holds, then breaks up and sinks. One-shot. |

## Pickups (`references/pickups.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `pickups/coin-spin` | loop | 1 | 32x32 |  | A gold coin spinning in place: the width squashes with the turn, a lit rim shows edge-on, an embossed face lit from the upper left, a sliding glint, a rim twinkle and a small bob. |
| `pickups/collect-burst` | once | 0.6 | 80x80 |  | A pickup is taken: a white flash, a ring that pops out and dithers away, eight twinkling stars thrown outward with drag, and a quick column of sparkles rising. |

## Projectiles (`references/projectiles.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `projectiles/magic-missile` | loop | 0.4 | 64x24 |  | Loop: a bright lens-shaped bolt with a white core and a pulsing glow, two strands braided around the flight line that widen and cool toward the tail, a core streak and sparkles shed backward. |
| `projectiles/muzzle-flash` | once | 0.8 | 72x44 | `smoke` | One-shot at a gun muzzle: a white star frame, a smaller hot star, an ember, a few fast forward sparks, a spent spark that pops up and falls behind, and a thin wisp of smoke that stretches forward and rises. |

## Sci-fi (`references/scifi.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `scifi/warp-in` | once+body | 1.4 | 64x72 |  | On-body spawn: a beam of scrolling scan lines drops onto a floor pad, a bright scan line draws the character in from head to feet with tinted, glitch-jolted rows (actorPaint; actorAlpha ramps 0 to 1 for sheets), a white body frame, then the beam lifts away and the pad ring spreads. |
| `scifi/force-field` | loop | 2 | 64x64 |  | Loop: a hex-patterned dome over the anchor; the far wall's hex lines sit behind the character, the near wall is light only, with a bright 1 px rim, cells that shimmer at random and a highlight band that travels across once per loop. |

## Shadow (`references/shadow.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `shadow/void-orb` | loop | 2 | 64x64 |  | A near-black sphere with a slow inner swirl, a thin violet rim and a running highlight; it squashes gently, pull rings contract onto it and motes spiral in. Pure loop. |
| `shadow/shadow-tendrils` | once | 2.2 | 96x72 |  | A dark pool spreads, tendrils whip up with a travelling wave, clench shut on a flash (the hit), squeeze, then curl back and sink; wisps leak up. One-shot. |

## Spawn (`references/spawn.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `spawn/poof` | once+body | 1.05 | 72x72 | `smoke` | On-body vanish: the body flashes white and is gone, a round cloud of white smoke puffs bursts out, stops hard, rises a little and thins, with a popping ring and gold stars that twinkle out. |
| `spawn/dissolve-burn` | once+body | 2 | 64x84 | `smoke` | On-body burn-away: a ragged, flickering edge climbs from the feet to the head; above it the body chars and glows white-hot, below it nothing remains; embers rise and cool from the edge, thin smoke curls up, the light follows the edge. |

## Status (`references/status.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `status/frozen` | loop+body | 2.4 | 60x60 |  | The body turned to ice by brightness with rime on its upper edges and a shine crossing once per loop, a frost outline with rime spikes, crystal clusters at the feet, flakes and floor mist. Loop. |
| `status/poisoned` | loop+body | 2 | 60x64 |  | The body pulses toward poison green in hard steps with a pulsing outline, bubbles rise off its sides and pop, and drops gather at the hand, fall and splat. Loop. |

## Transitions (`references/transitions.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `transitions/iris` | once | 1.6 | 320x180 | `cx` `cy` | A circle closes to black on a point (options `cx`, `cy`) with a dithered edge and a thin cool rim, holds shut while the scene swaps, then opens fast and settles. |
| `transitions/pixel-dissolve` | once | 1.2 | 320x180 | `cell` `sweep` | The screen goes black cell by cell in 16 ordered-dither steps along a slight diagonal sweep, the newest cells glint, it holds, then clears in reverse (options `cell`, `sweep`). |

## UI (`references/ui.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `ui/damage-number` | once | 1 | 88x60 | `text` | A damage number (option `text`) pops in at double size and white with burst ticks, drops to size with an overshoot, floats up, cools down the ramp and fades. |
| `ui/level-up` | once+act+body | 2.4 | 96x108 | `text` | On the body: motes gather, the body flashes white, a floor ring and a column of light burst up with stars spiralling round the body, the letters of `text` jump in one by one, a shine runs across them, then they rise and fade. |

## Water (`references/water.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `water/splash` | once | 1.6 | 112x76 |  | Something drops into water: a crown of spikes, droplets breaking off its tips and falling back, a centre jet and spreading ripple rings. |
| `water/bubbles` | loop | 2 | 48x60 |  | A loop of bubbles of mixed sizes wobbling upward and popping at a wavy surface line with tiny splashes. |

## Weather (`references/weather.md`)

| id | kind | s | size | options | description |
| --- | --- | --- | --- | --- | --- |
| `weather/rain` | loop | 1 | 320x180 | `wind` `far` `near` | Full-screen rain loop: diagonal streaks in two depths wrap across the screen, near drops land lower and splash a crown and a ripple on the floor, a cool mist light breathes along the floor line. |
| `weather/snow` | loop | 4 | 320x180 | `wind` | Full-screen snow loop: flakes in three sizes and depths drift down with sway and wind, the near ones twinkle and glow, flakes melt where they land, and a soft blended haze breathes along the floor. |
