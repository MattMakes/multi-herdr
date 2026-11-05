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

const shareOf = (fa, fb) => intersection(fa, fb) / Math.min(area(fa), area(fb));
const overlapShare = (a, b) => Math.max(0, ...a.frags.flatMap((fa) => b.frags.map((fb) => shareOf(fa, fb))));
const settled = (box, prev) => prev !== undefined && box.every((v, i) => Math.abs(v - prev[i]) <= SETTLED_PX);
const boxOut = (box) => box.map((v) => round(v, 1));

function offFrame(box, frame) {
  const t = EDGE_TOLERANCE_PX;
  return box[0] < -t || box[1] < -t || right(box) > frame.width + t || bottom(box) > frame.height + t;
}

const ZONES = [
  ["caption zone (bottom 18 %)", (box, frame) => bottom(box) > CAPTION_ZONE_Y * frame.height],
  ["app zone (right 15 %)", (box, frame) => frame.height > frame.width && right(box) > APP_ZONE_X * frame.width],
];
const safeZone = (box, frame) => ZONES.find(([, hit]) => hit(box, frame))?.[0];

// Each item rule takes (item, opts, still) and returns a partial finding or null.
// still is true when the box is settled since the previous sample.
function tooSmall(item, opts) {
  const phonePx = (item.px * PHONE_WIDTH) / opts.width;
  if (phonePx >= opts.minPhonePx) return null;
  const detail = `${round(phonePx)} px at ${PHONE_WIDTH} px width (font ${round(item.px)} px), needs ${opts.minPhonePx} px`;
  return { rule: "too_small", detail, phone_px: round(phonePx) };
}

function placeholder(item) {
  const m = PLACEHOLDER.exec(item.text);
  return m ? { rule: "placeholder", detail: `shows "${m[0]}"` } : null;
}

const offFrameFinding = (item, opts) => ({
  rule: "off_frame",
  detail: `box ${item.box.map(Math.round).join(",")} leaves the ${opts.width}x${opts.height} frame`,
  box: boxOut(item.box),
});
const offFrameRule = (item, opts, still) => (still && offFrame(item.box, opts) ? offFrameFinding(item, opts) : null);

function safeZoneRule(item, opts, still) {
  const zone = still && safeZone(item.box, opts);
  return zone ? { rule: "safe_zone", detail: `inside the ${zone}`, box: boxOut(item.box) } : null;
}

const ITEM_RULES = [tooSmall, placeholder, offFrameRule, safeZoneRule];

const overlapFinding = (share) =>
  share < OVERLAP_MIN ? null : { rule: "overlap", detail: `${Math.round(share * 100)} % of the smaller box is covered` };
// The same words on 2 layers (glow, outline) are not an overlap.
const overlapRule = (a, b) => (normText(a.text) === normText(b.text) ? null : overlapFinding(overlapShare(a, b)));

const tooShortFinding = (run, shown, need) => ({
  rule: "too_short",
  detail: `shown ${round(shown)} s, needs ${round(need)} s (0.4 + 0.2 s x ${words(run.text)} words)`,
  shown_sec: round(shown),
  need_sec: round(need),
});

function tooShort(run, step) {
  const shown = run.count * step;
  const need = minShowSec(run.text);
  return shown + 1e-9 >= need ? null : tooShortFinding(run, shown, need);
}

const toFinding = (text, t, partial) => ({ rule: partial.rule, text: label(text), time_sec: t, ...partial, owner: OWNER });
const byTimeThenRule = (a, b) => a.time_sec - b.time_sec || a.rule.localeCompare(b.rule);
const withId = (item) => ({ ...item, id: itemKey(item) });
const idAndBox = (item) => [item.id, item.box];

const ruleChecker = (auditor, item, t, still) => (rule) =>
  auditor.report(item.id, item.text, t, rule(item, auditor.opts, still));
const pairsOf = (auditor, t, keyed) => (a, i) => keyed.slice(i + 1).forEach(auditor.checkPair.bind(auditor, t, a));

// Walks the samples in order. A run is 1 continuous showing of 1 item.
class TextAudit {
  constructor(opts) {
    this.opts = opts;
    this.findings = [];
    this.seen = new Set();
    this.prevBox = new Map();
    this.runs = new Map(); // item id -> { text, start, count, last }
  }

  // Reports each rule once per item (or per pair of items).
  report(id, text, t, partial) {
    const key = partial && `${partial.rule}|${id}`;
    const fresh = Boolean(partial) && !this.seen.has(key);
    return fresh && (this.seen.add(key), this.findings.push(toFinding(text, t, partial)));
  }

  closeRun(run) {
    const partial = tooShort(run, this.opts.step);
    return partial && this.findings.push(toFinding(run.text, run.start, partial));
  }

  extend(run, index) {
    run.count += 1;
    run.last = index;
  }

  restart(item, index, t) {
    const old = this.runs.get(item.id);
    old && this.closeRun(old);
    this.runs.set(item.id, { text: item.text, start: t, count: 1, last: index });
  }

  track(item, index, t) {
    const run = this.runs.get(item.id);
    return run?.last === index - 1 ? this.extend(run, index) : this.restart(item, index, t);
  }

  checkItem(t, index, item) {
    this.track(item, index, t);
    const still = settled(item.box, this.prevBox.get(item.id));
    ITEM_RULES.forEach(ruleChecker(this, item, t, still));
  }

  checkPair(t, a, b) {
    this.report([a.id, b.id].sort().join("||"), `${a.text} / ${b.text}`, t, overlapRule(a, b));
  }

  step(sample, index) {
    const t = round(sample.time_sec, 3);
    const keyed = sample.items.map(withId);
    keyed.forEach(this.checkItem.bind(this, t, index));
    keyed.forEach(pairsOf(this, t, keyed));
    this.prevBox = new Map(keyed.map(idAndBox));
  }

  finish() {
    this.runs.forEach(this.closeRun.bind(this));
    return this.findings.sort(byTimeThenRule);
  }
}

// samples: [{ time_sec, items: [{ key, text, px, box: [x, y, w, h], frags: [[x, y, w, h], ...] }] }]
// opts: { width, height, step, minPhonePx }. Returns the findings, sorted by time.
export function audit(samples, opts) {
  const auditor = new TextAudit(opts);
  samples.forEach(auditor.step.bind(auditor));
  return auditor.finish();
}

const sampleAt = (step) => (_, i) => round(i * step, 6);
export const sampleTimes = (duration, step) =>
  Array.from({ length: Math.max(1, Math.ceil(duration / step - 1e-9)) }, sampleAt(step));

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

const timeoutError = (what, ms, Kind) => new Kind(`${what}: no reply in ${ms / 1000} s`);
const canceller = (timer) => () => clearTimeout(timer);

function withTimeout(promise, what, ms = CDP_TIMEOUT_MS, Kind = Error) {
  const late = Promise.withResolvers();
  const timer = setTimeout(late.reject, ms, timeoutError(what, ms, Kind));
  return Promise.race([promise, late.promise]).finally(canceller(timer));
}

const CLOSED = { error: { message: "CDP socket closed" } };
const eventKey = (method, sessionId) => `${sessionId ?? ""}|${method}`;
const callWith = (arg) => (fn) => fn(arg);
const ignore = () => undefined;
const settler = (method, reply) => (msg) =>
  msg.error ? reply.reject(new Error(`${method}: ${msg.error.message}`)) : reply.resolve(msg.result);
const messageHandler = (cdp) => (event) => cdp.onMessage(JSON.parse(event.data));
const closeHandler = (cdp) => () => cdp.failAll();

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 0;
    this.pending = new Map();
    this.waiters = new Map();
    ws.addEventListener("message", messageHandler(this));
    ws.addEventListener("close", closeHandler(this));
  }

  failAll() {
    this.pending.forEach(callWith(CLOSED));
    this.pending.clear();
  }

  onMessage(msg) {
    return msg.id === undefined ? this.onEvent(msg) : this.onReply(msg);
  }

  onEvent(msg) {
    const key = eventKey(msg.method, msg.sessionId);
    this.waiters.get(key)?.(msg.params);
    this.waiters.delete(key);
  }

  onReply(msg) {
    const settle = this.pending.get(msg.id);
    this.pending.delete(msg.id);
    settle?.(msg);
  }

  send(method, params = {}, sessionId) {
    const id = ++this.nextId;
    const reply = Promise.withResolvers();
    this.pending.set(id, settler(method, reply));
    this.ws.send(JSON.stringify({ id, method, params, sessionId }));
    return withTimeout(reply.promise, method);
  }

  once(method, sessionId) {
    const seen = Promise.withResolvers();
    this.waiters.set(eventKey(method, sessionId), seen.resolve);
    const timed = withTimeout(seen.promise, `${method} event`);
    timed.catch(ignore);
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

function matchDevtools(log, data, ok) {
  log.text = (log.text + data).slice(-4000);
  const m = log.text.match(/DevTools listening on (ws:\/\/\S+)/);
  return m && ok(m[1]);
}

const devtoolsMatcher = (log, ok) => (data) => matchDevtools(log, data, ok);
const startFailure = (no) => (e) => no(new Error(`Chrome failed to start: ${e.message}`));
const exitFailure = (log, no) => (code) => no(new Error(`Chrome exited with code ${code}: ${log.text.trim().slice(-500)}`));

function devtoolsUrl(proc) {
  const found = Promise.withResolvers();
  const log = { text: "" };
  proc.stderr.on("data", devtoolsMatcher(log, found.resolve));
  proc.on("error", startFailure(found.reject));
  proc.on("exit", exitFailure(log, found.reject));
  return withTimeout(found.promise, "Chrome DevTools URL", CHROME_START_MS, ChromeStartTimeout);
}

const rejectWith = (no, message) => () => no(new Error(message));

async function openSocket(url) {
  const ws = new WebSocket(url);
  const opened = Promise.withResolvers();
  ws.addEventListener("open", opened.resolve, { once: true });
  ws.addEventListener("error", rejectWith(opened.reject, `cannot connect to ${url}`), { once: true });
  await opened.promise;
  return ws;
}

const pidList = (out) => (out ?? "").split(/\s+/).filter(Boolean);
const childPids = (pid) => pidList(spawnSync("pgrep", ["-P", String(pid)], { encoding: "utf8" }).stdout);
const psPids = (pids) => pidList(spawnSync("ps", ["-o", "pid=", "-p", pids.join(",")], { encoding: "utf8" }).stdout);
const livePids = (pids) => (pids.length ? psPids(pids) : []);
const forceKill = (pids) => pids.length && spawnSync("kill", ["-KILL", ...pids]);
const running = (proc) => proc.exitCode === null && proc.signalCode === null;

// Chrome's helpers (network service, GPU, renderers) outlive the main process for some ms and
// write into the user-data dir as they exit. Poll up to 5 s for them, then SIGKILL the rest.
const pollAgain = (pids, tries) => () => helpersGone(pids, tries);
async function helpersGone(pids, tries = 500) {
  const alive = livePids(pids);
  const done = alive.length === 0 || tries === 0;
  return done ? forceKill(alive) : sleep(10).then(pollAgain(alive, tries - 1));
}

async function killMain(proc) {
  const exited = running(proc) ? once(proc, "exit").then(() => true) : Promise.resolve(true);
  proc.kill("SIGTERM");
  const ended = await Promise.race([exited, sleep(5000, false)]);
  return ended || proc.kill("SIGKILL");
}

async function stopChrome(browser) {
  browser.ws?.close();
  const helpers = childPids(browser.proc.pid);
  await killMain(browser.proc);
  await helpersGone(helpers);
  rmSync(browser.userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

const isStartTimeout = (e) => e instanceof ChromeStartTimeout;
// A start that hangs before the DevTools URL leaves stuck children; kill only those, never by name.
const killStuck = (browser, e) => isStartTimeout(e) && spawnSync("pkill", ["-KILL", "-P", String(browser.proc.pid)]);

async function abortLaunch(browser, tries, e) {
  killStuck(browser, e);
  await stopChrome(browser);
  return isStartTimeout(e) && tries > 1 ? launchChrome(tries - 1) : Promise.reject(e);
}

function spawnChrome() {
  const binary = chromePath();
  check(existsSync(binary), `Chrome not found at ${binary}. Set CHROME_PATH or install Google Chrome.`, UsageError);
  const userDataDir = mkdtempSync(join(tmpdir(), "video-qa-text-audit-chrome-"));
  const proc = spawn(binary, chromeArgs(userDataDir), { stdio: ["ignore", "ignore", "pipe"] });
  return { proc, userDataDir, ws: null };
}

async function connect(browser) {
  browser.ws = await openSocket(await devtoolsUrl(browser.proc));
  browser.cdp = new Cdp(browser.ws);
  return browser;
}

const aborter = (browser, tries) => (e) => abortLaunch(browser, tries, e);

function launchChrome(tries = CHROME_TRIES) {
  const browser = spawnChrome();
  return connect(browser).catch(aborter(browser, tries));
}

async function evaluate(cdp, sessionId, expression) {
  const res = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
  const d = res.exceptionDetails;
  return d ? fail(`page script failed: ${d.exception?.description ?? d.text}`) : res.result.value;
}

// ---------------------------------------------------------------- scene audit

const sceneUrl = (target) => (/^[a-z][a-z0-9+.-]*:/i.test(target) ? target : pathToFileURL(resolve(target)).href);
const isSet = (v) => v !== undefined && v !== null;
const pick = (...values) => values.find(isSet);
const isPage = (t) => t.type === "page";

// The tab Chrome opened at launch, with the canvas hook installed before any scene script.
async function attachPage(cdp) {
  const { targetInfos } = await cdp.send("Target.getTargets");
  const target = targetInfos.find(isPage) ?? fail("Chrome opened no page target");
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: target.targetId, flatten: true });
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: CANVAS_HOOK }, sessionId);
  return { cdp, sessionId };
}

const setViewport = (page, width, height) =>
  page.cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, page.sessionId);

async function loadScene(page, scene) {
  const loaded = page.cdp.once("Page.loadEventFired", page.sessionId);
  const nav = await page.cdp.send("Page.navigate", { url: sceneUrl(scene) }, page.sessionId);
  check(!nav.errorText, `navigation to ${scene} failed: ${nav.errorText}`);
  await loaded;
  const ready = await evaluate(page.cdp, page.sessionId, READY_SCRIPT);
  check(ready.seek === "function", `${scene}: typeof window.__seekToFrame is "${ready.seek}", expected "function"`);
  return ready.meta ?? {};
}

function frameOf(opts, meta) {
  const frame = {
    width: pick(opts.width, meta.width, 1920),
    height: pick(opts.height, meta.height, 1080),
    fps: pick(opts.fps, meta.fps, 30),
    duration: pick(opts.duration, meta.duration),
  };
  check(frame.duration > 0, "pass --duration or set window.__videoMeta.duration in the scene", UsageError);
  return frame;
}

async function takeSample(page, frame, time_sec) {
  const last = Math.max(0, Math.round(frame.duration * frame.fps) - 1);
  const f = Math.min(last, Math.round(time_sec * frame.fps));
  return { time_sec, items: await evaluate(page.cdp, page.sessionId, sampleScript(f, frame.fps)) };
}

const sampler = (page, frame, samples) => (prev, time_sec) =>
  prev.then(() => takeSample(page, frame, time_sec)).then(pusher(samples));
const pusher = (list) => (entry) => list.push(entry);

async function sampleScene(cdp, opts) {
  const page = await attachPage(cdp);
  await setViewport(page, opts.width ?? 1920, opts.height ?? 1080);
  const frame = frameOf(opts, await loadScene(page, opts.scene));
  await setViewport(page, frame.width, frame.height);
  const samples = [];
  await sampleTimes(frame.duration, opts.step).reduce(sampler(page, frame, samples), Promise.resolve());
  return { frame, samples };
}

const stopper = (browser) => () => stopChrome(browser);
const itemKeys = (s) => s.items.map(itemKey);
const ruleFailed = (findings) => (rule) => findings.some((f) => f.rule === rule);

function report(opts, { frame, samples }, findings) {
  return {
    passed: findings.length === 0,
    scene: opts.scene,
    width: frame.width, height: frame.height, fps: frame.fps, duration_sec: frame.duration,
    step_sec: opts.step, samples: samples.length, text_items: new Set(samples.flatMap(itemKeys)).size,
    phone_width_px: PHONE_WIDTH, min_phone_px: opts.minPhonePx,
    rules: RULES,
    failed_rules: RULES.filter(ruleFailed(findings)),
    findings,
  };
}

export async function auditScene(opts) {
  const browser = await launchChrome();
  const run = await sampleScene(browser.cdp, opts).finally(stopper(browser));
  const { width, height } = run.frame;
  return report(opts, run, audit(run.samples, { width, height, step: opts.step, minPhonePx: opts.minPhonePx }));
}

// ---------------------------------------------------------------- self-test

const json = (v) => JSON.stringify(v);
const assertEq = (actual, expected, what) => check(json(actual) === json(expected), `${what}: expected ${json(expected)}, got ${json(actual)}`);
const ruleOf = (f) => f.rule;
const ruleSet = (findings) => [...new Set(findings.map(ruleOf))].sort();

const item = (key, text, box, px = 64) => ({ key, text, px, box, frags: [box] });
const sampleOf = (at) => (_, i) => ({ time_sec: i * 0.1, items: at(i) });
const series = (n, at) => Array.from({ length: n }, sampleOf(at));
const always = (...items) => () => items;
const between = (from, to, ...items) => (i) => (i >= from && i < to ? items : []);
const counter = (i) => [item("a", `Users: ${i * 97}`, [100, 100, 400, 80])];
const slideIn = (i) => [item("a", "Launch", [Math.min(100, -400 + i * 100), 100, 400, 80])];
const UNIT_OPTS = { width: 1920, height: 1080, step: 0.1, minPhonePx: 8 };
const PORTRAIT = { ...UNIT_OPTS, width: 1080, height: 1920 };

// [name, samples, options, expected rules]
const UNIT_CASES = [
  ["clean title", series(20, always(item("a", "Ship it", [100, 100, 400, 80]))), UNIT_OPTS, []],
  ["tiny label", series(20, always(item("a", "beta", [100, 100, 60, 20], 20))), UNIT_OPTS, ["too_small"]],
  ["flashed word", series(20, between(5, 7, item("a", "Now", [100, 100, 200, 80]))), UNIT_OPTS, ["too_short"]],
  ["4 words for 1 s", series(20, between(0, 10, item("a", "one two three four", [100, 100, 600, 80]))), UNIT_OPTS, ["too_short"]],
  ["counter keeps 1 identity", series(20, counter), UNIT_OPTS, []],
  ["overlap", series(20, always(item("a", "Fast", [100, 100, 300, 80]), item("b", "Zero", [150, 120, 300, 80]))), UNIT_OPTS, ["overlap"]],
  ["duplicate layer", series(20, always(item("a", "Glow", [100, 100, 300, 80]), item("b", "Glow", [102, 102, 300, 80]))), UNIT_OPTS, []],
  ["off frame", series(20, always(item("a", "Launch", [1700, 100, 400, 80]))), UNIT_OPTS, ["off_frame"]],
  ["slide in", series(20, slideIn), UNIT_OPTS, []],
  ["caption zone", series(20, always(item("a", "Swipe", [100, 950, 300, 80]))), UNIT_OPTS, ["safe_zone"]],
  ["app zone", series(10, always(item("a", "Like", [950, 500, 100, 80], 72))), PORTRAIT, ["safe_zone"]],
  ["placeholder", series(20, always(item("a", "Total: NaN", [100, 100, 400, 80]))), UNIT_OPTS, ["placeholder"]],
];

const unitCase = ([name, samples, opts, rules]) => assertEq(ruleSet(audit(samples, opts)), rules, name);

function testUnits() {
  UNIT_CASES.forEach(unitCase);
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

const findingLine = (f) => `${f.rule} ${f.text}`;

async function testFixture(name, rules) {
  const result = await auditScene({ scene: join(FIXTURES, name), step: 0.1, minPhonePx: 8 });
  const lines = result.findings.map(findingLine).join("; ");
  assertEq(ruleSet(result.findings), rules, `${name} rules (${lines})`);
  check(result.findings.length === rules.length, `${name}: expected ${rules.length} findings, got ${lines}`);
  check(result.text_items > 0, `${name}: no text items found`);
}

const fixtureTest = ([name, rules]) => [`fixture ${name}`, () => testFixture(name, rules)];
const skipFixtures = () => [[`fixtures # SKIP no Chrome at ${chromePath()}`, ignore]];
const passLine = (name) => () => `ok - ${name}`;
const failLine = (name) => (e) => `not ok - ${name}: ${e.message}`;
const runTest = ([name, fn]) => Promise.resolve().then(fn).then(passLine(name), failLine(name));
const notOk = (line) => line.startsWith("not ok");

async function selfTest() {
  const fixtures = existsSync(chromePath()) ? FIXTURE_CASES.map(fixtureTest) : skipFixtures();
  const tests = [["rules on synthetic samples", testUnits], ["exit 2 without Chrome", testNoChrome], ...fixtures];
  const lines = [];
  await tests.reduce((prev, test) => prev.then(() => runTest(test)).then(pusher(lines)), Promise.resolve());
  const failed = lines.filter(notOk).length;
  console.log([...lines, failed ? `text_audit self-test: ${failed} failed` : "text_audit self-test: all passed"].join("\n"));
  return failed ? 1 : 0;
}

// ---------------------------------------------------------------- CLI

const HELP = `usage: text_audit.mjs --scene <index.html> [--out <report.json>] [--duration <sec>]
       [--fps <n>] [--width <px>] [--height <px>] [--step 0.1] [--min-phone-px 8]
       text_audit.mjs --self-test`;

const positiveOrFail = (n, text, flag) => (n > 0 ? n : fail(`${flag}: "${text}" is not a positive number`, UsageError));
const positive = (text, flag) => (text === undefined ? undefined : positiveOrFail(Number(text), text, flag));

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

const isUrl = (target) => /^[a-z][a-z0-9+.-]*:/i.test(target);

function auditOptions(v) {
  check(v.scene, `--scene is required\n${HELP}`, UsageError);
  check(isUrl(v.scene) || existsSync(v.scene), `scene not found: ${v.scene}`, UsageError);
  return {
    scene: v.scene, duration: positive(v.duration, "--duration"), fps: positive(v.fps, "--fps"),
    width: positive(v.width, "--width"), height: positive(v.height, "--height"),
    step: positive(v.step, "--step"), minPhonePx: positive(v["min-phone-px"], "--min-phone-px"),
  };
}

function writeReport(out, result) {
  mkdirSync(dirname(resolve(out)), { recursive: true });
  writeFileSync(out, `${JSON.stringify(result, null, 2)}\n`);
}

const failureLine = (f) => `FAIL ${f.rule} [${clock(f.time_sec)}] "${f.text}": ${f.detail} -> ${f.owner}`;
const summaryLine = (r) =>
  `${r.passed ? "PASS" : "FAIL"} text audit ${r.scene}: ${r.samples} samples, ${r.text_items} text items, ${r.findings.length} findings`;

async function runAudit(v) {
  const result = await auditScene(auditOptions(v));
  v.out && writeReport(v.out, result);
  console.log([...result.findings.map(failureLine), summaryLine(result)].join("\n"));
  return result.passed ? 0 : 1;
}

const showHelp = () => console.log(HELP) ?? 0;
const dispatch = (v) => (v.help ? showHelp() : v["self-test"] ? selfTest() : runAudit(v));
const main = () => dispatch(cliOptions(process.argv.slice(2)));
const isUsage = (e) => e instanceof UsageError || String(e.code).startsWith("ERR_PARSE_ARGS");

function die(e) {
  console.error(`text_audit: ${e.message}`);
  process.exit(isUsage(e) ? 2 : 1);
}

const exitWith = (code) => process.exit(code);
const isEntry = Boolean(process.argv[1]) && resolve(process.argv[1]) === SELF;
if (isEntry) Promise.resolve().then(main).then(exitWith, die);
