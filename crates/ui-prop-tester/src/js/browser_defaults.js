import { actions, always, chars, email, from, integer, text, weighted } from "ui-prop-tester";
import { back, click, forward, key, page, reload, scrollBy, scrollToTop, type, wait } from "ui-prop-tester/browser";

export { page };

const editingKeys = ["Backspace", "Tab", "Enter", "Escape"];
const textInputTypes = ["text", "search"];

function originOf(url) {
  const match = /^([a-z][a-z0-9+.-]*:)(\/\/[^/?#]*)?/i.exec(url || "");
  return match ? (match[1] + (match[2] || "")).toLowerCase() : "";
}

function isRealEntry(entry) {
  return !!entry && typeof entry.url === "string" && entry.url !== "" && !entry.url.startsWith("about:") && !entry.url.startsWith("chrome-error:");
}

export const noHttpErrors = always(() => {
  const status = page.current.navigation.status;
  return status === null || status === undefined || status < 400;
});

export const noUncaughtExceptions = always(() =>
  page.current.exceptions.every((exception) => exception.kind !== "exception"));

export const noUnhandledRejections = always(() =>
  page.current.exceptions.every((exception) => exception.kind !== "rejection"));

export const noConsoleErrors = always(() =>
  page.current.console.every((entry) => entry.level !== "error"));

export const clicks = actions(() =>
  page.current.clickables.map((candidate) => click(candidate))).weight(100);

export const inputs = actions(() => {
  const focused = page.current.focused;
  if (!focused) return null;
  const keys = key(from(editingKeys));
  if (focused.tag === "textarea") {
    return weighted([[4, type(text({ min: 1, max: 24 }))], [1, keys]]);
  }
  if (focused.tag !== "input") return null;
  if (focused.input_type === "email") {
    return weighted([[3, type(email())], [1, keys]]);
  }
  if (focused.input_type === "number") {
    return weighted([[3, type(chars("0123456789", { min: 1, max: 5 }))], [1, keys]]);
  }
  if (textInputTypes.includes(focused.input_type)) {
    return weighted([[3, type(text({ min: 1, max: 16 }))], [1, keys]]);
  }
  return null;
}).weight(100);

export const scrolling = actions(() => {
  const state = page.current;
  if (!state.document.is_html || !state.document.has_body) return null;
  if (state.scroll.y < state.scroll.max_y) {
    const most = Math.max(1, Math.floor(state.viewport.height / 2));
    return scrollBy(integer(1, most));
  }
  if (state.scroll.y > 0) return scrollToTop();
  return null;
}).weight(50);

export const navigation = actions(() => {
  const state = page.current;
  const entries = state.history.entries;
  const index = state.history.index;
  const choices = [];
  if (index > 0 && isRealEntry(entries[index - 1])) choices.push([10, back()]);
  const following = entries[index + 1];
  if (following && originOf(following.url) === originOf(state.url)) choices.push([1, forward()]);
  const last = state.last_action;
  if (!last || (last.kind !== "reload" && last.kind !== "wait")) choices.push([1, reload()]);
  return choices.length ? weighted(choices) : null;
}).weight(10);

export const waiting = actions(() => {
  const last = page.current.last_action;
  if (last && last.kind === "wait") return null;
  return wait(integer(100, 1000));
}).weight(1);
