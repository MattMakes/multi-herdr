// Self-test for seek-timeline.js. Node built-ins only. It loads the library
// in a small DOM stub and checks easing values, seek determinism in every
// order, stagger, labels and position offsets. Run: node seek-timeline.selftest.mjs
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const LIB = join(dirname(fileURLToPath(import.meta.url)), "seek-timeline.js");

function makeElement(id) {
  const attrs = new Map();
  return {
    id, style: {},
    setAttribute(k, v) { attrs.set(k, String(v)); },
    getAttribute(k) { return attrs.has(k) ? attrs.get(k) : null; },
    removeAttribute(k) { attrs.delete(k); },
    snapshot() { return JSON.stringify({ style: Object.entries(this.style).sort(), attrs: [...attrs].sort() }); },
  };
}

function load() {
  const els = {};
  const byClass = {};
  const document = {
    querySelectorAll(sel) {
      if (sel.startsWith("#")) return els[sel.slice(1)] ? [els[sel.slice(1)]] : [];
      if (sel.startsWith(".")) return byClass[sel.slice(1)] || [];
      return [];
    },
  };
  const window = { document, getComputedStyle: () => ({}) };
  window.window = window;
  vm.runInNewContext(readFileSync(LIB, "utf8"), window, { filename: LIB });
  const add = (id, cls) => {
    els[id] = makeElement(id);
    if (cls) (byClass[cls] ||= []).push(els[id]);
    return els[id];
  };
  return { ST: window.SeekTimeline, els, add };
}

const close = (a, b, eps = 1e-4) => assert.ok(Math.abs(a - b) < eps, `${a} != ${b}`);
const tests = [];
const test = (name, fn) => tests.push([name, fn]);

test("easing values at known points", () => {
  const { ST } = load();
  const e = ST.ease;
  for (const name of ["linear", "power1.in", "power4.inOut", "back.out", "elastic.out", "expo.in", "sine.inOut", "cubic-bezier(0.22, 1, 0.36, 1)"]) {
    close(e(name)(0), 0);
    close(e(name)(1), 1);
  }
  close(e("linear")(0.25), 0.25);
  close(e("power1.in")(0.5), 0.25);
  close(e("power2.in")(0.5), 0.125);
  close(e("power3.out")(0.5), 1 - 0.0625);
  close(e("power4.in")(0.5), 0.03125);
  close(e("power2.inOut")(0.5), 0.5);
  close(e("power2.inOut")(0.25), 0.0625);
  close(e("sine.in")(0.5), 1 - Math.cos(Math.PI / 4));
  close(e("sine.out")(0.5), Math.sin(Math.PI / 4));
  close(e("expo.in")(0.5), Math.pow(2, -5));
  close(e("expo.out")(0.5), 1 - Math.pow(2, -5));
  assert.ok(e("back.in")(0.2) < 0, "back.in dips below 0");
  assert.ok(e("back.out")(0.8) > 1, "back.out overshoots 1");
  assert.ok(e("elastic.out")(0.2) > 1, "elastic.out overshoots 1");
  close(e("cubic-bezier(0.25, 0.25, 0.75, 0.75)")(0.3), 0.3);
  close(e("cubic-bezier(0.42, 0, 1, 1)")(0.5), 0.3153, 1e-3); // CSS ease-in at 50%
  close(e("cubic-bezier(0, 0, 0.58, 1)")(0.5), 0.6847, 1e-3); // CSS ease-out at 50%
  assert.throws(() => e("bounce.wobble"), /unknown ease/);
});

function scene() {
  const ctx = load();
  const { ST, add } = ctx;
  const title = add("title");
  const items = [add("i0", "item"), add("i1", "item"), add("i2", "item")];
  const dot = add("dot");
  dot.setAttribute("r", "3");
  const canvas = { calls: [] };
  const tl = ST.timeline({ onSeek: (t) => { canvas.last = t; } });
  tl.fromTo("#title", { y: 40, opacity: 0 }, { y: 0, opacity: 1, duration: 0.5, ease: "expo.out" }, 0)
    .addLabel("list", 0.6)
    .to(".item", { x: 100, scale: 1.5, duration: 0.4, ease: "power2.out", stagger: 0.1 }, "list+=0.2")
    .to("#title", { color: "#ff0000", fontSize: "48px", duration: 0.5, ease: "linear" }, "list")
    .fromTo("#title", { color: "#000000" }, { color: "#ff0000", duration: 0.5, ease: "linear" }, "list")
    .to("#dot", { attr: { r: 12, fill: "rgb(0, 0, 255)" }, duration: 1, ease: "cubic-bezier(0.22, 1, 0.36, 1)" }, "<")
    .set("#title", { rotate: 5 }, "list+=1")
    .to("#title", { opacity: 0, duration: 0.3, ease: "power2.in" }, "-=0.1");
  return { ...ctx, tl, title, items, dot, canvas };
}

const state = (s) => [s.title, ...s.items, s.dot].map((el) => el.snapshot()).join("|");

test("seek forward, backward and random order give identical states", () => {
  const fps = 30;
  const frames = Array.from({ length: Math.ceil(scene().tl.duration() * fps) + 6 }, (_, i) => i);
  const forward = scene();
  const ref = frames.map((k) => (forward.tl.seek(k / fps), state(forward)));
  const backward = scene();
  [...frames].reverse().forEach((k) => {
    backward.tl.seek(k / fps);
    assert.equal(state(backward), ref[k], `frame ${k} backward`);
  });
  const random = scene();
  let seed = 7;
  for (let n = 0; n < 300; n++) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    const k = seed % frames.length;
    random.tl.seek(k / fps);
    assert.equal(state(random), ref[k], `frame ${k} random`);
    random.tl.seek(k / fps);
    assert.equal(state(random), ref[k], `frame ${k} repeated`);
  }
  assert.equal(random.canvas.last, frames[(seed % frames.length)] / fps, "onSeek gets t");
});

test("fromTo shows its start state before its start, and to-values at the end", () => {
  const s = scene();
  s.tl.seek(0);
  assert.equal(s.title.style.opacity, "0");
  assert.match(s.title.style.transform, /translate\(0px, 40px\)/);
  s.tl.seek(0.5);
  assert.equal(s.title.style.opacity, "1");
  assert.match(s.title.style.transform, /translate\(0px, 0px\)/);
  s.tl.seek(0.25);
  const y = Number(/translate\(0px, ([\d.]+)px\)/.exec(s.title.style.transform)[1]);
  close(y, 40 * Math.pow(2, -5), 1e-3); // expo.out at p = 0.5
});

test("stagger and label offsets", () => {
  const s = scene();
  assert.equal(s.tl.labels().list, 0.6);
  const x = (el) => Number(/translate\(([-\d.]+)px/.exec(el.style.transform)[1]);
  s.tl.seek(0.8); // item 0 starts at list+0.2 = 0.8
  assert.deepEqual(s.items.map(x), [0, 0, 0]);
  s.tl.seek(0.9); // item 1 starts, item 0 is 25% in
  close(x(s.items[0]), 100 * (1 - Math.pow(0.75, 3)));
  assert.equal(x(s.items[1]), 0);
  s.tl.seek(1.4); // item 2 ends at 1.0 + 0.4
  assert.deepEqual(s.items.map(x), [100, 100, 100]);
  assert.match(s.items[2].style.transform, /scale\(1\.5, 1\.5\)/);
});

test("colours, units, SVG attributes, set and relative positions", () => {
  const s = scene();
  s.tl.seek(0.85);
  assert.equal(s.title.style.color, "rgba(128, 0, 0, 1)");
  s.tl.seek(1.1);
  assert.equal(s.title.style.color, "rgba(255, 0, 0, 1)");
  assert.equal(s.title.style.fontSize, "48px");
  s.tl.seek(1.6); // "<" starts the dot at the start of the previous tween, 0.6
  assert.equal(s.dot.getAttribute("fill"), "rgba(0, 0, 255, 1)");
  assert.equal(s.dot.getAttribute("r"), "12");
  s.tl.seek(0.6);
  assert.equal(s.dot.getAttribute("r"), "3");
  assert.match(s.title.style.transform, /rotate\(0deg\)/);
  s.tl.seek(1.6);
  assert.match(s.title.style.transform, /rotate\(5deg\)/);
  // "-=0.1" starts the fade 0.1 s before the previous end (1.6), at 1.5.
  close(s.tl.duration(), 1.8);
  s.tl.seek(1.8);
  assert.equal(s.title.style.opacity, "0");
});

test("plain objects animate as properties, for Canvas drawing", () => {
  const { ST } = load();
  const ball = { x: 0, color: "#000" };
  const tl = ST.timeline().to(ball, { x: 200, duration: 2, ease: "linear" }, 1);
  tl.seek(2);
  assert.equal(ball.x, 100);
  tl.seek(0);
  assert.equal(ball.x, 0);
});

const t0 = process.hrtime.bigint();
let failed = 0;
for (const [name, fn] of tests) {
  try { fn(); console.log(`PASS ${name}`); } catch (e) { failed++; console.log(`FAIL ${name}: ${e.message}`); }
}
const ms = Number(process.hrtime.bigint() - t0) / 1e6;
console.log(`${tests.length - failed} of ${tests.length} passed in ${ms.toFixed(0)} ms`);
process.exit(failed ? 1 : 0);
