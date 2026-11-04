/*
 * seek-timeline.js - a paused, seekable animation timeline for video scenes.
 * No imports, no network, no clock. Load it with a plain <script> tag. It sets
 * the browser global `SeekTimeline`.
 *
 *   var tl = SeekTimeline.timeline({ onSeek: function (t) { drawCanvas(t); } });
 *   tl.fromTo("#title", { y: 40, opacity: 0 }, { y: 0, opacity: 1, duration: 0.55, ease: "expo.out" }, 0);
 *   tl.addLabel("cta", 2.0);
 *   tl.to(".item", { x: 120, duration: 0.4, stagger: 0.06 }, "cta+=0.2");
 *   tl.set("#badge", { attr: { fill: "#F2C14E" } }, "cta");
 *   window.__seekToFrame = function (frameIndex, fps) { tl.seek(frameIndex / fps); };
 *
 * seek(t) is a pure function of t: it computes every animated property from
 * t alone and writes it, so frames can be rendered in any order.
 */
{
  // A block, not a function: every name below is block-scoped with const.

  const fail = function (message) { throw new Error("SeekTimeline: " + message); };
  const has = function (obj, key) { return Object.prototype.hasOwnProperty.call(obj, key); };
  const orDefault = function (v, d) { return v === undefined || isNaN(v) ? d : v; };
  const clamp = function (x, lo, hi) { return Math.min(hi, Math.max(lo, x)); };
  const round4 = function (x) { return +x.toFixed(4); };

  // ------------------------------------------------------------ easing
  // A family makes an "in" curve. out(p) = 1 - in(1 - p); inOut joins halves.
  const power = function (n, p) { return Math.pow(p, n); };
  const sineIn = function (p) { return 1 - Math.cos(p * Math.PI / 2); };
  const expoIn = function (p) { return p === 0 ? 0 : Math.pow(2, 10 * (p - 1)); };
  const backIn = function (s, p) { return p * p * ((s + 1) * p - s); };
  const elasticIn = function (a, period, p) {
    var s = period / (2 * Math.PI) * Math.asin(1 / a);
    var wave = -a * Math.pow(2, 10 * (p - 1)) * Math.sin((p - 1 - s) * 2 * Math.PI / period);
    return p === 0 || p === 1 ? p : wave;
  };

  const FAMILIES = {
    power1: function () { return power.bind(null, 2); },
    power2: function () { return power.bind(null, 3); },
    power3: function () { return power.bind(null, 4); },
    power4: function () { return power.bind(null, 5); },
    sine: function () { return sineIn; },
    expo: function () { return expoIn; },
    back: function (args) { return backIn.bind(null, orDefault(args[0], 1.70158)); },
    elastic: function (args) {
      return elasticIn.bind(null, Math.max(1, orDefault(args[0], 1)), orDefault(args[1], 0.3));
    }
  };

  const easeOut = function (easeIn, p) { return 1 - easeIn(1 - p); };
  const easeInOut = function (easeIn, p) { return p < 0.5 ? easeIn(p * 2) / 2 : 1 - easeIn((1 - p) * 2) / 2; };
  const VARIANTS = {
    in: function (easeIn) { return easeIn; },
    out: function (easeIn) { return easeOut.bind(null, easeIn); },
    inOut: function (easeIn) { return easeInOut.bind(null, easeIn); }
  };

  // cubic-bezier(x1, y1, x2, y2) as in CSS: find u with x(u) = p, return y(u).
  // x(u) rises from 0 to 1, so bisection finds u.
  const bezier = function (a, b, u) {
    var v = 1 - u;
    return 3 * v * v * u * a + 3 * v * u * u * b + u * u * u;
  };
  // goal is { x1, x2, p }. Each step halves the range [lo, hi] that holds u.
  const bisectBezierX = function (goal, lo, hi, steps) {
    var mid = (lo + hi) / 2;
    var below = bezier(goal.x1, goal.x2, mid) < goal.p;
    return steps === 0 ? mid : bisectBezierX(goal, below ? mid : lo, below ? hi : mid, steps - 1);
  };
  const solveBezierX = function (x1, x2, p) { return bisectBezierX({ x1: x1, x2: x2, p: p }, 0, 1, 50); };
  const bezierEase = function (x1, y1, x2, y2, p) {
    var inside = p > 0 && p < 1;
    return inside ? bezier(y1, y2, solveBezierX(x1, x2, p)) : clamp(Math.round(p), 0, 1);
  };

  const linear = function (p) { return p; };
  const EASE_NAME = /^([\w-]+)(?:\.(in|out|inOut))?(?:\(([^)]*)\))?$/;

  const familyEase = function (m, args) {
    return has(FAMILIES, m[1]) ? VARIANTS[m[2] || "out"](FAMILIES[m[1]](args)) : null;
  };
  const cubicBezierEase = function (m, args) {
    return args.length === 4 ? bezierEase.bind(null, args[0], args[1], args[2], args[3]) : null;
  };
  const NAMED_EASES = {
    linear: function () { return linear; },
    none: function () { return linear; },
    "cubic-bezier": cubicBezierEase
  };
  const parseEase = function (name) {
    var m = EASE_NAME.exec(name) || [];
    var args = m[3] ? m[3].split(",").map(Number) : [];
    var make = has(NAMED_EASES, m[1]) ? NAMED_EASES[m[1]] : familyEase;
    return make(m, args) || fail("unknown ease " + JSON.stringify(name));
  };

  const easeCache = Object.create(null);
  const ease = function (spec) {
    if (typeof spec === "function") return spec;
    var name = String(spec === undefined ? "power1.out" : spec).replace(/\s+/g, "");
    easeCache[name] = easeCache[name] || parseEase(name);
    return easeCache[name];
  };

  // ------------------------------------------------------------ values
  // A value is { n: number, unit: string } or { c: [r, g, b, a] }.
  const NAMED = { transparent: [0, 0, 0, 0], black: [0, 0, 0, 1], white: [255, 255, 255, 1] };

  const hexColor = function (h) {
    var full = h.length < 6 ? h.replace(/./g, "$&$&") : h;
    var c = [0, 2, 4, 6].map(function (i) { return parseInt(full.slice(i, i + 2) || "ff", 16); });
    c[3] /= 255;
    return full.length === 6 || full.length === 8 ? c : null;
  };
  const rgbColor = function (body) {
    var n = body.split(/[\s,/]+/).filter(Boolean).map(Number);
    return [n[0], n[1], n[2], n.length > 3 ? n[3] : 1];
  };
  const namedColor = function (s) { return has(NAMED, s) ? NAMED[s].slice() : null; };
  const parseColor = function (s) {
    var hex = /^#([0-9a-f]{3,8})$/i.exec(s);
    var fn = /^rgba?\(([^)]+)\)$/i.exec(s);
    return hex ? hexColor(hex[1]) : fn ? rgbColor(fn[1]) : namedColor(s);
  };

  // A value from a string or number, or null when it is not a colour or a number.
  const NUMBER = /^(-?\d*\.?\d+(?:e[+-]?\d+)?)([a-z%]*)$/i;
  const readValue = function (v) {
    var s = String(v).trim();
    var c = parseColor(s);
    var m = NUMBER.exec(s);
    return c ? { c: c } : m ? { n: Number(m[1]), unit: m[2] } : null;
  };
  const parseValue = function (v) { return readValue(v) || fail("cannot animate the value " + JSON.stringify(v)); };
  const tryParse = function (raw) {
    var empty = raw === undefined || raw === null || raw === "";
    return empty ? null : readValue(raw);
  };

  const lerp = function (a, b, k) { return a + (b - a) * k; };
  const mixColor = function (a, b, k) {
    return b.map(function (x, i) { return lerp(a[i], x, k); });
  };
  const mixNumber = function (a, b, k) {
    return { n: lerp(a.n === undefined ? b.n : a.n, b.n, k), unit: b.unit || a.unit };
  };
  const mix = function (a, b, k) { return b.c ? { c: mixColor(a.c || b.c, b.c, k) } : mixNumber(a, b, k); };

  const channel = function (x, i) { return i < 3 ? Math.round(clamp(x, 0, 255)) : round4(clamp(x, 0, 1)); };
  const format = function (v, defaultUnit) {
    var color = v.c && "rgba(" + v.c.map(channel).join(", ") + ")";
    return color || round4(v.n) + (v.unit || defaultUnit || "");
  };

  // ------------------------------------------------------------ properties
  // A track animates 1 property of 1 target. Its kind says how to read and
  // write it: an element style, an attribute, a transform part, or a plain
  // object property (for Canvas drawing).
  const UNITLESS = { opacity: 1, zIndex: 1, fontWeight: 1, lineHeight: 1, flexGrow: 1, flexShrink: 1, order: 1 };

  const computedStyle = function (el, k) {
    var gcs = globalThis.getComputedStyle;
    return typeof gcs === "function" ? gcs(el)[k] : undefined;
  };
  const putAttr = function (el, k, raw) {
    return raw === null ? el.removeAttribute(k) : el.setAttribute(k, raw);
  };

  const TRANSFORM = { x: 0, y: 0, scale: 1, scaleX: 1, scaleY: 1, rotate: 0, skewX: 0, skewY: 0 };
  const KINDS = {
    attr: {
      raw: function (el, k) { return el.getAttribute(k); },
      base: function (el, k) { return tryParse(el.getAttribute(k)); },
      put: putAttr,
      text: function (v) { return format(v); }
    },
    style: {
      raw: function (el, k) { return el.style[k]; },
      base: function (el, k) { return tryParse(el.style[k]) || tryParse(computedStyle(el, k)); },
      put: function (el, k, raw) { el.style[k] = raw || ""; },
      text: function (v, k) { return format(v, has(UNITLESS, k) ? "" : "px"); }
    },
    prop: {
      raw: function (el, k) { return el[k]; },
      base: function (el, k) { return tryParse(el[k]); },
      put: function (el, k, raw) { el[k] = raw; },
      text: function (v) { return v.c ? format(v) : v.n; }
    },
    transform: {
      raw: function () { return null; },
      base: function (el, k) { return { n: TRANSFORM[k], unit: "" }; }
    }
  };

  const defaultTransform = function () {
    var parts = {};
    Object.keys(TRANSFORM).forEach(function (k) { parts[k] = KINDS.transform.base(null, k); });
    return parts;
  };
  const transformString = function (t) {
    var sx = round4(t.scaleX.n * t.scale.n), sy = round4(t.scaleY.n * t.scale.n);
    var move = "translate(" + format(t.x, "px") + ", " + format(t.y, "px") + ")";
    var turn = " rotate(" + format(t.rotate, "deg") + ") skew(" + format(t.skewX, "deg") + ", " + format(t.skewY, "deg") + ")";
    return move + turn + " scale(" + sx + ", " + sy + ")";
  };

  const isElement = function (t) { return !!t && typeof t === "object" && t.style !== undefined; };
  const isList = function (t) { return !!t && typeof t === "object" && typeof t.length === "number" && !isElement(t); };
  const toTargets = function (targets) {
    var found = typeof targets === "string" ? globalThis.document.querySelectorAll(targets) : targets;
    return isList(found) ? Array.from(found) : [found];
  };

  const RESERVED = { duration: 1, ease: 1, stagger: 1, attr: 1 };
  const kindOf = function (target, k) {
    var elementKind = has(TRANSFORM, k) ? "transform" : "style";
    return isElement(target) ? elementKind : "prop";
  };
  // The [kind, prop, value] list a vars object animates on 1 target.
  const entries = function (target, vars) {
    var own = Object.keys(vars || {}).filter(function (k) { return !has(RESERVED, k); });
    var attr = (vars && vars.attr) || {};
    var plain = own.map(function (k) { return [kindOf(target, k), k, vars[k]]; });
    return plain.concat(Object.keys(attr).map(function (k) { return ["attr", k, attr[k]]; }));
  };

  const STAGGER_FROM = {
    start: function (i) { return i; },
    end: function (i, n) { return n - 1 - i; },
    center: function (i, n) { return Math.abs(i - (n - 1) / 2); }
  };
  const staggerOffset = function (stagger, i, n) {
    var s = stagger && typeof stagger === "object" ? stagger : { each: stagger };
    var from = STAGGER_FROM[s.from] || STAGGER_FROM.start;
    return (s.each || 0) * from(i, n);
  };

  // ------------------------------------------------------------ track values
  const byStart = function (a, b) { return a.start - b.start || a.order - b.order; };

  const tweenAt = function (w, from, t) {
    var p = w.duration > 0 ? clamp((t - w.start) / w.duration, 0, 1) : 1;
    return mix(from, w.to, p >= 1 ? 1 : w.ease(p));
  };
  // A tween without a from value starts at the value its track has at its
  // start, so a later tween takes over smoothly from an earlier one.
  const chain = function (base, state, w) {
    var from = w.from || (state ? tweenAt(state.w, state.from, w.start) : base) || w.to;
    return { w: w, from: from };
  };
  // Before its first tween, a track shows that tween's from value, else its base.
  const valueAt = function (tr, t) {
    var started = tr.tweens.filter(function (w) { return w.start <= t; }).sort(byStart);
    var state = started.reduce(chain.bind(null, tr.base), null);
    return state ? tweenAt(state.w, state.from, t) : tr.tweens.slice().sort(byStart)[0].from || tr.base;
  };

  // A track with no value restores the raw value it had before the timeline.
  const putValue = function (tr, v) {
    var kind = KINDS[tr.kind];
    kind.put(tr.target, tr.prop, v ? kind.text(v, tr.prop) : tr.raw);
  };
  const setTransformPart = function (tr, v, transforms) {
    var parts = transforms.get(tr.target) || defaultTransform();
    parts[tr.prop] = v;
    transforms.set(tr.target, parts);
  };
  const applyTrack = function (tr, v, transforms) {
    return tr.kind === "transform" ? setTransformPart(tr, v, transforms) : putValue(tr, v);
  };

  // ------------------------------------------------------------ timeline
  // position: undefined (the end), seconds, "+=s", "-=s", "<" (start of the
  // previous add), ">" (its end), or a label, each optionally followed by
  // "+=s" or "-=s".
  const POSITION = /^(<|>|[A-Za-z_][\w-]*)?(?:([+-])=(\d*\.?\d+))?$/;
  const ANCHORS = {
    "<": function (tl) { return tl.last.start; },
    ">": function (tl) { return tl.last.end; }
  };
  const offset = function (sign, amount) { return sign ? (sign === "-" ? -1 : 1) * Number(amount) : 0; };

  const Timeline = function (options) {
    this.tracks = [];
    this.labelTimes = Object.create(null);
    this.end = 0;
    this.last = { start: 0, end: 0 };
    this.order = 0;
    this.onSeek = typeof options.onSeek === "function" ? options.onSeek : function () {};
  };

  Timeline.prototype._anchor = function (name) {
    var at = name === undefined ? this.end : has(ANCHORS, name) ? ANCHORS[name](this) : this.labelTimes[name];
    return at === undefined ? fail("unknown label " + name) : at;
  };

  Timeline.prototype._resolve = function (position) {
    return typeof position === "number" ? position : this._resolveText(String(position === undefined ? "" : position).trim());
  };

  Timeline.prototype._resolveText = function (text) {
    var m = POSITION.exec(text);
    return m ? this._anchor(m[1]) + offset(m[2], m[3]) : fail("bad position " + JSON.stringify(text));
  };

  Timeline.prototype._track = function (target, kind, prop) {
    var found = this.tracks.filter(function (t) { return t.target === target && t.kind === kind && t.prop === prop; })[0];
    var k = KINDS[kind];
    if (found) return found;
    var created = { target: target, kind: kind, prop: prop, base: k.base(target, prop), raw: k.raw(target, prop), tweens: [] };
    this.tracks.push(created);
    return created;
  };

  Timeline.prototype._addTweens = function (target, fromVars, toVars, timing) {
    var from = {};
    entries(target, fromVars).forEach(function (e) { from[e[0] + ":" + e[1]] = parseValue(e[2]); });
    entries(target, toVars).forEach(function (e) {
      var tween = { order: this.order++, from: from[e[0] + ":" + e[1]], to: parseValue(e[2]) };
      this._track(target, e[0], e[1]).tweens.push(Object.assign(tween, timing));
    }, this);
  };

  Timeline.prototype._add = function (targets, fromVars, toVars, position, isSet) {
    var list = toTargets(targets);
    var start = this._resolve(position);
    var timing = { duration: isSet ? 0 : orDefault(toVars.duration, 0.5), ease: ease(toVars.ease) };
    var ends = list.map(function (target, i) {
      var at = start + staggerOffset(toVars.stagger, i, list.length);
      this._addTweens(target, fromVars, toVars, Object.assign({ start: at }, timing));
      return at + timing.duration;
    }, this);
    this.last = { start: start, end: Math.max.apply(null, ends.concat(start)) };
    this.end = Math.max(this.end, this.last.end);
    return this;
  };

  Timeline.prototype.to = function (targets, vars, position) { return this._add(targets, null, vars, position, false); };
  Timeline.prototype.fromTo = function (targets, fromVars, toVars, position) {
    return this._add(targets, fromVars, toVars, position, false);
  };
  Timeline.prototype.set = function (targets, vars, position) { return this._add(targets, null, vars, position, true); };
  Timeline.prototype.addLabel = function (name, position) {
    this.labelTimes[name] = this._resolve(position);
    this.end = Math.max(this.end, this.labelTimes[name]);
    return this;
  };
  Timeline.prototype.labels = function () { return Object.assign({}, this.labelTimes); };
  Timeline.prototype.duration = function () { return this.end; };

  Timeline.prototype.seek = function (t) {
    var transforms = new Map();
    this.tracks.forEach(function (tr) { applyTrack(tr, valueAt(tr, t), transforms); });
    transforms.forEach(function (parts, el) { el.style.transform = transformString(parts); });
    this.onSeek(t);
    return this;
  };

  globalThis.SeekTimeline = {
    timeline: function (options) { return new Timeline(options || {}); },
    ease: ease
  };
}
