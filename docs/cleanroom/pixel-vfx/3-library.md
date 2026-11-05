# Clean-room spec 3: the `pixel-vfx-library` skill (example effects)

Written by the person who read the reference project. You have not read it and
must not. Design every effect yourself from the one-line briefs below and your
own knowledge. Read `1-skill-and-tool.md` and `2-acts-moves-previews.md` for context,
and read the finished `skills/pixel-vfx/` skill in the repo: you write effects
against ITS tool (`pxfx.mjs`, and `pxact.mjs` if it was split) and to ITS
contract and quality bar.

## What the library is

A second bundled skill, `skills/pixel-vfx-library/`, holding original example
effects as code blocks in Markdown, one page per category:
`references/<category>.md`. The `pixel-vfx` tool searches and extracts them
(`node pxfx.mjs lib <library-dir> list ...` and `get <id>`). Workers reuse them
with new options (palette, seed, flip, size) and use them as technique
references and as the quality benchmark for new effects.

Budget: the whole `skills/pixel-vfx-library/` dir <= 163840 bytes, `.md` only.
About 43 effects, so aim for ~2.5 KB of code per effect (hard cap 3.5 KB) and a
one-line description each. Compact code is fine; clear names, few comments.

## Page format (exact; the tool parses it)

```
# <Category title>

<one or two sentences on the category>

## <category>/<name>

<one-line description: what it is, how it moves>

```js
// id: <category>/<name>
import { ... } from './pxfx.mjs';
export const meta = { name, tags: [...], size, origin, duration, loop, fps, options: { palette, seed, ... } };
...
```
```

Library effects import the tool as `./pxfx.mjs` (and `./pxact.mjs` if the tool
was split) and other library effects as `./<category>-<name>.mjs`.
Every effect passes `check`; anything with a character passes `act`.
Every melee, impact and move effect has an `act` (and therefore timing).
Every status effect uses `meta.actor: true` and `o.actor`.

## Process for each effect (the skill's own method; do it, don't skip it)

1. Write the brief as a one-line art direction.
2. Draft; render (`render --gif`), open the contact sheet with the Read tool.
3. Fix the worst thing; at least 3 rounds; `act` strip for acted effects.
4. `check` until clean. Keep the code within the size cap.
Work in a scratch folder, then paste the final code into the page and
re-extract it with `lib get` to prove the page round-trips and passes `check`.

## The effects (category, name, kind, brief)

Kinds: L = pure loop, O = one-shot (simulated or pure), A = has an act,
B = on-body (`o.actor`).

fire
- fire/torch (L): a wall torch: a tall flickering flame on a short stick, embers drifting up, a warm light pool.
- fire/flame-pillar (O): a column of fire bursts up from a floor glyph, roars for a moment, collapses into smoke and embers.

explosions
- explosions/small-explosion (O): a quick fiery pop: white flash, orange ball, sparks, a puff of dark smoke that rises and thins.
- explosions/smoke-bomb (O): a grey cloud bursts out low and wide, rolls along the floor, hides the spot, then clears slowly.

water
- water/splash (O): something drops into water: a crown of droplets, a ring of ripples, a few drops falling back.
- water/bubbles (L): bubbles of mixed sizes wobble upward and pop at a surface line.

ice
- ice/frost-nova (O, A): a caster stamps; ice spikes erupt in a ring along the floor outward, a pale flash at the centre, shards spin away, frost stays then melts.
- ice/ice-shatter (O): a block of ice cracks with a white line, then breaks into tumbling shards and a puff of cold mist.

lightning
- lightning/lightning-strike (O): a jagged bolt drops from the top of the frame, forks, flashes white, leaves a scorch and crackling sparks.
- lightning/zap-hit (O, A): a short electric hit on the target: a crackle of small arcs, a yellow flash, sparks.

scifi
- scifi/warp-in (O, B): a beam of scan lines draws the character in from top to bottom with a bright line and glitch offsets (uses actorAlpha).
- scifi/force-field (L): a hex-patterned dome shimmers around the anchor, with a bright rim and a slow travelling highlight.

melee
- melee/sword-slash (O, A): a fast horizontal crescent with a white leading edge and a fading trail; the target gets hit at the end of the arc.
- melee/hammer-smash (O, A): an overhead hammer blow into the floor: impact flash, cracks, rocks and dust, heavy freeze and shake.

impacts
- impacts/hit-spark (O): a small star-shaped spark burst with a 1-frame white core, for any light hit.
- impacts/crit-hit (O, A): a heavy hit: a full white impact frame, radiating lines, big sparks, a red-orange flare, long freeze.

projectiles
- projectiles/magic-missile (L): a glowing bolt with a sparkling tail, flying right (the move or game moves it).
- projectiles/muzzle-flash (O): a gun muzzle: a 2-3 frame star flash, a thin smoke wisp, a spent spark.

spawn
- spawn/poof (O, B): the character vanishes in a round puff of white smoke and little stars (actorAlpha to 0).
- spawn/dissolve-burn (O, B): the body burns away from the feet up with a glowing edge and embers, leaving nothing.

arcane
- arcane/portal (L): an oval portal with a swirling purple interior, a bright rim and motes drifting in.
- arcane/spell-charge (O): energy gathers inward to the hand (streaks), a tightening glow, then a small release pop.

holy
- holy/heal (O, B): green-gold motes rise around the body, a soft halo flash, the body tints warm briefly.
- holy/holy-pillar (O): a beam of light drops on the anchor, widens, rings spread on the floor, feathers and motes drift.

shadow
- shadow/void-orb (L): a dark sphere with a thin violet rim that pulls in purple motes and wobbles.
- shadow/shadow-tendrils (O): black tendrils rise from a dark puddle, grasp, and sink back.

status
- status/frozen (L, B): the body recoloured icy blue with a frost outline, small crystals on it, flakes drifting.
- status/poisoned (L, B): green tint pulsing on the body, bubbles rising off it, a drip falling now and then.

nature
- nature/poison-cloud (O): a sickly green cloud swells from the floor, lingers with bubbles, thins away.
- nature/earth-spike (O, A): a stone spike bursts up under the target with rubble and dust, then crumbles.

weather (full screen 320x180, loops)
- weather/rain (L): diagonal rain streaks in two depths, splashes on the floor line.
- weather/snow (L): flakes of three sizes drifting and swaying, a light ground haze.

ui
- ui/damage-number (O): a pixel-font number pops up from the hit point, overshoots, floats up and fades (option `text`).
- ui/level-up (O, B): "LEVEL UP" text bursts in over the character, a rising column of light and stars around the body.

pickups
- pickups/coin-spin (L): a gold coin spinning in place (squash width over time), a glint passing.
- pickups/collect-burst (O): a pickup is taken: a ring pops, stars burst outward, a quick upward sparkle.

transitions (full screen 320x180)
- transitions/iris (O): a circle closes to black on a point, holds, then opens (dither edge).
- transitions/pixel-dissolve (O): the screen dissolves to black in ordered-dither steps, then back.

critters (loops)
- critters/bat (L): a small bat flapping in place, wings in 4 poses, slight bob.
- critters/slime (L): a green slime that squashes and stretches as it hops in place, with a shine.

moves (chains of the effects above, all A)
- moves/fireball-throw (O, A): the character gathers fire in the hand (a fire charge), throws; a fireball flies to the target; it bursts with small-explosion; the target is tinted burnt. Write a fire/fireball (L) projectile on the fire page for it.
- moves/sword-combo (O, A): three strokes stepping forward: a slash, a reverse slash, and a heavy overhead into the floor, reusing melee/sword-slash and melee/hammer-smash parts.
- moves/ice-lance-throw (O, A): a frost lance forms in the hand, is thrown like a javelin, and the target ices over with ice-shatter flakes.

So 43 effects plus fire/fireball = 44.

## Splitting the work

Several writers work in parallel, each owning whole category pages. Moves are
written last by whoever owns them, after the effects they chain exist.

## SKILL.md and the catalog (written at the end, by one writer)

- `SKILL.md`: frontmatter `name: pixel-vfx-library`, a `description` (<= 1024
  bytes, "Use when ..." trigger: reusing or starting from a ready-made pixel
  effect). Body: what is in the library, how to find (`lib list`), get
  (`lib get`) and reuse with options (palette, seed, flip, angle, size, text),
  how to use an effect as a technique reference and as the quality benchmark,
  and a short table of categories with their effects. It needs the `pixel-vfx`
  skill's tool; say so.
- `references/catalog.md`: every effect: id, kind, duration, size, one-line
  description, generated from the pages (write a tiny script in the scratch
  folder to build it; do not hand-type it).

## Acceptance

- Every effect extracted with the tool's `lib get` from the final pages passes
  `check`; acted ones pass `act`. Report the check summary per effect.
- You looked at every effect's contact sheet (and act strip where acted)
  and iterated at least 3 rounds on each; report one line per effect on what it
  looks like and its remaining weakness.
- Sizes within budget; `.md` only; no third-party names; no PENDING-colon
  lines; no spec-todo token.
