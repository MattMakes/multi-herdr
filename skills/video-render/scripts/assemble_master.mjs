#!/usr/bin/env node
// Assembles render/master.mp4 from timeline.json: scene clips, beat-checked transitions,
// Chrome-rendered transparent text layers and audio/mix.wav. Node built-ins only; ffmpeg and
// ffprobe run as subprocesses. Every input path is relative to the folder of timeline.json.
//
//   assemble_master.mjs --master <timeline.json> --out <render/master.mp4>
//   assemble_master.mjs --self-test
//
// Exit codes: 0 success, 1 missing input or assembly failure, 2 usage error.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { AAC_ARGS, UsageError, check, fail, ffmpeg, h264Args, loadTimeline, probe, run } from "./render_frames.mjs";

const SCRIPT = fileURLToPath(import.meta.url);
const SAMPLE_RATE = 48000;
const FADE_DEFAULT_SEC = 0.3;
const FADE_MIN_SEC = 0.25;
const FADE_MAX_SEC = 0.4;
const FIRST_FRAME = "frame_000000.png";

// Timeline transition names to ffmpeg xfade types. The storyboard names map to the
// nearest xfade type.
export const XFADE_TYPES = {
  fade: "fade", wipeleft: "wipeleft", smoothleft: "smoothleft", circleopen: "circleopen",
  crossfade: "fade", wipe: "wipeleft", "push-left": "smoothleft",
};

// Scene clip lookup order: the rendered HTML scene first, then the capture plate.
export const clipCandidates = (id) => [`render/scenes/${id}.mp4`, `captures/${id}.mp4`];

const clamp = (v, lo, hi) => Math.min(Math.max(v, lo), hi);
const json = (v) => JSON.stringify(v);

// ---------------------------------------------------------------- plan

const transitionSpec = (t) => (t && typeof t === "object" ? t : { type: t ?? "cut" });

export function parseTransition(t, sceneId) {
  const spec = transitionSpec(t);
  const name = String(spec.type ?? "cut").trim();
  if (name === "cut") return { type: "cut", sec: 0 };
  const type = XFADE_TYPES[name] ?? fail(`scene ${sceneId}: transition_out "${name}" is not one of cut, ${Object.keys(XFADE_TYPES).join(", ")}`);
  return { type, sec: clamp(Number(spec.duration_sec ?? FADE_DEFAULT_SEC) || FADE_DEFAULT_SEC, FADE_MIN_SEC, FADE_MAX_SEC) };
}

function filmShape(meta) {
  const fps = meta.fps;
  check(Number.isInteger(fps) && fps > 0, `meta.fps must be a positive integer, got ${json(fps)}`);
  const width = meta.width ?? 1920;
  const height = meta.height ?? 1080;
  check(width % 2 === 0 && height % 2 === 0, `meta.width and meta.height must be even, got ${width}x${height}`);
  return { fps, width, height, frames: Math.round(meta.duration_sec * fps) };
}

const LAST_CUT = { type: "cut", sec: 0 };
const toFrame = (sec, fps) => Math.round(sec * fps);
const sceneEnd = (next, film) => (next ? toFrame(next.start_sec, film.fps) : film.frames);
const sceneTransition = (s, next) => (next ? parseTransition(s.transition_out, s.id) : LAST_CUT);
const noFrames = (s) => `scene ${s.id}: start_sec ${s.start_sec} leaves it no frames before the next scene or meta.duration_sec`;

// A scene clip spans its own frames plus a tail handle as long as its outgoing transition.
function scenePlan(s, next, film) {
  const start = toFrame(s.start_sec, film.fps);
  const length = sceneEnd(next, film) - start;
  check(length > 0, noFrames(s));
  const transition = sceneTransition(s, next);
  const handle = toFrame(transition.sec, film.fps);
  return { id: s.id, start, frames: length + handle, handle, transition };
}

const planWith = (film) => (s, i, all) => scenePlan(s, all[i + 1], film);

export function planMaster(timeline) {
  const film = filmShape(timeline.meta);
  const scenes = timeline.scenes.map(planWith(film));
  check(scenes[0].start === 0, `scene ${scenes[0].id}: start_sec must be 0`);
  return { ...film, scenes };
}

// ---------------------------------------------------------------- inputs

const beatGrid = (g) => [...(g.beats_sec ?? []), ...(g.downbeats_sec ?? [])];
const distanceTo = (t) => (b) => Math.abs(b - t);
const nearest = (grid, t) => Math.min(...grid.map(distanceTo(t)));
const ms = (sec) => (sec * 1000).toFixed(1);
const beatMiss = (grid, fps) => (s) => ({ s, err: nearest(grid, s.start_sec), limit: 1 / fps });
const isMiss = (m) => m.err > m.limit + 1e-9;
const missText = (m) => `scene ${m.s.id} start_sec ${m.s.start_sec}: ${ms(m.err)} ms off the beat grid (limit ${ms(m.limit)} ms)`;

export function beatErrors(timeline, beats, fps) {
  const grid = beatGrid(beats);
  check(grid.length > 0, "audio/beats.json: beats_sec and downbeats_sec are both empty");
  return timeline.scenes.slice(1).map(beatMiss(grid, fps)).filter(isMiss).map(missText);
}

function checkBeats(root, timeline, fps) {
  const file = join(root, "audio/beats.json");
  if (!existsSync(file)) return false;
  const errors = beatErrors(timeline, JSON.parse(readFileSync(file, "utf8")), fps);
  check(errors.length === 0, `cuts miss the beat grid; fix timeline.json, do not shift cuts here:\n  ${errors.join("\n  ")}`);
  return true;
}

const MIX = "audio/mix.wav";
const OVERLAYS = "render/overlays";
const existsIn = (root) => (rel) => existsSync(join(root, rel));
const clipIn = (root) => (s) => clipCandidates(s.id).find(existsIn(root)) ?? null;
const missingClip = (s) => `scene ${s.id}: none of ${clipCandidates(s.id).join(", ")}`;
const isDir = (d) => d.isDirectory();
const overlayPath = (d) => `${OVERLAYS}/${d.name}`;
const hasFirstFrame = (root) => (rel) => existsSync(join(root, rel, FIRST_FRAME));
const byName = (a, b) => (a.name < b.name ? -1 : 1);

function overlayDirs(root) {
  const base = join(root, OVERLAYS);
  const entries = existsSync(base) ? readdirSync(base, { withFileTypes: true }) : [];
  return entries.filter(isDir).sort(byName).map(overlayPath).filter(hasFirstFrame(root));
}

const withoutClip = (clips) => (_, i) => !clips[i];
const mixMissing = (root) => (existsSync(join(root, MIX)) ? [] : [`audio: ${MIX}`]);

function missingInputs(root, scenes, clips) {
  return [...scenes.filter(withoutClip(clips)).map(missingClip), ...mixMissing(root)];
}

export function findInputs(root, plan) {
  const clips = plan.scenes.map(clipIn(root));
  const missing = missingInputs(root, plan.scenes, clips);
  check(missing.length === 0, `missing input in ${root}:\n  ${missing.join("\n  ")}`);
  return { clips, overlays: overlayDirs(root), mix: MIX };
}

// ---------------------------------------------------------------- filter graph

const secs = (frames, fps) => (frames / fps).toFixed(6);

const normalizeChain = (s, i, p) =>
  `[${i}:v]fps=${p.fps},scale=${p.width}:${p.height}:force_original_aspect_ratio=decrease:flags=lanczos,` +
  `pad=${p.width}:${p.height}:(ow-iw)/2:(oh-ih)/2,setsar=1,format=yuv420p,` +
  `tpad=stop_mode=clone:stop=-1,trim=end_frame=${s.frames},setpts=PTS-STARTPTS,settb=1/${p.fps}[c${i}]`;

function joinChain(p, i) {
  const prev = p.scenes[i - 1];
  const left = i === 1 ? "[c0]" : `[j${i - 1}]`;
  const op = prev.transition.type === "cut"
    ? `concat=n=2:v=1:a=0,settb=1/${p.fps}`
    : `xfade=transition=${prev.transition.type}:duration=${secs(prev.handle, p.fps)}:offset=${secs(p.scenes[i].start, p.fps)}`;
  return `${left}[c${i}]${op}[j${i}]`;
}

const range = (from, to) => Array.from({ length: Math.max(0, to - from) }, (_, i) => from + i);
const joinedLabel = (n) => (n === 1 ? "[c0]" : `[j${n - 1}]`);
const overlayTop = (n, count) => (count === 0 ? joinedLabel(n) : `[o${count - 1}]`);
const overlayChain = (n) => (k) => `${overlayTop(n, k)}[${n + k}:v]overlay=0:0:format=auto:eof_action=pass[o${k}]`;
const normalizeWith = (p) => (s, i) => normalizeChain(s, i, p);
const joinWith = (p) => (i) => joinChain(p, i);
const finalChain = (top, frames) => `${top}format=yuv420p,trim=end_frame=${frames},setpts=PTS-STARTPTS[v]`;

function audioChain(plan, input) {
  const samples = Math.round((plan.frames / plan.fps) * SAMPLE_RATE);
  return `[${input}:a]aresample=${SAMPLE_RATE},apad,atrim=end_sample=${samples}[a]`;
}

// Inputs: scene clips 0..n-1, then the text layers, then audio/mix.wav.
export function filterGraph(plan, overlayCount) {
  const n = plan.scenes.length;
  const video = [
    ...plan.scenes.map(normalizeWith(plan)),
    ...range(1, n).map(joinWith(plan)),
    ...range(0, overlayCount).map(overlayChain(n)),
  ];
  const tail = [finalChain(overlayTop(n, overlayCount), plan.frames), audioChain(plan, n + overlayCount)];
  return [...video, ...tail].join(";\n");
}

const clipArg = (root) => (rel) => ["-i", join(root, rel)];
const frameArg = (root, fps) => (rel) => ["-framerate", String(fps), "-start_number", "0", "-i", join(root, rel, "frame_%06d.png")];
const MAPS = ["-map", "[v]", "-map", "[a]"];

function inputArgs(root, plan, inputs) {
  const clips = inputs.clips.flatMap(clipArg(root));
  const overlays = inputs.overlays.flatMap(frameArg(root, plan.fps));
  return [...clips, ...overlays, "-i", join(root, inputs.mix)];
}

function ffmpegArgs(root, plan, inputs, out) {
  const graph = filterGraph(plan, inputs.overlays.length);
  return [...inputArgs(root, plan, inputs), "-filter_complex", graph, ...MAPS, ...h264Args(plan.fps), ...AAC_ARGS, out];
}

const sceneReport = (inputs, fps) => (s, i) => ({ id: s.id, source: inputs.clips[i], frames: s.frames, transition_out: s.transition.type, transition_sec: s.handle / fps });
const masterTimelinePath = (root) => join(root, "timeline-master.json");

function writeMasterTimeline(root, timeline) {
  const file = masterTimelinePath(root);
  writeFileSync(file, `${JSON.stringify(timeline, null, 2)}\n`);
  return file;
}

export async function assembleMaster(timelineFile, out) {
  const root = dirname(resolve(timelineFile));
  const timeline = loadTimeline(timelineFile);
  const plan = planMaster(timeline);
  const inputs = findInputs(root, plan);
  const beatChecked = checkBeats(root, timeline, plan.fps);
  mkdirSync(dirname(resolve(out)), { recursive: true });
  await ffmpeg(ffmpegArgs(root, plan, inputs, resolve(out)));
  const master_timeline = writeMasterTimeline(root, timeline);
  const scenes = plan.scenes.map(sceneReport(inputs, plan.fps));
  return { out, master_timeline, frames: plan.frames, duration_sec: plan.frames / plan.fps, beat_checked: beatChecked, scenes, overlays: inputs.overlays };
}

// ---------------------------------------------------------------- self-test

const assertEq = (actual, expected, what) => check(json(actual) === json(expected), `${what}: expected ${json(expected)}, got ${json(actual)}`);

const W = 640;
const H = 360;
const FPS = 30;

const TIMELINE = {
  meta: { id: "selftest", title: "Self test", aspect: "16:9", fps: FPS, width: W, height: H, duration_sec: 3.0 },
  scenes: [
    { id: "s01-hook", kind: "hook", start_sec: 0.0, duration_sec: 1.0, transition_out: "crossfade" },
    { id: "s02-demo", kind: "capture", start_sec: 1.0, duration_sec: 1.0, transition_out: "cut" },
    { id: "s03-cta", kind: "cta", start_sec: 2.0, duration_sec: 1.0, transition_out: "cut" },
  ],
};

const BEATS = { bpm: 120, beats_sec: [0, 0.5, 1.0, 1.5, 2.0, 2.5], downbeats_sec: [0, 2.0] };

// Plate colours and sizes. s03 has both a capture (yellow) and a rendered scene (blue);
// the rendered scene must win. s01 is 4:3, so it is letterboxed by the normalize step.
const PLATES = [
  ["captures/s01-hook.mp4", "red", "320x240", 1.5],
  ["render/scenes/s02-demo.mp4", "lime", "640x360", 1.0],
  ["captures/s03-cta.mp4", "yellow", "640x360", 1.0],
  ["render/scenes/s03-cta.mp4", "blue", "1280x720", 1.0],
];

const TRANSITION_CASES = [
  ["cut", { type: "cut", sec: 0 }],
  ["fade", { type: "fade", sec: 0.3 }],
  ["crossfade", { type: "fade", sec: 0.3 }],
  ["wipe", { type: "wipeleft", sec: 0.3 }],
  ["push-left", { type: "smoothleft", sec: 0.3 }],
  [{ type: "circleopen", duration_sec: 0.9 }, { type: "circleopen", sec: 0.4 }],
  [{ type: "smoothleft", duration_sec: 0.1 }, { type: "smoothleft", sec: 0.25 }],
  [undefined, { type: "cut", sec: 0 }],
];

const transitionCase = ([input, want]) => assertEq(parseTransition(input, "s"), want, `transition ${json(input)}`);

const noError = () => "";
const messageOf = (e) => e.message;
const errorOf = (fn) => Promise.resolve().then(fn).then(noError, messageOf);
const parseSpin = () => parseTransition("spin", "s09");

async function testTransitions() {
  TRANSITION_CASES.forEach(transitionCase);
  const error = await errorOf(parseSpin);
  check(error.includes('scene s09: transition_out "spin"'), `unknown type error was: ${error}`);
}

const frameCounts = (s) => [s.start, s.frames, s.handle];

function testPlan() {
  const p = planMaster(TIMELINE);
  assertEq(p.frames, 90, "film frames");
  assertEq(p.scenes.map(frameCounts), [[0, 39, 9], [30, 30, 0], [60, 30, 0]], "scene frames and handles");
}

function testBeats() {
  assertEq(beatErrors(TIMELINE, BEATS, FPS), [], "on-grid cuts");
  const off = { bpm: 120, beats_sec: [0, 0.95, 2.0] };
  assertEq(beatErrors(TIMELINE, off, FPS), ["scene s02-demo start_sec 1: 50.0 ms off the beat grid (limit 33.3 ms)"], "off-grid cut");
}

const plateArgs = ([, colour, size, sec]) => ["-f", "lavfi", "-i", `color=c=${colour}:s=${size}:r=25:d=${sec}`];

async function writePlate(root, plate) {
  mkdirSync(dirname(join(root, plate[0])), { recursive: true });
  await ffmpeg([...plateArgs(plate), "-c:v", "libx264", "-preset", "ultrafast", "-pix_fmt", "yuv420p", join(root, plate[0])]);
}

const plateWriter = (root) => (plate) => writePlate(root, plate);
const writePlates = (root) => Promise.all(PLATES.map(plateWriter(root)));

// A transparent layer with an opaque white 64x36 box at the top-left corner.
async function writeOverlay(root) {
  const dir = join(root, "render/overlays/captions");
  mkdirSync(dir, { recursive: true });
  const box = `geq=r=255:g=255:b=255:a='if(lt(X,64)*lt(Y,36),255,0)'`;
  await ffmpeg(["-f", "lavfi", "-i", `color=c=black:s=${W}x${H}:r=${FPS}`, "-vf", `format=rgba,${box}`, "-frames:v", "90", "-start_number", "0", join(dir, "frame_%06d.png")]);
}

// mix.wav is 2.8 s at 44.1 kHz, so the master must resample it and pad it to 3.0 s.
async function writeAudio(root, beats) {
  mkdirSync(join(root, "audio"), { recursive: true });
  writeFileSync(join(root, "audio/beats.json"), json(beats));
  await ffmpeg(["-f", "lavfi", "-i", "sine=frequency=440:sample_rate=44100:duration=2.8", "-ac", "2", join(root, "audio/mix.wav")]);
}

async function writeProject(root) {
  writeFileSync(join(root, "timeline.json"), json(TIMELINE));
  await writeAudio(root, BEATS);
  await writePlates(root);
  await writeOverlay(root);
}

// Mean RGB of frame k, sampled from the centre half of the picture or from a w x h box at x,y.
async function meanColour(file, k, area = "crop=iw/2:ih/2") {
  const vf = `select=eq(n\\,${k}),${area},scale=1:1:flags=area,format=rgb24`;
  const raw = await run("ffmpeg", ["-v", "error", "-i", file, "-vf", vf, "-frames:v", "1", "-f", "rawvideo", "-"]);
  return [...Buffer.from(raw, "latin1")];
}

const near = (a, b, tol = 24) => a.every((v, i) => Math.abs(v - b[i]) <= tol);

const VIDEO_SPEC = ["h264", "High", 42, "yuv420p", W, H, `${FPS}/1`, `${FPS}/1`, "90"];
const AUDIO_SPEC = ["aac", "48000", 2];
const videoSpec = (v) => [v.codec_name, v.profile, v.level, v.pix_fmt, v.width, v.height, v.r_frame_rate, v.avg_frame_rate, v.nb_frames];
const audioSpec = (a) => [a?.codec_name, a?.sample_rate, a?.channels];
const secText = (v) => Number(v).toFixed(3);

async function checkStreams(file) {
  const p = await probe(file);
  assertEq(videoSpec(p.video), VIDEO_SPEC, "video stream");
  assertEq(audioSpec(p.audio), AUDIO_SPEC, "audio stream");
  const off = Math.abs(Number(p.video.duration) - 3.0);
  check(off <= 1 / FPS, `video duration ${p.video.duration} s is not 3.0 s within 1 frame`);
  return `${p.video.nb_frames} frames, ${secText(p.video.duration)} s, audio ${secText(p.audio.duration)} s`;
}

// Frames 25, 45 and 85 sit inside s01, s02 and s03; frame 34 is mid-way through the 9-frame xfade.
const PROBE_FRAMES = [25, 34, 45, 85];
const SCENE_COLOURS = [[255, 0, 0], [0, 255, 0], [0, 0, 255]];
const colourAt = (file) => (k) => meanColour(file, k);
const allNear = (got, want) => got.every((c, i) => near(c, want[i]));
const isBlend = (mid, a, b) => !near(mid, a, 40) && !near(mid, b, 40) && mid[0] > 40 && mid[1] > 40;

async function checkPictures(file) {
  const [red, mid, green, blue] = await Promise.all(PROBE_FRAMES.map(colourAt(file)));
  check(allNear([red, green, blue], SCENE_COLOURS), `scene colours ${json([red, green, blue])}; s03 must come from render/scenes`);
  check(isBlend(mid, red, green), `frame 34 ${json(mid)} is not a red-green blend`);
  const corner = await meanColour(file, 50, "crop=32:18:8:8");
  check(near(corner, [255, 255, 255]), `overlay corner is ${json(corner)}, not white`);
  return `xfade frame 34 ${json(mid)}`;
}

async function testAssemble(root) {
  await writeProject(root);
  const report = await assembleMaster(join(root, "timeline.json"), join(root, "render/master.mp4"));
  assertEq([report.beat_checked, report.overlays], [true, ["render/overlays/captions"]], "report");
  assertEq(JSON.parse(readFileSync(report.master_timeline, "utf8")), TIMELINE, "timeline-master.json");
  const file = join(root, "render/master.mp4");
  return `${await checkStreams(file)}; ${await checkPictures(file)}`;
}

const assembledOffBeat = () => fail("an off-grid cut assembled");

function offBeatError(e) {
  check(e.message.includes("scene s02-demo start_sec 1: 100.0 ms off the beat grid"), `error was: ${e.message}`);
}

function testOffBeat(root) {
  writeFileSync(join(root, "audio/beats.json"), json({ beats_sec: [0, 0.9, 2.0] }));
  return assembleMaster(join(root, "timeline.json"), join(root, "render/off.mp4")).then(assembledOffBeat, offBeatError);
}

function testMissingInput(root) {
  rmSync(join(root, "audio/mix.wav"));
  rmSync(join(root, "render/scenes/s02-demo.mp4"));
  const r = spawnSync(process.execPath, [SCRIPT, "--master", join(root, "timeline.json"), "--out", join(root, "render/x.mp4")], { encoding: "utf8" });
  check(r.status === 1, `exit code ${r.status}, want 1`);
  check(r.stderr.includes("scene s02-demo: none of render/scenes/s02-demo.mp4, captures/s02-demo.mp4"), `stderr: ${r.stderr}`);
  check(r.stderr.includes("audio: audio/mix.wav"), `stderr: ${r.stderr}`);
}

const TESTS = [
  ["transition_out names map to xfade types, 0.25-0.40 s", testTransitions],
  ["plan gives frame counts and tail handles", testPlan],
  ["beat check passes on-grid cuts and names off-grid cuts", testBeats],
  ["--master assembles a 3-scene project with an xfade, a cut, a text layer and mix.wav", testAssemble],
  ["--master fails on a cut off the beat grid", testOffBeat],
  ["--master exits 1 and names each missing input", testMissingInput],
];

const passedWith = (note) => ({ ok: true, note });
const failedWith = (e) => ({ ok: false, note: e.message });
const attempt = async (fn, arg) => fn(arg);
const testLine = (name, o) => (o.ok ? `ok - ${name}${o.note ? ` (${o.note})` : ""}` : `not ok - ${name}: ${o.note}`);

async function runTest(root, [name, fn]) {
  const outcome = await attempt(fn, root).then(passedWith, failedWith);
  console.log(testLine(name, outcome));
  return outcome.ok;
}

async function selfTest() {
  const root = mkdtempSync(join(tmpdir(), "video-render-master-"));
  const results = [];
  for (const t of TESTS) results.push(await runTest(root, t));
  rmSync(root, { recursive: true, force: true });
  const passed = results.filter(Boolean).length;
  console.log(`${passed} of ${results.length} tests passed`);
  return passed === results.length ? 0 : 1;
}

// ---------------------------------------------------------------- CLI

const OPTIONS = { master: { type: "string" }, out: { type: "string" }, "self-test": { type: "boolean" }, help: { type: "boolean", short: "h" } };

const uncomment = (line) => line.replace(/^\/\/ ?/, "");

function showHelp() {
  const header = readFileSync(SCRIPT, "utf8").split("\n").slice(1, 9);
  process.stdout.write(`${header.map(uncomment).join("\n")}\n`);
  return 0;
}

async function masterMode(v) {
  check(v.out, "--master needs --out", UsageError);
  const report = await assembleMaster(v.master, v.out);
  console.log(JSON.stringify({ ...report, out: relative(process.cwd(), resolve(report.out)) || report.out }, null, 2));
  return 0;
}

const MODES = [["help", showHelp], ["self-test", selfTest], ["master", masterMode]];
const modeIn = (values) => ([flag]) => values[flag];

function main() {
  const { values } = parseArgs({ options: OPTIONS, strict: true });
  const mode = MODES.find(modeIn(values)) ?? fail("pass --master <timeline.json> --out <mp4> or --self-test (see --help)", UsageError);
  return mode[1](values);
}

function die(e) {
  process.stderr.write(`assemble_master: ${e.message}\n`);
  process.exit(e instanceof UsageError || String(e.code).startsWith("ERR_PARSE_ARGS") ? 2 : 1);
}

if (process.argv[1] && realpathSync(process.argv[1]) === SCRIPT) Promise.resolve().then(main).then((code) => process.exit(code), die);
