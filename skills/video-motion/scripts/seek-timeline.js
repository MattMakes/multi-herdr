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
(function (root) {
  "use strict";

  // ------------------------------------------------------------ easing
  // Each family is an "in" curve. out(p) = 1 - in(1 - p), and inOut joins
  // the two halves.
  var TAU = Math.PI * 2;
  var FAMILIES = {
    power1: function () { return function (p) { return p * p; }; },
    power2: function () { return function (p) { return p * p * p; }; },
    power3: function () { return function (p) { return p * p * p * p; }; },
    power4: function () { return function (p) { return p * p * p * p * p; }; },
    sine: function () { return function (p) { return 1 - Math.cos(p * Math.PI / 2); }; },
    expo: function () { return function (p) { return p === 0 ? 0 : Math.pow(2, 10 * (p - 1)); }; },
    back: function (overshoot) {
      var s = overshoot === undefined ? 1.70158 : overshoot;
      return function (p) { return p * p * ((s + 1) * p - s); };
    },
    elastic: function (amplitude, period) {
      var a = Math.max(1, amplitude === undefined ? 1 : amplitude);
      var per = period === undefined ? 0.3 : period;
      var s = per / TAU * Math.asin(1 / a);
      return function (p) {
        if (p === 0 || p === 1) return p;
        return -a * Math.pow(2, 10 * (p - 1)) * Math.sin((p - 1 - s) * TAU / per);
      };
    }
  };

  function variant(easeIn, kind) {
    if (kind === "in") return easeIn;
    if (kind === "out") return function (p) { return 1 - easeIn(1 - p); };
    return function (p) {
      return p < 0.5 ? easeIn(p * 2) / 2 : 1 - easeIn((1 - p) * 2) / 2;
    };
  }

  // cubic-bezier(x1, y1, x2, y2) as in CSS: solve x(u) = p, return y(u).
  function cubicBezier(x1, y1, x2, y2) {
    function curve(a, b, u) {
      var v = 1 - u;
      return 3 * v * v * u * a + 3 * v * u * u * b + u * u * u;
    }
    function slope(a, b, u) {
      var v = 1 - u;
      return 3 * v * v * a + 6 * v * u * (b - a) + 3 * u * u * (1 - b);
    }
    return function (p) {
      if (p <= 0 || p >= 1) return p <= 0 ? 0 : 1;
      var u = p;
      for (var i = 0; i < 8; i++) {
        var d = slope(x1, x2, u);
        if (Math.abs(d) < 1e-7) break;
        var next = u - (curve(x1, x2, u) - p) / d;
        if (next < 0 || next > 1) break;
        u = next;
      }
      if (Math.abs(curve(x1, x2, u) - p) > 1e-7) {
        var lo = 0, hi = 1;
        for (var j = 0; j < 60; j++) {
          u = (lo + hi) / 2;
          if (curve(x1, x2, u) < p) lo = u; else hi = u;
        }
      }
      return curve(y1, y2, u);
    };
  }

  var easeCache = {};
  function ease(spec) {
    if (typeof spec === "function") return spec;
    var name = String(spec === undefined ? "power1.out" : spec).replace(/\s+/g, "");
    if (easeCache[name]) return easeCache[name];
    var fn;
    var args = [];
    var m = /^([\w-]+)(?:\.(in|out|inOut))?(?:\(([^)]*)\))?$/.exec(name);
    if (m && m[3]) args = m[3].split(",").map(Number);
    if (name === "linear" || name === "none") {
      fn = function (p) { return p; };
    } else if (m && m[1] === "cubic-bezier" && args.length === 4) {
      fn = cubicBezier(args[0], args[1], args[2], args[3]);
    } else if (m && FAMILIES[m[1]]) {
      fn = variant(FAMILIES[m[1]].apply(null, args), m[2] || "out");
    } else {
      throw new Error("SeekTimeline: unknown ease " + JSON.stringify(spec));
    }
    easeCache[name] = fn;
    return fn;
  }

  // ------------------------------------------------------------ values
  // A value is { n: [numbers], unit } or { c: [r, g, b, a] }.
  var NAMED = { transparent: [0, 0, 0, 0], black: [0, 0, 0, 1], white: [255, 255, 255, 1] };

  function parseColor(s) {
    if (NAMED[s]) return NAMED[s].slice();
    var hex = /^#([0-9a-f]{3,8})$/i.exec(s);
    if (hex) {
      var h = hex[1];
      if (h.length === 3 || h.length === 4) h = h.replace(/./g, "$&$&");
      if (h.length !== 6 && h.length !== 8) return null;
      var c = [0, 2, 4].map(function (i) { return parseInt(h.slice(i, i + 2), 16); });
      c.push(h.length === 8 ? parseInt(h.slice(6, 8), 16) / 255 : 1);
      return c;
    }
    var fn = /^rgba?\(([^)]+)\)$/i.exec(s);
    if (fn) {
      var parts = fn[1].split(/[\s,/]+/).filter(Boolean).map(Number);
      return [parts[0], parts[1], parts[2], parts.length > 3 ? parts[3] : 1];
    }
    return null;
  }

  function parseValue(v) {
    if (typeof v === "number") return { n: [v], unit: "" };
    var s = String(v).trim();
    var c = parseColor(s);
    if (c) return { c: c };
    var m = /^(-?\d*\.?\d+(?:e[+-]?\d+)?)([a-z%]*)$/i.exec(s);
    if (m) return { n: [Number(m[1])], unit: m[2] };
    throw new Error("SeekTimeline: cannot animate the value " + JSON.stringify(v));
  }

  function mix(a, b, k) {
    if (b.c) {
      var from = a.c || b.c;
      return { c: b.c.map(function (x, i) { return from[i] + (x - from[i]) * k; }) };
    }
    var f = a.n ? a.n[0] : b.n[0];
    return { n: [f + (b.n[0] - f) * k], unit: b.unit || a.unit || "" };
  }

  function format(v, defaultUnit) {
    if (v.c) {
      var c = v.c.map(function (x, i) { return i < 3 ? Math.round(Math.min(255, Math.max(0, x))) : +Math.min(1, Math.max(0, x)).toFixed(4); });
      return "rgba(" + c.join(", ") + ")";
    }
    return +v.n[0].toFixed(4) + (v.unit || defaultUnit || "");
  }

  // ------------------------------------------------------------ properties
  var TRANSFORM = { x: 0, y: 0, scale: 1, scaleX: 1, scaleY: 1, rotate: 0, skewX: 0, skewY: 0 };
  var UNITLESS = { opacity: 1, zIndex: 1, fontWeight: 1, lineHeight: 1, flexGrow: 1, flexShrink: 1, order: 1 };
  var RESERVED = { duration: 1, ease: 1, stagger: 1, attr: 1, immediateRender: 1 };

  function isElement(t) { return t && typeof t === "object" && t.style !== undefined; }

  // What a property holds before the timeline touches it, read once: the
  // raw value to restore, and the value to tween from.
  function readRaw(target, kind, prop) {
    if (kind === "attr") return target.getAttribute(prop);
    if (kind === "style") return target.style[prop];
    return kind === "prop" ? target[prop] : null;
  }

  function restore(tr) {
    if (tr.kind === "attr" && tr.raw === null) tr.target.removeAttribute(tr.prop);
    else if (tr.kind === "attr") tr.target.setAttribute(tr.prop, tr.raw);
    else if (tr.kind === "style") tr.target.style[tr.prop] = tr.raw || "";
    else if (tr.kind === "prop") tr.target[tr.prop] = tr.raw;
  }

  function readBase(target, kind, prop) {
    var raw;
    if (kind === "transform") return { n: [TRANSFORM[prop]], unit: "" };
    if (kind === "attr") raw = target.getAttribute(prop);
    else if (kind === "prop") raw = target[prop];
    else {
      raw = target.style[prop];
      if ((raw === undefined || raw === "") && typeof root.getComputedStyle === "function") {
        raw = root.getComputedStyle(target)[prop];
      }
    }
    if (raw === undefined || raw === null || raw === "") return null;
    try { return parseValue(raw); } catch (e) { return null; }
  }

  function write(target, kind, prop, v) {
    if (kind === "attr") target.setAttribute(prop, format(v));
    else if (kind === "prop") target[prop] = v.c ? format(v) : v.n[0];
    else target.style[prop] = format(v, UNITLESS[prop] ? "" : "px");
  }

  function transformString(t) {
    var sx = t.scaleX.n[0] * t.scale.n[0], sy = t.scaleY.n[0] * t.scale.n[0];
    return "translate(" + format(t.x, "px") + ", " + format(t.y, "px") + ") rotate(" + format(t.rotate, "deg") +
      ") skew(" + format(t.skewX, "deg") + ", " + format(t.skewY, "deg") + ") scale(" + +sx.toFixed(4) + ", " + +sy.toFixed(4) + ")";
  }

  function toTargets(targets) {
    if (typeof targets === "string") return Array.prototype.slice.call(root.document.querySelectorAll(targets));
    if (Array.isArray(targets)) return targets.slice();
    if (targets && typeof targets.length === "number" && !isElement(targets) && typeof targets !== "function") {
      return Array.prototype.slice.call(targets);
    }
    return [targets];
  }

  // The list of (kind, prop, value) a vars object animates.
  function entries(target, vars) {
    var out = [];
    Object.keys(vars || {}).forEach(function (k) {
      if (RESERVED[k]) return;
      var kind = !isElement(target) ? "prop" : TRANSFORM.hasOwnProperty(k) ? "transform" : "style";
      out.push([kind, k, vars[k]]);
    });
    Object.keys((vars && vars.attr) || {}).forEach(function (k) { out.push(["attr", k, vars.attr[k]]); });
    return out;
  }

  function staggerOffset(stagger, i, n) {
    if (!stagger) return 0;
    var each = typeof stagger === "number" ? stagger : stagger.each || 0;
    var from = typeof stagger === "object" ? stagger.from : "start";
    if (from === "end") return (n - 1 - i) * each;
    if (from === "center") return Math.abs(i - (n - 1) / 2) * each;
    return i * each;
  }

  // ------------------------------------------------------------ timeline
  function timeline(options) {
    options = options || {};
    var tracks = [];      // one per (target, kind, prop): { target, kind, prop, base, tweens }
    var labels = {};
    var end = 0;          // end time of the whole timeline, in seconds
    var last = { start: 0, end: 0 };
    var order = 0;
    var tl = {};

    function track(target, kind, prop) {
      for (var i = 0; i < tracks.length; i++) {
        var t = tracks[i];
        if (t.target === target && t.kind === kind && t.prop === prop) return t;
      }
      var created = { target: target, kind: kind, prop: prop, base: readBase(target, kind, prop), raw: readRaw(target, kind, prop), tweens: [] };
      tracks.push(created);
      return created;
    }

    // position: undefined (the end), seconds, "+=s", "-=s", "<", ">", "label", or any of
    // "<", ">", "label" followed by "+=s" or "-=s".
    function resolve(position) {
      if (position === undefined) return end;
      if (typeof position === "number") return position;
      var m = /^(<|>|[A-Za-z_][\w-]*)?(?:([+-])=(\d*\.?\d+))?$/.exec(String(position).trim());
      if (!m) throw new Error("SeekTimeline: bad position " + JSON.stringify(position));
      var at = end;
      if (m[1] === "<") at = last.start;
      else if (m[1] === ">") at = last.end;
      else if (m[1] !== undefined) {
        if (!labels.hasOwnProperty(m[1])) throw new Error("SeekTimeline: unknown label " + m[1]);
        at = labels[m[1]];
      }
      if (m[2]) at += (m[2] === "-" ? -1 : 1) * Number(m[3]);
      return at;
    }

    function add(targets, fromVars, toVars, position, isSet) {
      var list = toTargets(targets);
      var start = resolve(position);
      var duration = isSet ? 0 : toVars.duration === undefined ? 0.5 : toVars.duration;
      var easeFn = ease(toVars.ease);
      var groupEnd = start;
      list.forEach(function (target, i) {
        var at = start + staggerOffset(toVars.stagger, i, list.length);
        var from = {};
        entries(target, fromVars).forEach(function (e) { from[e[0] + ":" + e[1]] = parseValue(e[2]); });
        entries(target, toVars).forEach(function (e) {
          track(target, e[0], e[1]).tweens.push({
            start: at, duration: duration, ease: easeFn, order: order++,
            from: from[e[0] + ":" + e[1]], to: parseValue(e[2])
          });
        });
        groupEnd = Math.max(groupEnd, at + duration);
      });
      last = { start: start, end: groupEnd };
      end = Math.max(end, groupEnd);
      return tl;
    }

    function tweenAt(w, from, t) {
      var p = w.duration > 0 ? Math.min(1, Math.max(0, (t - w.start) / w.duration)) : 1;
      return mix(from, w.to, p >= 1 ? 1 : w.ease(p));
    }

    // The value of one track at time t. Tweens run in start order. A tween
    // without a from value starts at the value its track has at its start, so
    // a later tween takes over smoothly from an earlier one. Before the first
    // tween the track shows that tween's from value, else its base value.
    function valueAt(tr, t) {
      var sorted = tr.tweens.slice().sort(function (a, b) { return a.start - b.start || a.order - b.order; });
      var active = null, activeFrom = null;
      for (var i = 0; i < sorted.length && sorted[i].start <= t; i++) {
        var w = sorted[i];
        activeFrom = w.from || (active ? tweenAt(active, activeFrom, w.start) : tr.base) || w.to;
        active = w;
      }
      if (!active) return sorted[0].from || tr.base;
      return tweenAt(active, activeFrom, t);
    }

    tl.to = function (targets, vars, position) { return add(targets, null, vars, position, false); };
    tl.fromTo = function (targets, fromVars, toVars, position) { return add(targets, fromVars, toVars, position, false); };
    tl.set = function (targets, vars, position) { return add(targets, null, vars, position, true); };
    tl.addLabel = function (name, position) {
      labels[name] = resolve(position);
      end = Math.max(end, labels[name]);
      return tl;
    };
    tl.labels = function () { return Object.assign({}, labels); };
    tl.duration = function () { return end; };

    tl.seek = function (t) {
      var transforms = [];
      tracks.forEach(function (tr) {
        var v = valueAt(tr, t);
        if (!v) return restore(tr);
        if (tr.kind !== "transform") return write(tr.target, tr.kind, tr.prop, v);
        var rec = null;
        for (var i = 0; i < transforms.length; i++) if (transforms[i].el === tr.target) rec = transforms[i];
        if (!rec) {
          rec = { el: tr.target, parts: {} };
          Object.keys(TRANSFORM).forEach(function (k) { rec.parts[k] = { n: [TRANSFORM[k]], unit: "" }; });
          transforms.push(rec);
        }
        rec.parts[tr.prop] = v;
      });
      transforms.forEach(function (rec) { rec.el.style.transform = transformString(rec.parts); });
      if (typeof options.onSeek === "function") options.onSeek(t);
      return tl;
    };

    return tl;
  }

  root.SeekTimeline = { timeline: timeline, ease: ease };
})(typeof window !== "undefined" ? window : globalThis);
