// capture.mjs - deterministic CDP capture. Node built-ins only.
// Usage: node "<skill-dir>/scripts/capture.mjs" <project-dir> <scene-id> [--timings]
//        node "<skill-dir>/scripts/capture.mjs" --self-test [--timings]
// --timings prints the real time of each phase (Chrome start, page step, screenshot, encode).
import { execFile, spawn, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const scriptsDir = dirname(fileURLToPath(import.meta.url));
if (process.argv[2] === '--self-test') process.exit(await selfTest(process.argv.includes('--timings')));
const tm = {}, tStart = performance.now(); // --timings: real ms spent in each phase
async function timed(phase, fn) {
  const t = performance.now();
  try { return await fn(); } finally { tm[phase] = (tm[phase] || 0) + performance.now() - t; }
}

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
  const realRaf = requestAnimationFrame.bind(globalThis); // the harness waits on real frames, the page never sees them
  globalThis.requestAnimationFrame = (fn) => { const id = nextId++; rafs.set(id, fn); return id; };
  globalThis.cancelAnimationFrame = (id) => rafs.delete(id);
  const run = (fn, a) => { try { typeof fn === 'function' && fn(...a); } catch (e) { reportError(e); } };
  let inflight = 0; const landed = () => { inflight--; }; // the harness waits on this count
  const realFetch = fetch.bind(globalThis), realSend = XMLHttpRequest.prototype.send;
  globalThis.fetch = (...a) => { inflight++; return realFetch(...a).finally(landed); };
  XMLHttpRequest.prototype.send = function (...a) { inflight++; this.addEventListener('loadend', landed, { once: true }); return realSend.apply(this, a); };
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
      return new Promise((done) => realRaf(() => realRaf(done))); // resolve after a real frame holds this state
    },
    inflight: () => inflight,
    ready: async () => { await document.fonts.ready; await Promise.all([...document.images].map((i) => i.decode().catch(() => {}))); return document.fonts.status; },
  };
})();`;

// ---- Chrome + CDP ----
const userDir = mkdtempSync(join(tmpdir(), 'cap-chrome-'));
const chromePath = process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const chromeArgs = ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${userDir}`,
  '--hide-scrollbars', '--mute-audio', '--no-first-run', '--no-default-browser-check',
  '--force-color-profile=srgb', '--font-render-hinting=none', '--disable-lcd-text', 'about:blank'];
// Chrome can deadlock in its own start-up fork, before it prints the DevTools URL. Give each start 15 s,
// then kill that Chrome and its child processes and start again, 3 tries in total.
function launchChrome() {
  const p = spawn(chromePath, chromeArgs, { stdio: ['ignore', 'ignore', 'pipe'] });
  const url = new Promise((ok, fail) => {
    let buf = '';
    p.stderr.on('data', (d) => { buf += d; const m = buf.match(/DevTools listening on (ws:\/\/\S+)/); if (m) ok(m[1]); });
    p.on('exit', (c) => fail(new Error(`chrome exited ${c}`)));
    setTimeout(ok, 15000, '').unref();
  });
  return { p, url };
}
async function startChrome(tries) {
  const c = launchChrome(), url = await c.url;
  if (url) return { chrome: c.p, wsUrl: url };
  spawnSync('pkill', ['-KILL', '-P', String(c.p.pid)]); // only the children of this stuck Chrome
  c.p.kill('SIGKILL');
  if (tries <= 1) throw new Error('chrome did not print its DevTools URL in 15 s, 3 tries');
  return startChrome(tries - 1);
}
const { chrome, wsUrl } = await timed('chrome-start', () => startChrome(3));
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
const held = []; // fixture responses with delay_ms, sent when the virtual clock reaches `at`
const fulfil = ({ requestId, fx }) => {
  const body = Buffer.from(typeof fx.body === 'string' ? fx.body : JSON.stringify(fx.json ?? {})).toString('base64');
  return send('Fetch.fulfillRequest', { requestId, responseCode: fx.status ?? 200, body,
    responseHeaders: [{ name: 'Content-Type', value: fx.type ?? 'application/json' }, { name: 'Access-Control-Allow-Origin', value: '*' }] });
};
const isLocal = (url) => /^(file:|data:|about:)/.test(url) || cfg.allow.some((p) => url.startsWith(p));
const hold = ({ requestId }, fx) => held.push({ requestId, fx, at: vt + fx.delay_ms });
const passThrough = ({ requestId }) => send('Fetch.continueRequest', { requestId });
function block({ requestId, request }) {
  errors.push(`blocked live request: ${request.url}`);
  return send('Fetch.failRequest', { requestId, errorReason: 'BlockedByClient' });
}
const ROUTES = [ // first match wins: a delayed fixture, a fixture, a local URL, then everything else fails
  [(r, fx) => fx?.delay_ms > 0, hold],
  [(r, fx) => fx, ({ requestId }, fx) => fulfil({ requestId, fx })],
  [(r) => isLocal(r.request.url), passThrough],
  [() => true, block],
];
const route = (r) => { const fx = cfg.fixtures[r.request.url]; return ROUTES.find(([when]) => when(r, fx))[1](r, fx); };
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
const pause = (ms) => new Promise((ok) => setTimeout(ok, ms));
const heldOnly = (n) => n <= held.length; // no request is in flight except the held ones
const isDue = (d) => d.at <= vt + 1e-6;
const sendHeld = (d) => { held.splice(held.indexOf(d), 1); return fulfil(d); };
const inflightCalm = async (isCalm) => isCalm(await evaluate('__cap.inflight()'));
const retryCalm = (isCalm, tries) => pause(10).then(() => pageCalm(isCalm, tries - 1));
// Poll the page request count for up to 2 s of real time.
const pageCalm = async (isCalm, tries = 200) => (tries === 0 || (await inflightCalm(isCalm)) ? undefined : retryCalm(isCalm, tries));
const sendAll = (list) => list.reduce((chain, d) => chain.then(() => sendHeld(d)), Promise.resolve());
const settleAfter = (due) => (due.length ? pageCalm(heldOnly) : undefined);
// Send the delayed responses that are due, then let the page take them.
const release = () => Promise.resolve(held.filter(isDue)).then((due) => sendAll(due).then(() => settleAfter(due)));
const ripplesNow = () => ripples.filter((r) => vt - r.t0 < 240).map((r) => ({ x: r.x, y: r.y, p: (vt - r.t0) / 240 }));
const stepPage = () => evaluate(`__cap.step(${frameMs}, ${pos.x}, ${pos.y}, ${JSON.stringify(ripplesNow())})`);
const screenshot = async () => Buffer.from((await send('Page.captureScreenshot', { format: 'png' })).data, 'base64');
async function advance() { // every request the page issued has reached route(), then the clock moves 1 frame
  await timed('calm', () => pageCalm(heldOnly));
  vt += frameMs;
  await timed('release', release);
}
async function shoot() { // advance the page 1 frame of virtual time, return a PNG
  await advance();
  await timed('step', stepPage);
  return timed('screenshot', screenshot);
}
async function emit(png) {
  if (out >= totalFrames) throw new Error(`scene is longer than timeline duration_sec ${scene.duration_sec} s`);
  out++; if (!ff.stdin.write(png)) await timed('encode-backpressure', () => new Promise((ok) => ff.stdin.once('drain', ok)));
}
async function loadPage(url) {
  await send('Page.navigate', { url });
  for (let i = 0; i < 200 && (await evaluate('document.readyState')) !== 'complete'; i++) await new Promise((r) => setTimeout(r, 25));
  await evaluate('__cap.ready()');
}
const easeInOut = (u) => (u < 0.5 ? 4 * u * u * u : 1 - (-2 * u + 2) ** 3 / 2);
const smooth = (u) => u * u * (3 - 2 * u);

const h = {
  W, H, fps, rand,
  termUrl: pathToFileURL(join(scriptsDir, 'term.html')).href, // the terminal page next to this script
  async goto(url) {
    await timed('navigate', () => loadPage(url));
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
  ff.stdin.end(); const code = await timed('encode-finish', () => ffDone);
  tel.duration_sec = +(out / fps).toFixed(4);
  writeFileSync(join(projectDir, 'captures', `${sceneId}.json`), JSON.stringify(tel, null, 2) + '\n');
  ws.close(); await timed('chrome-exit', () => new Promise((ok) => { chrome.once('exit', ok); chrome.kill(); })); rmSync(userDir, { recursive: true, force: true });
  if (code !== 0) errors.push(`ffmpeg exited ${code}`);
}
if (errors.length) { console.error(errors.join('\n')); process.exit(1); }
console.log(`captures/${sceneId}.mp4: ${out} frames at ${fps} fps`);
if (process.argv.includes('--timings')) printTimings();

function printTimings() {
  const parts = Object.entries(tm).map(([k, v]) => `${k} ${(v / 1000).toFixed(2)} s`);
  console.log(`timing ${sceneId}: total ${((performance.now() - tStart) / 1000).toFixed(2)} s; ${parts.join('; ')}`);
}

// ---- self-test: a web scene and a terminal scene pass the plate gate, console.error fails, 3 runs match ----
// Function declarations only: the self-test runs before the module-level constants above are set.
// The captures run in parallel, each web run in its own project folder. Parallel runs also load the host,
// so the 3 identical runs check determinism under load.
async function selfTest(timings) {
  selfTest.timings = timings;
  const t0 = Date.now(), dir = mkdtempSync(join(tmpdir(), 'cap-selftest-'));
  process.on('exit', rmSync.bind(null, dir, { recursive: true, force: true })); // remove the temp dir on every exit
  const dirs = [1, 2, 3].map((i) => join(dir, `run-${i}`));
  dirs.forEach(writeFixtures);
  const [main] = dirs, webs = Promise.all(dirs.map((d) => captureIn(d, 'web'))), err = captureIn(main, 'err');
  const rec = testRecCast(await runAsync(main, process.execPath, recCastArgs()), main); // term replays this cast
  const term = await captureIn(main, 'term');
  const results = [...rec, ...testDeterminism(dirs, await webs), ...testTelemetry(main), ...testPlates(main, term), ...testErrors(await err)];
  const passed = results.filter(Boolean).length;
  console.log(`capture self-test: ${passed} of ${results.length} checks pass in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
  return passed === results.length ? 0 : 1;
}

function outcome(ok, what) {
  console.log(`${ok ? 'pass' : 'FAIL'}: ${what}`);
  return ok;
}

function runIn(dir, cmd, args) {
  const t = Date.now(), r = spawnSync(cmd, args, { cwd: dir, encoding: 'utf8' });
  if (selfTest.timings) printStepTiming(cmd, args, r, Date.now() - t);
  return r;
}

function runAsync(dir, cmd, args) { // resolves to the spawnSync result shape: { status, stdout, stderr }
  const t = Date.now();
  return new Promise((ok) => execFile(cmd, args, { cwd: dir, encoding: 'utf8' }, (e, stdout, stderr) => {
    const r = { status: e ? e.code ?? 1 : 0, stdout, stderr };
    if (selfTest.timings) printStepTiming(cmd, args, r, Date.now() - t);
    ok(r);
  }));
}

function printStepTiming(cmd, args, r, ms) {
  const name = [cmd, ...args].map((a) => a.split('/').at(-1)).join(' ').slice(0, 60);
  for (const line of r.stdout.split('\n')) if (line.startsWith('timing ')) console.log(`  ${line}`);
  console.log(`timing: ${name}: ${(ms / 1000).toFixed(2)} s`);
}

function captureIn(dir, id) {
  const extra = selfTest.timings ? ['--timings'] : [];
  return runAsync(dir, process.execPath, [fileURLToPath(import.meta.url), '.', id, ...extra]);
}

function exitNote(r) {
  return r.status ? `: ${r.stderr.trim()}` : '';
}

function frameHash(dir, id) {
  return runIn(dir, 'ffmpeg', ['-v', 'error', '-i', `captures/${id}.mp4`, '-f', 'framemd5', '-']).stdout.replace(/^#.*\n/gm, '');
}

function writeFixtures(dir) {
  mkdirSync(join(dir, 'app'), { recursive: true });
  mkdirSync(join(dir, 'captures/src'), { recursive: true });
  const files = fixtureFiles();
  for (const rel of Object.keys(files)) writeFileSync(join(dir, rel), files[rel]);
}

function fixtureFiles() {
  return {
    'timeline.json': fixtureTimeline(),
    'app/index.html': fixtureWebPage(),
    'app/err.html': fixtureErrPage(),
    'captures/src/web.mjs': fixtureWebTake(),
    'captures/src/term.mjs': fixtureTermTake(),
    'captures/src/err.mjs': fixtureErrTake(),
  };
}

function fixtureTimeline() {
  return JSON.stringify({ meta: { fps: 30, width: 640, height: 360 }, scenes: [
      { id: 'web', kind: 'capture', duration_sec: 2.5 },
      { id: 'term', kind: 'capture', duration_sec: 1.5 },
      { id: 'err', kind: 'capture', duration_sec: 0.5 }] });
}

function fixtureWebPage() {
  return `<!doctype html><meta charset="utf-8">
<style>html,body{margin:0;height:100%;background:#0d1117;color:#e6edf3;font:24px/1.4 Menlo,monospace}
input,button{font:inherit}@keyframes spin{to{transform:rotate(360deg)}}
#s{width:24px;height:24px;margin:16px;background:#3fb950;animation:spin 1s linear infinite}</style>
<input id="q" size="10"> <button id="go">Go</button><div id="s"></div><p id="api"></p><p id="res"></p>
<script>
  const x = new XMLHttpRequest(); x.open('GET', 'https://api.example.test/items', false); x.send();
  document.getElementById('api').textContent = 'items: ' + JSON.parse(x.responseText).count;
  document.getElementById('go').onclick = () => fetch('https://api.example.test/slow').then((r) => r.json()).then((j) => {
    document.getElementById('res').textContent = j.n + ' ' + Math.random().toFixed(4) + ' ' + new Date().toISOString();
  });
</script>`;
}

function fixtureErrPage() {
  return '<!doctype html><p>err</p><img src="https://live.example.test/x.png"><script>console.error("boom")</script>';
}

function fixtureWebTake() {
  return `export const config = { fixtures: { 'https://api.example.test/items': { json: { count: 3 } },
  'https://api.example.test/slow': { json: { n: 7 }, delay_ms: 800 } } };
export default async (h) => {
  await h.goto(new URL('../../app/index.html', import.meta.url).href);
  await h.hold(200);
  await h.click('#q');
  await h.zoomTo('#q', 1.6);
  await h.type('ab');
  await h.click('#go');
  await h.wait('document.querySelector("#res").textContent !== ""');
  h.zoom(1);
  await h.hold(200);
};`;
}

function fixtureTermTake() {
  return `export const config = { dsf: 1, cursor: false };
export default async (h) => {
  await h.goto(h.termUrl);
  await h.term('$ ');
  await h.termType('make');
  await h.term('\\r\\n');
  await h.termCast('captures/src/term.cast');
};`;
}

function fixtureErrTake() {
  return `export default async (h) => { await h.goto(new URL('../../app/err.html', import.meta.url).href); await h.hold(100); };`;
}

function recCastArgs() {
  return [join(scriptsDir, 'rec-cast.mjs'), 'captures/src/term.cast', '--', 'sh', '-c', 'echo building; sleep 0.6; echo done'];
}

function testRecCast(rec, dir) {
  const cast = rec.status === 0 ? readFileSync(join(dir, 'captures/src/term.cast'), 'utf8') : '';
  return [outcome(cast.includes('done'), 'rec-cast.mjs records an asciicast v2 file')];
}

function testDeterminism(dirs, webs) {
  const runs = webs.map((r, i) => webRun(dirs[i], r, i + 1));
  const hashes = new Set(runs.map(hashOf));
  const frames = runs[0].hash.split('\n').length - 1;
  const same = hashes.size === 1 && frames > 0;
  return [...runs.map(okOf), outcome(same, `3 web runs give identical decoded frames (${frames} frames)`)];
}

function webRun(dir, r, i) {
  return { ok: outcome(r.status === 0, `web capture run ${i} exits 0${exitNote(r)}`), hash: frameHash(dir, 'web') };
}

function hashOf(run) {
  return run.hash;
}

function okOf(run) {
  return run.ok;
}

function testTelemetry(dir) {
  const tel = JSON.parse(readFileSync(join(dir, 'captures/web.json'), 'utf8'));
  const counts = [tel.width, tel.clicks.length, tel.keys.length, tel.speed_ramps.length];
  const ok = JSON.stringify(counts) === JSON.stringify([1280, 2, 2, 1]) && tel.zoom_keyframes.length >= 3;
  return [outcome(ok, `web telemetry: ${tel.width}x${tel.height}, ${tel.clicks.length} clicks, ${tel.keys.length} keys, ` +
    `${tel.zoom_keyframes.length} zoom keyframes, ${tel.speed_ramps.length} speed ramps`)];
}

function testPlates(dir, term) {
  const results = [outcome(term.status === 0, `term capture exits 0${exitNote(term)}`)];
  for (const id of ['web', 'term']) {
    const g = runIn(dir, 'python3', [join(scriptsDir, 'check_plate.py'), id]);
    results.push(outcome(g.status === 0, `${id} plate gate: ${g.stdout.trim()}`));
  }
  return results;
}

function testErrors(err) {
  const ok = err.status === 1 && err.stderr.includes('console.error: boom') && err.stderr.includes('blocked live request');
  return [outcome(ok, `console.error and a live request make the capture exit 1 (exit ${err.status})`)];
}
