// Self-test for seek-timeline.js. Node built-ins only. It loads the library
// in a small DOM stub and checks easing values, seek determinism in every
// order, stagger, labels and position offsets. Run: node seek-timeline.selftest.mjs
import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const LIB = join(dirname(fileURLToPath(import.meta.url)), "seek-timeline.js");
const SOURCE = readFileSync(LIB, "utf8");
const FPS = 30;

function makeElement(id) {
  const attrs = new Map();
  return {
    id, style: {},
    setAttribute(k, v) { attrs.set(k, String(v)); },
    getAttribute(k) { return attrs.get(k) ?? null; },
    removeAttribute(k) { attrs.delete(k); },
    attributes() { return [...attrs].sort(); },
  };
}

// A fresh library copy in a fresh global, with elements found by "#id" or ".class".
function load() {
  const bySelector = new Map();
  const document = { querySelectorAll(sel) { return bySelector.get(sel) ?? []; } };
  const window = { document, getComputedStyle() { return {}; } };
  vm.runInNewContext(SOURCE, window, { filename: LIB });
  function add(id, cls) {
    const el = makeElement(id);
    [`#${id}`, `.${cls}`].forEach(function (sel) { bySelector.set(sel, [...(bySelector.get(sel) ?? []), el]); });
    return el;
  }
  return { ST: window.SeekTimeline, add };
}

const close = (a, b, eps = 1e-4) => assert.ok(Math.abs(a - b) < eps, `${a} != ${b}`);
const snapshot = (el) => JSON.stringify([Object.entries(el.style).sort(), el.attributes()]);

// [ease, p, expected value]
const EASE_POINTS = [
  ["linear", 0.25, 0.25],
  ["power1.in", 0.5, 0.25],
  ["power2.in", 0.5, 0.125],
  ["power3.out", 0.5, 1 - 0.0625],
  ["power4.in", 0.5, 0.03125],
  ["power2.inOut", 0.5, 0.5],
  ["power2.inOut", 0.25, 0.0625],
  ["sine.in", 0.5, 1 - Math.cos(Math.PI / 4)],
  ["sine.out", 0.5, Math.sin(Math.PI / 4)],
  ["expo.in", 0.5, Math.pow(2, -5)],
  ["expo.out", 0.5, 1 - Math.pow(2, -5)],
  ["cubic-bezier(0.25, 0.25, 0.75, 0.75)", 0.3, 0.3],
  ["cubic-bezier(0.42, 0, 1, 1)", 0.5, 0.3153], // CSS ease-in at 50 %
  ["cubic-bezier(0, 0, 0.58, 1)", 0.5, 0.6847], // CSS ease-out at 50 %
];
const ENDPOINT_EASES = ["linear", "power1.in", "power4.inOut", "back.out", "elastic.out", "expo.in", "sine.inOut",
  "cubic-bezier(0.22, 1, 0.36, 1)"];
// [ease, p, true when the curve leaves [0, 1] there]
const OVERSHOOTS = [["back.in", 0.2, (v) => v < 0], ["back.out", 0.8, (v) => v > 1], ["elastic.out", 0.2, (v) => v > 1]];

test("easing values at known points", () => {
  const { ease } = load().ST;
  EASE_POINTS.forEach(function ([name, p, want]) { close(ease(name)(p), want, 1e-3); });
  assert.throws(function () { ease("bounce.wobble"); }, /unknown ease/);
});

test("every ease runs from 0 to 1", () => {
  const { ease } = load().ST;
  ENDPOINT_EASES.forEach(function (name) { close(ease(name)(0), 0, 1e-9); close(ease(name)(1), 1, 1e-9); });
});

test("back and elastic leave the 0 to 1 range", () => {
  const { ease } = load().ST;
  OVERSHOOTS.forEach(function ([name, p, leaves]) { assert.ok(leaves(ease(name)(p)), `${name} at ${p}`); });
});

// The scene: a title, 3 staggered items, an SVG dot, labels and offsets.
function addMotion(tl) {
  tl.fromTo("#title", { y: 40, opacity: 0 }, { y: 0, opacity: 1, duration: 0.5, ease: "expo.out" }, 0)
    .addLabel("list", 0.6)
    .to(".item", { x: 100, scale: 1.5, duration: 0.4, ease: "power2.out", stagger: 0.1 }, "list+=0.2")
    .to("#title", { color: "#ff0000", fontSize: "48px", duration: 0.5, ease: "linear" }, "list")
    .fromTo("#title", { color: "#000000" }, { color: "#ff0000", duration: 0.5, ease: "linear" }, "list")
    .to("#dot", { attr: { r: 12, fill: "rgb(0, 0, 255)" }, duration: 1, ease: "cubic-bezier(0.22, 1, 0.36, 1)" }, "<")
    .set("#title", { rotate: 5 }, "list+=1")
    .to("#title", { opacity: 0, duration: 0.3, ease: "power2.in" }, "-=0.1");
}

function scene() {
  const { ST, add } = load();
  const els = [add("title"), add("i0", "item"), add("i1", "item"), add("i2", "item"), add("dot")];
  els[4].setAttribute("r", "3");
  const seen = [];
  const tl = ST.timeline({ onSeek(t) { seen.push(t); } });
  addMotion(tl);
  function at(t) {
    tl.seek(t);
    return els.map(snapshot).join("|");
  }
  return { tl, at, seen, title: els[0], items: els.slice(1, 4), dot: els[4] };
}

// The state of each frame of a fresh scene, seeked in the given order. A
// repeated frame keeps its last state.
function statesInOrder(frames) {
  const s = scene();
  return new Map(frames.map(function (k) { return [k, s.at(k / FPS)]; }));
}

// Every frame twice, in a scrambled order.
function scrambled(frames, step) {
  const twice = [...frames, ...frames];
  return twice.map(function (k, i) { return twice[(i * step) % twice.length]; });
}

test("seek forward, backward and random order give identical states", () => {
  const frames = [...Array(Math.ceil(scene().tl.duration() * FPS) + 6).keys()];
  const forward = statesInOrder(frames);
  const orders = { backward: [...frames].reverse(), random: scrambled(frames, 37) };
  Object.entries(orders).forEach(function ([name, order]) {
    statesInOrder(order).forEach(function (state, k) { assert.equal(state, forward.get(k), `frame ${k} ${name}`); });
  });
});

test("onSeek gets t, and fromTo shows its start state before its start", () => {
  const s = scene();
  s.at(0);
  assert.deepEqual([s.title.style.opacity, s.seen], ["0", [0]]);
  assert.match(s.title.style.transform, /translate\(0px, 40px\)/);
  s.at(0.5);
  assert.equal(s.title.style.opacity, "1");
  s.at(0.25);
  close(Number(/translate\(0px, ([\d.]+)px\)/.exec(s.title.style.transform)[1]), 40 * Math.pow(2, -5), 1e-3);
});

const translateX = (el) => Number(/translate\(([-\d.]+)px/.exec(el.style.transform)[1]);

test("stagger and label offsets", () => {
  const s = scene();
  assert.equal(s.tl.labels().list, 0.6);
  s.at(0.8); // item 0 starts at list+=0.2 = 0.8 s
  assert.deepEqual(s.items.map(translateX), [0, 0, 0]);
  s.at(0.9); // item 1 starts, item 0 is 25 % in
  close(translateX(s.items[0]), 100 * (1 - Math.pow(0.75, 3)));
  s.at(1.4); // item 2 ends at 1.0 + 0.4 s
  assert.deepEqual(s.items.map(translateX), [100, 100, 100]);
  assert.match(s.items[2].style.transform, /scale\(1\.5, 1\.5\)/);
});

test("colours, units and SVG attributes", () => {
  const s = scene();
  s.at(0.85);
  assert.equal(s.title.style.color, "rgba(128, 0, 0, 1)");
  s.at(1.6); // "<" starts the dot at the start of the previous tween, 0.6 s
  assert.deepEqual([s.title.style.fontSize, s.dot.getAttribute("fill"), s.dot.getAttribute("r")],
    ["48px", "rgba(0, 0, 255, 1)", "12"]);
  s.at(0.6);
  assert.equal(s.dot.getAttribute("r"), "3");
});

test("set and relative positions", () => {
  const s = scene();
  s.at(0.6);
  assert.match(s.title.style.transform, /rotate\(0deg\)/);
  s.at(1.6);
  assert.match(s.title.style.transform, /rotate\(5deg\)/);
  close(s.tl.duration(), 1.8); // "-=0.1" starts the fade 0.1 s before the end, 1.6 s
  s.at(1.8);
  assert.equal(s.title.style.opacity, "0");
  assert.throws(() => s.tl.to("#title", { x: 1 }, "nope+=1"), /unknown label/);
  assert.throws(() => s.tl.to("#title", { x: 1 }, "1s"), /bad position/);
});

test("plain objects animate as properties, for Canvas drawing", () => {
  const ball = { x: 0 };
  const tl = load().ST.timeline().to(ball, { x: 200, duration: 2, ease: "linear" }, 1);
  assert.deepEqual([tl.seek(2) && ball.x, tl.seek(0) && ball.x], [100, 0]);
});
