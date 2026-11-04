# Capture harness: templates and commands

Templates for `video-capture`. They ran on Chrome 156, Node v26.12.1 and
ffmpeg 8.1: 10 of 10 runs gave identical frames.

The harness drives Chrome through the Chrome DevTools Protocol (CDP) with
Node built-ins only: no `playwright` import, no `npm install`. Its prelude
replaces Playwright's `page.clock.install({ time })`.

## Files

Put these files in the video project under `captures/src/`: `capture.mjs`
(the harness, copy it unchanged), 1 take script `<scene-id>.mjs` per capture
scene, and for CLI scenes `term.html` and `rec-cast.mjs`.

Run 1 take: `node captures/src/capture.mjs . <scene-id>`. The harness reads
`timeline.json` (`meta.fps`, `meta.width`, `meta.height` and the scene's
`duration_sec`), writes `captures/<scene-id>.mp4` and
`captures/<scene-id>.json`, and exits 1 on any page error. A 6 s take runs
in about 25 s.

## How the harness works

- The prelude freezes `Date` at `clockIso` and runs `setTimeout`,
  `setInterval`, `requestAnimationFrame` and `performance.now` on a virtual
  clock. Each frame advances it by exactly `1000 / fps` ms.
- `Animation.setPlaybackRate` 0 stops the real animation clock, and each
  frame moves every running CSS animation by 1 frame. Without it,
  compositor animations drift by a few ms between runs.
- `Fetch.enable` sends fixtures for `config.fixtures` URLs, passes `file:`,
  `data:`, `about:` and `config.allow` prefixes, and fails the take on any
  other request.
- `wait()` buffers dead-time frames, then keeps a subset on the speed curve.
  The plate is already ramped. After the take script ends, the last state
  holds until `round(duration_sec * fps)` frames.

## Take script API (`h`)

| Call | Effect | Telemetry |
|---|---|---|
| `goto(url)` | Navigate, wait for load, `document.fonts.ready` and image decode. | - |
| `hold(ms)` | Interactive pause at 1x. | - |
| `move(x, y, ms?)` | Cubic Bezier path in CSS px, ease-in-out timing. Default `ms` is 400-700 by distance. | `cursor[]` per frame |
| `click(selector, button?)` | Move to the element centre, press and release, 240 ms ripple, 240 ms hold. | `cursor[]`, `clicks[]` |
| `type(text)` | Key events per character, 35-55 ms apart (seeded). | `keys[]` |
| `press(key)` | `Enter`, `Tab`, `Backspace` or `Escape`, then a 120 ms hold. | `keys[]` |
| `zoom(scale, cx?, cy?, ms?, ease?)` | Records a camera move. Default 500 ms `cubic-bezier(0.22, 1, 0.36, 1)`. The plate does not zoom. | `zoom_keyframes[]` |
| `zoomTo(selector, scale?)` | `zoom` centred on an element. Default 1.6x. | `zoom_keyframes[]` |
| `wait(ms \| jsExpression, timeoutMs?)` | Dead time with no input. 500 ms or longer is speed-ramped. | `speed_ramps[]` |
| `term(data)` | Terminal page: print output now. | - |
| `termType(text)` | Terminal page: echo a typed command at 35-55 ms per character. | `keys[]` |
| `termCast(file)` | Terminal page: replay an asciicast v2 file. Gaps of 500 ms or longer go through `wait`. | `keys[]`, `speed_ramps[]` |

`config` keys: `dsf` (1 or 2, default 2), `seed` (default 1), `clockIso`
(default `2026-01-15T10:00:00Z`), `cursor` (default `true`; set `false` for
terminal scenes), `fixtures` (`{ url: { status?, json? | body?, type? } }`),
`allow` (URL prefixes that may reach a local server, for example
`http://127.0.0.1:5173/`).

Speed ramp table (dead span `D` is virtual time):

| D | Factor | Pad in and pad out |
|---|---|---|
| 500 ms to under 1 s | 2x | 150 ms |
| 1 s to under 2 s | 3x | 200 ms |
| 2 s or more | 4x | 250 ms |

The speed rises from 1x to the factor over the pad in, holds, and falls back
to 1x over the pad out (smoothstep).

## `captures/src/capture.mjs`

```js
// capture.mjs - deterministic CDP capture. Node built-ins only.
// Usage: node captures/src/capture.mjs <project-dir> <scene-id>
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const [projectDir, sceneId] = process.argv.slice(2).map(String);
const timeline = JSON.parse(readFileSync(join(projectDir, 'timeline.json'), 'utf8'));
const scene = timeline.scenes.find((s) => s.id === sceneId);
if (!scene) throw new Error(`no scene ${sceneId} in timeline.json`);
const mod = await import(pathToFileURL(resolve(projectDir, 'captures/src', `${sceneId}.mjs`)));
const cfg = { dsf: 2, seed: 1, clockIso: '2026-01-15T10:00:00Z', cursor: true, fixtures: {}, allow: [], ...mod.config };
const { fps, width: W, height: H } = timeline.meta;
const frameMs = 1000 / fps, totalFrames = Math.round(scene.duration_sec * fps);

function prng(seed) { // mulberry32: same seed, same sequence
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0; let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1); t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
const rand = prng(cfg.seed + 1);

// ---- page prelude: virtual clock, seeded Math.random, CSS locks, cursor overlay ----
const prelude = `(() => {
  const s0 = ${cfg.seed}, t0 = ${Date.parse(cfg.clockIso)};
  Math.random = (${prng.toString()})(s0);
  let now = 0, nextId = 1; const timers = new Map(), rafs = new Map(), R = Date;
  class D extends R { constructor(...a) { a.length ? super(...a) : super(t0 + now); } static now() { return t0 + now; } }
  globalThis.Date = D; performance.now = () => now;
  const add = (fn, ms, args, every) => { const id = nextId++; timers.set(id, { at: now + Math.max(every || 0, +ms || 0), every, fn, args }); return id; };
  globalThis.setTimeout = (fn, ms, ...a) => add(fn, ms, a, 0);
  globalThis.setInterval = (fn, ms, ...a) => add(fn, ms, a, Math.max(1, +ms || 0));
  globalThis.clearTimeout = globalThis.clearInterval = (id) => timers.delete(id);
  globalThis.requestAnimationFrame = (fn) => { const id = nextId++; rafs.set(id, fn); return id; };
  globalThis.cancelAnimationFrame = (id) => rafs.delete(id);
  const run = (fn, a) => { try { typeof fn === 'function' && fn(...a); } catch (e) { reportError(e); } };
  const css = '*,*::before,*::after{caret-color:transparent!important;cursor:none!important}' +
    '::-webkit-scrollbar{display:none!important}html{scrollbar-width:none!important}';
  const ARROW = '<svg width="28" height="28" viewBox="0 0 28 28"><path d="M4 2 L4 22 L9.5 17 L13 25 L16.5 23.5 L13 15.8 L20.5 15.8 Z" fill="#111" stroke="#fff" stroke-width="1.6" stroke-linejoin="round"/></svg>';
  let ov, cur;
  const overlay = () => {
    if (ov) return;
    const st = document.createElement('style'); st.textContent = css; document.documentElement.append(st);
    ov = document.createElement('div');
    ov.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483647';
    cur = document.createElement('div'); cur.innerHTML = ${cfg.cursor} ? ARROW : '';
    cur.style.cssText = 'position:absolute;left:0;top:0;width:28px;height:28px;margin:-2px 0 0 -4px';
    ov.append(cur); document.documentElement.append(ov);
  };
  window.__cap = {
    step(dt, x, y, ripples) {
      overlay(); const end = now + dt;
      for (let n = 0; n < 10000; n++) {
        let id = 0, t = null;
        for (const [k, v] of timers) if (v.at <= end && (!t || v.at < t.at)) { id = k; t = v; }
        if (!t) break;
        now = t.at; t.every ? (t.at += t.every) : timers.delete(id); run(t.fn, t.args);
      }
      now = end;
      const due = [...rafs.values()]; rafs.clear(); for (const fn of due) run(fn, [now]);
      for (const a of document.getAnimations()) {
        if (a.playState === 'running') a.currentTime = (a.currentTime || 0) + dt * (a.playbackRate || 1);
      }
      cur.style.transform = 'translate(' + x + 'px,' + y + 'px)';
      ov.querySelectorAll('.__rip').forEach((r) => r.remove());
      for (const r of ripples) {
        const d = document.createElement('div'); d.className = '__rip'; const rad = 6 + 22 * r.p;
        d.style.cssText = 'position:absolute;border-radius:50%;border:3px solid rgba(59,130,246,' + (0.85 * (1 - r.p)) + ');' +
          'left:' + (r.x - rad) + 'px;top:' + (r.y - rad) + 'px;width:' + 2 * rad + 'px;height:' + 2 * rad + 'px;box-sizing:border-box';
        ov.append(d);
      }
    },
    ready: async () => { await document.fonts.ready; await Promise.all([...document.images].map((i) => i.decode().catch(() => {}))); return document.fonts.status; },
  };
})();`;

// ---- Chrome + CDP ----
const userDir = mkdtempSync(join(tmpdir(), 'cap-chrome-'));
const chromePath = process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const chrome = spawn(chromePath, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${userDir}`,
  '--hide-scrollbars', '--mute-audio', '--no-first-run', '--no-default-browser-check',
  '--force-color-profile=srgb', '--font-render-hinting=none', '--disable-lcd-text', 'about:blank'],
  { stdio: ['ignore', 'ignore', 'pipe'] });
const wsUrl = await new Promise((ok, fail) => {
  let buf = '';
  chrome.stderr.on('data', (d) => { buf += d; const m = buf.match(/DevTools listening on (ws:\/\/\S+)/); if (m) ok(m[1]); });
  chrome.on('exit', (c) => fail(new Error(`chrome exited ${c}`)));
});
const ws = new WebSocket(wsUrl);
await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail; });
let msgId = 0, sessionId; const pending = new Map(), events = [];
ws.onmessage = (e) => {
  const m = JSON.parse(e.data);
  if (m.id) { const p = pending.get(m.id); pending.delete(m.id); m.error ? p.fail(new Error(m.error.message)) : p.ok(m.result); }
  else for (const fn of events) fn(m);
};
const send = (method, params = {}) => new Promise((ok, fail) => {
  const id = ++msgId; pending.set(id, { ok, fail }); ws.send(JSON.stringify({ id, method, params, sessionId }));
});
const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text);
  return r.result.value;
};

const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
sessionId = (await send('Target.attachToTarget', { targetId, flatten: true })).sessionId;
const errors = [];
events.push((m) => {
  if (m.method === 'Runtime.exceptionThrown') errors.push(`pageerror: ${m.params.exceptionDetails.exception?.description || m.params.exceptionDetails.text}`);
  if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') errors.push(`console.error: ${m.params.args.map((a) => a.value ?? a.description).join(' ')}`);
  if (m.method === 'Fetch.requestPaused') route(m.params);
});
async function route({ requestId, request }) { // fixtures answer, local URLs pass, the rest fail
  const fx = cfg.fixtures[request.url];
  if (fx) {
    const body = Buffer.from(typeof fx.body === 'string' ? fx.body : JSON.stringify(fx.json ?? {})).toString('base64');
    return send('Fetch.fulfillRequest', { requestId, responseCode: fx.status ?? 200, body,
      responseHeaders: [{ name: 'Content-Type', value: fx.type ?? 'application/json' }, { name: 'Access-Control-Allow-Origin', value: '*' }] });
  }
  if (/^(file:|data:|about:)/.test(request.url) || cfg.allow.some((p) => request.url.startsWith(p))) return send('Fetch.continueRequest', { requestId });
  errors.push(`blocked live request: ${request.url}`);
  return send('Fetch.failRequest', { requestId, errorReason: 'BlockedByClient' });
}
for (const d of ['Page', 'Runtime', 'Animation']) await send(`${d}.enable`);
await send('Animation.setPlaybackRate', { playbackRate: 0 }); // the page clock alone moves CSS animations
await send('Fetch.enable', { patterns: [{ urlPattern: '*' }] });
await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: cfg.dsf, mobile: false });
await send('Emulation.setTimezoneOverride', { timezoneId: 'UTC' });
await send('Emulation.setLocaleOverride', { locale: 'en-US' });
await send('Page.addScriptToEvaluateOnNewDocument', { source: prelude });

// ---- ffmpeg: PNG frames in, CFR H.264 out ----
const outMp4 = join(projectDir, 'captures', `${sceneId}.mp4`);
const ff = spawn('ffmpeg', ['-v', 'error', '-y', '-f', 'image2pipe', '-c:v', 'png', '-framerate', String(fps), '-i', '-',
  '-vsync', 'cfr', '-r', String(fps), '-c:v', 'libx264', '-preset', 'slow', '-crf', '14', '-pix_fmt', 'yuv420p', outMp4],
  { stdio: ['pipe', 'inherit', 'inherit'] });
const ffDone = new Promise((ok) => ff.on('exit', ok));

// ---- telemetry and frame loop ----
const tel = { scene_id: sceneId, fps, width: W * cfg.dsf, height: H * cfg.dsf, duration_sec: 0,
  cursor: [], clicks: [], keys: [], zoom_keyframes: [{ t_sec: 0, scale: 1, cx: 0.5, cy: 0.5, ease: 'linear' }], speed_ramps: [] };
let out = 0, vt = 0, owed = 0; const pos = { x: W / 2, y: H / 2 }, ripples = [];
const tNow = () => +(out / fps).toFixed(4);
const px = (v) => Math.round(v * cfg.dsf);
async function shoot() { // advance the page 1 frame of virtual time, return a PNG
  vt += frameMs;
  const rip = ripples.filter((r) => vt - r.t0 < 240).map((r) => ({ x: r.x, y: r.y, p: (vt - r.t0) / 240 }));
  await evaluate(`__cap.step(${frameMs}, ${pos.x}, ${pos.y}, ${JSON.stringify(rip)})`);
  return Buffer.from((await send('Page.captureScreenshot', { format: 'png' })).data, 'base64');
}
async function emit(png) {
  if (out >= totalFrames) throw new Error(`scene is longer than timeline duration_sec ${scene.duration_sec} s`);
  out++; if (!ff.stdin.write(png)) await new Promise((ok) => ff.stdin.once('drain', ok));
}
const easeInOut = (u) => (u < 0.5 ? 4 * u * u * u : 1 - (-2 * u + 2) ** 3 / 2);
const smooth = (u) => u * u * (3 - 2 * u);

const h = {
  W, H, fps, rand,
  async goto(url) {
    await send('Page.navigate', { url });
    for (let i = 0; i < 200 && (await evaluate('document.readyState')) !== 'complete'; i++) await new Promise((r) => setTimeout(r, 25));
    await evaluate('__cap.ready()');
  },
  async hold(ms) { // interactive pause at 1x
    owed += ms;
    while (owed >= frameMs - 1e-6) { owed -= frameMs; await emit(await shoot()); }
  },
  async move(x, y, ms) { // cubic Bezier path, eased timing, 400-700 ms
    const dx = x - pos.x, dy = y - pos.y, dist = Math.hypot(dx, dy);
    ms ??= Math.min(700, Math.max(400, 400 + dist * 0.3));
    const bow = (rand() < 0.5 ? -1 : 1) * Math.min(80, dist * 0.12), nx = dist ? -dy / dist : 0, ny = dist ? dx / dist : 0;
    const p0 = { ...pos }, c1 = { x: p0.x + dx / 3 + nx * bow, y: p0.y + dy / 3 + ny * bow }, c2 = { x: p0.x + (2 * dx) / 3 + nx * bow * 0.6, y: p0.y + (2 * dy) / 3 + ny * bow * 0.6 };
    const n = Math.max(1, Math.round(ms / frameMs));
    for (let i = 1; i <= n; i++) {
      const u = easeInOut(i / n), v = 1 - u;
      pos.x = v * v * v * p0.x + 3 * v * v * u * c1.x + 3 * v * u * u * c2.x + u * u * u * x;
      pos.y = v * v * v * p0.y + 3 * v * v * u * c1.y + 3 * v * u * u * c2.y + u * u * u * y;
      await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: pos.x, y: pos.y });
      tel.cursor.push({ t_sec: tNow(), x: px(pos.x), y: px(pos.y) });
      await emit(await shoot());
    }
  },
  async box(sel) {
    const b = await evaluate(`(() => { const r = document.querySelector(${JSON.stringify(sel)})?.getBoundingClientRect(); return r && { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`);
    if (!b) throw new Error(`no element ${sel}`); return b;
  },
  async click(sel, button = 'left') {
    const b = await h.box(sel); await h.move(b.x, b.y);
    for (const type of ['mousePressed', 'mouseReleased']) await send('Input.dispatchMouseEvent', { type, x: pos.x, y: pos.y, button, clickCount: 1 });
    tel.clicks.push({ t_sec: tNow(), x: px(pos.x), y: px(pos.y), button }); ripples.push({ x: pos.x, y: pos.y, t0: vt });
    await h.hold(240);
  },
  async type(text) { // 35-55 ms per char, seeded variance
    for (const ch of text) {
      await send('Input.dispatchKeyEvent', { type: 'keyDown', key: ch, text: ch });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch });
      tel.keys.push({ t_sec: tNow(), key: ch }); await h.hold(35 + 20 * rand());
    }
  },
  async press(key) {
    const k = { Enter: [13, '\r'], Tab: [9, '\t'], Backspace: [8, ''], Escape: [27, ''] }[key];
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code: key, windowsVirtualKeyCode: k[0], text: k[1] || undefined });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code: key, windowsVirtualKeyCode: k[0] });
    tel.keys.push({ t_sec: tNow(), key }); await h.hold(120);
  },
  zoom(scale, cx = 0.5, cy = 0.5, ms = 500, ease = 'cubic-bezier(0.22, 1, 0.36, 1)') { // compositor applies it
    const last = tel.zoom_keyframes.at(-1);
    if (tNow() > last.t_sec) tel.zoom_keyframes.push({ ...last, t_sec: tNow(), ease: 'linear' });
    tel.zoom_keyframes.push({ t_sec: +(tNow() + ms / 1000).toFixed(4), scale, cx, cy, ease });
  },
  async zoomTo(sel, scale = 1.6) { const b = await h.box(sel); h.zoom(scale, +(b.x / W).toFixed(4), +(b.y / H).toFixed(4)); },
  term: (data) => evaluate(`termWrite(${JSON.stringify(data)})`), // terminal page: print output now
  async termType(text) { // terminal page: echo a typed command at 35-55 ms per char
    for (const ch of text) { await evaluate(`termWrite(${JSON.stringify(ch)})`); tel.keys.push({ t_sec: tNow(), key: ch }); await h.hold(35 + 20 * rand()); }
  },
  async termCast(file) { // asciicast v2 replay: header line, then [t_sec, "o", data] lines
    let last = 0;
    for (const line of readFileSync(file, 'utf8').trim().split('\n').slice(1)) {
      const [t, kind, data] = JSON.parse(line); if (kind !== 'o') continue;
      const gap = (t - last) * 1000; last = t;
      await (gap >= 500 ? h.wait(gap) : h.hold(gap));
      await h.term(data);
    }
  },
  async wait(msOrExpr, timeoutMs = 30000) { // dead time: no input. >= 500 ms is speed-ramped
    const frames = [];
    for (let v = 0; ; v += frameMs) {
      const done = typeof msOrExpr === 'number' ? v >= msOrExpr - 1e-6 : frames.length && (await evaluate(msOrExpr));
      if (done) break;
      if (v > timeoutMs) throw new Error(`wait timed out: ${msOrExpr}`);
      frames.push(await shoot());
    }
    const D = (frames.length * frameMs) / 1000;
    if (D < 0.5) { for (const f of frames) await emit(f); return; }
    const [F, P] = D < 1 ? [2, 0.15] : D < 2 ? [3, 0.2] : [4, 0.25];
    const T = 2 * P + (D - P * (F + 1)) / F, N = Math.max(1, Math.round(T * fps));
    const speed = (u) => 1 + (F - 1) * smooth(Math.min(1, u / P, (T - u) / P));
    const cum = [0]; for (let j = 0; j < N; j++) cum.push(cum[j] + speed(((j + 0.5) / N) * T));
    const t0 = tNow();
    for (let j = 0; j < N; j++) await emit(frames[Math.min(frames.length - 1, Math.floor((cum[j] / cum[N]) * frames.length))]);
    tel.speed_ramps.push({ in_sec: t0, out_sec: tNow(), factor: F, pad_in_sec: P, pad_out_sec: P });
  },
};

try {
  await mod.default(h);
  while (out < totalFrames) await emit(await shoot()); // hold the last state to the timeline duration
} finally {
  ff.stdin.end(); const code = await ffDone;
  tel.duration_sec = +(out / fps).toFixed(4);
  writeFileSync(join(projectDir, 'captures', `${sceneId}.json`), JSON.stringify(tel, null, 2) + '\n');
  ws.close(); await new Promise((ok) => { chrome.once('exit', ok); chrome.kill(); }); rmSync(userDir, { recursive: true, force: true });
  if (code !== 0) errors.push(`ffmpeg exited ${code}`);
}
if (errors.length) { console.error(errors.join('\n')); process.exit(1); }
console.log(`captures/${sceneId}.mp4: ${out} frames at ${fps} fps`);
```

## Example take: web app (`captures/src/s02-demo.mjs`)

```js
export const config = { dsf: 1, fixtures: { 'https://api.example.test/items': { json: { count: 3 } } } };
export default async (h) => {
  await h.goto(new URL('../../app/index.html', import.meta.url).href);
  await h.hold(300);
  await h.click('#q');
  await h.zoomTo('#q', 1.6);
  await h.type('flaky tests');
  await h.click('#go');
  await h.wait('document.querySelector("#res").textContent !== ""');
  h.zoom(1);
  await h.hold(500);
};
```

## Terminal scenes

Default recipe: record the real command once, then replay it in
`term.html`. The replay is deterministic, and a long build becomes a ramped
`wait`.

```sh
node captures/src/rec-cast.mjs captures/src/s03-cli.cast -- sh -c 'npm test'
node captures/src/capture.mjs . s03-cli
```

An existing `asciinema` `.cast` file (v2) replays the same way. A live
`ttyd` session in the browser is not deterministic: its output arrives in
real time, outside the virtual clock. Use it only when the operator accepts
a take that can differ between runs.

`captures/src/rec-cast.mjs`:

```js
// node rec-cast.mjs <out.cast> -- <command> [args...]: run a command, save its output and timing as asciicast v2
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
const [out, , cmd, ...args] = process.argv.slice(2);
const t0 = performance.now(), ev = [];
const p = spawn(cmd, args, { env: { ...process.env, FORCE_COLOR: '1', CLICOLOR_FORCE: '1' } });
for (const s of [p.stdout, p.stderr]) s.on('data', (d) => ev.push([+((performance.now() - t0) / 1000).toFixed(3), 'o', d.toString().replace(/\r?\n/g, '\r\n')]));
p.on('close', (code) => {
  writeFileSync(out, [JSON.stringify({ version: 2, width: 100, height: 30 }), ...ev.map((e) => JSON.stringify(e))].join('\n') + '\n');
  process.exit(code ?? 1);
});
```

`captures/src/term.html`:

```html
<!doctype html>
<meta charset="utf-8">
<style>
  html, body { margin: 0; height: 100%; background: #0d1117; }
  #t { box-sizing: border-box; height: 100%; margin: 0; padding: 48px 56px; color: #e6edf3;
       font: 26px/1.45 "SF Mono", Menlo, monospace; white-space: pre-wrap; overflow: hidden; }
  .cur { background: #e6edf3; color: #0d1117; }
</style>
<pre id="t"></pre>
<script>
  // termWrite(data): append terminal output. Handles \n, \r, \b and SGR colours; drops other escapes.
  const PAL = ['#484f58', '#ff7b72', '#3fb950', '#d29922', '#58a6ff', '#bc8cff', '#39c5cf', '#b1bac4'];
  const t = document.getElementById('t'); let lines = [[]], style = {};
  const sgr = (codes) => { for (const c of codes.split(';').map(Number)) {
    if (c === 0) style = {}; else if (c === 1) style.b = 1; else if (c === 39) delete style.c;
    else if (c >= 30 && c <= 37) style.c = PAL[c - 30]; else if (c >= 90 && c <= 97) style.c = PAL[c - 90]; } };
  window.termWrite = (data) => {
    let row = lines.at(-1);
    for (let i = 0; i < data.length; i++) {
      const ch = data[i];
      if (ch === '\x1b') { const m = data.slice(i).match(/^\x1b\[([0-9;?]*)([A-Za-z])/); if (m) { if (m[2] === 'm') sgr(m[1] || '0'); i += m[0].length - 1; } continue; }
      if (ch === '\n') { lines.push(row = []); continue; }
      if (ch === '\r') { if (data[i + 1] !== '\n') row.length = 0; continue; }
      if (ch === '\b') { row.pop(); continue; }
      row.push({ ch, ...style });
    }
    t.replaceChildren(...lines.flatMap((r, n) => [...r.map((g) => {
      const s = document.createElement('span'); s.textContent = g.ch;
      if (g.c) s.style.color = g.c; if (g.b) s.style.fontWeight = 700; return s;
    }), n < lines.length - 1 ? '\n' : (() => { const c = document.createElement('span'); c.className = 'cur'; c.textContent = ' '; return c; })()]));
  };
  termWrite('');
</script>
```

A terminal take: `goto` of `term.html`, `term('$ ')` before the first frame,
`termType('npm test')`, `term('\r\n')`, `termCast(<cast path>)`, and
`config.cursor: false`.

## ffmpeg CFR transcode

The harness encodes PNG frames from stdin:

```sh
ffmpeg -f image2pipe -c:v png -framerate 60 -i - \
  -vsync cfr -r 60 -c:v libx264 -preset slow -crf 14 -pix_fmt yuv420p captures/<scene-id>.mp4
```

Convert any other recording (a QuickTime screen recording) to a constant
60 fps plate before it goes into `captures/`:

```sh
ffmpeg -y -i in.mov -vsync cfr -r 60 -c:v libx264 -preset slow -crf 14 -pix_fmt yuv420p \
  -an captures/<scene-id>.mp4
```

ffmpeg 8.1 warns that `-vsync` is deprecated. `-fps_mode cfr` is equal. Do not burn text into a plate with `drawtext`: the local ffmpeg has
no `drawtext`, `subtitles` or `ass` filter. Render text in Chrome as HTML.

## Self-test

Run the example take 2 times in a temp folder. The decoded frame hashes
(`ffmpeg -v error -i <plate> -f framemd5 - | grep -v '^#' | md5`) must be
equal. A difference means a real-time source that the take does not lock:
a CSS animation, a live request or a `<video>` element.
