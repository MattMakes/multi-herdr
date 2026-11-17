import "ui-prop-tester";

const engine = globalThis.__uiPropTesterEngine;

export function registerStateAdapter(fn) {
  engine.registerStateAdapter(fn);
}

export function registerCustomAction(name, fn) {
  engine.registerCustomAction(name, fn);
}
