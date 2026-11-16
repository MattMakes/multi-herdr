import { always, extract, oneOf } from "ui-prop-tester";
import { controlKeys, safeText } from "ui-prop-tester/terminal";

const exitStatus = extract((state) => state.exit);

const replacementCharacterVisible = extract((state) =>
  state.cells.some((cell) => cell.text.includes("�")),
);

export const exitsCleanly = always(() => {
  const exit = exitStatus.current;
  return exit === null || exit.signal !== null || exit.code === 0;
});

export const noReplacementCharacters = always(() => !replacementCharacterVisible.current);

export const typing = oneOf(safeText, controlKeys);
