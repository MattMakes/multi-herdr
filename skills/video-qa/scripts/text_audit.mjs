#!/usr/bin/env node
// Text audit over time for 1 HTML scene. Node built-ins only; Chrome is driven over the
// Chrome DevTools Protocol. The script steps window.__seekToFrame every --step seconds and
// reads every visible text item: DOM text nodes (box from a Range, font size from the
// computed style) and canvas text (a fillText/strokeText wrapper installed before load).
//
//   text_audit.mjs --scene <index.html> [--out <report.json>] [--duration <sec>]
//                  [--fps <n>] [--width <px>] [--height <px>] [--step 0.1] [--min-phone-px 8]
//   text_audit.mjs --self-test
//
// Size, fps and duration default to window.__videoMeta, then 1920x1080, 30 fps.
// Exit codes: 0 no finding, 1 at least 1 finding or a scene failure, 2 usage error or no Chrome.

import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const DEFAULT_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const CDP_TIMEOUT_MS = 60_000;
const CHROME_START_MS = 15_000;
const CHROME_TRIES = 3;
const SELF = fileURLToPath(import.meta.url);
const FIXTURES = join(dirname(SELF), "fixtures", "text-audit");

export const PHONE_WIDTH = 360; // px: the width a phone shows the whole video at
export const RULES = ["too_small", "too_short", "overlap", "off_frame", "safe_zone", "placeholder"];
const CAPTION_ZONE_Y = 0.82; // same zone as qa_video.py safe_zone_flash
const APP_ZONE_X = 0.85; // portrait only: the right UI column of short-form apps
const EDGE_TOLERANCE_PX = 1;
const SETTLED_PX = 1; // a box that moved at most this much since the previous sample is settled
const OVERLAP_MIN = 0.1; // share of the smaller box that must be covered
const MIN_OPACITY = 0.05;
const PLACEHOLDER = /\bNaN\b|\bundefined\b|\[object Object\]/;
const OWNER = "video-animator";

class UsageError extends Error {}
const fail = (message, Kind = Error) => {
  throw new Kind(message);
};
const check = (cond, message, Kind) => cond || fail(message, Kind);
const chromePath = () => process.env.CHROME_PATH || DEFAULT_CHROME;

// ---------------------------------------------------------------- rules (pure)

export const minShowSec = (text) => 0.4 + 0.2 * words(text);
const words = (text) => text.split(/\s+/).filter(Boolean).length;
// A counter keeps 1 identity while its digits change: "Users: 1,234" -> "Users: #".
const normText = (text) => text.replace(/\s+/g, " ").trim().replace(/\d[\d.,]*/g, "#");
const itemKey = (item) => `${item.key}|${normText(item.text)}`;
const right = (b) => b[0] + b[2];
const bottom = (b) => b[1] + b[3];
const area = (b) => b[2] * b[3];
export const clock = (sec) => {
  const ms = Math.round(sec * 1000);
  const mm = String(Math.floor(ms / 60000)).padStart(2, "0");
  return `${mm}:${String(Math.floor((ms % 60000) / 1000)).padStart(2, "0")}.${String(ms % 1000).padStart(3, "0")}`;
};
const round = (n, d = 2) => Number(n.toFixed(d));
const label = (text) => (text.length > 60 ? `${text.slice(0, 57)}...` : text);

function intersection(a, b) {
  const w = Math.min(right(a), right(b)) - Math.max(a[0], b[0]);
  const h = Math.min(bottom(a), bottom(b)) - Math.max(a[1], b[1]);
  return w > 0 && h > 0 ? w * h : 0;
}

function overlapShare(a, b) {
  let best = 0;
  for (const fa of a.frags) {
    for (const fb of b.frags) best = Math.max(best, intersection(fa, fb) / Math.min(area(fa), area(fb)));
  }
  return best;
}

const settled = (box, prev) => prev !== undefined && box.every((v, i) => Math.abs(v - prev[i]) <= SETTLED_PX);

function offFrame(box, frame) {
  const t = EDGE_TOLERANCE_PX;
  return box[0] < -t || box[1] < -t || right(box) > frame.width + t || bottom(box) > frame.height + t;
}

function safeZone(box, frame) {
  if (bottom(box) > CAPTION_ZONE_Y * frame.height) return "caption zone (bottom 18 %)";
  if (frame.height > frame.width && right(box) > APP_ZONE_X * frame.width) return "app zone (right 15 %)";
  return null;
}

// samples: [{ time_sec, items: [{ key, text, px, box: [x, y, w, h], frags: [[x, y, w, h], ...] }] }]
// opts: { width, height, step, minPhonePx }. Returns the findings, sorted by time.
export function audit(samples, opts) {
  const findings = [];
  const seen = new Set();
  const once1 = (id, finding) => seen.has(id) || (seen.add(id), findings.push({ ...finding, owner: OWNER }));
  const phoneScale = PHONE_WIDTH / opts.width;
  const prevBox = new Map();
  const runs = new Map(); // key -> { text, start, count, last }
  const closeRun = (key, run) => {
    const shown = run.count * opts.step;
    const need = minShowSec(run.text);
    if (shown + 1e-9 < need) {
      findings.push({ rule: "too_short", text: label(run.text), time_sec: run.start, owner: OWNER,
        detail: `shown ${round(shown)} s, needs ${round(need)} s (0.4 + 0.2 s x ${words(run.text)} words)`,
        shown_sec: round(shown), need_sec: round(need) });
    }
    runs.delete(key);
  };
  samples.forEach((sample, index) => {
    const t = round(sample.time_sec, 3);
    const keyed = sample.items.map((item) => ({ ...item, id: itemKey(item) }));
    const nowBox = new Map();
    for (const item of keyed) {
      nowBox.set(item.id, item.box);
      const run = runs.get(item.id);
      if (run && run.last === index - 1) {
        run.count += 1;
        run.last = index;
      } else {
        if (run) closeRun(item.id, run);
        runs.set(item.id, { text: item.text, start: t, count: 1, last: index });
      }
      const phonePx = item.px * phoneScale;
      if (phonePx < opts.minPhonePx) {
        once1(`too_small|${item.id}`, { rule: "too_small", text: label(item.text), time_sec: t,
          detail: `${round(phonePx)} px at ${PHONE_WIDTH} px width (font ${round(item.px)} px), needs ${opts.minPhonePx} px`,
          phone_px: round(phonePx) });
      }
      if (PLACEHOLDER.test(item.text)) {
        once1(`placeholder|${item.id}`, { rule: "placeholder", text: label(item.text), time_sec: t,
          detail: `shows "${PLACEHOLDER.exec(item.text)[0]}"` });
      }
      const still = settled(item.box, prevBox.get(item.id));
      if (still && offFrame(item.box, opts)) {
        once1(`off_frame|${item.id}`, { rule: "off_frame", text: label(item.text), time_sec: t,
          detail: `box ${item.box.map((v) => Math.round(v)).join(",")} leaves the ${opts.width}x${opts.height} frame`,
          box: item.box.map((v) => round(v, 1)) });
      }
      const zone = still ? safeZone(item.box, opts) : null;
      if (zone) {
        once1(`safe_zone|${item.id}`, { rule: "safe_zone", text: label(item.text), time_sec: t,
          detail: `inside the ${zone}`, box: item.box.map((v) => round(v, 1)) });
      }
    }
    for (let i = 0; i < keyed.length; i++) {
      for (let j = i + 1; j < keyed.length; j++) {
        const [a, b] = [keyed[i], keyed[j]];
        if (normText(a.text) === normText(b.text)) continue; // a duplicate layer (glow, outline) of the same words
        const share = overlapShare(a, b);
        if (share < OVERLAP_MIN) continue;
        const pair = [a.id, b.id].sort().join("||");
        once1(`overlap|${pair}`, { rule: "overlap", text: label(`${a.text} / ${b.text}`), time_sec: t,
          detail: `${Math.round(share * 100)} % of the smaller box is covered` });
      }
    }
    prevBox.clear();
    nowBox.forEach((box, id) => prevBox.set(id, box));
  });
  runs.forEach((run, key) => closeRun(key, run));
  return findings.sort((a, b) => a.time_sec - b.time_sec || a.rule.localeCompare(b.rule));
}

export const sampleTimes = (duration, step) => {
  const n = Math.max(1, Math.ceil(duration / step - 1e-9));
  return Array.from({ length: n }, (_, i) => round(i * step, 6));
};

// ---------------------------------------------------------------- in-page scripts

// Installed before any scene script runs. Each canvas keeps the text drawn on it. A
// clearRect or fillRect that covers a text box removes that text.
const CANVAS_HOOK = `(() => {
  const P = CanvasRenderingContext2D.prototype;
  const store = new WeakMap();
  const list = (c) => { if (!store.has(c)) store.set(c, []); return store.get(c); };
  const bbox = (m, l, t, r, b) => {
    const pts = [[l, t], [r, t], [l, b], [r, b]].map(([x, y]) => [m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f]);
    const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
    return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
  };
  function record(ctx, text, x, y, maxWidth) {
    const s = String(text), m = ctx.measureText(s), t = ctx.getTransform();
    const k = maxWidth !== undefined && m.width > maxWidth ? maxWidth / m.width : 1;
    const box = bbox(t, x - m.actualBoundingBoxLeft * k, y - m.actualBoundingBoxAscent,
      x + m.actualBoundingBoxRight * k, y + m.actualBoundingBoxDescent);
    const px = parseFloat((/([\\d.]+)px/.exec(ctx.font) || [0, 0])[1]) * Math.sqrt(Math.abs(t.a * t.d - t.b * t.c));
    list(ctx.canvas).push({ text: s, box, px, alpha: ctx.globalAlpha });
  }
  function cover(ctx, x, y, w, h) {
    const r = bbox(ctx.getTransform(), x, y, x + w, y + h);
    const items = list(ctx.canvas);
    const keep = items.filter((i) => !(i.box[0] >= r[0] && i.box[1] >= r[1] && i.box[2] <= r[2] && i.box[3] <= r[3]));
    items.splice(0, items.length, ...keep);
  }
  for (const name of ["fillText", "strokeText"]) {
    const orig = P[name];
    P[name] = function (text, x, y, maxWidth) { record(this, text, x, y, maxWidth); return orig.apply(this, arguments); };
  }
  for (const name of ["clearRect", "fillRect"]) {
    const orig = P[name];
    P[name] = function (x, y, w, h) { cover(this, x, y, w, h); return orig.apply(this, arguments); };
  }
  window.__textAuditCanvas = (c) => store.get(c) || [];
})();`;

// Returns the visible text items of the current frame, in CSS px of the viewport.
const SAMPLE_FN = `() => {
  const SKIP = new Set(["SCRIPT", "STYLE", "NOSCRIPT", "TEMPLATE", "TITLE"]);
  const W = innerWidth, H = innerHeight;
  const opacityOf = (el) => {
    let o = 1;
    for (let e = el; e && e.nodeType === 1; e = e.parentElement) {
      const s = getComputedStyle(e);
      if (s.display === "none") return 0;
      o *= Number(s.opacity);
    }
    return getComputedStyle(el).visibility === "visible" ? o : 0;
  };
  // Clip to the overflow boxes of the ancestors (a mask reveal hides its text). A box that
  // holds the whole frame (the stage, body) is skipped, so text cut by the frame edge counts.
  const clipBy = (el) => {
    let c = [-1e9, -1e9, 1e9, 1e9];
    for (let e = el; e && e !== document.body && e !== document.documentElement; e = e.parentElement) {
      const s = getComputedStyle(e);
      if (s.overflowX === "visible" && s.overflowY === "visible") continue;
      const r = e.getBoundingClientRect();
      if (r.left <= 0 && r.top <= 0 && r.right >= W && r.bottom >= H) continue;
      c = [Math.max(c[0], r.left), Math.max(c[1], r.top), Math.min(c[2], r.right), Math.min(c[3], r.bottom)];
    }
    return c;
  };
  const cut = (r, c) => {
    const x0 = Math.max(r[0], c[0]), y0 = Math.max(r[1], c[1]);
    const x1 = Math.min(r[0] + r[2], c[2]), y1 = Math.min(r[1] + r[3], c[3]);
    return x1 > x0 && y1 > y0 ? [x0, y0, x1 - x0, y1 - y0] : null;
  };
  const inFrame = (r) => r[0] < W && r[1] < H && r[0] + r[2] > 0 && r[1] + r[3] > 0;
  const union = (fs) => {
    const x0 = Math.min(...fs.map((f) => f[0])), y0 = Math.min(...fs.map((f) => f[1]));
    const x1 = Math.max(...fs.map((f) => f[0] + f[2])), y1 = Math.max(...fs.map((f) => f[1] + f[3]));
    return [x0, y0, x1 - x0, y1 - y0];
  };
  const scaleOf = (el) => {
    if (el instanceof SVGElement) { const m = el.getScreenCTM(); return m ? Math.sqrt(Math.abs(m.a * m.d - m.b * m.c)) : 1; }
    return el.offsetHeight ? el.getBoundingClientRect().height / el.offsetHeight : 1;
  };
  const pathOf = (el) => {
    const parts = [];
    for (let e = el; e && e.parentElement; e = e.parentElement) {
      parts.unshift(e.tagName.toLowerCase() + ":" + Array.prototype.indexOf.call(e.parentElement.children, e));
    }
    return parts.join("/");
  };
  const items = [];
  const add = (key, text, px, rects, el, alpha) => {
    const c = clipBy(el);
    const frags = rects.map((r) => cut(r, c)).filter(Boolean).filter(inFrame);
    if (!frags.length || alpha <= ${MIN_OPACITY}) return;
    items.push({ key, text, px, box: union(frags), frags });
  };
  const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n.parentElement, text = n.nodeValue.replace(/\\s+/g, " ").trim();
    if (!el || !text || SKIP.has(el.tagName)) continue;
    const range = document.createRange();
    range.selectNodeContents(n);
    const rects = Array.from(range.getClientRects(), (r) => [r.left, r.top, r.width, r.height]).filter((r) => r[2] > 0 && r[3] > 0);
    const px = parseFloat(getComputedStyle(el).fontSize) * scaleOf(el);
    add(pathOf(el) + "#" + Array.prototype.indexOf.call(el.childNodes, n), text, px, rects, el, opacityOf(el));
  }
  Array.from(document.querySelectorAll("canvas")).forEach((cv, ci) => {
    const r = cv.getBoundingClientRect();
    if (!cv.width || !cv.height) return;
    const sx = r.width / cv.width, sy = r.height / cv.height, op = opacityOf(cv);
    for (const t of window.__textAuditCanvas(cv)) {
      const text = t.text.replace(/\\s+/g, " ").trim();
      if (!text) continue;
      const box = [r.left + t.box[0] * sx, r.top + t.box[1] * sy, (t.box[2] - t.box[0]) * sx, (t.box[3] - t.box[1]) * sy];
      const visible = cut(box, [r.left, r.top, r.right, r.bottom]);
      add("canvas:" + ci, text, t.px * sy, visible ? [visible] : [], cv, op * t.alpha);
    }
  });
  return items;
}`;

const READY_SCRIPT = `(async () => {
  await document.fonts.ready;
  await Promise.all(Array.from(document.images, (img) => img.decode().catch(() => null)));
  return { seek: typeof window.__seekToFrame, meta: window.__videoMeta ?? null };
})()`;

const sampleScript = (frame, fps) => `(async () => {
  await window.__seekToFrame(${frame}, ${fps});
  await document.fonts.ready;
  return (${SAMPLE_FN})();
})()`;

// ---------------------------------------------------------------- CDP client

function withTimeout(promise, what, ms = CDP_TIMEOUT_MS, Kind = Error) {
  const { promise: late, reject } = Promise.withResolvers();
  const timer = setTimeout(reject, ms, new Kind(`${what}: no reply in ${ms / 1000} s`));
  return Promise.race([promise, late]).finally(() => clearTimeout(timer));
}

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 0;
    this.pending = new Map();
    this.waiters = new Map();
    ws.addEventListener("message", (event) => this.onMessage(JSON.parse(event.data)));
    ws.addEventListener("close", () => {
      this.pending.forEach((settle) => settle({ error: { message: "CDP socket closed" } }));
      this.pending.clear();
    });
  }

  onMessage(msg) {
    if (msg.id === undefined) {
      const key = `${msg.sessionId ?? ""}|${msg.method}`;
      this.waiters.get(key)?.(msg.params);
      this.waiters.delete(key);
      return;
    }
    const settle = this.pending.get(msg.id);
    this.pending.delete(msg.id);
    settle?.(msg);
  }

  send(method, params = {}, sessionId) {
    const id = ++this.nextId;
    const reply = Promise.withResolvers();
    this.pending.set(id, (msg) => (msg.error ? reply.reject(new Error(`${method}: ${msg.error.message}`)) : reply.resolve(msg.result)));
    this.ws.send(JSON.stringify({ id, method, params, sessionId }));
    return withTimeout(reply.promise, method);
  }

  once(method, sessionId) {
    const seen = Promise.withResolvers();
    this.waiters.set(`${sessionId ?? ""}|${method}`, seen.resolve);
    const timed = withTimeout(seen.promise, `${method} event`);
    timed.catch(() => undefined);
    return timed;
  }
}

class ChromeStartTimeout extends Error {}

const chromeArgs = (userDataDir) => [
  "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${userDataDir}`,
  "--window-size=1920,1920", "--force-device-scale-factor=1", "--disable-gpu",
  "--font-render-hinting=none", "--hide-scrollbars", "--mute-audio",
  "--disable-background-timer-throttling", "--disable-renderer-backgrounding",
  "--disable-backgrounding-occluded-windows", "--disable-extensions",
  "--no-first-run", "--no-default-browser-check", "--disable-sync",
  "--disable-component-update", "about:blank",
];

function devtoolsUrl(proc) {
  const found = Promise.withResolvers();
  let log = "";
  proc.stderr.on("data", (data) => {
    log = (log + data).slice(-4000);
    const m = log.match(/DevTools listening on (ws:\/\/\S+)/);
    if (m) found.resolve(m[1]);
  });
  proc.on("error", (e) => found.reject(new Error(`Chrome failed to start: ${e.message}`)));
  proc.on("exit", (code) => found.reject(new Error(`Chrome exited with code ${code}: ${log.trim().slice(-500)}`)));
  return withTimeout(found.promise, "Chrome DevTools URL", CHROME_START_MS, ChromeStartTimeout);
}

async function openSocket(url) {
  const ws = new WebSocket(url);
  const opened = Promise.withResolvers();
  ws.addEventListener("open", opened.resolve, { once: true });
  ws.addEventListener("error", () => opened.reject(new Error(`cannot connect to ${url}`)), { once: true });
  await opened.promise;
  return ws;
}

const pidList = (out) => out.split(/\s+/).filter(Boolean);
const childPids = (pid) => pidList(spawnSync("pgrep", ["-P", String(pid)], { encoding: "utf8" }).stdout ?? "");
const livePids = (pids) => (pids.length ? pidList(spawnSync("ps", ["-o", "pid=", "-p", pids.join(",")], { encoding: "utf8" }).stdout ?? "") : []);

// Chrome's helpers (network service, GPU, renderers) outlive the main process for some ms and
// write into the user-data dir as they exit. Wait up to 5 s for them, then SIGKILL the rest.
async function helpersGone(pids) {
  for (let tries = 0; tries < 500; tries++) {
    if (!livePids(pids).length) return;
    await sleep(10);
  }
  const alive = livePids(pids);
  if (alive.length) spawnSync("kill", ["-KILL", ...alive]);
}

async function stopChrome(browser) {
  browser.ws?.close();
  const { proc } = browser;
  const helpers = childPids(proc.pid);
  if (proc.exitCode === null && proc.signalCode === null) {
    const exited = once(proc, "exit");
    proc.kill("SIGTERM");
    if (!(await Promise.race([exited.then(() => true), sleep(5000, false)]))) proc.kill("SIGKILL");
  }
  await helpersGone(helpers);
  rmSync(browser.userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

async function launchChrome(tries = CHROME_TRIES) {
  const binary = chromePath();
  check(existsSync(binary), `Chrome not found at ${binary}. Set CHROME_PATH or install Google Chrome.`, UsageError);
  const userDataDir = mkdtempSync(join(tmpdir(), "video-qa-text-audit-chrome-"));
  const proc = spawn(binary, chromeArgs(userDataDir), { stdio: ["ignore", "ignore", "pipe"] });
  const browser = { proc, userDataDir, ws: null };
  try {
    browser.ws = await openSocket(await devtoolsUrl(proc));
  } catch (e) {
    // A start that hangs before the DevTools URL leaves stuck children; kill only those.
    if (e instanceof ChromeStartTimeout) spawnSync("pkill", ["-KILL", "-P", String(proc.pid)]);
    await stopChrome(browser);
    if (e instanceof ChromeStartTimeout && tries > 1) return launchChrome(tries - 1);
    throw e;
  }
  browser.cdp = new Cdp(browser.ws);
  return browser;
}

async function evaluate(cdp, sessionId, expression) {
  const res = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
  const d = res.exceptionDetails;
  return d ? fail(`page script failed: ${d.exception?.description ?? d.text}`) : res.result.value;
}

// ---------------------------------------------------------------- scene audit

const sceneUrl = (target) => (/^[a-z][a-z0-9+.-]*:/i.test(target) ? target : pathToFileURL(resolve(target)).href);
const pick = (...values) => values.find((v) => v !== undefined && v !== null);

async function sampleScene(cdp, opts) {
  const { targetInfos } = await cdp.send("Target.getTargets");
  const page = targetInfos.find((t) => t.type === "page") ?? fail("Chrome opened no page target");
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: page.targetId, flatten: true });
  const viewport = (width, height) =>
    cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, sessionId);
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: CANVAS_HOOK }, sessionId);
  await viewport(opts.width ?? 1920, opts.height ?? 1080);
  const loaded = cdp.once("Page.loadEventFired", sessionId);
  const nav = await cdp.send("Page.navigate", { url: sceneUrl(opts.scene) }, sessionId);
  check(!nav.errorText, `navigation to ${opts.scene} failed: ${nav.errorText}`);
  await loaded;
  const ready = await evaluate(cdp, sessionId, READY_SCRIPT);
  check(ready.seek === "function", `${opts.scene}: typeof window.__seekToFrame is "${ready.seek}", expected "function"`);
  const meta = ready.meta ?? {};
  const frame = {
    width: pick(opts.width, meta.width, 1920),
    height: pick(opts.height, meta.height, 1080),
    fps: pick(opts.fps, meta.fps, 30),
    duration: pick(opts.duration, meta.duration),
  };
  check(frame.duration > 0, "pass --duration or set window.__videoMeta.duration in the scene", UsageError);
  await viewport(frame.width, frame.height);
  const last = Math.max(0, Math.round(frame.duration * frame.fps) - 1);
  const samples = [];
  for (const time_sec of sampleTimes(frame.duration, opts.step)) {
    const f = Math.min(last, Math.round(time_sec * frame.fps));
    samples.push({ time_sec, items: await evaluate(cdp, sessionId, sampleScript(f, frame.fps)) });
  }
  return { frame, samples };
}

export async function auditScene(opts) {
  const browser = await launchChrome();
  const { frame, samples } = await sampleScene(browser.cdp, opts).finally(() => stopChrome(browser));
  const findings = audit(samples, { width: frame.width, height: frame.height, step: opts.step, minPhonePx: opts.minPhonePx });
  const items = new Set(samples.flatMap((s) => s.items.map(itemKey)));
  return {
    passed: findings.length === 0,
    scene: opts.scene,
    width: frame.width, height: frame.height, fps: frame.fps, duration_sec: frame.duration,
    step_sec: opts.step, samples: samples.length, text_items: items.size,
    phone_width_px: PHONE_WIDTH, min_phone_px: opts.minPhonePx,
    rules: RULES,
    failed_rules: RULES.filter((r) => findings.some((f) => f.rule === r)),
    findings,
  };
}

// ---------------------------------------------------------------- self-test

const json = (v) => JSON.stringify(v);
const assertEq = (actual, expected, what) => check(json(actual) === json(expected), `${what}: expected ${json(expected)}, got ${json(actual)}`);
const ruleSet = (findings) => [...new Set(findings.map((f) => f.rule))].sort();

const item = (key, text, box, px = 64) => ({ key, text, px, box, frags: [box] });
const series = (n, at) => Array.from({ length: n }, (_, i) => ({ time_sec: i * 0.1, items: at(i) }));
const UNIT_OPTS = { width: 1920, height: 1080, step: 0.1, minPhonePx: 8 };

const UNIT_CASES = [
  ["clean title", series(20, () => [item("a", "Ship it", [100, 100, 400, 80])]), []],
  ["tiny label", series(20, () => [item("a", "beta", [100, 100, 60, 20], 20)]), ["too_small"]],
  ["flashed word", series(20, (i) => (i >= 5 && i < 7 ? [item("a", "Now", [100, 100, 200, 80])] : [])), ["too_short"]],
  ["4 words for 1 s", series(20, (i) => (i < 10 ? [item("a", "one two three four", [100, 100, 600, 80])] : [])), ["too_short"]],
  ["counter keeps 1 identity", series(20, (i) => [item("a", `Users: ${i * 97}`, [100, 100, 400, 80])]), []],
  ["overlap", series(20, () => [item("a", "Fast", [100, 100, 300, 80]), item("b", "Zero", [150, 120, 300, 80])]), ["overlap"]],
  ["duplicate layer", series(20, () => [item("a", "Glow", [100, 100, 300, 80]), item("b", "Glow", [102, 102, 300, 80])]), []],
  ["off frame", series(20, () => [item("a", "Launch", [1700, 100, 400, 80])]), ["off_frame"]],
  ["slide in", series(20, (i) => [item("a", "Launch", [Math.min(100, -400 + i * 100), 100, 400, 80])]), []],
  ["caption zone", series(20, () => [item("a", "Swipe", [100, 950, 300, 80])]), ["safe_zone"]],
  ["placeholder", series(20, () => [item("a", "Total: NaN", [100, 100, 400, 80])]), ["placeholder"]],
];

function testUnits() {
  for (const [name, samples, rules] of UNIT_CASES) assertEq(ruleSet(audit(samples, UNIT_OPTS)), rules, name);
  const portrait = { ...UNIT_OPTS, width: 1080, height: 1920 };
  assertEq(ruleSet(audit(series(10, () => [item("a", "Like", [950, 500, 100, 80], 72)]), portrait)), ["safe_zone"], "app zone");
  assertEq(minShowSec("one two"), 0.8, "min show for 2 words");
  assertEq(sampleTimes(0.3, 0.1), [0, 0.1, 0.2], "sample times");
  assertEq(clock(61.5), "01:01.500", "clock");
}

function testNoChrome() {
  const env = { ...process.env, CHROME_PATH: join(tmpdir(), "no-such-chrome") };
  const res = spawnSync(process.execPath, [SELF, "--scene", join(FIXTURES, "clean.html")], { env, encoding: "utf8" });
  assertEq(res.status, 2, "exit code without Chrome");
  check(res.stderr.includes("Chrome not found"), `stderr names the missing Chrome: ${res.stderr}`);
}

const FIXTURE_CASES = [
  ["fail-four.html", ["overlap", "placeholder", "too_short", "too_small"]],
  ["fail-position.html", ["off_frame", "safe_zone"]],
  ["clean.html", []],
];

async function testFixture([name, rules]) {
  const report = await auditScene({ scene: join(FIXTURES, name), step: 0.1, minPhonePx: 8 });
  const lines = report.findings.map((f) => `${f.rule} ${f.text}`).join("; ");
  assertEq(ruleSet(report.findings), rules, `${name} rules (${lines})`);
  check(report.findings.length === rules.length, `${name}: expected ${rules.length} findings, got ${lines}`);
  check(report.text_items > 0, `${name}: no text items found`);
}

async function selfTest() {
  const tests = [["rules on synthetic samples", testUnits], ["exit 2 without Chrome", testNoChrome]];
  const chrome = existsSync(chromePath());
  for (const c of FIXTURE_CASES) tests.push([`fixture ${c[0]}`, () => testFixture(c)]);
  let failed = 0;
  for (const [name, fn] of tests) {
    if (name.startsWith("fixture") && !chrome) {
      console.log(`ok - ${name} # SKIP no Chrome at ${chromePath()}`);
      continue;
    }
    try {
      await fn();
      console.log(`ok - ${name}`);
    } catch (e) {
      failed += 1;
      console.log(`not ok - ${name}: ${e.message}`);
    }
  }
  console.log(failed ? `text_audit self-test: ${failed} failed` : "text_audit self-test: all passed");
  return failed ? 1 : 0;
}

// ---------------------------------------------------------------- CLI

const HELP = `usage: text_audit.mjs --scene <index.html> [--out <report.json>] [--duration <sec>]
       [--fps <n>] [--width <px>] [--height <px>] [--step 0.1] [--min-phone-px 8]
       text_audit.mjs --self-test`;

function positive(text, flag) {
  if (text === undefined) return undefined;
  const n = Number(text);
  return n > 0 ? n : fail(`${flag}: "${text}" is not a positive number`, UsageError);
}

function cliOptions(argv) {
  const { values } = parseArgs({
    args: argv,
    options: {
      scene: { type: "string" }, out: { type: "string" }, duration: { type: "string" },
      fps: { type: "string" }, width: { type: "string" }, height: { type: "string" },
      step: { type: "string", default: "0.1" }, "min-phone-px": { type: "string", default: "8" },
      "self-test": { type: "boolean" }, help: { type: "boolean", short: "h" },
    },
  });
  return values;
}

async function main() {
  const v = cliOptions(process.argv.slice(2));
  if (v.help) return console.log(HELP) ?? 0;
  if (v["self-test"]) return selfTest();
  check(v.scene, `--scene is required\n${HELP}`, UsageError);
  check(/^[a-z][a-z0-9+.-]*:/i.test(v.scene) || existsSync(v.scene), `scene not found: ${v.scene}`, UsageError);
  const report = await auditScene({
    scene: v.scene, out: v.out, duration: positive(v.duration, "--duration"), fps: positive(v.fps, "--fps"),
    width: positive(v.width, "--width"), height: positive(v.height, "--height"),
    step: positive(v.step, "--step"), minPhonePx: positive(v["min-phone-px"], "--min-phone-px"),
  });
  if (v.out) {
    mkdirSync(dirname(resolve(v.out)), { recursive: true });
    writeFileSync(v.out, `${JSON.stringify(report, null, 2)}\n`);
  }
  for (const f of report.findings) console.log(`FAIL ${f.rule} [${clock(f.time_sec)}] "${f.text}": ${f.detail} -> ${f.owner}`);
  console.log(`${report.passed ? "PASS" : "FAIL"} text audit ${v.scene}: ${report.samples} samples, ${report.text_items} text items, ${report.findings.length} findings`);
  return report.passed ? 0 : 1;
}

if (process.argv[1] && resolve(process.argv[1]) === SELF) {
  main().then(
    (code) => process.exit(code),
    (e) => {
      console.error(`text_audit: ${e.message}`);
      process.exit(e instanceof UsageError || String(e.code).startsWith("ERR_PARSE_ARGS") ? 2 : 1);
    },
  );
}
