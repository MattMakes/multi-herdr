import { action, always, extract, next, not, now } from "ui-prop-tester";
import { registerCustomAction } from "ui-prop-tester/internal";

export const page = extract((state) => {
  const result = {};
  for (const key of Object.keys(state)) {
    if (key !== "document" && key !== "window") result[key] = state[key];
  }
  return result;
});

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
  return (args) => custom(name, args);
}

export function resourceGrowth({ metric, window: size, minimumGrowth }) {
  if (!Number.isInteger(size) || size < 1) {
    throw new Error("resourceGrowth needs a window of at least 1 state");
  }
  const growth = minimumGrowth === undefined ? 1 : minimumGrowth;
  const read = () => {
    const value = page.current.resources[metric];
    if (typeof value !== "number") {
      throw new Error("resourceGrowth: unknown resource metric " + metric);
    }
    return value;
  };
  const grows = (previous, start, remaining) => {
    if (remaining === 0) return previous - start >= growth;
    return next(now(() => {
      const value = read();
      return value > previous && grows(value, start, remaining - 1);
    }));
  };
  return always(now(() => {
    const start = read();
    return not(grows(start, start, size));
  }));
}
