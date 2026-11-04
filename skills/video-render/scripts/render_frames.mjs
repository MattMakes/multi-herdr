#!/usr/bin/env node
// Deterministic frame-stepping renderer and publish packager for the video pipeline.
// Node built-ins only. Chrome is driven over the Chrome DevTools Protocol; ffmpeg and
// ffprobe run as subprocesses.
//
//   render_frames.mjs --scene <path-or-url> --out <mp4-or-frames-dir> [--fps 60]
//                     [--width 1920] [--height 1080] [--duration <sec>]
//                     [--frames <count> | --frames <i,j,k>] [--reverse] [--transparent]
//   render_frames.mjs --package <timeline.json> --master <render/master.mp4>
//                     --out-dir <render/> [--aspects 16x9,9x16,1x1] [--cover-sec <sec>]
//                     [--scene-clips <dir>]
//   render_frames.mjs --self-test
//
// Exit codes: 0 success, 1 render or package failure, 2 usage error.

import { spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const DEFAULT_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const CDP_TIMEOUT_MS = 60_000;
const SKILL_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SKILL_MD_MAX_BYTES = 12288;
const CLEAR = { r: 0, g: 0, b: 0, a: 0 };

export const ASPECTS = {
  "16x9": { width: 1920, height: 1080 },
  "9x16": { width: 1080, height: 1920 },
  "1x1": { width: 1080, height: 1080 },
};

class UsageError extends Error {}

function fail(message, Kind = Error) {
  throw new Kind(message);
}

const check = (cond, message, Kind) => cond || fail(message, Kind);
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

async function run(cmd, args) {
  const child = spawn(cmd, args, { stdio: ["ignore", "pipe", "pipe"] });
  const stdout = collect(child.stdout);
  const stderr = collect(child.stderr);
  const [code] = await once(child, "close");
  return code === 0 ? stdout() : fail(runError(cmd, code, stderr()));
}

const ffmpeg = (args) => run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...args]);
const streamOf = (data, type) => data.streams.find((s) => s.codec_type === type) ?? null;
const rate = (text) => text.split("/").map(Number).reduce((num, den) => num / den);

async function probe(file) {
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
function chromeArgs(userDataDir, width, height) {
  return [
    "--headless=new", "--remote-debugging-port=0", `--user-data-dir=${userDataDir}`,
    `--window-size=${width},${height}`, "--force-device-scale-factor=1",
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
  const url = await withTimeout(started.promise, "Chrome DevTools URL");
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

async function stopChrome(browser) {
  browser.ws?.close();
  await killChrome(browser.proc);
  rmSync(browser.userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
}

async function abortLaunch(browser, error) {
  await stopChrome(browser);
  throw error;
}

async function launchChrome(width, height) {
  const binary = chromePath();
  check(existsSync(binary), `Chrome not found at ${binary}. Set CHROME_PATH.`);
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

async function captureFrame(page, f) {
  await evaluate(page.cdp, page.sessionId, frameScript(f, page.fps));
  const params = { format: "png", fromSurface: true, captureBeyondViewport: false, clip: page.clip };
  const shot = await page.cdp.send("Page.captureScreenshot", params, page.sessionId);
  writeFileSync(join(page.framesDir, frameName(f)), Buffer.from(shot.data, "base64"));
}

async function encodeTarget(target, fps) {
  mkdirSync(dirname(target.out), { recursive: true });
  await encodeFrames(target.framesDir, fps, target.out);
}

async function renderInBrowser(cdp, opts, target) {
  const sessionId = await openPage(cdp, opts);
  const meta = await loadScene(cdp, sessionId, opts.scene);
  warnMeta(meta, opts);
  const order = frameList({ ...opts, metaDuration: meta.duration });
  check(!target.toMp4 || order.every((f) => f < order.length), "an .mp4 --out needs contiguous frames 0..N-1; write a frames directory instead", UsageError);
  const clip = { x: 0, y: 0, width: opts.width, height: opts.height, scale: 1 };
  await inSequence(order, (f) => captureFrame({ cdp, sessionId, fps: opts.fps, clip, framesDir: target.framesDir }, f));
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

const SCENE_HTML = `<!doctype html><html><head><style>
html,body{margin:0;background:#101820}#b{position:absolute;top:10px;width:16px;height:16px;background:#f2aa4c}
</style></head><body><div id="b"></div><script>
window.__videoMeta = { width: 64, height: 48, fps: 10, duration: 0.5 };
window.__seekToFrame = function (frameIndex, fps) {
  document.getElementById("b").style.left = (frameIndex * 9) + "px";
};
</script></body></html>`;

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

async function testMissingSeek(root) {
  const bad = join(sceneBase(root).scene, "..", "bad.html");
  writeFileSync(bad, "<!doctype html><p>no clock</p>");
  const error = await renderScene({ ...sceneBase(root), scene: bad, out: join(root, "bad"), frames: "1" }).then(() => null, (e) => e);
  check(error?.message.includes('typeof window.__seekToFrame is "undefined"'), `error was: ${error?.message}`);
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
];

const CHROME_TESTS = [
  ["Chrome cold-forward and warm-reverse frames are byte-identical", testForwardReverse],
  ["Chrome render to .mp4 uses __videoMeta.duration", testSceneMp4],
  ["Chrome --transparent writes RGBA frames with a clear background", testTransparent],
  ["Chrome render fails without window.__seekToFrame", testMissingSeek],
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
  return { scene: v.scene, out: v.out, fps, ...size, duration: positive(v.duration, "--duration"), ...flags };
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
  const header = readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 14);
  process.stdout.write(`${header.map(uncomment).join("\n")}\n`);
  return 0;
}

const selfTestMode = async () => ((await selfTest()) ? 0 : 1);
const MODES = [["help", showHelp], ["self-test", selfTestMode], ["scene", sceneMode], ["package", packageMode]];

const modeIn = (values) => ([flag]) => values[flag];

function main() {
  const { values } = parseArgs({ options: OPTIONS, strict: true });
  const mode = MODES.find(modeIn(values)) ?? fail("pass --scene, --package or --self-test (see --help)", UsageError);
  return mode[1](values);
}

function die(e) {
  process.stderr.write(`render_frames: ${e.message}\n`);
  process.exit(e instanceof UsageError || String(e.code).startsWith("ERR_PARSE_ARGS") ? 2 : 1);
}

Promise.resolve().then(main).then((code) => process.exit(code), die);
