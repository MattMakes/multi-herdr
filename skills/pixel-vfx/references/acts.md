# Acts: the character and the target

A hit effect alone looks like a sticker. An act puts a stand-in character behind it and a
target in front of it, so you judge the effect where it lives: in a wind-up, a hit and a
recovery. The act is data on the effect; `node pxfx.mjs act fx/<name>.mjs` renders it.

## The character

An original figure about 24 x 37 px: three-quarter view, facing right, six colours with a dark
outline, feet on y = 0 at the effect origin. Head, torso and boots are small sprites; arms and
legs are outlined lines solved with two-bone IK, so any hand position gives a bent elbow.
Shoulders sit about 25 px above the feet and arms reach about 15 px, so a long strike steps
the body in (`x`) rather than stretching the arm. Legs bend when the hips drop (`crouch`).

## Poses

A pose is a small object with only the fields that differ from `STAND`:

| Field | Meaning | STAND |
| --- | --- | --- |
| `x` | feet offset, px (+ forward); moves smoothly between keys | 0 |
| `crouch` | hips drop, px; knees bend | 0 |
| `lean` | torso and head shift at the top, px (+ forward) | 0 |
| `hx`, `hy` | near (front) hand, px from the feet, y negative up | 3, -12 |
| `bx`, `by` | far (back) hand, same | -3, -13 |
| `weapon` | `none`, `sword`, `staff`, `hammer`, `bow`, `gun` (held in the near hand) | `none` |
| `wa` | weapon angle, radians, 0 = forward, negative = up, clockwise | 1.2 |
| `ghost` | leave two dark after-images of the last drawings | false |

Starter poses: `READY` (guard up), `WINDUP_BACK` (coiled back, weapon behind), `STRIKE_FORWARD`
(stepped in, arm out), `CAST_UP` (hand raised), `CAST_FORWARD` (both hands out),
`CROUCH`, `HURT` (knocked back). Import them from `./pxact.mjs` and override fields:
`{ ...STRIKE_FORWARD, weapon: 'sword', wa: 0.4 }`. `check` fails on unknown fields (typos)
and unknown weapons.

## Act data

```js
export const act = {
  poses: [[t, pose, how], ...],           // sorted by effect time; negative t is the wind-up
  target: { x: 27 },                      // a training dummy at local x, or false
  hits: [{ t: HIT, freeze: 0.1, shake: 0.18, amp: 2, push: 5, flash: 2, tint: '#88ccff' }],
};
```

- `how` says how a key is reached: `smooth` (default), `snap` (hold the previous drawing,
  then jump on the key), `out` (fast, then settle), `in` (slow, then fast), `line`.
- Limbs move on whole 12 fps steps from the first key (held drawings, as hand animators
  do); the feet move smoothly. Put key times on that grid (`-1 / 12`, `7 / 12`).
- Hits: `t` from the effect's own constant, `freeze` 0..0.3 s, `shake` s and `amp` px for
  the camera, `push` px for the dummy, `flash` frames of white, `tint` a colour that stays
  on the dummy until a later hit sets another or `null`.
- Without `meta.timing`, timing is derived: windup = minus the first key's t, the hits,
  recovery = last key t minus last hit t. Sheets carry the timing and the act.
- `check` also fails a hand that moves more than 10 px in one 12 fps step unless a `snap`
  key lies in that step: a teleporting hand. Add a key between, or make it a deliberate snap.

## The target and the preview

The dummy is a post with a round straw body. A hit pushes its top away (the post bends), then
it springs back with a damped wobble; `flash` turns it white; `tint` recolours it (ice blue,
poison green, shock yellow, burn red). In the act preview everything holds during a hit's
freeze (effect, character, dummy); then the whole picture shakes `amp` px, decaying over
`shake` s. Labels show effect time, `F` marks frozen frames, yellow marks the hit frame.

## How to judge an act strip

Read it left to right as a flip book of consecutive frames. You should see: a ready pose; a
wind-up that moves AWAY from the blow and holds for a frame or two; one release drawing with a
big silhouette change and the effect's biggest frame; the freeze on contact with the dummy
white and pushed; follow-through past the end pose; a settle back. If you cannot point to
those frames, fix the keys before you touch the effect.

## How to make it sell

- Wind up opposite the blow: back before forward, down before up, crouch before a jump.
- Change the silhouette a lot on the release frame; reach it with `snap` and `ghost: true`.
- Heavier hits freeze longer (0.06 light, 0.1 sword, up to 0.2 heavy) and shake more.
- Follow-through overshoots the strike pose (lower, further, weapon past the target), then
  settles over 0.3 to 0.4 s.
- Put the hand where the effect starts: arc centres on the shoulder, muzzle
  flashes and casts at `o.actor.hand`.
- Arms reach about 15 px: step the body in with `x` for long strikes.
- On a swing the blade trails the wrist: keep `wa` behind for one key, then let it whip past.

## On-body effects

Status, spawn, death, teleport and aura effects set `meta.actor: true` and receive
`o.actor = { w, h, x, y, mask, pixels, head, chest, hand }`: the character's silhouette
(`mask[j * w + i]` is 1 where opaque) and its own colours (`pixels[k]` is `[r, g, b]`), in
local space, origin between the feet; `x, y` is the top-left; `head`, `chest`, `hand` are
points. It is a live object: in an act it changes every frame, so read it in `draw`, not in
`create`. The effect may export:

- `actorAlpha(t, o)`: 0..1, the character's opacity (spawn in, death, blink). Dithered.
- `actorPaint(p, t, o)`: draws the character itself, usually with
  `p.body(o.actor, (c, x, y) => newColour)` to recolour it (frozen, stone, poison, burn).

Outline the body with `p.outline(o.actor, o.actor.x, o.actor.y, c)` for auras and rims.
`render` draws the stock character under such effects by default; `sheet` does not (add
`--actor` for a preview sheet), and its JSON lists `actorAlpha` per frame. In a game, pass
your own sprite as `actor` in the options with the same fields.
