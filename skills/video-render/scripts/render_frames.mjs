#!/usr/bin/env node
// Deterministic frame-stepping renderer and publish packager for the video pipeline.
// Node built-ins only. Chrome is driven over the Chrome DevTools Protocol; ffmpeg and
// ffprobe run as subprocesses.
//
//   render_frames.mjs --scene <path-or-url> --out <mp4-or-frames-dir> [--fps 60]
//                     [--width 1920] [--height 1080] [--duration <sec>]
//                     [--frames <count> | --frames <i,j,k>] [--reverse] [--transparent]
//                     [--subframes 1|2|4|8] [--raster-sizes <json>]
//   render_frames.mjs --scenes <timeline.json> --out-dir <dir> [--workers <n>]
//                     [--subframes 1|2|4|8] [--sample]
//   render_frames.mjs --scope-check <timeline.json> --before <dir> --after <dir> --fix <id,id>
//   render_frames.mjs --self-notes <qa/self-notes.md> [--timeline <timeline.json>]
//   render_frames.mjs --package <timeline.json> --master <render/master.mp4>
//                     --out-dir <render/> [--aspects 16x9,9x16,1x1] [--cover-sec <sec>]
//                     [--scene-clips <dir>]
//   render_frames.mjs --self-test
//
// Exit codes: 0 success, 1 render or package failure, 2 usage error.

import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";
import { availableParallelism, tmpdir } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const DEFAULT_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const CDP_TIMEOUT_MS = 60_000;
const CHROME_START_MS = 15_000;
const CHROME_TRIES = 3;
const SKILL_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SKILL_MD_MAX_BYTES = 12288;
const CLEAR = { r: 0, g: 0, b: 0, a: 0 };

export const ASPECTS = {
  "16x9": { width: 1920, height: 1080 },
  "9x16": { width: 1080, height: 1920 },
  "1x1": { width: 1080, height: 1080 },
};

export class UsageError extends Error {}

export function fail(message, Kind = Error) {
  throw new Kind(message);
}

export const check = (cond, message, Kind) => cond || fail(message, Kind);
const chromePath = () => process.env.CHROME_PATH || DEFAULT_CHROME;
const range = (n) => Array.from({ length: n }, (_, i) => i);
const sum = (list) => list.reduce((a, b) => a + b, 0);
const chunk = (list, size) => range(Math.ceil(list.length / size)).map((i) => list.slice(i * size, i * size + size));
const inSequence = (items, fn) => items.reduce((p, item) => p.then(() => fn(item)), Promise.resolve());
const ignore = () => undefined;
const pusher = (list) => (item) => list.push(item);
const callWith = (arg) => (fn) => fn(arg);

// ---------------------------------------------------------------- subprocesses

const joiner = (chunks) => () => Buffer.concat(chunks).toString();

function collect(stream) {
  const chunks = [];
  stream.on("data", pusher(chunks));
  return joiner(chunks);
}

const runError = (cmd, code, stderr) => `${cmd} exited ${code}: ${stderr.trim().split("\n").slice(-8).join("\n")}`;

export async function run(cmd, args) {
  const child = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"] });
  const stdout = collect(child.stdout);
  const stderr = collect(child.stderr);
  const [code] = await once(child, "close");
  return code === 0 ? stdout() : fail(runError(cmd, code, stderr()));
}

export const ffmpeg = (args) => run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...args]);
const streamOf = (data, type) => data.streams.find((s) => s.codec_type === type) ?? null;
const rate = (text) => text.split("/").map(Number).reduce((num, den) => num / den);

export async function probe(file) {
  const data = JSON.parse(await run("ffprobe", ["-v", "error", "-show_streams", "-show_format", "-of", "json", file]));
  const video = streamOf(data, "video") ?? fail(`${file}: no video stream`);
  const audio = streamOf(data, "audio");
  return { video, audio, width: video.width, height: video.height, fps: rate(video.r_frame_rate), duration: Number(data.format.duration) };
}

// The master and every export share these H.264 settings: CFR, High profile at level 4.2,
// a 1 s GOP and 2 B-frames, with the moov atom at the front for streaming.
export function h264Args(fps) {
  return [
    "-c:v", "libx264", "-preset", "slow", "-crf", "16", "-pix_fmt", "yuv420p",
    "-profile:v", "high", "-level:v", "4.2", "-g", String(fps), "-bf", "2",
    "-r", String(fps), "-fps_mode", "cfr", "-movflags", "+faststart",
  ];
}

export const AAC_ARGS = ["-c:a", "aac", "-b:a", "320k", "-ar", "48000", "-ac", "2"];

export function encodeFrames(framesDir, fps, outFile) {
  const input = ["-framerate", String(fps), "-start_number", "0", "-i", join(framesDir, "frame_%06d.png")];
  return ffmpeg([...input, ...h264Args(fps), outFile]);
}

// ---------------------------------------------------------------- CDP client

const timeoutError = (what) => new Error(`${what}: no reply in ${CDP_TIMEOUT_MS / 1000} s`);
const eventKey = (method, sessionId) => `${sessionId ?? ""}|${method}`;
const settler = (method, ok, no) => (msg) => (msg.error ? no(new Error(`${method}: ${msg.error.message}`)) : ok(msg.result));

function timeoutAfter(what) {
  const { promise, reject } = Promise.withResolvers();
  const timer = setTimeout(reject, CDP_TIMEOUT_MS, timeoutError(what));
  return { promise, cancel: clearTimeout.bind(null, timer) };
}

function withTimeout(promise, what) {
  const deadline = timeoutAfter(what);
  return Promise.race([promise, deadline.promise]).finally(deadline.cancel);
}

// Chrome can deadlock in its own start-up fork before it prints the DevTools URL.
// Each start gets CHROME_START_MS. A stuck Chrome is killed and started again, CHROME_TRIES in all.
class ChromeStartTimeout extends Error {}

function chromeStartDeadline() {
  const { promise, reject } = Promise.withResolvers();
  const error = new ChromeStartTimeout(`Chrome did not print its DevTools URL in ${CHROME_START_MS / 1000} s`);
  const timer = setTimeout(reject, CHROME_START_MS, error);
  return { promise, cancel: clearTimeout.bind(null, timer) };
}

function withStartTimeout(promise) {
  const deadline = chromeStartDeadline();
  return Promise.race([promise, deadline.promise]).finally(deadline.cancel);
}

const CLOSED = { error: { message: "CDP socket closed" } };

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 0;
    this.pending = new Map();
    this.waiters = new Map();
    ws.addEventListener("message", (event) => this.onMessage(JSON.parse(event.data)));
    ws.addEventListener("close", () => this.failAll());
  }

  onMessage(msg) {
    return msg.id === undefined ? this.onEvent(msg) : this.onReply(msg);
  }

  onReply(msg) {
    const call = this.pending.get(msg.id);
    this.pending.delete(msg.id);
    call?.(msg);
  }

  onEvent(msg) {
    const key = eventKey(msg.method, msg.sessionId);
    this.waiters.get(key)?.(msg.params);
    this.waiters.delete(key);
  }

  failAll() {
    this.pending.forEach(callWith(CLOSED));
    this.pending.clear();
  }

  send(method, params = {}, sessionId) {
    const id = ++this.nextId;
    const reply = Promise.withResolvers();
    this.pending.set(id, settler(method, reply.resolve, reply.reject));
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

// Flags that remove the main sources of pixel drift between runs: software raster,
// fixed device scale, sRGB output, grayscale font antialiasing, no background throttling.
// A window below Chrome's minimum size can stall Page.captureScreenshot for good, so the
// window never goes below WINDOW_MIN; Emulation.setDeviceMetricsOverride sets the viewport.
const WINDOW_MIN = { width: 800, height: 600 };

function chromeArgs(userDataDir, width, height) {
  return [
    "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${userDataDir}`,
    `--window-size=${Math.max(width, WINDOW_MIN.width)},${Math.max(height, WINDOW_MIN.height)}`, "--force-device-scale-factor=1",
    "--disable-gpu", "--force-color-profile=srgb", "--disable-lcd-text",
    "--font-render-hinting=none", "--hide-scrollbars", "--mute-audio",
    "--disable-background-timer-throttling", "--disable-renderer-backgrounding",
    "--disable-backgrounding-occluded-windows", "--disable-extensions",
    "--no-first-run", "--no-default-browser-check", "--disable-sync",
    "--disable-component-update", "--metrics-recording-only", "about:blank",
  ];
}

function matchDevtools(log, data, ok) {
  log.text = (log.text + data).slice(-4000);
  const m = log.text.match(/DevTools listening on (ws:\/\/\S+)/);
  return m && ok(m[1]);
}

const devtoolsMatcher = (log, ok) => (data) => matchDevtools(log, data, ok);
const startFailure = (no) => (e) => no(new Error(`Chrome failed to start: ${e.message}`));
const exitFailure = (log, no) => (code) => no(new Error(`Chrome exited with code ${code}: ${log.text.trim().slice(-500)}`));
const rejectWith = (no, message) => () => no(new Error(message));

function watchChrome(proc, ok, no) {
  const log = { text: "" };
  proc.stderr.on("data", devtoolsMatcher(log, ok));
  proc.on("error", startFailure(no));
  proc.on("exit", exitFailure(log, no));
}

function watchSocket(ws, url, ok, no) {
  ws.addEventListener("open", ok, { once: true });
  ws.addEventListener("error", rejectWith(no, `cannot connect to ${url}`), { once: true });
}

async function connect(proc) {
  const started = Promise.withResolvers();
  watchChrome(proc, started.resolve, started.reject);
  const url = await withStartTimeout(started.promise);
  const ws = new WebSocket(url);
  const opened = Promise.withResolvers();
  watchSocket(ws, url, opened.resolve, opened.reject);
  await opened.promise;
  return ws;
}

const running = (proc) => proc.exitCode === null && proc.signalCode === null;

async function killChrome(proc) {
  const exited = running(proc) ? once(proc, "exit").then(() => true) : Promise.resolve(true);
  proc.kill("SIGTERM");
  const ended = await Promise.race([exited, sleep(5000, false)]);
  return ended || proc.kill("SIGKILL");
}

const pidList = (out) => out.split(/\s+/).filter(Boolean);
const childPids = (pid) => pidList(spawnSync("pgrep", ["-P", String(pid)], { encoding: "utf8" }).stdout ?? "");
const livePids = (pids) => (pids.length ? pidList(spawnSync("ps", ["-o", "pid=", "-p", pids.join(",")], { encoding: "utf8" }).stdout ?? "") : []);
const forceKill = (pids) => pids.length && spawnSync("kill", ["-KILL", ...pids]);

// Poll for up to 5 s of real time, then SIGKILL the helpers that are still alive.
async function helpersGone(pids, tries = 500) {
  const alive = livePids(pids);
  if (!alive.length || tries === 0) return forceKill(alive);
  await sleep(10);
  return helpersGone(alive, tries - 1);
}

// The Chrome helpers (network service, GPU, renderers) outlive the main process for some ms,
// and the network service writes into userDataDir as it exits. Wait for them, then remove the dir.
async function stopChrome(browser) {
  browser.ws?.close();
  const helpers = childPids(browser.proc.pid);
  await killChrome(browser.proc);
  await helpersGone(helpers);
  rmSync(browser.userDataDir, { recursive: true, force: true });
}

function killChildren(proc) {
  spawnSync("pkill", ["-KILL", "-P", String(proc.pid)]); // only the children of this stuck Chrome, never by name
}

const isStartTimeout = (error) => error instanceof ChromeStartTimeout;
const killIfStuck = (browser, error) => isStartTimeout(error) && killChildren(browser.proc);

async function abortLaunch(browser, error) {
  killIfStuck(browser, error);
  await stopChrome(browser);
  throw error;
}

const giveUp = (error) => Promise.reject(isStartTimeout(error) ? new Error(`${error.message}, ${CHROME_TRIES} tries`) : error);

function retryStart(width, height, tries, error) {
  const again = isStartTimeout(error) && tries > 1;
  return again ? launchChrome(width, height, tries - 1) : giveUp(error);
}

function launchChrome(width, height, tries = CHROME_TRIES) {
  return startChrome(width, height).catch(retryStart.bind(null, width, height, tries));
}

async function startChrome(width, height) {
  const binary = chromePath();
  check(existsSync(binary), `Chrome not found at ${binary}. Set CHROME_PATH or install Google Chrome.`, UsageError);
  const userDataDir = mkdtempSync(join(tmpdir(), "video-render-chrome-"));
  const proc = spawn(binary, chromeArgs(userDataDir, width, height), { stdio: ["ignore", "ignore", "pipe"] });
  const browser = { proc, userDataDir, ws: null };
  browser.ws = await connect(proc).catch((e) => abortLaunch(browser, e));
  browser.cdp = new Cdp(browser.ws);
  return browser;
}

async function evaluate(cdp, sessionId, expression) {
  const res = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
  const d = res.exceptionDetails;
  return d ? fail(`page script failed: ${d.exception?.description ?? d.text}`) : res.result.value;
}

// ---------------------------------------------------------------- scene render mode

export function sceneUrl(target) {
  const q = target.indexOf("?");
  const path = q < 0 ? target : target.slice(0, q);
  const query = q < 0 ? "" : target.slice(q);
  return /^[a-z][a-z0-9+.-]*:/i.test(target) ? target : pathToFileURL(resolve(path)).href + query;
}

function parseNonNegInt(text, flag) {
  const n = Number(text.trim());
  return Number.isInteger(n) && n >= 0 ? n : fail(`${flag}: "${text}" is not a non-negative integer`, UsageError);
}

const nonBlank = (s) => s.trim() !== "";
const frameIndex = (s) => parseNonNegInt(s, "--frames");
const parseFrames = (frames) => (frames.includes(",") ? frames.split(",").filter(nonBlank).map(frameIndex) : range(frameIndex(frames)));

function framesFromTime(duration, metaDuration, fps) {
  const sec = duration ?? metaDuration;
  check(sec !== undefined && sec !== null, "pass --duration or --frames, or set window.__videoMeta.duration in the scene", UsageError);
  return range(Math.round(sec * fps));
}

// "--frames 30" is a count (frames 0..29). "--frames 0,15,30" is a list. Write a single
// frame index as "30," so it is not read as a count.
export function frameList({ frames, duration, fps, metaDuration, reverse }) {
  const raw = frames === undefined ? framesFromTime(duration, metaDuration, fps) : parseFrames(frames);
  check(raw.length > 0, "no frames to render", UsageError);
  const list = [...new Set(raw)].sort((a, b) => a - b);
  return reverse ? list.reverse() : list;
}

const READY_SCRIPT = `(async () => {
  await document.fonts.ready;
  await Promise.all(Array.from(document.images, (img) =>
    img.decode().catch(() => { throw new Error("image did not decode: " + img.src); })));
  return { seek: typeof window.__seekToFrame, meta: window.__videoMeta ?? null };
})()`;

const frameScript = (f, fps) => `(async () => {
  window.__rasterProbe?.nextFrame();
  await window.__seekToFrame(${f}, ${fps});
  await document.fonts.ready;
})()`;

export const frameName = (i) => `frame_${String(i).padStart(6, "0")}.png`;

function renderTarget({ out, width, height, transparent }) {
  check(width % 2 === 0 && height % 2 === 0, "--width and --height must be even for yuv420p", UsageError);
  const toMp4 = extname(out).toLowerCase() === ".mp4";
  check(!(toMp4 && transparent), "--transparent keeps alpha only in PNG frames; write a frames directory", UsageError);
  const framesDir = toMp4 ? mkdtempSync(join(tmpdir(), "video-render-frames-")) : resolve(out);
  mkdirSync(framesDir, { recursive: true });
  return { toMp4, framesDir, out: resolve(out) };
}

// Use the tab Chrome opened at launch. A tab from Target.createTarget can stay in the
// background, where it stops producing frames and Page.captureScreenshot never returns.
async function openPage(cdp, { width, height, transparent }) {
  const { targetInfos } = await cdp.send("Target.getTargets");
  const page = targetInfos.find((t) => t.type === "page") ?? fail("Chrome opened no page target");
  const { sessionId } = await cdp.send("Target.attachToTarget", { targetId: page.targetId, flatten: true });
  await cdp.send("Page.enable", {}, sessionId);
  await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, sessionId);
  await (transparent ? cdp.send("Emulation.setDefaultBackgroundColorOverride", { color: CLEAR }, sessionId) : null);
  return sessionId;
}

async function loadScene(cdp, sessionId, scene) {
  const loaded = cdp.once("Page.loadEventFired", sessionId);
  const nav = await cdp.send("Page.navigate", { url: sceneUrl(scene) }, sessionId);
  check(!nav.errorText, `navigation to ${scene} failed: ${nav.errorText}`);
  await loaded;
  const ready = await evaluate(cdp, sessionId, READY_SCRIPT);
  check(ready.seek === "function", `${scene}: typeof window.__seekToFrame is "${ready.seek}", expected "function"`);
  return ready.meta ?? {};
}

const metaDiffers = (meta, opts) => (k) => meta[k] !== undefined && meta[k] !== opts[k];
const metaWarning = (meta, opts) => (k) => `warning: window.__videoMeta.${k} is ${meta[k]}, render uses ${opts[k]}\n`;

function warnMeta(meta, opts) {
  const lines = ["width", "height", "fps"].filter(metaDiffers(meta, opts)).map(metaWarning(meta, opts));
  process.stderr.write(lines.join(""));
}

async function captureAt(page, time, file) {
  await evaluate(page.cdp, page.sessionId, frameScript(time, page.fps));
  const params = { format: "png", fromSurface: true, captureBeyondViewport: false, clip: page.clip };
  const shot = await page.cdp.send("Page.captureScreenshot", params, page.sessionId);
  writeFileSync(file, Buffer.from(shot.data, "base64"));
}

const captureFrame = (page, f) => captureAt(page, f, join(page.framesDir, frameName(f)));

// ---------------------------------------------------------------- subframe motion blur

export const SUBFRAME_COUNTS = [1, 2, 4, 8];
export const subName = (i) => `sub_${String(i).padStart(6, "0")}.png`;
const avgName = (i) => `avg_${String(i).padStart(6, "0")}.png`;

// The shutter of frame k opens at k and spans 1 frame: subframe j seeks k + j / n.
// n is a power of 2, so every seek time is exact in binary.
export const subframeTimes = (f, n) => range(n).map((j) => f + j / n);

// Subframe file of frame f: by its rank in the sorted frame list, so a --reverse
// capture writes the same files.
const shotAt = (page, base) => (t, j) => [t, join(page.subDir, subName(base + j))];
const takeShot = (page) => ([t, file]) => captureAt(page, t, file);

function captureSubframes(page, sorted, f) {
  const shots = subframeTimes(f, page.subframes).map(shotAt(page, sorted.indexOf(f) * page.subframes));
  return inSequence(shots, takeShot(page));
}

// ffmpeg tmix averages each window of n subframes; select keeps the last frame of each
// window, the first one with a full window. Output frame i belongs to sorted[i].
export async function averageSubframes(subDir, n, sorted, framesDir) {
  const filter = `tmix=frames=${n},select='eq(mod(n,${n}),${n - 1})'`;
  const input = ["-framerate", String(n), "-start_number", "0", "-i", join(subDir, "sub_%06d.png")];
  await ffmpeg([...input, "-vf", filter, "-fps_mode", "passthrough", "-start_number", "0", join(subDir, "avg_%06d.png")]);
  sorted.forEach((f, i) => renameSync(join(subDir, avgName(i)), join(framesDir, frameName(f))));
}

const ascending = (a, b) => a - b;
const subframesOf = (page, sorted) => (f) => captureSubframes(page, sorted, f);

async function blurInto(page, order) {
  const sorted = [...order].sort(ascending);
  await inSequence(order, subframesOf(page, sorted));
  await averageSubframes(page.subDir, page.subframes, sorted, page.framesDir);
}

const removeDir = (dir) => () => rmSync(dir, { recursive: true, force: true });

function captureBlurred(page, order) {
  page.subDir = mkdtempSync(join(tmpdir(), "video-render-sub-"));
  return blurInto(page, order).finally(removeDir(page.subDir));
}

const captureAll = (page, order) =>
  page.subframes > 1 ? captureBlurred(page, order) : inSequence(order, (f) => captureFrame(page, f));

async function encodeTarget(target, fps) {
  mkdirSync(dirname(target.out), { recursive: true });
  await encodeFrames(target.framesDir, fps, target.out);
}

async function renderInBrowser(cdp, opts, target) {
  const sessionId = await openPage(cdp, opts);
  await (opts.rasterSizes ? installRasterProbe(cdp, sessionId) : null);
  const meta = await loadScene(cdp, sessionId, opts.scene);
  warnMeta(meta, opts);
  const order = frameList({ ...opts, metaDuration: meta.duration });
  check(!target.toMp4 || order.every((f) => f < order.length), "an .mp4 --out needs contiguous frames 0..N-1; write a frames directory instead", UsageError);
  const clip = { x: 0, y: 0, width: opts.width, height: opts.height, scale: 1 };
  const page = { cdp, sessionId, fps: opts.fps, clip, framesDir: target.framesDir, subframes: opts.subframes ?? 1 };
  await captureAll(page, order);
  await (opts.rasterSizes ? measureRasterSizes(page, order, opts.rasterSizes) : null);
  await (target.toMp4 ? encodeTarget(target, opts.fps) : null);
  return { frames: order, out: target.out };
}

const dropFrames = (target) => target.toMp4 && rmSync(target.framesDir, { recursive: true, force: true });

const stopper = (browser) => () => stopChrome(browser);
const frameDropper = (target) => () => dropFrames(target);

async function launchAndRender(opts, target) {
  const browser = await launchChrome(opts.width, opts.height);
  return renderInBrowser(browser.cdp, opts, target).finally(stopper(browser));
}

export async function renderScene(opts) {
  const target = renderTarget(opts);
  return launchAndRender(opts, target).finally(frameDropper(target));
}

// ---------------------------------------------------------------- raster size report

// video-qa check raster_upscale reads natural vs drawn pixel sizes of every raster image.
// Format and rules: skills/video-qa/references/motion-checks.md, check 18. How the
// render samples them: references/raster-sizes.md.
export const RASTER_STEP_SEC = 0.1;

// Runs in the page before the scene scripts. It keeps the <img> draws of every 2D canvas
// for the current seek: the first drawImage or clearRect after a seek drops older draws.
function rasterProbe() {
  const probe = { gen: 0, canvases: new Map() };
  probe.nextFrame = () => { probe.gen += 1; };
  const drawsOf = (canvas) => {
    const entry = probe.canvases.get(canvas);
    if (entry && entry.gen === probe.gen) return entry.draws;
    const fresh = { gen: probe.gen, draws: [] };
    probe.canvases.set(canvas, fresh);
    return fresh.draws;
  };
  const proto = CanvasRenderingContext2D.prototype;
  const { drawImage, clearRect } = proto;
  proto.clearRect = function (...args) {
    drawsOf(this.canvas);
    return clearRect.apply(this, args);
  };
  proto.drawImage = function (img, ...rest) {
    const draws = drawsOf(this.canvas);
    if (img instanceof HTMLImageElement && img.naturalWidth > 0) {
      const crop = rest.length === 8;
      const source = crop ? [rest[2], rest[3]] : [img.naturalWidth, img.naturalHeight];
      const dest = crop ? [rest[6], rest[7]] : rest.length === 4 ? [rest[2], rest[3]] : source;
      const m = this.getTransform();
      const size = [Math.abs(dest[0]) * Math.hypot(m.a, m.b), Math.abs(dest[1]) * Math.hypot(m.c, m.d)];
      draws.push({ src: img.currentSrc || img.src, natural: source.map(Math.abs), size });
    }
    return drawImage.call(this, img, ...rest);
  };
  window.__rasterProbe = probe;
}

const installRasterProbe = (cdp, sessionId) =>
  cdp.send("Page.addScriptToEvaluateOnNewDocument", { source: `(${rasterProbe})();` }, sessionId);

// The functions below run in the page (measureScript sends their source). They use only
// browser globals and each other.
function rasterName(url) {
  const base = new URL(".", location.href).href;
  if (url.startsWith(base)) return decodeURIComponent(url.slice(base.length));
  return url.startsWith("data:") ? `${url.slice(0, 40)}...` : url;
}

function isRaster(url) {
  return Boolean(url) && !/^data:image\/svg|\.svg(?:[?#]|$)/i.test(url);
}

function isShown(el, r) {
  const onScreen = r.width > 0 && r.height > 0 && r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight;
  return onScreen && el.checkVisibility({ opacityProperty: true, visibilityProperty: true });
}

function naturalSize(url) {
  const cache = (window.__rasterNatural ??= new Map());
  if (!cache.has(url)) {
    cache.set(url, new Promise((ok) => {
      const img = new Image();
      img.onload = () => ok([img.naturalWidth, img.naturalHeight]);
      img.onerror = () => ok(null);
      img.src = url;
    }));
  }
  return cache.get(url);
}

// The drawn size of an image of natural size n in a box, by object-fit rules.
function fitSize(mode, box, n) {
  const contain = Math.min(box[0] / n[0], box[1] / n[1]);
  const scales = { contain, cover: Math.max(box[0] / n[0], box[1] / n[1]), none: 1, "scale-down": Math.min(1, contain) };
  return mode in scales ? [n[0] * scales[mode], n[1] * scales[mode]] : box;
}

// Split a computed CSS list at its top-level commas.
function splitLayers(text) {
  const layers = [""];
  let depth = 0;
  for (const ch of text) {
    if (ch === "," && depth === 0) {
      layers.push("");
      continue;
    }
    depth += ch === "(" ? 1 : ch === ")" ? -1 : 0;
    layers[layers.length - 1] += ch;
  }
  return layers.map((s) => s.trim());
}

function layerUrl(layer) {
  const m = /^url\("(.*)"\)$/.exec(layer);
  return m ? m[1] : null;
}

function axisLength(token, boxLen) {
  if (token === "auto") return null;
  return token.endsWith("%") ? (parseFloat(token) / 100) * boxLen : parseFloat(token);
}

function backgroundSize(text, box, n) {
  if (text === "cover" || text === "contain") return fitSize(text, box, n);
  const [a, b = "auto"] = text.split(/\s+/);
  const w = axisLength(a, box[0]);
  const h = axisLength(b, box[1]);
  if (w === null && h === null) return n;
  return [w ?? (h * n[0]) / n[1], h ?? (w * n[1]) / n[0]];
}

function imageSamples(add) {
  for (const img of document.images) {
    const r = img.getBoundingClientRect();
    const n = [img.naturalWidth, img.naturalHeight];
    const url = img.currentSrc || img.src;
    if (isRaster(url) && n[0] > 0 && isShown(img, r)) add(url, n, fitSize(getComputedStyle(img).objectFit, [r.width, r.height], n));
  }
}

function canvasSamples(add) {
  for (const [canvas, entry] of window.__rasterProbe?.canvases ?? []) {
    const r = canvas.getBoundingClientRect();
    if (!canvas.isConnected || !canvas.width || !canvas.height || !isShown(canvas, r)) continue;
    const k = [r.width / canvas.width, r.height / canvas.height];
    for (const d of entry.draws) add(d.src, d.natural, [d.size[0] * k[0], d.size[1] * k[1]]);
  }
}

async function svgImageSamples(add) {
  for (const el of document.querySelectorAll("image")) {
    const r = el.getBoundingClientRect();
    const url = el instanceof SVGImageElement ? new URL(el.href.baseVal, document.baseURI).href : null;
    if (!isRaster(url) || !isShown(el, r)) continue;
    const par = el.preserveAspectRatio.baseVal;
    const mode = par.align === SVGPreserveAspectRatio.SVG_PRESERVEASPECTRATIO_NONE ? "fill" : par.meetOrSlice === SVGPreserveAspectRatio.SVG_MEETORSLICE_SLICE ? "cover" : "contain";
    const n = await naturalSize(url);
    if (n) add(url, n, fitSize(mode, [r.width, r.height], n));
  }
}

// A poster shows until the video has a frame, or while it is paused at 0 and never played.
async function posterSamples(add) {
  for (const v of document.querySelectorAll("video[poster]")) {
    const r = v.getBoundingClientRect();
    const posterUp = v.readyState < 2 || (v.paused && v.played.length === 0 && v.currentTime === 0);
    if (!posterUp || !isRaster(v.poster) || !isShown(v, r)) continue;
    const n = await naturalSize(v.poster);
    if (n) add(v.poster, n, fitSize(getComputedStyle(v).objectFit, [r.width, r.height], n));
  }
}

async function elementBackgrounds(el, add) {
  const style = getComputedStyle(el);
  const r = el.getBoundingClientRect();
  if (style.backgroundImage === "none" || !isShown(el, r)) return;
  const sizes = splitLayers(style.backgroundSize);
  const box = [el.clientWidth, el.clientHeight];
  const scale = [el.offsetWidth ? r.width / el.offsetWidth : 1, el.offsetHeight ? r.height / el.offsetHeight : 1];
  for (const [i, layer] of splitLayers(style.backgroundImage).entries()) {
    const url = layerUrl(layer);
    const n = isRaster(url) ? await naturalSize(url) : null;
    if (n) add(url, n, backgroundSize(sizes[i % sizes.length], box, n).map((v, k) => v * scale[k]));
  }
}

async function backgroundSamples(add) {
  for (const el of document.querySelectorAll("*")) await elementBackgrounds(el, add);
}

async function measureRasters() {
  const found = [];
  const round = (v) => Math.round(v * 10) / 10;
  const add = (url, n, size) => {
    if (n[0] > 0 && n[1] > 0) found.push({ src: rasterName(url), natural_px: n.map(round), drawn_px: size.map((v) => round(v * devicePixelRatio)) });
  };
  imageSamples(add);
  canvasSamples(add);
  await svgImageSamples(add);
  await posterSamples(add);
  await backgroundSamples(add);
  return found;
}

const PAGE_FUNCTIONS = [rasterName, isRaster, isShown, naturalSize, fitSize, splitLayers, layerUrl, axisLength, backgroundSize,
  imageSamples, canvasSamples, svgImageSamples, posterSamples, elementBackgrounds, backgroundSamples, measureRasters];

const measureScript = (f, fps) => `(async () => {
${PAGE_FUNCTIONS.map(String).join("\n")}
await (${frameScript(f, fps)});
return measureRasters();
})()`;

// The frames nearest the RASTER_STEP_SEC grid, inside frames 0..count-1.
export function rasterFrames(count, fps) {
  const steps = Math.ceil(count / fps / RASTER_STEP_SEC - 1e-9);
  return [...new Set(range(steps).map((k) => Math.round(k * RASTER_STEP_SEC * fps)))].filter((f) => f < count);
}

const roundTime = (t) => Math.round(t * 1000) / 1000;
const scaleOf = (s) => Math.max(s.drawn_px[0] / s.natural_px[0], s.drawn_px[1] / s.natural_px[1]);
const byTimeThenSrc = (a, b) => a.t_sec - b.t_sec || (a.src < b.src ? -1 : Number(a.src > b.src));

function keepLargest(best, s) {
  const key = `${s.t_sec}|${s.src}`;
  const old = best.get(key);
  if (!old || scaleOf(s) > scaleOf(old)) best.set(key, s);
  return best;
}

// 1 sample per image per time, the largest scale, so 2 copies of 1 image do not double a run.
export const largestPerSource = (samples) => [...samples.reduce(keepLargest, new Map()).values()].sort(byTimeThenSrc);

// 1 sample a line, so a long film stays readable and small.
function writeRasterReport(file, samples) {
  const lines = largestPerSource(samples).map((s) => `    ${JSON.stringify(s)}`).join(",\n");
  mkdirSync(dirname(file), { recursive: true });
  writeFileSync(file, `{\n  "step_sec": ${RASTER_STEP_SEC},\n  "samples": [${lines ? `\n${lines}\n  ` : ""}]\n}\n`);
}
const atTime = (t) => (s) => ({ t_sec: t, ...s });

async function measureFrame(page, f) {
  const found = await evaluate(page.cdp, page.sessionId, measureScript(f, page.fps));
  return found.map(atTime(roundTime(f / page.fps)));
}

// After the capture, the same page seeks the grid frames again and reads sizes only, with
// no screenshot, so the report costs about 1 seek per 0.1 s of scene.
async function measureRasterSizes(page, order, file) {
  const samples = [];
  await inSequence(rasterFrames(Math.max(...order) + 1, page.fps), async (f) => samples.push(...(await measureFrame(page, f))));
  writeRasterReport(file, samples);
}

// Scene samples move to film time. Samples in the tail handle are dropped: the next
// scene covers that time.
function filmSamples(s, file) {
  if (!existsSync(file)) return [];
  const inScene = (x) => x.t_sec < s.duration_sec - 1e-9;
  return JSON.parse(readFileSync(file, "utf8")).samples.filter(inScene).map((x) => ({ ...x, t_sec: roundTime(s.start_sec + x.t_sec) }));
}

function writeFilmRaster(root, timeline, dir) {
  const file = join(root, "render", "raster-sizes.json");
  writeRasterReport(file, timeline.scenes.flatMap((s) => filmSamples(s, join(dir, `${s.id}.json`))));
  return file;
}

// ---------------------------------------------------------------- timeline helpers

function checkScene(file, s) {
  check(typeof s.id === "string" && s.id, `${file}: a scene has no "id"`);
  check(typeof s.start_sec === "number" && typeof s.duration_sec === "number", `${file}: scene ${s.id} needs numeric "start_sec" and "duration_sec"`);
}

export function loadTimeline(file) {
  const t = JSON.parse(readFileSync(file, "utf8"));
  check(t.meta && Array.isArray(t.scenes) && t.scenes.length > 0, `${file}: needs "meta" and a non-empty "scenes" array`);
  t.scenes.forEach((s) => checkScene(file, s));
  return t;
}

function sceneText(s) {
  const narration = typeof s.narration === "string" ? s.narration : s.narration?.text;
  return (narration || s.caption || "").trim().replace(/\s+/g, " ");
}

function addWord(lines, word, maxChars) {
  const last = lines.at(-1);
  const fits = last !== undefined && last.length + 1 + word.length <= maxChars;
  return fits ? [...lines.slice(0, -1), `${last} ${word}`] : [...lines, word];
}

// Greedy wrap into lines of at most maxChars. A word longer than maxChars gets its own line.
const addWordUpTo = (maxChars) => (lines, word) => addWord(lines, word, maxChars);
export const wrapLines = (words, maxChars) => words.reduce(addWordUpTo(maxChars), []);

const wordCount = (lines) => lines.join(" ").split(" ").length;
const cumulative = (list) => list.map((_, i) => sum(list.slice(0, i + 1)));
const cueClock = (s, total) => (n) => s.start_sec + (s.duration_sec * n) / total;
const cueFor = (at, ends) => (lines, i) => ({ start: at(ends[i - 1] ?? 0), end: at(ends[i]), lines });

function sceneCues(s, maxChars, maxLines) {
  const words = sceneText(s).split(" ").filter(Boolean);
  const groups = chunk(wrapLines(words, maxChars), maxLines);
  const ends = cumulative(groups.map(wordCount));
  return groups.map(cueFor(cueClock(s, words.length), ends));
}

const cuesWithin = (maxChars, maxLines) => (s) => sceneCues(s, maxChars, maxLines);

// One or more cues per scene: narration (or caption) text, at most 2 lines of 42 chars
// per cue. A scene's time span is shared between its cues in proportion to word count.
export function captionCues(timeline, { maxChars = 42, maxLines = 2 } = {}) {
  return timeline.scenes.flatMap(cuesWithin(maxChars, maxLines));
}

const pad = (v, w = 2) => String(v).padStart(w, "0");
const pad2 = (v) => pad(v);

function clock(sec, sep) {
  const ms = Math.round(sec * 1000);
  const hms = [Math.floor(ms / 3_600_000), Math.floor((ms % 3_600_000) / 60_000), Math.floor((ms % 60_000) / 1000)];
  return `${hms.map(pad2).join(":")}${sep}${pad(ms % 1000, 3)}`;
}

const cueTimes = (c, sep) => `${clock(c.start, sep)} --> ${clock(c.end, sep)}`;
const srtCue = (c, i) => `${i + 1}\n${cueTimes(c, ",")}\n${c.lines.join("\n")}\n`;
const vttCue = (c) => `${cueTimes(c, ".")}\n${c.lines.join("\n")}\n`;

export const toSrt = (cues) => cues.map(srtCue).join("\n");
export const toVtt = (cues) => `WEBVTT\n\n${cues.map(vttCue).join("\n")}`;

const hasTitle = (s) => typeof s.title === "string" && s.title.trim() !== "";
const chapterTitle = (s, i, meta) => (s.title || (i === 0 && meta.title) || s.caption || s.id).trim();
const shortStamp = (sec) => `${Math.floor(sec / 60)}:${pad(sec % 60)}`;
const longStamp = (sec) => `${Math.floor(sec / 3600)}:${pad(Math.floor((sec % 3600) / 60))}:${pad(sec % 60)}`;

const titledAfterFirst = (s, i) => i > 0 && hasTitle(s);
const chapterOf = (meta) => (s, i) => ({ sec: i === 0 ? 0 : Math.floor(s.start_sec), title: chapterTitle(s, i, meta) });
const chapterLength = (list, total) => (c, i) => ({ title: c.title, len: (list[i + 1]?.sec ?? total) - c.sec });
const isShort = (c) => c.len < 10;
const shortWarning = (c) => `chapters: "${c.title}" is ${c.len} s; YouTube needs 10 s or more`;

function chapterList({ scenes, meta }) {
  const titled = scenes.filter(titledAfterFirst);
  const picked = titled.length ? [scenes[0], ...titled] : scenes;
  return picked.map(chapterOf(meta));
}

function chapterWarnings(list, total) {
  const few = list.length < 3 ? [`chapters: ${list.length} chapters; YouTube shows chapters only when there are 3 or more`] : [];
  const short = list.map(chapterLength(list, total)).filter(isShort).map(shortWarning);
  return [...few, ...short];
}

// A chapter starts at scene 0 and at every scene with a "title". When no scene has a
// title, every scene starts a chapter. Times are floored to whole seconds.
export function chapters(timeline) {
  const list = chapterList(timeline);
  const total = Number(timeline.meta.duration_sec) || 0;
  const stamp = total >= 3600 ? longStamp : shortStamp;
  return { text: list.map((c) => `${stamp(c.sec)} ${c.title}\n`).join(""), warnings: chapterWarnings(list, total) };
}

// ---------------------------------------------------------------- parallel scenes and scope samples

// About 60 % of the CPUs, at least 1 and at most 8. Each worker runs its own Chrome.
export const defaultWorkers = (cpus = availableParallelism()) => Math.max(1, Math.min(8, Math.floor(cpus * 0.6)));

const isHtml = (path) => /\.html?(\?|$)/i.test(path);
const platePath = (s) => (typeof s.plate === "string" && s.plate ? s.plate : null);

// The source of a scene: an HTML page that renders frames, or a file that is compared as bytes.
export function sceneSource(root, s) {
  const plate = platePath(s);
  const html = plate && isHtml(plate) ? plate : `scenes/${s.id}/index.html`;
  const page = (plate && isHtml(plate)) || existsSync(join(root, html.split("?")[0]));
  return page ? { kind: "html", path: join(root, html) } : { kind: "file", path: join(root, plate ?? `captures/${s.id}.mp4`) };
}

// 5 frames per scene: first, quarters and last, inside the scene's own frames.
export const sampleFrames = (count) => [...new Set([0, Math.floor(count / 4), Math.floor(count / 2), Math.floor((3 * count) / 4), count - 1])];

function sceneJob(root, plan, s, opts) {
  const frames = plan.scenes.find((p) => p.id === s.id).frames;
  const list = opts.sample ? `${sampleFrames(frames).join(",")},` : String(frames);
  const out = join(opts.outDir, opts.sample ? s.id : `${s.id}.mp4`);
  const rasterFile = opts.rasterDir ? join(opts.rasterDir, `${s.id}.json`) : null;
  return { id: s.id, source: sceneSource(root, s), frames: list, out, rasterFile };
}

function workerArgs(job, plan, opts) {
  const size = ["--fps", plan.fps, "--width", plan.width, "--height", plan.height].map(String);
  const blur = ["--subframes", String(opts.sample ? 1 : opts.subframes)];
  const raster = job.rasterFile ? ["--raster-sizes", job.rasterFile] : [];
  return [fileURLToPath(import.meta.url), "--scene", job.source.path, "--out", job.out, ...size, "--frames", job.frames, ...blur, ...raster];
}

function hashPlate(job) {
  check(existsSync(job.source.path), `scene ${job.id}: no HTML page and no plate at ${job.source.path}`);
  mkdirSync(job.out, { recursive: true });
  const digest = createHash("sha256").update(readFileSync(job.source.path)).digest("hex");
  writeFileSync(join(job.out, "plate.sha256"), `${digest}\n`);
}

const runJob = (plan, opts) => (job) =>
  job.source.kind === "html" ? run(process.execPath, workerArgs(job, plan, opts)) : hashPlate(job);

// A lane takes the next job when its last one ends. Jobs keep their timeline order in the report.
async function lane(queue, fn, settled) {
  const job = queue.shift();
  if (!job) return;
  const [result] = await Promise.allSettled([fn(job)]);
  settled.set(job, result);
  await lane(queue, fn, settled);
}

const startLane = (queue, fn, settled) => () => lane(queue, fn, settled);
const resultOf = (settled) => (job) => ({ job, result: settled.get(job) });

async function runPool(jobs, size, fn) {
  const queue = [...jobs];
  const settled = new Map();
  await Promise.all(range(Math.min(size, jobs.length)).map(startLane(queue, fn, settled)));
  return jobs.map(resultOf(settled));
}

const jobFailure = ({ job, result }) => `scene ${job.id}: ${result.reason?.message ?? result.reason}`;
const jobLine = ({ job }) => ({ id: job.id, kind: job.source.kind, out: job.out, frames: job.frames });

async function runScenes(opts) {
  const { planMaster } = await import("./assemble_master.mjs");
  const root = dirname(resolve(opts.timelineFile));
  const timeline = loadTimeline(opts.timelineFile);
  const plan = planMaster(timeline);
  const jobs = timeline.scenes.map((s) => sceneJob(root, plan, s, opts)).filter((j) => opts.sample || j.source.kind === "html");
  const done = await runPool(jobs, opts.workers, runJob(plan, opts));
  const failures = done.filter((d) => d.result.status === "rejected").map(jobFailure);
  check(failures.length === 0, failures.join("\n"));
  const rasterSizes = opts.rasterDir ? writeFilmRaster(root, timeline, opts.rasterDir) : undefined;
  return { workers: opts.workers, subframes: opts.sample ? 1 : opts.subframes, sample: opts.sample, scenes: done.map(jobLine), raster_sizes: rasterSizes };
}

// --scenes renders every HTML scene to <out-dir>/<id>.mp4 with its tail handle, and writes
// render/raster-sizes.json next to timeline.json. With --sample it writes 5 sampled frames
// per HTML scene and a plate hash per file scene to <out-dir>/<id>/, and no raster report.
export async function renderScenes(opts) {
  const rasterDir = opts.sample ? null : mkdtempSync(join(tmpdir(), "video-render-raster-"));
  return runScenes({ ...opts, rasterDir }).finally(() => rasterDir && rmSync(rasterDir, { recursive: true, force: true }));
}

const filesIn = (dir) => (existsSync(dir) ? readdirSync(dir).sort() : null);

const hasFiles = (list) => Boolean(list?.length);
const fileDiffers = (before, after, id) => (f) => !sameBytes(join(before, id, f), join(after, id, f));

function sampleChange(before, after, id, names) {
  const differs = names.filter(fileDiffers(before, after, id));
  return differs.length ? `changed: ${differs.join(", ")}` : null;
}

const isHit = ([hit]) => hit;

// The first problem with the 2 sample lists of a scene, or null when they match by name.
function listProblem(before, after, a, b) {
  const rules = [
    [!hasFiles(a), `no samples in ${before}`],
    [!hasFiles(b), `no samples in ${after}`],
    [json(a) !== json(b), `sample files differ: ${json(a)} vs ${json(b)}`],
  ];
  return rules.find(isHit)?.[1] ?? null;
}

function sceneChange(before, after, id) {
  const [a, b] = [filesIn(join(before, id)), filesIn(join(after, id))];
  return listProblem(before, after, a, b) ?? sampleChange(before, after, id, a);
}

const sceneId = (s) => s.id;
const notIn = (list) => (id) => !list.includes(id);
const changeOf = (before, after) => (id) => ({ id, reason: sceneChange(before, after, id) });
const hasReason = (c) => Boolean(c.reason);

// Revision scope check: every scene outside the fix must keep byte-identical samples.
export function scopeCheck({ timelineFile, before, after, fix }) {
  const ids = loadTimeline(timelineFile).scenes.map(sceneId);
  const unknown = fix.filter(notIn(ids));
  check(unknown.length === 0, `--fix names unknown scene ids: ${unknown.join(", ")}`, UsageError);
  const checked = ids.filter(notIn(fix));
  const changed = checked.map(changeOf(before, after)).filter(hasReason);
  return { passed: changed.length === 0, fix, checked, changed };
}

// ---------------------------------------------------------------- self-notes gate

export const SELF_NOTES_MIN = 3;
// A note is a list item that starts with a film time, M:SS or M:SS.s, bare or in brackets.
const NOTE_LINE = /^\s*(?:[-*]|\d+\.)\s+\[?(\d+):([0-5]\d(?:\.\d+)?)\]?\s+\S/;

const noteTime = (line) => {
  const m = NOTE_LINE.exec(line);
  return m ? Number(m[1]) * 60 + Number(m[2]) : null;
};

const isTime = (t) => t !== null;
const laterThan = (end) => (t) => end !== undefined && t > end;
const lateNote = (end) => (t) => `note at ${t} s is after the film end at ${end} s`;
const tooFew = (n) => (n < SELF_NOTES_MIN ? [`${n} timestamped notes, need ${SELF_NOTES_MIN} or more`] : []);

export function selfNotes(text, durationSec) {
  const times = text.split("\n").map(noteTime).filter(isTime);
  const late = times.filter(laterThan(durationSec)).map(lateNote(durationSec));
  const errors = [...tooFew(times.length), ...late];
  return { passed: errors.length === 0, notes: times.length, times, errors };
}

// ---------------------------------------------------------------- reframing

const even = (v) => Math.max(2, Math.floor(v / 2) * 2);
const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);

function cropSize(src, ratio) {
  const wide = src.width / src.height > ratio;
  return wide ? { w: even(src.height * ratio), h: even(src.height) } : { w: even(src.width), h: even(src.width / ratio) };
}

const startOf = (axis, len) => (r) => r[axis] * len;
const endOf = (axis, len) => (r) => (r[axis] + r[axis + 2]) * len;

function regionSpan(regions, axis, srcLen) {
  const starts = regions.map(startOf(axis, srcLen));
  const ends = regions.map(endOf(axis, srcLen));
  return [Math.min(...starts), Math.max(...ends)];
}

function axisStart(center, len, span, srcLen) {
  const start = center - len / 2;
  const held = span ? clamp(start, span[1] - len, span[0]) : start;
  return Math.floor(clamp(held, 0, srcLen - len) / 2) * 2;
}

const spanFits = (span, len) => !span || span[1] - span[0] <= len;

// The largest crop of the target aspect that fits the source, centred on the focal point
// and then shifted so the union of critical regions stays inside. When the union cannot
// fit in the crop, the result is "fit": the whole frame is scaled and padded.
export function cropWindow(src, out, focal, regions = []) {
  const size = cropSize(src, out.width / out.height);
  const spans = regions.length ? [regionSpan(regions, 0, src.width), regionSpan(regions, 1, src.height)] : [null, null];
  const center = Array.isArray(focal) ? focal : [0.5, 0.5];
  return spanFits(spans[0], size.w) && spanFits(spans[1], size.h) ? placeCrop(src, size, center, spans) : { mode: "fit" };
}

function placeCrop(src, size, center, spans) {
  const x = axisStart(center[0] * src.width, size.w, spans[0], src.width);
  const y = axisStart(center[1] * src.height, size.h, spans[1], src.height);
  return { mode: "crop", x, y, w: size.w, h: size.h };
}

const fitFilter = ({ width: W, height: H }) =>
  `scale=${W}:${H}:force_original_aspect_ratio=decrease:flags=lanczos,pad=${W}:${H}:(ow-iw)/2:(oh-ih)/2:color=black`;
const sceneFrames = (s, fps) => [Math.round(s.start_sec * fps), Math.round((s.start_sec + s.duration_sec) * fps)];
const sceneClip = (dir, id, aspect) => (dir && existsSync(join(dir, `${id}.${aspect}.mp4`)) ? join(dir, `${id}.${aspect}.mp4`) : null);
const inputIndex = (clipFiles, i) => 1 + clipFiles.slice(0, i).filter(Boolean).length;

const clipChain = (k, i, n, fps, size) =>
  `[${k}:v]fps=${fps},${fitFilter(size)},setsar=1,tpad=stop_mode=clone:stop=-1,trim=end_frame=${n},setpts=PTS-STARTPTS[v${i}]`;
const cropFilter = (win, size) =>
  (win.mode === "crop" ? `crop=${win.w}:${win.h}:${win.x}:${win.y},scale=${size.width}:${size.height}:flags=lanczos` : fitFilter(size));
const masterChain = (i, [a, b], filter) => `[m${i}]trim=start_frame=${a}:end_frame=${b},setpts=PTS-STARTPTS,${filter},setsar=1[v${i}]`;

function clipPlan(s, i, frames, ctx) {
  const clip = ctx.clipFiles[i];
  const chain = clipChain(inputIndex(ctx.clipFiles, i), i, frames[1] - frames[0], ctx.info.fps, ASPECTS[ctx.aspect]);
  return { clip, chain, mode: { scene: s.id, mode: "clip", file: clip } };
}

function masterPlan(s, i, frames, ctx) {
  const size = ASPECTS[ctx.aspect];
  const win = cropWindow(ctx.info, size, s.focal_point, s.critical_regions);
  return { clip: null, chain: masterChain(i, frames, cropFilter(win, size)), mode: { scene: s.id, ...win } };
}

function planScene(s, i, ctx) {
  const frames = sceneFrames(s, ctx.info.fps);
  return ctx.clipFiles[i] ? clipPlan(s, i, frames, ctx) : masterPlan(s, i, frames, ctx);
}

const masterTap = (p, i) => (p.clip ? "" : `[m${i}]`);
const outLabel = (_, i) => `[v${i}]`;
const chainOf = (p) => p.chain;
const modeOf = (p) => p.mode;
const asInput = (file) => ["-i", file];
const audioArgs = (info) => (info.audio ? ["-map", "0:a:0", ...AAC_ARGS] : []);
const clipFileFor = (ctx) => (s) => sceneClip(ctx.sceneClips, s.id, ctx.aspect);
const planFor = (ctx) => (s, i) => planScene(s, i, ctx);

function aspectGraph(plans) {
  const taps = plans.map(masterTap).filter(Boolean);
  const split = taps.length ? [`[0:v]split=${taps.length}${taps.join("")}`] : [];
  const concat = `${plans.map(outLabel).join("")}concat=n=${plans.length}:v=1:a=0[v]`;
  return [...split, ...plans.map(chainOf), concat].join(";");
}

async function exportAspect(ctx) {
  const clipFiles = ctx.timeline.scenes.map(clipFileFor(ctx));
  const plans = ctx.timeline.scenes.map(planFor({ ...ctx, clipFiles }));
  const args = ["-i", ctx.master, ...clipFiles.filter(Boolean).flatMap(asInput), "-filter_complex", aspectGraph(plans), "-map", "[v]"];
  await ffmpeg([...args, ...audioArgs(ctx.info), ...h264Args(ctx.info.fps), "-shortest", ctx.outFile]);
  return plans.map(modeOf);
}

// ---------------------------------------------------------------- package mode

const GIF_FILTER = "fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5";
const PREVIEW_MP4_ARGS = ["-vf", "scale=1280:-2:flags=lanczos", "-c:v", "libx264", "-preset", "slow", "-crf", "20", "-pix_fmt", "yuv420p", "-movflags", "+faststart"];
const hookScene = (timeline) => timeline.scenes.find((s) => s.kind === "hook") ?? timeline.scenes[0];
const checkAspect = (aspect) => check(ASPECTS[aspect], `--aspects: unknown aspect "${aspect}"; use 16x9, 9x16 or 1x1`, UsageError);
const fitWarning = (aspect, scene) => `${aspect}: scene ${scene} critical_regions do not fit a crop; it is letterboxed. Re-render it with ?aspect=${aspect}.`;

const writeIn = (dir) => ([name, text]) => writeFileSync(join(dir, name), text);
const inDir = (dir) => (name) => join(dir, name);

function writeTextFiles({ timeline, outDir, report }) {
  const cues = captionCues(timeline);
  const ch = chapters(timeline);
  const files = { "captions.srt": toSrt(cues), "captions.vtt": toVtt(cues), "chapters.txt": ch.text };
  Object.entries(files).forEach(writeIn(outDir));
  report.outputs.push(...Object.keys(files).map(inDir(outDir)));
  report.warnings.push(...ch.warnings);
}

async function writeCover({ master, outDir, hook, coverSec, report }) {
  const at = coverSec ?? hook.start_sec + hook.duration_sec * 0.75;
  const file = join(outDir, "cover.jpg");
  await ffmpeg(["-ss", String(at), "-i", master, "-frames:v", "1", "-vf", fitFilter(ASPECTS["16x9"]), "-q:v", "2", file]);
  report.outputs.push(file);
}

async function writePreviews({ master, outDir, hook, info, report }) {
  const len = Math.min(5, info.duration - hook.start_sec);
  const clipArgs = ["-ss", String(hook.start_sec), "-t", String(len), "-i", master, "-an"];
  const [mp4, gif] = [join(outDir, "preview.mp4"), join(outDir, "preview.gif")];
  await ffmpeg([...clipArgs, ...PREVIEW_MP4_ARGS, mp4]);
  await ffmpeg([...clipArgs, "-filter_complex", GIF_FILTER, "-loop", "0", gif]);
  report.outputs.push(mp4, gif);
  report.warnings.push(...(len < 5 ? [`preview: master holds ${len.toFixed(2)} s after the hook start, not 5 s`] : []));
}

async function writeAspect(ctx, aspect) {
  const outFile = join(ctx.outDir, `${aspect}.mp4`);
  const modes = await exportAspect({ ...ctx, aspect, outFile });
  ctx.report.reframe[aspect] = modes;
  ctx.report.outputs.push(outFile);
  ctx.report.warnings.push(...modes.filter((m) => m.mode === "fit").map((m) => fitWarning(aspect, m.scene)));
}

export async function packagePublish(opts) {
  opts.aspects.forEach(checkAspect);
  const timeline = loadTimeline(opts.timelineFile);
  const info = await probe(opts.master);
  mkdirSync(opts.outDir, { recursive: true });
  const ctx = { ...opts, timeline, info, hook: hookScene(timeline), report: { outputs: [], warnings: [], reframe: {} } };
  writeTextFiles(ctx);
  await writeCover(ctx);
  await writePreviews(ctx);
  await inSequence(opts.aspects, (aspect) => writeAspect(ctx, aspect));
  return ctx.report;
}

// ---------------------------------------------------------------- self-test

const json = (v) => JSON.stringify(v);
const assertEq = (actual, expected, what) => check(json(actual) === json(expected), `${what}: expected ${json(expected)}, got ${json(actual)}`);

const FIXTURE = {
  meta: { id: "demo", title: "Demo Launch", aspect: "16:9", fps: 30, width: 1920, height: 1080, duration_sec: 3.0 },
  scenes: [
    { id: "s01-hook", kind: "hook", start_sec: 0.0, duration_sec: 1.0, caption: "Ship it.",
      narration: "Stop debugging flaky browser tests by hand every single morning before standup.",
      focal_point: [0.5, 0.45], critical_regions: [[0.3, 0.3, 0.4, 0.3]] },
    { id: "s02-demo", kind: "capture", title: "The demo", start_sec: 1.0, duration_sec: 1.0, caption: "One command",
      focal_point: [0.9, 0.5], critical_regions: [[0.8, 0.2, 0.15, 0.4]] },
    { id: "s03-cta", kind: "cta", title: "Get started", start_sec: 2.0, duration_sec: 1.0, caption: "",
      narration: "Install it today.", focal_point: [0.5, 0.5], critical_regions: [[0.05, 0.1, 0.9, 0.5]] },
  ],
};

const EXPECTED_SRT = `1
00:00:00,000 --> 00:00:01,000
Stop debugging flaky browser tests by hand
every single morning before standup.

2
00:00:01,000 --> 00:00:02,000
One command

3
00:00:02,000 --> 00:00:03,000
Install it today.
`;

const sceneHtml = (box = "#f2aa4c") => `<!doctype html><html><head><style>
html,body{margin:0;background:#101820}#b{position:absolute;top:10px;width:16px;height:16px;background:${box}}
</style></head><body><div id="b"></div><script>
window.__videoMeta = { width: 64, height: 48, fps: 10, duration: 0.5 };
window.__seekToFrame = function (frameIndex, fps) {
  document.getElementById("b").style.left = (frameIndex * 9) + "px";
};
</script></body></html>`;
const SCENE_HTML = sceneHtml();

// 2 HTML scenes and 1 capture plate. Scene a has a 0.3 s crossfade tail handle.
const SCOPE_TIMELINE = {
  meta: { id: "scope", fps: 10, width: 64, height: 48, duration_sec: 1.5 },
  scenes: [
    { id: "a", start_sec: 0, duration_sec: 0.5, plate: "scenes/a/index.html", transition_out: "crossfade" },
    { id: "b", start_sec: 0.5, duration_sec: 0.5, plate: "scenes/b/index.html", transition_out: "cut" },
    { id: "c", start_sec: 1.0, duration_sec: 0.5, plate: "captures/c.mp4" },
  ],
};

const NOTES_OK = `# Self-notes
- 0:00.4 s01-hook: the title lands 2 frames before the downbeat. Fix: video-animator.
- [0:02] s02-cta: the button text is small at phone width. Fix: video-animator.
1. 0:03.5 s02-cta: the fade ends on a flat frame. Fix: video-editor.
- no time on this line, so it does not count
`;
const NOTE_CASES = [
  [[NOTES_OK, 5.5], { passed: true, notes: 3 }, "3 timestamped notes"],
  [[NOTES_OK.replace("1. 0:03.5", "1. later"), 5.5], { passed: false, notes: 2 }, "2 timestamped notes"],
  [[NOTES_OK, 3.0], { passed: false, notes: 3 }, "a note after the film end"],
  [["- 0:01 one\n- 0:02 two\n- 0:03 three\n"], { passed: true, notes: 3 }, "no timeline"],
];

const CAPTION_HTML = '<!doctype html><p style="margin:0;color:#fff">Hi</p><script>window.__seekToFrame=()=>{};</script>';

const HD = ASPECTS["16x9"];
const CROP_CASES = [
  [[HD, ASPECTS["9x16"], [0.5, 0.45], [[0.3, 0.3, 0.4, 0.3]]], { mode: "fit" }, "too-wide region"],
  [[HD, ASPECTS["9x16"], [0.9, 0.5], [[0.8, 0.2, 0.15, 0.4]]], { mode: "crop", x: 1314, y: 0, w: 606, h: 1080 }, "right focal"],
  [[HD, ASPECTS["9x16"], [0.2, 0.5], [[0.4, 0.2, 0.1, 0.1]]], { mode: "crop", x: 354, y: 0, w: 606, h: 1080 }, "shift to region"],
  [[HD, ASPECTS["1x1"], [0.5, 0.5]], { mode: "crop", x: 420, y: 0, w: 1080, h: 1080 }, "square centre"],
  [[HD, HD, [0.1, 0.1]], { mode: "crop", x: 0, y: 0, w: 1920, h: 1080 }, "same aspect"],
  [[HD, ASPECTS["1x1"], null], { mode: "crop", x: 420, y: 0, w: 1080, h: 1080 }, "no focal point"],
];

const FRAME_CASES = [
  [{ frames: "4", fps: 60 }, [0, 1, 2, 3], "count"],
  [{ frames: "30,0,15", fps: 60 }, [0, 15, 30], "list"],
  [{ frames: "0,15,30", fps: 60, reverse: true }, [30, 15, 0], "reverse"],
  [{ duration: 0.1, fps: 60 }, [0, 1, 2, 3, 4, 5], "duration"],
  [{ frames: "7,", fps: 60 }, [7], "single index"],
];

const listFiles = (dir) => readdirSync(dir).flatMap((name) => fileOrTree(join(dir, name)));
const fileOrTree = (path) => (statSync(path).isDirectory() ? listFiles(path) : [path]);
const sameBytes = (a, b) => readFileSync(a).equals(readFileSync(b));

function testSkillMd() {
  const md = readFileSync(join(SKILL_DIR, "SKILL.md"));
  check(md.length < SKILL_MD_MAX_BYTES, `SKILL.md is ${md.length} bytes`);
  check(/^---\nname: video-render\ndescription: .+\n---\n/.test(md.toString()), "frontmatter needs name: video-render and description:");
  return `${md.length} bytes`;
}

const notText = (f) => ![".md", ".mjs"].includes(extname(f));

function testTextOnly() {
  const bad = listFiles(SKILL_DIR).filter(notText);
  assertEq(bad, [], "non-text files");
}

function testSrt() {
  const srt = toSrt(captionCues(FIXTURE));
  assertEq(srt, EXPECTED_SRT, "srt");
  assertEq(toSrt(captionCues(FIXTURE)), srt, "second run");
}

function testVtt() {
  const vtt = toVtt(captionCues(FIXTURE));
  check(vtt.startsWith("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\n"), "vtt header and first cue");
  assertEq(vtt.split("-->").length - 1, 3, "vtt cue count");
  check(vtt.endsWith("00:00:02.000 --> 00:00:03.000\nInstall it today.\n"), "vtt last cue");
}

function testChapters() {
  const ch = chapters(FIXTURE);
  assertEq(ch.text, "0:00 Demo Launch\n0:01 The demo\n0:02 Get started\n", "chapters");
  assertEq(ch.warnings.length, 3, "YouTube 10 s warnings");
}

const testCrop = () => CROP_CASES.forEach(([args, want, what]) => assertEq(cropWindow(...args), want, what));
const testFrames = () => FRAME_CASES.forEach(([opts, want, what]) => assertEq(frameList(opts), want, what));

async function testEncode(root) {
  const dir = join(root, "frames");
  mkdirSync(dir);
  await ffmpeg(["-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30", "-frames:v", "12", "-start_number", "0", join(dir, "frame_%06d.png")]);
  await encodeFrames(dir, 30, join(root, "cfr.mp4"));
  const p = await probe(join(root, "cfr.mp4"));
  assertEq([p.video.codec_name, p.video.pix_fmt, p.video.profile], ["h264", "yuv420p", "High"], "codec");
  assertEq([p.video.r_frame_rate, p.video.avg_frame_rate, p.video.nb_frames], ["30/1", "30/1", "12"], "cfr");
}

async function makeMaster(file) {
  const sources = ["-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30:duration=3", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=3"];
  await ffmpeg([...sources, "-ac", "2", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", ...AAC_ARGS, "-shortest", file]);
}

async function checkAspectExport(outDir, [aspect, size]) {
  const p = await probe(join(outDir, `${aspect}.mp4`));
  assertEq([p.width, p.height, p.video.r_frame_rate, p.video.nb_frames, p.video.profile], [size.width, size.height, "30/1", "90", "High"], aspect);
  assertEq([p.audio?.codec_name, p.audio?.sample_rate, p.audio?.channels], ["aac", "48000", 2], `${aspect} audio`);
}

async function checkStills(outDir) {
  const cover = await probe(join(outDir, "cover.jpg"));
  assertEq([cover.width, cover.height], [1920, 1080], "cover size");
  const prev = await probe(join(outDir, "preview.mp4"));
  check(prev.audio === null && prev.width === 1280, "preview.mp4 is muted and 1280 px wide");
  check(statSync(join(outDir, "preview.gif")).size > 0, "preview.gif written");
}

async function testPackage(root) {
  const [timelineFile, master, outDir] = [join(root, "timeline.json"), join(root, "master.mp4"), join(root, "render")];
  writeFileSync(timelineFile, json(FIXTURE));
  await makeMaster(master);
  const report = await packagePublish({ timelineFile, master, outDir, aspects: Object.keys(ASPECTS) });
  assertEq(readFileSync(join(outDir, "captions.srt"), "utf8"), EXPECTED_SRT, "packaged srt");
  await inSequence(Object.entries(ASPECTS), (entry) => checkAspectExport(outDir, entry));
  assertEq(report.reframe["9x16"].map((m) => m.mode), ["fit", "crop", "fit"], "9x16 modes");
  await checkStills(outDir);
}

function sceneBase(root) {
  const scene = join(root, "scene", "index.html");
  mkdirSync(dirname(scene), { recursive: true });
  writeFileSync(scene, SCENE_HTML);
  return { scene, fps: 10, width: 64, height: 48 };
}

async function testForwardReverse(root) {
  const base = sceneBase(root);
  const [fwd, rev] = [join(root, "fwd"), join(root, "rev")];
  await renderScene({ ...base, out: fwd, frames: "0,2,4" });
  const r = await renderScene({ ...base, out: rev, frames: "0,2,4", reverse: true });
  assertEq(r.frames, [4, 2, 0], "reverse order");
  [0, 2, 4].forEach((f) => check(sameBytes(join(fwd, frameName(f)), join(rev, frameName(f))), `frame ${f} differs`));
  check(!sameBytes(join(fwd, frameName(0)), join(fwd, frameName(4))), "frames 0 and 4 must differ");
}

async function testSceneMp4(root) {
  const out = join(root, "scene.mp4");
  await renderScene({ ...sceneBase(root), out });
  const p = await probe(out);
  assertEq([p.width, p.height, p.video.r_frame_rate, p.video.nb_frames], [64, 48, "10/1", "5"], "scene mp4");
}

async function testTransparent(root) {
  const scene = join(sceneBase(root).scene, "..", "caption.html");
  writeFileSync(scene, CAPTION_HTML);
  const dir = join(root, "alpha");
  await renderScene({ ...sceneBase(root), scene, out: dir, frames: "1", transparent: true });
  const png = join(dir, frameName(0));
  assertEq((await probe(png)).video.pix_fmt, "rgba", "pix_fmt");
  const alpha = await run("ffmpeg", ["-v", "error", "-i", png, "-vf", "crop=1:1:63:47,alphaextract", "-f", "rawvideo", "-pix_fmt", "gray", "-"]);
  assertEq(Buffer.from(alpha, "latin1")[0], 0, "corner alpha");
}

async function testMissingChrome() {
  const saved = process.env.CHROME_PATH, missing = "/nonexistent/chrome-for-self-test";
  process.env.CHROME_PATH = missing;
  const error = await launchChrome(64, 48).then(() => null, (e) => e).finally(() => restoreEnv("CHROME_PATH", saved));
  check(error instanceof UsageError, `expected a UsageError, got: ${error}`);
  assertEq(error.message, `Chrome not found at ${missing}. Set CHROME_PATH or install Google Chrome.`, "message");
}

const restoreEnv = (key, value) => (value === undefined ? delete process.env[key] : (process.env[key] = value));

async function testMissingSeek(root) {
  const bad = join(sceneBase(root).scene, "..", "bad.html");
  writeFileSync(bad, "<!doctype html><p>no clock</p>");
  const error = await renderScene({ ...sceneBase(root), scene: bad, out: join(root, "bad"), frames: "1" }).then(() => null, (e) => e);
  check(error?.message.includes('typeof window.__seekToFrame is "undefined"'), `error was: ${error?.message}`);
}


const testSubframeTimes = () => {
  assertEq(subframeTimes(3, 1), [3], "1 subframe");
  assertEq(subframeTimes(3, 4), [3, 3.25, 3.5, 3.75], "4 subframes");
};

const testWorkers = () => {
  assertEq([1, 4, 10, 14, 64].map(defaultWorkers), [1, 2, 6, 8, 8], "60 % of CPUs, 1 to 8");
};

const testSamples = () => {
  assertEq(sampleFrames(8), [0, 2, 4, 6, 7], "8 frames");
  assertEq(sampleFrames(2), [0, 1], "2 frames");
};

const pick = (r) => ({ passed: r.passed, notes: r.notes });
const testSelfNotes = () => NOTE_CASES.forEach(([args, want, what]) => assertEq(pick(selfNotes(...args)), want, what));

// A raw row of a PNG, as bytes: gray by default, or 3 bytes a pixel for rgb24.
function grayRow(png, y, width, format = "gray") {
  const args = ["-v", "error", "-i", png, "-vf", `crop=${width}:1:0:${y},format=${format}`, "-f", "rawvideo", "-"];
  return [...spawnSync("ffmpeg", args).stdout];
}

// Subframe i is a flat grey of value 30 x i.
const greyPlate = (sub) => (i) =>
  ffmpeg(["-f", "lavfi", "-i", `color=c=0x${pad((i * 30).toString(16)).repeat(3)}:s=4x4,format=rgb24`, "-frames:v", "1", join(sub, subName(i))]);
const redOf = (dir) => (f) => grayRow(join(dir, frameName(f)), 0, 1, "rgb24")[0];

async function testAverage(root) {
  const [sub, out] = [join(root, "avg-sub"), join(root, "avg-out")];
  mkdirSync(sub);
  mkdirSync(out);
  await inSequence(range(8), greyPlate(sub));
  await averageSubframes(sub, 4, [3, 9], out);
  assertEq([3, 9].map(redOf(out)), [45, 165], "window means");
}

// Pixels between the background (22) and the box (180) on the row through the box.
const blurWidth = (png) => grayRow(png, 18, 64).filter((v) => v > 32 && v < 170).length;

const renderBlur = (root) => (n) => renderScene({ ...sceneBase(root), out: join(root, `blur${n}`), frames: "2,", subframes: n });
const blurOf = (root) => (n) => blurWidth(join(root, `blur${n}`, frameName(2)));

async function testBlurWidth(root) {
  await inSequence([1, 8], renderBlur(root));
  const widths = [1, 8].map(blurOf(root));
  check(widths[0] === 0 && widths[1] > 1, `blur width at 1 and 8 subframes: ${widths.join(", ")} px`);
  return `${widths[1]} px at 8 subframes`;
}

async function testBlurMp4(root) {
  const out = join(root, "blur.mp4");
  await renderScene({ ...sceneBase(root), out, subframes: 4 });
  const p = await probe(out);
  assertEq([p.video.r_frame_rate, p.video.avg_frame_rate, p.video.nb_frames], ["10/1", "10/1", "5"], "blurred mp4 frames");
}

async function testBlurReverse(root) {
  const [fwd, rev] = [join(root, "bfwd"), join(root, "brev")];
  await renderScene({ ...sceneBase(root), out: fwd, frames: "1,3", subframes: 4 });
  await renderScene({ ...sceneBase(root), out: rev, frames: "1,3", subframes: 4, reverse: true });
  [1, 3].forEach((f) => check(sameBytes(join(fwd, frameName(f)), join(rev, frameName(f))), `blurred frame ${f} differs`));
}

function scopeProject(root) {
  const dir = join(root, "scope");
  ["scenes/a", "scenes/b", "captures"].forEach((d) => mkdirSync(join(dir, d), { recursive: true }));
  writeFileSync(join(dir, "timeline.json"), json(SCOPE_TIMELINE));
  writeFileSync(join(dir, "scenes/a/index.html"), sceneHtml());
  writeFileSync(join(dir, "scenes/b/index.html"), sceneHtml());
  writeFileSync(join(dir, "captures/c.mp4"), "plate bytes");
  return { dir, timelineFile: join(dir, "timeline.json") };
}

const frameCountOf = (dir) => async (id) => (await probe(join(dir, `${id}.mp4`))).video.nb_frames;

async function testParallelScenes(root) {
  const { dir, timelineFile } = scopeProject(root);
  const report = await renderScenes({ timelineFile, outDir: join(dir, "render/scenes"), workers: 2, subframes: 1, sample: false });
  assertEq(report.scenes.map(sceneId), ["a", "b"], "HTML scenes only");
  const counts = await Promise.all(["a", "b"].map(frameCountOf(join(dir, "render/scenes"))));
  assertEq(counts, ["8", "5"], "frames with the tail handle");
}

const sampleInto = (timelineFile, outDir) => renderScenes({ timelineFile, outDir, workers: 2, subframes: 1, sample: true });

// Recolour the box of scene `id`, sample every scene into <dir>/<round>, and check
// the samples against <dir>/before with a fix in scene a.
async function recolourRound({ dir, timelineFile }, id, colour, round) {
  writeFileSync(join(dir, `scenes/${id}/index.html`), sceneHtml(colour));
  await sampleInto(timelineFile, join(dir, round));
  return scopeCheck({ timelineFile, before: join(dir, "before"), after: join(dir, round), fix: ["a"] });
}

async function testScope(root) {
  const project = scopeProject(root);
  await sampleInto(project.timelineFile, join(project.dir, "before"));
  const one = await recolourRound(project, "a", "#4cf2aa", "fix1");
  assertEq([one.passed, one.checked], [true, ["b", "c"]], "a fix inside scope a");
  const two = await recolourRound(project, "b", "#4c4cf2", "fix2");
  assertEq([two.passed, two.changed.map(sceneId)], [false, ["b"]], "a change outside scope a");
}

const testRasterFrames = () => {
  assertEq(rasterFrames(5, 10), [0, 1, 2, 3, 4], "10 fps");
  assertEq(rasterFrames(10, 24), [0, 2, 5, 7], "24 fps, nearest frames");
  const at30 = rasterFrames(84, 30);
  assertEq([at30.length, at30[1], at30.at(-1)], [28, 3, 81], "30 fps, 2.8 s");
};

const testLargestPerSource = () => {
  const s = (t, src, w) => ({ t_sec: t, src, natural_px: [100, 100], drawn_px: [w, w] });
  const got = largestPerSource([s(0.1, "b.png", 100), s(0.1, "a.png", 110), s(0.1, "a.png", 130), s(0, "a.png", 100)]);
  assertEq(got, [s(0, "a.png", 100), s(0.1, "a.png", 130), s(0.1, "b.png", 100)], "largest scale per image and time");
};

// Each fixture draws a 100 px PNG. One <img> scene, and one scene with every other image kind.
const imgHtml = (px, file = "img.png") => `<!doctype html><html><body style="margin:0;background:#111">
<img src="${file}" style="width:${px}px;height:${px}px"><script>
window.__videoMeta = { duration: 0.5 };
window.__seekToFrame = function () {};
</script></body></html>`;

const KINDS_HTML = `<!doctype html><html><body style="margin:0;display:flex;align-items:flex-start">
<img id="src" src="canvas.png" style="display:none">
<div style="width:200px;height:130px;background:url(bg.png) no-repeat;background-size:130px auto"></div>
<canvas id="c" width="130" height="130"></canvas>
<video poster="poster.png" width="130" height="130"></video>
<svg width="130" height="130"><image href="svg.png" width="130" height="130"/></svg>
<img src="vector.svg" style="width:130px;height:130px">
<script>
window.__videoMeta = { duration: 0.2 };
window.__seekToFrame = function () {
  const ctx = document.getElementById("c").getContext("2d");
  ctx.clearRect(0, 0, 130, 130);
  ctx.drawImage(document.getElementById("src"), 0, 0, 130, 130);
};
</script></body></html>`;

const VECTOR_SVG = '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>';

async function rasterDir(root, name, files) {
  const dir = join(root, name);
  mkdirSync(dir, { recursive: true });
  await ffmpeg(["-f", "lavfi", "-i", "color=c=0x4cf2aa:s=100x100", "-frames:v", "1", join(dir, files[0])]);
  files.slice(1).forEach((f) => copyFileSync(join(dir, files[0]), join(dir, f)));
  return dir;
}

const readJson = (file) => JSON.parse(readFileSync(file, "utf8"));
const rasterRender = (scene, out, rasterSizes) => renderScene({ scene, out, fps: 10, width: 720, height: 160, rasterSizes });
const sizeLine = (s) => `${s.src} ${s.natural_px.join("x")} -> ${s.drawn_px.join("x")}`;

async function testRasterKinds(root) {
  const dir = await rasterDir(root, "raster-kinds", ["bg.png", "canvas.png", "poster.png", "svg.png"]);
  writeFileSync(join(dir, "vector.svg"), VECTOR_SVG);
  writeFileSync(join(dir, "index.html"), KINDS_HTML);
  await rasterRender(join(dir, "index.html"), join(dir, "frames"), join(dir, "raster-sizes.json"));
  const report = readJson(join(dir, "raster-sizes.json"));
  assertEq(report.step_sec, RASTER_STEP_SEC, "step_sec");
  const at = (t) => report.samples.filter((s) => s.t_sec === t).map(sizeLine);
  const want = ["bg.png", "canvas.png", "poster.png", "svg.png"].map((f) => `${f} 100x100 -> 130x130`);
  assertEq([at(0), at(0.1)], [want, want], "samples at 0 and 0.1 s");
}

const QA_SCRIPTS = join(SKILL_DIR, "..", "video-qa", "scripts");
const RASTER_QA = `import pathlib, sys, types
sys.path.insert(0, sys.argv[1])
import qa_video
ctx = types.SimpleNamespace(raster_sizes=pathlib.Path(sys.argv[2]), warnings=[])
detail, errors = qa_video.check_raster_upscale(ctx)
print(detail, errors)
sys.exit(1 if errors or detail.get("skipped") else 0)`;

// The video-qa check itself, run on a report: true when it passes.
function qaRasterPasses(file) {
  const r = spawnSync("python3", ["-I", "-B", "-c", RASTER_QA, QA_SCRIPTS, file], { encoding: "utf8" });
  check(r.status === 0 || r.status === 1, `qa_video raster_upscale exited ${r.status}: ${r.stderr}`);
  return r.status === 0;
}

const RASTER_TIMELINE = {
  meta: { id: "raster", fps: 10, width: 720, height: 160, duration_sec: 1.0 },
  scenes: [
    { id: "a", start_sec: 0, duration_sec: 0.5, plate: "scenes/a/index.html", transition_out: "crossfade" },
    { id: "b", start_sec: 0.5, duration_sec: 0.5, plate: "scenes/b/index.html", transition_out: "cut" },
  ],
};

// --scene with a 100 px image at 100 px passes; --scenes with one at 130 px in scene b fails.
async function testRasterQa(root) {
  const dir = await rasterDir(root, "raster-qa", ["img.png"]);
  writeFileSync(join(dir, "clean.html"), imgHtml(100));
  await rasterRender(join(dir, "clean.html"), join(dir, "clean"), join(dir, "clean.json"));
  ["scenes/a", "scenes/b"].forEach((d) => mkdirSync(join(dir, d), { recursive: true }));
  writeFileSync(join(dir, "scenes/a/index.html"), imgHtml(100, "../../img.png"));
  writeFileSync(join(dir, "scenes/b/index.html"), imgHtml(130, "../../img.png"));
  writeFileSync(join(dir, "timeline.json"), json(RASTER_TIMELINE));
  const report = await renderScenes({ timelineFile: join(dir, "timeline.json"), outDir: join(dir, "render/scenes"), workers: 2, subframes: 1, sample: false });
  const film = readJson(report.raster_sizes);
  assertEq(report.raster_sizes, join(dir, "render", "raster-sizes.json"), "report path");
  assertEq(film.samples.map((s) => `${s.t_sec} ${s.drawn_px[0]}`), [0, 0.1, 0.2, 0.3, 0.4].map((t) => `${t} 100`).concat(
    [0.5, 0.6, 0.7, 0.8, 0.9].map((t) => `${t} 130`)), "film times, tail handle dropped");
  if (!existsSync(join(QA_SCRIPTS, "qa_video.py"))) return "qa_video.py is not next to this skill; report format checked only";
  check(qaRasterPasses(join(dir, "clean.json")), "qa_video raster_upscale fails the clean 1x report");
  check(!qaRasterPasses(report.raster_sizes), "qa_video raster_upscale passes a 1.3x image");
  return "1x passes, 1.3x fails in qa_video.py";
}

const BASE_TESTS = [
  ["SKILL.md is under 12288 bytes with name: video-render", testSkillMd],
  ["skill folder holds only .md and .mjs text files", testTextOnly],
  ["captions.srt is deterministic and wraps at 42 chars, 2 lines", testSrt],
  ["captions.vtt matches the srt cues", testVtt],
  ["chapters.txt uses scene titles and start_sec", testChapters],
  ["crop window follows focal_point and keeps critical_regions", testCrop],
  ["--frames parses counts, lists and --reverse", testFrames],
  ["ffmpeg encodes PNG frames to CFR H.264 High yuv420p", testEncode],
  ["--package writes captions, chapters, cover, previews and 3 aspects", testPackage],
  ["a missing Chrome binary gives a usage error with a clear message", testMissingChrome],
  ["subframe n of frame k seeks k + n / count", testSubframeTimes],
  ["default workers are 60 % of CPUs, 1 to 8", testWorkers],
  ["scope samples are first, quarters and last", testSamples],
  ["--self-notes needs 3 timestamped notes inside the film", testSelfNotes],
  ["tmix averages each window of subframes into 1 frame", testAverage],
  ["raster samples use the frames nearest the 0.1 s grid", testRasterFrames],
  ["raster report keeps the largest scale per image and time", testLargestPerSource],
];

const CHROME_TESTS = [
  ["Chrome cold-forward and warm-reverse frames are byte-identical", testForwardReverse],
  ["Chrome render to .mp4 uses __videoMeta.duration", testSceneMp4],
  ["Chrome --transparent writes RGBA frames with a clear background", testTransparent],
  ["Chrome render fails without window.__seekToFrame", testMissingSeek],
  ["8 subframes blur a moving square, 1 subframe keeps it sharp", testBlurWidth],
  ["a blurred .mp4 keeps CFR and duration x fps frames", testBlurMp4],
  ["blurred frames are byte-identical forward and reverse", testBlurReverse],
  ["--scenes renders HTML scenes in parallel workers with tail handles", testParallelScenes],
  ["scope check passes a fix inside scope and names a changed scene outside it", testScope],
  ["raster sizes cover background, canvas, poster and SVG image, not SVG files", testRasterKinds],
  ["raster_upscale passes a 1x image and fails a 1.3x image from --scenes", testRasterQa],
];

const skipChrome = () => [[`headless Chrome render # SKIP no Chrome at ${chromePath()}`, () => undefined]];
const testLine = (name, o) => (o.ok ? `ok - ${name}${o.note ? ` (${o.note})` : ""}` : `not ok - ${name}: ${o.note}`);

const passedWith = (note) => ({ ok: true, note });
const failedWith = (e) => ({ ok: false, note: e.message });
const attempt = async (fn, arg) => fn(arg);

async function runTest(root, [name, fn]) {
  const outcome = await attempt(fn, root).then(passedWith, failedWith);
  console.log(testLine(name, outcome));
  return outcome.ok;
}

async function selfTest() {
  const root = mkdtempSync(join(tmpdir(), "video-render-selftest-"));
  const tests = [...BASE_TESTS, ...(existsSync(chromePath()) ? CHROME_TESTS : skipChrome())];
  const results = [];
  await inSequence(tests, async (t) => results.push(await runTest(root, t)));
  rmSync(root, { recursive: true, force: true });
  const passed = results.filter(Boolean).length;
  console.log(`${passed} of ${results.length} tests passed`);
  return passed === results.length;
}

// ---------------------------------------------------------------- CLI

const OPTIONS = {
  scene: { type: "string" }, out: { type: "string" }, fps: { type: "string" },
  width: { type: "string" }, height: { type: "string" }, duration: { type: "string" },
  frames: { type: "string" }, reverse: { type: "boolean" }, transparent: { type: "boolean" },
  package: { type: "string" }, master: { type: "string" }, "out-dir": { type: "string" },
  aspects: { type: "string" }, "cover-sec": { type: "string" }, "scene-clips": { type: "string" },
  subframes: { type: "string" }, scenes: { type: "string" }, workers: { type: "string" }, sample: { type: "boolean" },
  "scope-check": { type: "string" }, before: { type: "string" }, after: { type: "string" }, fix: { type: "string" },
  "self-notes": { type: "string" }, timeline: { type: "string" }, "raster-sizes": { type: "string" },
  "self-test": { type: "boolean" }, help: { type: "boolean", short: "h" },
};

function positive(value, flag, fallback) {
  const n = Number(value);
  const valid = Number.isFinite(n) && n > 0;
  return value === undefined ? fallback : valid ? n : fail(`${flag}: "${value}" is not a positive number`, UsageError);
}

function sceneOptions(v) {
  const fps = positive(v.fps, "--fps", 60);
  check(Number.isInteger(fps), "--fps must be an integer", UsageError);
  const size = { width: positive(v.width, "--width", 1920), height: positive(v.height, "--height", 1080) };
  const flags = { frames: v.frames, reverse: Boolean(v.reverse), transparent: Boolean(v.transparent) };
  const rasterSizes = v["raster-sizes"] ? resolve(v["raster-sizes"]) : undefined;
  return { scene: v.scene, out: v.out, fps, ...size, duration: positive(v.duration, "--duration"), ...flags, subframes: subframes(v), rasterSizes };
}

function subframes(v) {
  const n = positive(v.subframes, "--subframes", 1);
  return SUBFRAME_COUNTS.includes(n) ? n : fail(`--subframes must be one of ${SUBFRAME_COUNTS.join(", ")}`, UsageError);
}

function workers(v) {
  const n = positive(v.workers, "--workers", defaultWorkers());
  return Number.isInteger(n) ? n : fail("--workers must be an integer", UsageError);
}

async function scenesMode(v) {
  check(v["out-dir"], "--scenes needs --out-dir", UsageError);
  const opts = { timelineFile: v.scenes, outDir: resolve(v["out-dir"]), workers: workers(v), subframes: subframes(v), sample: Boolean(v.sample) };
  console.log(JSON.stringify(await renderScenes(opts), null, 2));
  return 0;
}

const idList = (text) => (text ?? "").split(",").map((s) => s.trim()).filter(Boolean);

function scopeMode(v) {
  check(v.before && v.after, "--scope-check needs --before and --after", UsageError);
  const report = scopeCheck({ timelineFile: v["scope-check"], before: resolve(v.before), after: resolve(v.after), fix: idList(v.fix) });
  console.log(JSON.stringify(report, null, 2));
  return report.passed ? 0 : 1;
}

function selfNotesMode(v) {
  const duration = v.timeline ? loadTimeline(v.timeline).meta.duration_sec : undefined;
  const report = selfNotes(readFileSync(v["self-notes"], "utf8"), duration);
  console.log(JSON.stringify(report, null, 2));
  return report.passed ? 0 : 1;
}

async function sceneMode(v) {
  check(v.out, "--scene needs --out", UsageError);
  const r = await renderScene(sceneOptions(v));
  console.log(json({ out: r.out, frame_count: r.frames.length, first: r.frames[0], last: r.frames.at(-1) }));
  return 0;
}

function packageOptions(v) {
  const aspects = (v.aspects ?? "16x9,9x16,1x1").split(",").map((s) => s.trim()).filter(Boolean);
  const coverSec = v["cover-sec"] === undefined ? undefined : Number(v["cover-sec"]);
  const sceneClips = v["scene-clips"] ? resolve(v["scene-clips"]) : undefined;
  return { timelineFile: v.package, master: v.master, outDir: resolve(v["out-dir"]), aspects, coverSec, sceneClips };
}

async function packageMode(v) {
  check(v.master && v["out-dir"], "--package needs --master and --out-dir", UsageError);
  const report = await packagePublish(packageOptions(v));
  console.log(JSON.stringify(report, null, 2));
  report.warnings.forEach((w) => process.stderr.write(`warning: ${w}\n`));
  return 0;
}

const uncomment = (line) => line.replace(/^\/\/ ?/, "");

function showHelp() {
  const header = readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 19);
  process.stdout.write(`${header.map(uncomment).join("\n")}\n`);
  return 0;
}

const selfTestMode = async () => ((await selfTest()) ? 0 : 1);
const MODES = [
  ["help", showHelp], ["self-test", selfTestMode], ["scene", sceneMode], ["package", packageMode],
  ["scenes", scenesMode], ["scope-check", scopeMode], ["self-notes", selfNotesMode],
];

const modeIn = (values) => ([flag]) => values[flag];

function main() {
  const { values } = parseArgs({ options: OPTIONS, strict: true });
  const mode = MODES.find(modeIn(values)) ?? fail("pass --scene, --scenes, --package, --scope-check, --self-notes or --self-test (see --help)", UsageError);
  return mode[1](values);
}

function die(e) {
  process.stderr.write(`render_frames: ${e.message}\n`);
  process.exit(e instanceof UsageError || String(e.code).startsWith("ERR_PARSE_ARGS") ? 2 : 1);
}

// Run the CLI only when this file is the entry point, so assemble_master.mjs can import it.
const isEntry = process.argv[1] && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
if (isEntry) Promise.resolve().then(main).then((code) => process.exit(code), die);
