import { action, always, extract, next, not, now } from "ui-prop-tester";
import { registerCustomAction } from "ui-prop-tester/internal";

export const page = extract((state) => Object.fromEntries(
  Object.entries(state).filter((entry) => entry[0] !== "document" && entry[0] !== "window")));

export const resources = extract((state) => state.resources);

export function click(candidate) {
  return action({ kind: "click", target: candidate.target, point: candidate.point });
}

export function doubleClick(candidate) {
  return action({ kind: "double_click", target: candidate.target, point: candidate.point });
}

export function type(textOrGenerator) {
  return action({ kind: "type", text: textOrGenerator });
}

export function key(name) {
  return action({ kind: "key", key: name });
}

export function scrollBy(dy) {
  return action({ kind: "scroll", dy });
}

export function scrollToTop() {
  return action({ kind: "scroll_to_top" });
}

export function back() {
  return action({ kind: "back" });
}

export function forward() {
  return action({ kind: "forward" });
}

export function reload() {
  return action({ kind: "reload" });
}

export function wait(ms) {
  return action({ kind: "wait", ms });
}

export function drag({ from, to, steps = 10, delayMs = 0 }) {
  return action({ kind: "drag", from, to, steps, delay_ms: delayMs });
}

export function resize(width, height) {
  return action({ kind: "resize", width, height });
}

export function setFiles(selector, paths) {
  return action({ kind: "set_files", selector, paths });
}

export function custom(name, args) {
  return action({ kind: "custom", name, args: args === undefined ? null : args });
}

export function defineAction(name, handler) {
  registerCustomAction(name, handler);
  function customAction(args) {
    return custom(name, args);
  }
  return customAction;
}

function failGrowth(message) {
  throw new Error("resourceGrowth" + message);
}

function requireGrowthWindow(size) {
  return Number.isInteger(size) && size >= 1 ? size : failGrowth(" needs a window of at least 1 state");
}

function readResource(metric) {
  const value = resources.current[metric];
  return typeof value === "number" ? value : failGrowth(": unknown resource metric " + metric);
}

function grows(rule, previous, start, remaining) {
  function grewAgain() {
    const value = readResource(rule.metric);
    return value > previous && grows(rule, value, start, remaining - 1);
  }
  return remaining === 0 ? previous - start >= rule.growth : next(now(grewAgain));
}

export function resourceGrowth({ metric, window: size, minimumGrowth }) {
  requireGrowthWindow(size);
  const rule = { metric, growth: minimumGrowth === undefined ? 1 : minimumGrowth };
  function neverGrows() {
    const start = readResource(metric);
    return not(grows(rule, start, start, size));
  }
  return always(now(neverGrows));
}
