import { action, from } from "ui-prop-tester";

export function step(name) {
  return action({ kind: "step", name });
}

function stepOf(name) {
  return { kind: "step", name };
}

export function steps(...names) {
  return from(names.flat()).map(stepOf);
}
