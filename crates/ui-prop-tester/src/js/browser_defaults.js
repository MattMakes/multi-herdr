import { actions, always, chars, email, extract, from, integer, text, weighted } from "ui-prop-tester";
import { back, click, forward, key, page, reload, resources, scrollBy, scrollToTop, type, wait } from "ui-prop-tester/browser";

export { page, resources };

const editingKeys = ["Backspace", "Tab", "Enter", "Escape"];
const textInputTypes = ["text", "search"];

function originOf(url) {
  const match = /^([a-z][a-z0-9+.-]*:)(\/\/[^/?#]*)?/i.exec(url || "");
  return match ? (match[1] + (match[2] || "")).toLowerCase() : "";
}

function isRealEntry(entry) {
  return !!entry && typeof entry.url === "string" && entry.url !== "" && !entry.url.startsWith("about:") && !entry.url.startsWith("chrome-error:");
}

export const httpStatus = extract((state) => state.navigation.status ?? null);

export const uncaughtExceptions = extract((state) =>
  state.exceptions.filter((exception) => exception.kind === "exception"));

export const unhandledRejections = extract((state) =>
  state.exceptions.filter((exception) => exception.kind === "rejection"));

export const consoleErrors = extract((state) =>
  state.console.filter((entry) => entry.level === "error"));

export const noHttpErrors = always(() => httpStatus.current === null || httpStatus.current < 400);

export const noUncaughtExceptions = always(() => uncaughtExceptions.current.length === 0);

export const noUnhandledRejections = always(() => unhandledRejections.current.length === 0);

export const noConsoleErrors = always(() => consoleErrors.current.length === 0);

export const clicks = actions(() =>
  page.current.clickables.map((candidate) => click(candidate))).weight(100);

function plainTextFor(inputType) {
  return textInputTypes.includes(inputType) ? text({ min: 1, max: 16 }) : null;
}

function digitsOrTextFor(inputType) {
  return inputType === "number" ? chars("0123456789", { min: 1, max: 5 }) : plainTextFor(inputType);
}

function inputTextFor(inputType) {
  return inputType === "email" ? email() : digitsOrTextFor(inputType);
}

function typedInputField(focused) {
  const generator = focused.tag === "input" ? inputTextFor(focused.input_type) : null;
  return generator ? [3, generator] : null;
}

function typedInput(focused) {
  return focused.tag === "textarea" ? [4, text({ min: 1, max: 24 })] : typedInputField(focused);
}

function inputChoices() {
  const focused = page.current.focused;
  if (!focused) return null;
  const keys = key(from(editingKeys));
  const typed = typedInput(focused);
  return typed ? weighted([[typed[0], type(typed[1])], [1, keys]]) : null;
}

export const inputs = actions(inputChoices).weight(100);

function scrollDown(state) {
  const most = Math.max(1, Math.floor(state.viewport.height / 2));
  return scrollBy(integer(1, most));
}

function scrollBackUp(state) {
  return state.scroll.y > 0 ? scrollToTop() : null;
}

function scrollChoice(state) {
  return state.scroll.y < state.scroll.max_y ? scrollDown(state) : scrollBackUp(state);
}

function scrollChoices() {
  const state = page.current;
  return state.content.is_html && state.content.has_body ? scrollChoice(state) : null;
}

export const scrolling = actions(scrollChoices).weight(50);

function canGoBack(history) {
  return history.index > 0 && isRealEntry(history.entries[history.index - 1]);
}

function canGoForward(state) {
  const following = state.history.entries[state.history.index + 1];
  return following && originOf(following.url) === originOf(state.url);
}

function canReload(last) {
  return !last || (last.kind !== "reload" && last.kind !== "wait");
}

function backChoices(state) {
  return canGoBack(state.history) ? [[10, back()]] : [];
}

function forwardChoices(state) {
  return canGoForward(state) ? [[1, forward()]] : [];
}

function reloadChoices(state) {
  return canReload(state.last_action) ? [[1, reload()]] : [];
}

function navigationChoices() {
  const state = page.current;
  const choices = [...backChoices(state), ...forwardChoices(state), ...reloadChoices(state)];
  return choices.length ? weighted(choices) : null;
}

export const navigation = actions(navigationChoices).weight(10);

function waitChoice() {
  const last = page.current.last_action;
  return last && last.kind === "wait" ? null : wait(integer(100, 1000));
}

export const waiting = actions(waitChoice).weight(1);
