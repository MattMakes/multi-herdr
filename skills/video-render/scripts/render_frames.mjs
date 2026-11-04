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
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, extname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";

const DEFAULT_CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const CDP_TIMEOUT_MS = 60_000;
const SKILL_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SKILL_MD_MAX_BYTES = 12288;

export const ASPECTS = {
  "16x9": { width: 1920, height: 1080 },
  "9x16": { width: 1080, height: 1920 },
  "1x1": { width: 1080, height: 1080 },
};

class UsageError extends Error {}

// ---------------------------------------------------------------- subprocesses

function run(cmd, args, { input } = {}) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(cmd, args, { stdio: ["pipe", "pipe", "pipe"] });
    const out = [];
    const err = [];
    child.stdout.on("data", (d) => out.push(d));
    child.stderr.on("data", (d) => err.push(d));
    child.on("error", (e) => reject(new Error(`${cmd}: ${e.message}`)));
    child.on("close", (code) => {
      const stdout = Buffer.concat(out).toString();
      const stderr = Buffer.concat(err).toString();
      if (code === 0) resolvePromise(stdout);
      else reject(new Error(`${cmd} exited ${code}: ${stderr.trim().split("\n").slice(-8).join("\n")}`));
    });
    child.stdin.end(input);
  });
}

const ffmpeg = (args) => run("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", ...args]);

async function probe(file) {
  const json = await run("ffprobe", ["-v", "error", "-show_streams", "-show_format", "-of", "json", file]);
  const data = JSON.parse(json);
  const video = data.streams.find((s) => s.codec_type === "video");
  if (!video) throw new Error(`${file}: no video stream`);
  const [num, den] = video.r_frame_rate.split("/").map(Number);
  return {
    video,
    audio: data.streams.find((s) => s.codec_type === "audio") ?? null,
    width: video.width,
    height: video.height,
    fps: num / den,
    duration: Number(data.format.duration),
  };
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

export async function encodeFrames(framesDir, fps, outFile) {
  await ffmpeg([
    "-framerate", String(fps), "-start_number", "0", "-i", join(framesDir, "frame_%06d.png"),
    ...h264Args(fps), outFile,
  ]);
}

// ---------------------------------------------------------------- CDP client

class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.nextId = 0;
    this.pending = new Map();
    this.waiters = [];
    ws.addEventListener("message", (event) => this.onMessage(JSON.parse(event.data)));
    ws.addEventListener("close", () => {
      for (const p of this.pending.values()) p.reject(new Error(`CDP socket closed during ${p.method}`));
      this.pending.clear();
    });
  }

  onMessage(msg) {
    if (msg.id !== undefined) {
      const p = this.pending.get(msg.id);
      if (!p) return;
      this.pending.delete(msg.id);
      clearTimeout(p.timer);
      if (msg.error) p.reject(new Error(`${p.method}: ${msg.error.message}`));
      else p.resolve(msg.result);
      return;
    }
    for (const w of [...this.waiters]) {
      if (w.method === msg.method && w.sessionId === msg.sessionId) {
        this.waiters.splice(this.waiters.indexOf(w), 1);
        clearTimeout(w.timer);
        w.resolve(msg.params);
      }
    }
  }

  send(method, params = {}, sessionId) {
    const id = ++this.nextId;
    return new Promise((resolvePromise, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`${method}: no reply in ${CDP_TIMEOUT_MS / 1000} s`));
      }, CDP_TIMEOUT_MS);
      this.pending.set(id, { resolve: resolvePromise, reject, method, timer });
      this.ws.send(JSON.stringify({ id, method, params, sessionId }));
    });
  }

  once(method, sessionId) {
    return new Promise((resolvePromise, reject) => {
      const waiter = { method, sessionId, resolve: resolvePromise };
      waiter.timer = setTimeout(() => {
        this.waiters.splice(this.waiters.indexOf(waiter), 1);
        reject(new Error(`${method}: event not seen in ${CDP_TIMEOUT_MS / 1000} s`));
      }, CDP_TIMEOUT_MS);
      this.waiters.push(waiter);
    });
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

async function launchChrome(width, height) {
  const chromePath = process.env.CHROME_PATH || DEFAULT_CHROME;
  if (!existsSync(chromePath)) throw new Error(`Chrome not found at ${chromePath}. Set CHROME_PATH.`);
  const userDataDir = mkdtempSync(join(tmpdir(), "video-render-chrome-"));
  const proc = spawn(chromePath, chromeArgs(userDataDir, width, height), { stdio: ["ignore", "ignore", "pipe"] });
  const wsUrl = await new Promise((resolvePromise, reject) => {
    let log = "";
    const timer = setTimeout(() => reject(new Error("Chrome did not print a DevTools URL in 30 s")), 30_000);
    proc.stderr.on("data", (d) => {
      log = (log + d).slice(-4000);
      const m = log.match(/DevTools listening on (ws:\/\/\S+)/);
      if (m) {
        clearTimeout(timer);
        resolvePromise(m[1]);
      }
    });
    proc.on("error", (e) => reject(new Error(`Chrome failed to start: ${e.message}`)));
    proc.on("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`Chrome exited with code ${code}: ${log.trim().slice(-500)}`));
    });
  });
  const ws = new WebSocket(wsUrl);
  await new Promise((resolvePromise, reject) => {
    ws.addEventListener("open", resolvePromise, { once: true });
    ws.addEventListener("error", () => reject(new Error(`cannot connect to ${wsUrl}`)), { once: true });
  });
  const close = async () => {
    ws.close();
    if (proc.exitCode === null) {
      const exited = new Promise((r) => proc.once("exit", r));
      proc.kill("SIGTERM");
      await Promise.race([exited, new Promise((r) => setTimeout(r, 5000))]);
      if (proc.exitCode === null) proc.kill("SIGKILL");
    }
    rmSync(userDataDir, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 });
  };
  return { cdp: new Cdp(ws), close };
}

async function evaluate(cdp, sessionId, expression) {
  const res = await cdp.send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true }, sessionId);
  if (res.exceptionDetails) {
    const d = res.exceptionDetails;
    throw new Error(`page script failed: ${d.exception?.description ?? d.text}`);
  }
  return res.result.value;
}

// ---------------------------------------------------------------- scene render mode

export function sceneUrl(target) {
  if (/^[a-z][a-z0-9+.-]*:/i.test(target)) return target;
  const q = target.indexOf("?");
  const path = q < 0 ? target : target.slice(0, q);
  return pathToFileURL(resolve(path)).href + (q < 0 ? "" : target.slice(q));
}

// "--frames 30" is a count (frames 0..29). "--frames 0,15,30" is a list. Write a single
// frame index as "30," so it is not read as a count.
export function frameList({ frames, duration, fps, metaDuration, reverse }) {
  let list;
  if (frames !== undefined) {
    if (frames.includes(",")) {
      list = frames.split(",").filter((s) => s.trim() !== "").map((s) => parseNonNegInt(s, "--frames"));
    } else {
      const count = parseNonNegInt(frames, "--frames");
      list = Array.from({ length: count }, (_, i) => i);
    }
  } else {
    const sec = duration ?? metaDuration;
    if (sec === undefined || sec === null) {
      throw new UsageError("pass --duration or --frames, or set window.__videoMeta.duration in the scene");
    }
    list = Array.from({ length: Math.round(sec * fps) }, (_, i) => i);
  }
  if (list.length === 0) throw new UsageError("no frames to render");
  list = [...new Set(list)].sort((a, b) => a - b);
  return reverse ? list.reverse() : list;
}

function parseNonNegInt(text, flag) {
  const n = Number(text.trim());
  if (!Number.isInteger(n) || n < 0) throw new UsageError(`${flag}: "${text}" is not a non-negative integer`);
  return n;
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

export async function renderScene(opts) {
  const { scene, out, fps, width, height, frames, duration, reverse, transparent } = opts;
  if (width % 2 || height % 2) throw new UsageError("--width and --height must be even for yuv420p");
  const toMp4 = extname(out).toLowerCase() === ".mp4";
  if (toMp4 && transparent) throw new UsageError("--transparent keeps alpha only in PNG frames; write a frames directory");
  const framesDir = toMp4 ? mkdtempSync(join(tmpdir(), "video-render-frames-")) : resolve(out);
  mkdirSync(framesDir, { recursive: true });
  const browser = await launchChrome(width, height);
  try {
    const { cdp } = browser;
    // Use the tab Chrome opened at launch. A tab from Target.createTarget can stay in the
    // background, where it stops producing frames and Page.captureScreenshot never returns.
    const { targetInfos } = await cdp.send("Target.getTargets");
    const page = targetInfos.find((t) => t.type === "page");
    if (!page) throw new Error("Chrome opened no page target");
    const targetId = page.targetId;
    const { sessionId } = await cdp.send("Target.attachToTarget", { targetId, flatten: true });
    await cdp.send("Page.enable", {}, sessionId);
    await cdp.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 1, mobile: false }, sessionId);
    if (transparent) {
      await cdp.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } }, sessionId);
    }
    const loaded = cdp.once("Page.loadEventFired", sessionId);
    const nav = await cdp.send("Page.navigate", { url: sceneUrl(scene) }, sessionId);
    if (nav.errorText) throw new Error(`navigation to ${scene} failed: ${nav.errorText}`);
    await loaded;
    const ready = await evaluate(cdp, sessionId, READY_SCRIPT);
    if (ready.seek !== "function") {
      throw new Error(`${scene}: typeof window.__seekToFrame is "${ready.seek}", expected "function"`);
    }
    const meta = ready.meta ?? {};
    for (const [key, value] of Object.entries({ width, height, fps })) {
      if (meta[key] !== undefined && meta[key] !== value) {
        process.stderr.write(`warning: window.__videoMeta.${key} is ${meta[key]}, render uses ${value}\n`);
      }
    }
    const order = frameList({ frames, duration, fps, metaDuration: meta.duration, reverse });
    if (toMp4 && order.some((f) => f >= order.length)) {
      throw new UsageError("an .mp4 --out needs contiguous frames 0..N-1; write a frames directory instead");
    }
    for (const f of order) {
      await evaluate(cdp, sessionId, frameScript(f, fps));
      const shot = await cdp.send("Page.captureScreenshot", {
        format: "png", fromSurface: true, captureBeyondViewport: false,
        clip: { x: 0, y: 0, width, height, scale: 1 },
      }, sessionId);
      writeFileSync(join(framesDir, frameName(f)), Buffer.from(shot.data, "base64"));
    }
    if (toMp4) {
      mkdirSync(dirname(resolve(out)), { recursive: true });
      await encodeFrames(framesDir, fps, resolve(out));
    }
    return { frames: order, out: resolve(out) };
  } finally {
    await browser.close();
    if (toMp4) rmSync(framesDir, { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------- timeline helpers

export function loadTimeline(file) {
  const t = JSON.parse(readFileSync(file, "utf8"));
  if (!t.meta || !Array.isArray(t.scenes) || t.scenes.length === 0) {
    throw new Error(`${file}: needs "meta" and a non-empty "scenes" array`);
  }
  for (const s of t.scenes) {
    if (typeof s.id !== "string" || !s.id) throw new Error(`${file}: a scene has no "id"`);
    if (typeof s.start_sec !== "number" || typeof s.duration_sec !== "number") {
      throw new Error(`${file}: scene ${s.id} needs numeric "start_sec" and "duration_sec"`);
    }
  }
  return t;
}

const sceneText = (s) => {
  const n = typeof s.narration === "string" ? s.narration : s.narration?.text;
  return (n || s.caption || "").trim().replace(/\s+/g, " ");
};

// Greedy wrap into lines of at most maxChars. A word longer than maxChars gets its own line.
export function wrapLines(words, maxChars) {
  const lines = [];
  let line = "";
  for (const w of words) {
    if (line && line.length + 1 + w.length > maxChars) {
      lines.push(line);
      line = w;
    } else {
      line = line ? `${line} ${w}` : w;
    }
  }
  if (line) lines.push(line);
  return lines;
}

// One or more cues per scene: narration (or caption) text, at most 2 lines of 42 chars
// per cue. A scene's time span is shared between its cues in proportion to word count.
export function captionCues(timeline, { maxChars = 42, maxLines = 2 } = {}) {
  const cues = [];
  for (const s of timeline.scenes) {
    const text = sceneText(s);
    if (!text) continue;
    const lines = wrapLines(text.split(" "), maxChars);
    const chunks = [];
    for (let i = 0; i < lines.length; i += maxLines) chunks.push(lines.slice(i, i + maxLines));
    const total = text.split(" ").length;
    let done = 0;
    for (const chunk of chunks) {
      const n = chunk.join(" ").split(" ").length;
      const start = s.start_sec + (s.duration_sec * done) / total;
      done += n;
      const end = s.start_sec + (s.duration_sec * done) / total;
      cues.push({ start, end, lines: chunk });
    }
  }
  return cues;
}

function clock(sec, sep) {
  const ms = Math.round(sec * 1000);
  const h = Math.floor(ms / 3_600_000);
  const m = Math.floor((ms % 3_600_000) / 60_000);
  const s = Math.floor((ms % 60_000) / 1000);
  const p = (v, w = 2) => String(v).padStart(w, "0");
  return `${p(h)}:${p(m)}:${p(s)}${sep}${p(ms % 1000, 3)}`;
}

export function toSrt(cues) {
  return cues.map((c, i) => `${i + 1}\n${clock(c.start, ",")} --> ${clock(c.end, ",")}\n${c.lines.join("\n")}\n`).join("\n");
}

export function toVtt(cues) {
  return `WEBVTT\n\n${cues.map((c) => `${clock(c.start, ".")} --> ${clock(c.end, ".")}\n${c.lines.join("\n")}\n`).join("\n")}`;
}

// A chapter starts at scene 0 and at every scene with a "title". When no scene has a
// title, every scene starts a chapter. Times are floored to whole seconds.
export function chapters(timeline) {
  const titled = timeline.scenes.filter((s, i) => i > 0 && typeof s.title === "string" && s.title.trim());
  const picked = titled.length ? [timeline.scenes[0], ...titled] : timeline.scenes;
  const total = Number(timeline.meta.duration_sec) || 0;
  const list = picked.map((s, i) => ({
    sec: i === 0 ? 0 : Math.floor(s.start_sec),
    title: (s.title || (i === 0 && timeline.meta.title) || s.caption || s.id).trim(),
  }));
  const warnings = [];
  if (list.length < 3) warnings.push(`chapters: ${list.length} chapters; YouTube shows chapters only when there are 3 or more`);
  list.forEach((c, i) => {
    const next = i + 1 < list.length ? list[i + 1].sec : total;
    if (next - c.sec < 10) warnings.push(`chapters: "${c.title}" is ${next - c.sec} s; YouTube needs 10 s or more`);
  });
  const long = total >= 3600;
  const stamp = (sec) => {
    const h = Math.floor(sec / 3600);
    const m = Math.floor((sec % 3600) / 60);
    const s = String(sec % 60).padStart(2, "0");
    return long ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
  };
  return { text: list.map((c) => `${stamp(c.sec)} ${c.title}\n`).join(""), warnings };
}

// ---------------------------------------------------------------- reframing

const even = (v) => Math.max(2, Math.floor(v / 2) * 2);
const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);

// The largest crop of the target aspect that fits the source, centred on the focal point
// and then shifted so the union of critical regions stays inside. When the union cannot
// fit in the crop, the result is "fit": the whole frame is scaled and padded.
export function cropWindow(srcW, srcH, outW, outH, focal, regions = []) {
  const ratio = outW / outH;
  const w = srcW / srcH > ratio ? even(srcH * ratio) : even(srcW);
  const h = srcW / srcH > ratio ? even(srcH) : even(srcW / ratio);
  const [fx, fy] = Array.isArray(focal) ? focal : [0.5, 0.5];
  let x = fx * srcW - w / 2;
  let y = fy * srcH - h / 2;
  if (regions.length) {
    const x0 = Math.min(...regions.map((r) => r[0] * srcW));
    const y0 = Math.min(...regions.map((r) => r[1] * srcH));
    const x1 = Math.max(...regions.map((r) => (r[0] + r[2]) * srcW));
    const y1 = Math.max(...regions.map((r) => (r[1] + r[3]) * srcH));
    if (x1 - x0 > w || y1 - y0 > h) return { mode: "fit" };
    x = clamp(x, x1 - w, x0);
    y = clamp(y, y1 - h, y0);
  }
  x = Math.floor(clamp(x, 0, srcW - w) / 2) * 2;
  y = Math.floor(clamp(y, 0, srcH - h) / 2) * 2;
  return { mode: "crop", x, y, w, h };
}

const fitFilter = (W, H) =>
  `scale=${W}:${H}:force_original_aspect_ratio=decrease:flags=lanczos,pad=${W}:${H}:(ow-iw)/2:(oh-ih)/2:color=black`;

async function exportAspect({ timeline, master, info, aspect, outFile, sceneClips }) {
  const { width: W, height: H } = ASPECTS[aspect];
  const fps = info.fps;
  const inputs = ["-i", master];
  const chains = [];
  const modes = [];
  const fromMaster = [];
  timeline.scenes.forEach((s, i) => {
    const a = Math.round(s.start_sec * fps);
    const b = Math.round((s.start_sec + s.duration_sec) * fps);
    const clip = sceneClips ? join(sceneClips, `${s.id}.${aspect}.mp4`) : null;
    if (clip && existsSync(clip)) {
      const k = inputs.length / 2;
      inputs.push("-i", clip);
      chains.push(`[${k}:v]fps=${fps},${fitFilter(W, H)},setsar=1,tpad=stop_mode=clone:stop=-1,trim=end_frame=${b - a},setpts=PTS-STARTPTS[v${i}]`);
      modes.push({ scene: s.id, mode: "clip", file: clip });
      return;
    }
    const win = cropWindow(info.width, info.height, W, H, s.focal_point, s.critical_regions);
    const frame = win.mode === "crop" ? `crop=${win.w}:${win.h}:${win.x}:${win.y},scale=${W}:${H}:flags=lanczos` : fitFilter(W, H);
    fromMaster.push(i);
    chains.push(`[m${i}]trim=start_frame=${a}:end_frame=${b},setpts=PTS-STARTPTS,${frame},setsar=1[v${i}]`);
    modes.push({ scene: s.id, ...win });
  });
  const split = fromMaster.length
    ? [`[0:v]split=${fromMaster.length}${fromMaster.map((i) => `[m${i}]`).join("")}`]
    : [];
  const concat = `${timeline.scenes.map((_, i) => `[v${i}]`).join("")}concat=n=${timeline.scenes.length}:v=1:a=0[v]`;
  const graph = [...split, ...chains, concat].join(";");
  await ffmpeg([
    ...inputs, "-filter_complex", graph, "-map", "[v]",
    ...(info.audio ? ["-map", "0:a:0", ...AAC_ARGS] : []),
    ...h264Args(fps), "-shortest", outFile,
  ]);
  return modes;
}

// ---------------------------------------------------------------- package mode

export async function packagePublish({ timelineFile, master, outDir, aspects, coverSec, sceneClips }) {
  const timeline = loadTimeline(timelineFile);
  const info = await probe(master);
  mkdirSync(outDir, { recursive: true });
  const report = { outputs: [], warnings: [], reframe: {} };
  const write = (name, text) => {
    writeFileSync(join(outDir, name), text);
    report.outputs.push(join(outDir, name));
  };

  const cues = captionCues(timeline);
  write("captions.srt", toSrt(cues));
  write("captions.vtt", toVtt(cues));
  const ch = chapters(timeline);
  write("chapters.txt", ch.text);
  report.warnings.push(...ch.warnings);

  const hook = timeline.scenes.find((s) => s.kind === "hook") ?? timeline.scenes[0];
  const cover = coverSec ?? hook.start_sec + hook.duration_sec * 0.75;
  await ffmpeg(["-ss", String(cover), "-i", master, "-frames:v", "1", "-vf", fitFilter(1920, 1080), "-q:v", "2", join(outDir, "cover.jpg")]);
  report.outputs.push(join(outDir, "cover.jpg"));

  const previewLen = Math.min(5, info.duration - hook.start_sec);
  if (previewLen < 5) report.warnings.push(`preview: master holds ${previewLen.toFixed(2)} s after the hook start, not 5 s`);
  const clipArgs = ["-ss", String(hook.start_sec), "-t", String(previewLen), "-i", master, "-an"];
  await ffmpeg([...clipArgs, "-vf", "scale=1280:-2:flags=lanczos", "-c:v", "libx264", "-preset", "slow", "-crf", "20",
    "-pix_fmt", "yuv420p", "-movflags", "+faststart", join(outDir, "preview.mp4")]);
  await ffmpeg([...clipArgs, "-filter_complex",
    "fps=15,scale=480:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=5",
    "-loop", "0", join(outDir, "preview.gif")]);
  report.outputs.push(join(outDir, "preview.mp4"), join(outDir, "preview.gif"));

  for (const aspect of aspects) {
    if (!ASPECTS[aspect]) throw new UsageError(`--aspects: unknown aspect "${aspect}"; use 16x9, 9x16 or 1x1`);
    const outFile = join(outDir, `${aspect}.mp4`);
    const modes = await exportAspect({ timeline, master, info, aspect, outFile, sceneClips });
    report.reframe[aspect] = modes;
    for (const m of modes) {
      if (m.mode === "fit") {
        report.warnings.push(`${aspect}: scene ${m.scene} critical_regions do not fit a crop; it is letterboxed. Re-render it with ?aspect=${aspect}.`);
      }
    }
    report.outputs.push(outFile);
  }
  return report;
}

// ---------------------------------------------------------------- self-test

function assertEq(actual, expected, what) {
  const a = JSON.stringify(actual);
  const e = JSON.stringify(expected);
  if (a !== e) throw new Error(`${what}: expected ${e}, got ${a}`);
}

function assert(cond, what) {
  if (!cond) throw new Error(what);
}

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

async function selfTest() {
  const root = mkdtempSync(join(tmpdir(), "video-render-selftest-"));
  const results = [];
  const test = async (name, fn) => {
    try {
      const note = await fn();
      results.push(true);
      console.log(`ok - ${name}${note ? ` (${note})` : ""}`);
    } catch (e) {
      results.push(false);
      console.log(`not ok - ${name}: ${e.message}`);
    }
  };
  const timelineFile = join(root, "timeline.json");
  writeFileSync(timelineFile, JSON.stringify(FIXTURE));
  const tl = loadTimeline(timelineFile);

  await test("SKILL.md is under 12288 bytes with name: video-render", () => {
    const md = readFileSync(join(SKILL_DIR, "SKILL.md"));
    assert(md.length < SKILL_MD_MAX_BYTES, `SKILL.md is ${md.length} bytes`);
    assert(/^---\nname: video-render\ndescription: .+\n---\n/.test(md.toString()), "frontmatter needs name: video-render and description:");
    return `${md.length} bytes`;
  });

  await test("skill folder holds only .md and .mjs text files", () => {
    const walk = (d) => readdirSync(d).flatMap((n) => (statSync(join(d, n)).isDirectory() ? walk(join(d, n)) : [join(d, n)]));
    const bad = walk(SKILL_DIR).filter((f) => ![".md", ".mjs"].includes(extname(f)));
    assertEq(bad, [], "non-text files");
  });

  await test("captions.srt is deterministic and wraps at 42 chars, 2 lines", () => {
    const srt = toSrt(captionCues(tl));
    assertEq(srt, EXPECTED_SRT, "srt");
    assertEq(toSrt(captionCues(tl)), srt, "second run");
  });

  await test("captions.vtt matches the srt cues", () => {
    const vtt = toVtt(captionCues(tl));
    assert(vtt.startsWith("WEBVTT\n\n00:00:00.000 --> 00:00:01.000\n"), "vtt header and first cue");
    assertEq(vtt.split("-->").length - 1, 3, "vtt cue count");
    assert(vtt.endsWith("00:00:02.000 --> 00:00:03.000\nInstall it today.\n"), "vtt last cue");
  });

  await test("chapters.txt uses scene titles and start_sec", () => {
    const ch = chapters(tl);
    assertEq(ch.text, "0:00 Demo Launch\n0:01 The demo\n0:02 Get started\n", "chapters");
    assertEq(ch.warnings.length, 3, "YouTube 10 s warnings");
  });

  await test("crop window follows focal_point and keeps critical_regions", () => {
    assertEq(cropWindow(1920, 1080, 1080, 1920, [0.5, 0.45], [[0.3, 0.3, 0.4, 0.3]]), { mode: "fit" }, "too-wide region");
    assertEq(cropWindow(1920, 1080, 1080, 1920, [0.9, 0.5], [[0.8, 0.2, 0.15, 0.4]]), { mode: "crop", x: 1314, y: 0, w: 606, h: 1080 }, "right focal");
    assertEq(cropWindow(1920, 1080, 1080, 1920, [0.2, 0.5], [[0.4, 0.2, 0.1, 0.1]]), { mode: "crop", x: 354, y: 0, w: 606, h: 1080 }, "shift to region");
    assertEq(cropWindow(1920, 1080, 1080, 1080, [0.5, 0.5]), { mode: "crop", x: 420, y: 0, w: 1080, h: 1080 }, "square centre");
    assertEq(cropWindow(1920, 1080, 1920, 1080, [0.1, 0.1]), { mode: "crop", x: 0, y: 0, w: 1920, h: 1080 }, "same aspect");
  });

  await test("--frames parses counts, lists and --reverse", () => {
    assertEq(frameList({ frames: "4", fps: 60 }), [0, 1, 2, 3], "count");
    assertEq(frameList({ frames: "30,0,15", fps: 60 }), [0, 15, 30], "list");
    assertEq(frameList({ frames: "0,15,30", fps: 60, reverse: true }), [30, 15, 0], "reverse");
    assertEq(frameList({ duration: 0.1, fps: 60 }), [0, 1, 2, 3, 4, 5], "duration");
    assertEq(frameList({ frames: "7,", fps: 60 }), [7], "single index");
  });

  await test("ffmpeg encodes PNG frames to CFR H.264 High yuv420p", async () => {
    const dir = join(root, "frames");
    mkdirSync(dir);
    await ffmpeg(["-f", "lavfi", "-i", "testsrc2=size=320x180:rate=30", "-frames:v", "12", "-start_number", "0", join(dir, "frame_%06d.png")]);
    const out = join(root, "cfr.mp4");
    await encodeFrames(dir, 30, out);
    const p = await probe(out);
    assertEq([p.video.codec_name, p.video.pix_fmt, p.video.profile], ["h264", "yuv420p", "High"], "codec");
    assertEq([p.video.r_frame_rate, p.video.avg_frame_rate, p.video.nb_frames], ["30/1", "30/1", "12"], "cfr");
  });

  await test("--package writes captions, chapters, cover, previews and 3 aspects", async () => {
    const master = join(root, "master.mp4");
    await ffmpeg(["-f", "lavfi", "-i", "testsrc2=size=1920x1080:rate=30:duration=3", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000:duration=3",
      "-ac", "2", "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", ...AAC_ARGS, "-shortest", master]);
    const outDir = join(root, "render");
    const report = await packagePublish({ timelineFile, master, outDir, aspects: Object.keys(ASPECTS) });
    assertEq(readFileSync(join(outDir, "captions.srt"), "utf8"), EXPECTED_SRT, "packaged srt");
    for (const [aspect, size] of Object.entries(ASPECTS)) {
      const p = await probe(join(outDir, `${aspect}.mp4`));
      assertEq([p.width, p.height, p.video.r_frame_rate, p.video.nb_frames, p.video.profile], [size.width, size.height, "30/1", "90", "High"], aspect);
      assertEq([p.audio?.codec_name, p.audio?.sample_rate, p.audio?.channels], ["aac", "48000", 2], `${aspect} audio`);
    }
    assertEq(report.reframe["9x16"].map((m) => m.mode), ["fit", "crop", "fit"], "9x16 modes");
    const cover = await probe(join(outDir, "cover.jpg"));
    assertEq([cover.width, cover.height], [1920, 1080], "cover size");
    const prev = await probe(join(outDir, "preview.mp4"));
    assert(prev.audio === null && prev.width === 1280, "preview.mp4 is muted and 1280 px wide");
    assert(statSync(join(outDir, "preview.gif")).size > 0, "preview.gif written");
  });

  const chrome = process.env.CHROME_PATH || DEFAULT_CHROME;
  if (!existsSync(chrome)) {
    console.log(`ok - headless Chrome render # SKIP no Chrome at ${chrome}`);
  } else {
    const scene = join(root, "scene", "index.html");
    mkdirSync(dirname(scene));
    writeFileSync(scene, SCENE_HTML);
    const base = { scene, fps: 10, width: 64, height: 48 };
    await test("Chrome cold-forward and warm-reverse frames are byte-identical", async () => {
      const fwd = join(root, "fwd");
      const rev = join(root, "rev");
      await renderScene({ ...base, out: fwd, frames: "0,2,4" });
      const r = await renderScene({ ...base, out: rev, frames: "0,2,4", reverse: true });
      assertEq(r.frames, [4, 2, 0], "reverse order");
      for (const f of [0, 2, 4]) {
        assert(readFileSync(join(fwd, frameName(f))).equals(readFileSync(join(rev, frameName(f)))), `frame ${f} differs`);
      }
      assert(!readFileSync(join(fwd, frameName(0))).equals(readFileSync(join(fwd, frameName(4)))), "frames 0 and 4 must differ");
    });
    await test("Chrome render to .mp4 uses __videoMeta.duration", async () => {
      const out = join(root, "scene.mp4");
      await renderScene({ ...base, out });
      const p = await probe(out);
      assertEq([p.width, p.height, p.video.r_frame_rate, p.video.nb_frames], [64, 48, "10/1", "5"], "scene mp4");
    });
    await test("Chrome --transparent writes RGBA frames with a clear background", async () => {
      const overlay = join(root, "scene", "caption.html");
      writeFileSync(overlay, '<!doctype html><p style="margin:0;color:#fff">Hi</p><script>window.__seekToFrame=()=>{};</script>');
      const dir = join(root, "alpha");
      await renderScene({ ...base, scene: overlay, out: dir, frames: "1", transparent: true });
      const png = join(dir, frameName(0));
      assertEq((await probe(png)).video.pix_fmt, "rgba", "pix_fmt");
      const corner = await new Promise((res, rej) => {
        const c = spawn("ffmpeg", ["-v", "error", "-i", png, "-vf", "crop=1:1:63:47,format=rgba", "-f", "rawvideo", "-"]);
        const out = [];
        c.stdout.on("data", (d) => out.push(d));
        c.on("close", (code) => (code === 0 ? res(Buffer.concat(out)) : rej(new Error(`ffmpeg exited ${code}`))));
      });
      assertEq(corner[3], 0, "corner alpha");
    });
    await test("Chrome render fails without window.__seekToFrame", async () => {
      const bad = join(root, "scene", "bad.html");
      writeFileSync(bad, "<!doctype html><p>no clock</p>");
      let msg = "";
      await renderScene({ ...base, scene: bad, out: join(root, "bad"), frames: "1" }).catch((e) => { msg = e.message; });
      assert(msg.includes('typeof window.__seekToFrame is "undefined"'), `error was: ${msg}`);
    });
  }

  rmSync(root, { recursive: true, force: true });
  const failed = results.filter((r) => !r).length;
  console.log(`${results.length - failed} of ${results.length} tests passed`);
  return failed === 0;
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
  if (value === undefined) return fallback;
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) throw new UsageError(`${flag}: "${value}" is not a positive number`);
  return n;
}

async function main() {
  const { values: v } = parseArgs({ options: OPTIONS, strict: true });
  if (v.help) {
    process.stdout.write(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 13).map((l) => l.replace(/^\/\/ ?/, "")).join("\n") + "\n");
    return 0;
  }
  if (v["self-test"]) return (await selfTest()) ? 0 : 1;
  if (v.scene) {
    if (!v.out) throw new UsageError("--scene needs --out");
    const fps = positive(v.fps, "--fps", 60);
    if (!Number.isInteger(fps)) throw new UsageError("--fps must be an integer");
    const r = await renderScene({
      scene: v.scene, out: v.out, fps,
      width: positive(v.width, "--width", 1920), height: positive(v.height, "--height", 1080),
      duration: v.duration === undefined ? undefined : positive(v.duration, "--duration"),
      frames: v.frames, reverse: Boolean(v.reverse), transparent: Boolean(v.transparent),
    });
    console.log(JSON.stringify({ out: r.out, frame_count: r.frames.length, first: r.frames[0], last: r.frames.at(-1) }));
    return 0;
  }
  if (v.package) {
    if (!v.master || !v["out-dir"]) throw new UsageError("--package needs --master and --out-dir");
    const report = await packagePublish({
      timelineFile: v.package, master: v.master, outDir: resolve(v["out-dir"]),
      aspects: (v.aspects ?? "16x9,9x16,1x1").split(",").map((s) => s.trim()).filter(Boolean),
      coverSec: v["cover-sec"] === undefined ? undefined : Number(v["cover-sec"]),
      sceneClips: v["scene-clips"] ? resolve(v["scene-clips"]) : undefined,
    });
    console.log(JSON.stringify(report, null, 2));
    for (const w of report.warnings) process.stderr.write(`warning: ${w}\n`);
    return 0;
  }
  throw new UsageError("pass --scene, --package or --self-test (see --help)");
}

main().then((code) => process.exit(code), (e) => {
  process.stderr.write(`render_frames: ${e.message}\n`);
  process.exit(e instanceof UsageError || e.code?.startsWith?.("ERR_PARSE_ARGS") ? 2 : 1);
});
