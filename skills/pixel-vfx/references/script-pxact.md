# Script: pxact.mjs

Usage: no command of its own; `node pxfx.mjs act <fx.mjs>`, `render --actor` and `check` load it, and effects import poses and `chain` from it.

`pxact.mjs` holds the stand-in character (an original figure about 24 x 37 px, facing right,
built from part sprites and two-bone arms and legs), the starter poses, the act runner with a
training dummy, freezes and shake, the act checks, and `chain` for moves. It imports
`./pxfx.mjs`, so the two files must sit in the same folder. Copy the block into a file and run
it: save it as `pxact.mjs` next to `pxfx.mjs` and use the `pxfx.mjs` commands. Fields and
advice are in `references/acts.md` and `references/moves.md`.

| Export | What it does |
| --- | --- |
| `STAND`, `READY`, `WINDUP_BACK`, `STRIKE_FORWARD`, `CAST_UP`, `CAST_FORWARD`, `CROUCH`, `HURT` | Starter poses. Each lists only what differs from `STAND`; spread and override: `{ ...WINDUP_BACK, weapon: 'sword', wa: -2.8 }`. |
| `WEAPONS`, `PAL` | `'none', 'sword', 'staff', 'hammer', 'bow', 'gun'`; the colours by key (character: `o k h t d b`). |
| `rig(pose)` | The character as an actor `{ w, h, x, y, mask, pixels, head, chest, hand }`, feet at `(pose.x, 0)`. |
| `poseAt(keys, t)` | The full pose at time t: limbs held on whole 12 fps steps, feet x smooth. |
| `shiftKeys(keys, dt, dx)`, `shiftHits(hits, dt)` | Copies of act keys or hits, later by dt and (keys) dx px further on. |
| `chain(cues)` | A `create(o)` that plays effect modules as one (a move). |
| `dummy(p, x, push, white, tint)` | Draws the training dummy at local x. |
| `actFrames(mod, extra)` | The act as `{ w, h, pivot, frames: [{ f, t, frozen, hot, ox, oy }] }`. |
| `checkAct(mod)` | `[status, name, message]` lines for the act. |

```js
// pxact.mjs - stand-in character, poses, acts, target dummy and moves. Lives next to pxfx.mjs.
import { frame, instance, clamp, lerp, ease, hash, rgb } from './pxfx.mjs';

// ---------------------------------------------------------------- the stock character
// Palette keys: o outline, k skin, h hair, t tunic, d tunic shade, b boots and belt;
// s m steel light and dark, w wood, g gold (weapons); y Y straw, r red, R rope (dummy).
export const PAL = { o: '#1a1422', k: '#efb38a', h: '#8a4424', t: '#3f78c2', d: '#284c86', b: '#4a3028',
  s: '#dde4ee', m: '#7c889a', w: '#7a4c26', g: '#e2b240', y: '#caa05a', Y: '#8c6a34', r: '#b83238', R: '#5a3a22' };
const KEYS = Object.keys(PAL), COL = Object.values(PAL).map(rgb);
// Part sprites, facing right ('.' is clear). The outline is added around the whole figure.
const HEAD = ['..hhhh..', '.hhhhhhh', 'hhhhhhhk', 'hhhhkkkk', 'hhhkkkok', 'hhhkkkkk', '.hhkkkk.', '..hkkk..'];
const TORSO = ['..dttt..', '.ddtttt.', 'dddttttt', 'dddttttt', '.ddttttt', '.ddtttt.', '.ddtttt.', '.bbbbbb.', '.ddtttt.', '.ddttttt', 'dddttttt', 'dd....tt'];
export const WEAPONS = ['none', 'sword', 'staff', 'hammer', 'bow', 'gun'];
// A pose lists only what differs from STAND. Hands are px from the feet (y negative is up).
export const STAND = { x: 0, crouch: 0, lean: 0, hx: 3, hy: -12, bx: -3, by: -13, weapon: 'none', wa: 1.2, ghost: false };
export const READY = { crouch: 2, lean: 1, hx: 8, hy: -20, bx: 3, by: -19, wa: -0.7 };
export const WINDUP_BACK = { x: -1, crouch: 3, lean: -3, hx: -10, hy: -24, bx: -7, by: -19, wa: -2.6 };
export const STRIKE_FORWARD = { x: 3, crouch: 4, lean: 4, hx: 17, hy: -19, bx: 6, by: -17, wa: 0.15 };
export const CAST_UP = { lean: -1, hx: 4, hy: -38, bx: -5, by: -35, wa: -1.6 };
export const CAST_FORWARD = { x: 1, crouch: 2, lean: 3, hx: 17, hy: -24, bx: 13, by: -23, wa: -0.1 };
export const CROUCH = { crouch: 7, lean: 3, hx: 8, hy: -11, bx: 0, by: -10 };
export const HURT = { x: -2, crouch: 2, lean: -4, hx: -5, hy: -23, bx: -9, by: -19 };
const NUM = ['x', 'crouch', 'lean', 'hx', 'hy', 'bx', 'by', 'wa'];

// raster: an index buffer in local px (x0, y0 is its top-left) with small drawing helpers.
function raster(w, h, x0, y0) {
  const buf = new Uint8Array(w * h);
  const R = {
    put(x, y, ch) { const i = Math.round(x) - x0, j = Math.round(y) - y0; if (i >= 0 && j >= 0 && i < w && j < h) buf[j * w + i] = KEYS.indexOf(ch) + 1; },
    seg(xa, ya, xb, yb, ch, wd = 1) {
      const n = Math.max(1, Math.ceil(Math.hypot(xb - xa, yb - ya)));
      for (let s = 0; s <= n; s++) for (let j = 0; j < wd; j++) for (let i = 0; i < wd; i++)
        R.put(Math.round(lerp(xa, xb, s / n)) + i - (wd >> 1), Math.round(lerp(ya, yb, s / n)) + j - (wd >> 1), ch);
    },
    // limb: an outlined thick line through points (outline wd + 2, fill wd)
    limb(pts, ch, wd) { for (const c of ['o', ch]) for (let i = 1; i < pts.length; i++) R.seg(...pts[i - 1], ...pts[i], c, c === 'o' ? wd + 2 : wd); },
    spr(rows, x, y, shear = 0) { rows.forEach((r, j) => [...r].forEach((ch, i) => ch !== '.' && R.put(x + i + Math.round((shear * (rows.length - 1 - j)) / (rows.length - 1)), y + j, ch))); },
    // done: outline the figure, then return { w, h, x, y, mask, pixels } placed at dx.
    done(dx = 0) {
      const o = KEYS.indexOf('o') + 1, solid = (i, j) => i >= 0 && j >= 0 && i < w && j < h && buf[j * w + i] && buf[j * w + i] !== o, add = [];
      for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) if (!buf[j * w + i] && (solid(i - 1, j) || solid(i + 1, j) || solid(i, j - 1) || solid(i, j + 1))) add.push(j * w + i);
      for (const k of add) buf[k] = o;
      const mask = new Uint8Array(w * h), pixels = new Array(w * h);
      buf.forEach((v, k) => { if (v) { mask[k] = 1; pixels[k] = COL[v - 1]; } });
      return { w, h, x: x0 + dx, y: y0, mask, pixels };
    },
  };
  return R;
}
// ik: the middle joint of a two-bone limb from a towards b (bend +1 or -1 picks the side).
function ik([ax, ay], [bx, by], l1, l2, bend) {
  let dx = bx - ax, dy = by - ay, d = Math.hypot(dx, dy) || 1e-6;
  if (d > l1 + l2 - 0.01) { const s = (l1 + l2 - 0.01) / d; dx *= s; dy *= s; d = l1 + l2 - 0.01; }
  const k = Math.acos(clamp((l1 * l1 + d * d - l2 * l2) / (2 * l1 * d), -1, 1)) * bend, base = Math.atan2(dy, dx);
  return [[ax + Math.cos(base + k) * l1, ay + Math.sin(base + k) * l1], [ax + dx, ay + dy]];
}
function weapon(R, [hx, hy], kind, a) {
  const c = Math.cos(a), s = Math.sin(a), P = (d, n = 0) => [hx + c * d - s * n, hy + s * d + c * n];
  if (kind === 'sword') { R.seg(...P(-2), ...P(1), 'w'); R.seg(...P(1, -2), ...P(1, 2), 'g'); R.seg(...P(2), ...P(13), 's'); }
  else if (kind === 'staff') { R.seg(...P(-9), ...P(13), 'w'); R.seg(...P(14), ...P(15), 'g', 2); }
  else if (kind === 'hammer') { R.seg(...P(-2), ...P(10), 'w'); for (let n = -2; n <= 2; n++) R.seg(...P(10, n), ...P(13, n), n < 0 ? 's' : 'm'); }
  else if (kind === 'bow') { const b = [-7, -4, 0, 4, 7].map((n) => P(3 - (n * n) / 14, n)); for (let i = 1; i < 5; i++) R.seg(...b[i - 1], ...b[i], 'w'); R.seg(...P(-1, -7), ...P(-1, 7), 's'); }
  else if (kind === 'gun') { R.seg(...P(0, -1), ...P(7, -1), 'm', 2); R.put(...P(8, -1), 's'); R.seg(...P(0), ...P(-1, 2), 'b'); }
}
// rig(pose): the character as an actor { w, h, x, y, mask, pixels, head, chest, hand } in local
// px with the feet at (pose.x, 0). Arms reach about 15 px from the shoulder.
export function rig(pose = {}) {
  const q = { ...STAND, ...pose }, c = Math.round(q.crouch), L = q.lean, R = raster(76, 64, -34, -60), hip = -15 + c;
  const sh = [L + 2, -25 + c], shB = [L - 2, -25 + c], hand = [q.hx, q.hy], handB = [q.bx, q.by];
  R.limb([shB, ...ik(shB, handB, 7, 7, 1)], 'd', 1);
  R.seg(...handB, ...handB, 'k', 2);
  for (const [hx, fx, ch] of [[-2, -3, 'b'], [1, 2, 'b']]) {
    const [knee, foot] = ik([hx, hip], [fx, -2], 7, 6, -1);
    R.limb([[hx, hip], knee, foot], ch, 2);
    R.seg(foot[0] - 1, -2, foot[0] + 2, -2, 'b'); R.seg(foot[0] - 1, -1, foot[0] + 2, -1, 'b');
  }
  R.spr(TORSO, -4, -27 + c, Math.round(L));
  R.spr(HEAD, Math.round(L) - 4, -35 + c);
  if (q.weapon !== 'none') weapon(R, hand, q.weapon, q.wa);
  R.limb([sh, ...ik(sh, hand, 7, 7, 1)], 't', 1);
  R.seg(...hand, ...hand, 'k', 2);
  const dx = Math.round(q.x), out = R.done(dx);
  return Object.assign(out, { head: [L + dx, -31 + c], chest: [L * 0.6 + dx, -21 + c], hand: [hand[0] + dx, hand[1]] });
}

// ---------------------------------------------------------------- poses over time
const EASES = { smooth: ease.inOutSine, out: ease.outCubic, in: ease.inQuad, line: ease.linear, snap: (u) => (u >= 1 ? 1 : 0) };
// poseAt(keys, t): the full pose at time t. Limbs hold on whole 12 fps steps; feet x is smooth.
// Each key is [t, pose, how]; how ('smooth', 'snap', 'out', 'in', 'line') is how it is reached.
export function poseAt(keys, t) {
  const at = (tt) => {
    let i = 0;
    while (i < keys.length - 1 && keys[i + 1][0] <= tt) i++;
    const a = { ...STAND, ...keys[i][1] };
    if (i === keys.length - 1 || tt <= keys[i][0]) return a;
    const [tb, B, how = 'smooth'] = keys[i + 1], b = { ...STAND, ...B }, u = (EASES[how] ?? EASES.smooth)((tt - keys[i][0]) / (tb - keys[i][0]));
    for (const f of NUM) a[f] = lerp(a[f], b[f], u);
    if (how === 'snap' ? u >= 1 : u >= 0.5) { a.weapon = b.weapon; a.ghost = b.ghost; }
    return a;
  };
  const t0 = keys[0][0], p = at(t0 + Math.floor((t - t0) * 12 + 1e-6) / 12);
  p.x = at(t).x;
  return p;
}
// shiftKeys / shiftHits: reuse another effect's act inside a longer move (later by dt, dx px on).
export const shiftKeys = (keys, dt = 0, dx = 0) => keys.map(([t, pose, how]) => [t + dt, { ...pose, x: (pose.x ?? 0) + dx }, ...(how ? [how] : [])]);
export const shiftHits = (hits, dt = 0) => hits.map((h) => ({ ...h, t: h.t + dt }));

// ---------------------------------------------------------------- the target dummy
// dummy(p, x, push, white, tint): a training dummy standing at local x, its top pushed by
// push px (the post bends), all white when white is true, recoloured towards tint.
export function dummy(p, x, push = 0, white = false, tint = null) {
  const R = raster(30, 44, -15, -41), b = (y) => push * (-y / 34) ** 2;
  const ell = (cx, cy, rx, ry, f) => { for (let y = Math.floor(cy - ry); y <= cy + ry; y++) for (let xx = Math.floor(cx - rx); xx <= cx + rx; xx++) if (((xx - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 <= 1) R.put(xx + b(y), y, f(xx - cx, y - cy)); };
  R.seg(-4, -1, 4, -1, 'w'); R.seg(-3, -2, 3, -2, 'w');
  for (let y = -14; y <= -3; y++) { R.put(-1 + b(y), y, 'R'); R.put(b(y), y, 'w'); }
  ell(0, -23, 6, 8.5, (dx, dy) => (Math.abs(dy - 4.5) < 0.6 || Math.abs(dy + 5) < 0.6 ? 'R' : (dx - 1) ** 2 + dy * dy <= 2.5 ? (dx === 1 && dy === 0 ? 'y' : 'r') : dx < -3 ? 'Y' : 'y'));
  ell(0, -34, 3.5, 3.5, (dx) => (dx < -1.5 ? 'Y' : 'y'));
  const m = R.done(), T = tint && rgb(tint), o = COL[0];
  p.sub(x, 0).body(m, (c) => (c === o ? c : white ? [255, 255, 255] : T ? T.map((v, k) => lerp(c[k], (v * (c[0] + c[1] + c[2])) / 400, 0.7)) : c), 1);
}

// ---------------------------------------------------------------- acting it out
const GHOST = ['#33508c', '#222c52'], HOLD = 0.25, HOWS = Object.keys(EASES), HITF = ['t', 'freeze', 'shake', 'amp', 'push', 'flash', 'tint'];
// actFrames(mod, extra): the act at consecutive 12 fps steps, with freezes and shake, as
// { w, h, pivot, frames: [{ f, t, frozen, hot, ox, oy }] }. t is effect time (frozen = held).
export function actFrames(mod, extra = {}) {
  const act = mod.act, keys = act.poses, hits = act.hits ?? [], tx = act.target ? act.target.x : null;
  const actor = rig(keys[0][1]), inst = instance(mod, { ...extra, actor }), fl = inst.o.flip ? -1 : 1;
  const [w, h] = mod.meta.size, [ox, oy] = mod.meta.origin, steps = [];
  const frz = hits.reduce((s, x) => s + (x.freeze ?? 0), 0), end = keys.at(-1)[0] + frz + HOLD + 1e-6;
  let L0 = -ox, L1 = w - ox, T0 = -oy, T1 = h - oy, amp = 0;
  for (let n = 0, T = keys[0][0]; T <= end; T = keys[0][0] + ++n / 12) {
    let acc = 0, t = null, frozen = false;
    for (const x of hits) {
      if (T - acc < x.t - 1e-9) break;
      if (T - acc < x.t + (x.freeze ?? 0) - 1e-9) { t = x.t; frozen = true; break; }
      acc += x.freeze ?? 0;
    }
    if (t === null) t = T - acc;
    const pose = poseAt(keys, t), body = rig(pose);
    for (let k = 0; k < body.mask.length; k++) if (body.mask[k]) {
      const lx = (body.x + (k % body.w)) * fl, ly = body.y + Math.floor(k / body.w);
      L0 = Math.min(L0, lx); L1 = Math.max(L1, lx + 1); T0 = Math.min(T0, ly); T1 = Math.max(T1, ly + 1);
    }
    steps.push({ n, t, frozen, pose, body });
  }
  for (const x of hits) amp = Math.max(amp, x.amp ?? 0);
  if (tx != null) { const pu = Math.max(0, ...hits.map((x) => Math.abs(x.push ?? 0))); L0 = Math.min(L0, (tx - 9) * fl - pu, (tx + 9) * fl - pu); L1 = Math.max(L1, (tx + 9) * fl + pu, (tx - 9) * fl + pu); T0 = Math.min(T0, -40); }
  const M = Math.ceil(amp) + 2, SW = L1 - L0 + 2 * M, SH = T1 - T0 + 2 * M, OX = -L0 + M, OY = -T0 + M;
  const first = hits.map((x) => steps.find((s) => s.t >= x.t - 1e-9)?.n), ghosts = [], out = [];
  for (const s of steps) {
    const { t, frozen, pose, body } = s, live = t >= 0;
    let sx = 0, sy = 0, push = 0, white = false, tint = null;
    hits.forEach((x, i) => {
      if (t < x.t - 1e-9) return;
      const since = t - x.t, d = s.n - first[i];
      push += (x.push ?? 0) * Math.exp(-6 * since) * Math.cos(18 * since);
      if (d < (x.flash ?? 0)) white = true;
      if ('tint' in x) tint = x.tint;
      if (!frozen && since < (x.shake ?? 0)) {
        const k = (x.amp ?? 0) * (1 - since / x.shake);
        sx += Math.round((hash(s.n, 1, i) * 2 - 1) * k); sy += Math.round((hash(s.n, 2, i) * 2 - 1) * k);
      }
    });
    const f = frame(SW, SH), p = inst.paint(f, OX + sx, OY + sy);
    Object.assign(actor, body);
    if (live) { inst.seek(t); inst.draw(p, 'back'); }
    if (tx != null) dummy(p, tx, push, white, tint);
    if (pose.ghost) ghosts.forEach((g, i) => p.body(g, () => GHOST[i]));
    if (live) inst.drawActor(p); else p.body(actor);
    if (live) inst.draw(p, 'front');
    ghosts.unshift(body); ghosts.length = Math.min(ghosts.length, 2);
    out.push({ f, t, frozen, hot: first.includes(s.n), ox: OX + sx, oy: OY + sy });
  }
  return { w: SW, h: SH, pivot: { x: OX, y: OY }, frames: out };
}

// checkAct(mod): [status, name, message] lines for the act data.
export function checkAct(mod) {
  const act = mod.act, k = act?.poses, out = [], bad = [];
  if (!Array.isArray(k) || !k.length) return [['FAIL', 'act keys', 'act.poses must be a non-empty array of [t, pose, how]']];
  k.forEach((key, i) => {
    if (!Array.isArray(key) || typeof key[0] !== 'number' || typeof key[1] !== 'object') return bad.push(`key ${i} is not [t, pose, how]`);
    if (i && key[0] <= k[i - 1][0]) bad.push(`key ${i}: t ${key[0]} is not after key ${i - 1}`);
    if (key[2] !== undefined && !HOWS.includes(key[2])) bad.push(`key ${i}: how '${key[2]}' (use ${HOWS.join(', ')})`);
    for (const f in key[1]) if (!(f in STAND)) bad.push(`key ${i}: unknown pose field '${f}'`);
    if (key[1].weapon !== undefined && !WEAPONS.includes(key[1].weapon)) bad.push(`key ${i}: weapon '${key[1].weapon}' (use ${WEAPONS.join(', ')})`);
  });
  out.push([bad.length ? 'FAIL' : 'PASS', 'act keys', bad.length ? bad.join('; ') : `${k.length} keys, t ${+k[0][0].toFixed(3)}..${+k.at(-1)[0].toFixed(3)}`]);
  if (bad.length) return out;
  const hits = act.hits ?? [], hb = [], t0 = k[0][0], t1 = k.at(-1)[0];
  hits.forEach((x, i) => {
    if (!(x.t >= t0 && x.t <= t1)) hb.push(`hit ${i}: t outside the act (${t0}..${t1})`);
    if (i && x.t < hits[i - 1].t) hb.push(`hit ${i}: hits not sorted by t`);
    if (!((x.freeze ?? 0) >= 0 && (x.freeze ?? 0) <= 0.3)) hb.push(`hit ${i}: freeze outside 0..0.3`);
    for (const f in x) if (!HITF.includes(f)) hb.push(`hit ${i}: unknown field '${f}'`);
  });
  if (act.target && typeof act.target.x !== 'number') hb.push('target must be { x } or false');
  out.push([hb.length ? 'FAIL' : 'PASS', 'act hits', hb.length ? hb.join('; ') : `${hits.length} hit(s), ${act.target ? `target at x ${act.target.x}` : 'no target'}`]);
  let worst = 0, when = t0, prev = null, tp = t0;
  for (let n = 0, t = t0; t <= t1 + 1e-9; t = t0 + ++n / 12) {
    const q = poseAt(k, t), pts = [q.x + q.hx, q.hy, q.x + q.bx, q.by];
    if (prev && !k.some(([kt, , how]) => how === 'snap' && kt > tp + 1e-9 && kt <= t + 1e-9)) {
      const j = Math.max(Math.hypot(pts[0] - prev[0], pts[1] - prev[1]), Math.hypot(pts[2] - prev[2], pts[3] - prev[3]));
      if (j > worst) { worst = j; when = t; }
    }
    prev = pts; tp = t;
  }
  out.push([worst > 10 ? 'FAIL' : 'PASS', 'act hands', `largest hand move ${worst.toFixed(1)} px per 12 fps step${worst > 10 ? ` at t ${when.toFixed(2)}: add a key between, or reach it with 'snap'` : ' (limit 10 without snap)'}`]);
  return out;
}

// ---------------------------------------------------------------- moves
// chain(cues): a create(o) that plays effect modules as one. A cue is { fx, t, until, x, y,
// flip, ...options }; x and y may be functions of the move's time. Cue i gets seed o.seed + i.
export function chain(cues) {
  const create = (o) => {
    let t = 0;
    const live = cues.map(() => null), end = (c) => c.until ?? (c.fx.meta.loop ? Infinity : c.t + c.fx.meta.duration);
    const v = (q) => (typeof q === 'function' ? q(t) : q ?? 0);
    const each = (fn) => cues.forEach((c, i) => {
      if (t < c.t || t >= end(c)) return;
      const { fx, t: _t, until, x, y, flip, ...opts } = c;
      live[i] ??= instance(fx, { seed: (o.seed ?? 0) + i, actor: o.actor, ...opts });
      live[i].seek(t - c.t);
      fn?.(live[i], c);
    });
    const draw = (p, layer) => each((L, c) => L.draw(p.sub(v(c.x), v(c.y), c.flip, L.o.light ?? 1), layer));
    return { update(dt) { t += dt; each(); }, drawBack: (p) => draw(p, 'back'), draw: (p) => draw(p, 'front') };
  };
  create.cues = cues;
  return create;
}
```
