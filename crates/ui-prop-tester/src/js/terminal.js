import { action, chars, oneOf } from "ui-prop-tester";
import { registerStateAdapter } from "ui-prop-tester/internal";

const DEFAULT_CELL = {
  text: " ",
  wide_continuation: false,
  fg: null,
  bg: null,
  underline_color: null,
  underline: null,
  bold: false,
  italic: false,
  blink: false,
  inverse: false,
  strikethrough: false,
  dim: false,
  invisible: false,
  overline: false,
};

function isIndex(value, count) {
  return Number.isInteger(value) && value >= 0 && !(value >= count);
}

function cellEntry(cell) {
  return [cell.row + ":" + cell.column, cell];
}

function indexCells(cells) {
  return new Map(cells.map(cellEntry));
}

function cellLookup(cells, rowCount, columnCount) {
  let index = null;
  function lookup(row, column) {
    if (!isIndex(row, rowCount) || !isIndex(column, columnCount)) return null;
    index = index === null ? indexCells(cells) : index;
    const found = index.get(row + ":" + column);
    return Object.assign({}, DEFAULT_CELL, { row, column }, found || {});
  }
  return lookup;
}

function isTerminalState(state) {
  return state !== null && typeof state === "object" && Array.isArray(state.cells);
}

function adaptScrollback(state) {
  const scrollback = state.scrollback || { lines: [], cells: [] };
  const rowCount = (scrollback.lines || []).length;
  const cell = cellLookup(scrollback.cells || [], rowCount, state.columns);
  return Object.assign({}, scrollback, { cell });
}

function withCellLookups(state) {
  const cell = cellLookup(state.cells, state.rows, state.columns);
  return Object.assign({}, state, { scrollback: adaptScrollback(state), cell });
}

function adaptTerminalState(state) {
  return isTerminalState(state) ? withCellLookups(state) : state;
}

registerStateAdapter(adaptTerminalState);

const ESC = "\u001b";
const CSI = ESC + "[";

export function write(textOrGenerator) {
  return action({ kind: "write", text: textOrGenerator });
}

export function writeBytes(arrayOfNumbers) {
  return action({ kind: "write", bytes: arrayOfNumbers });
}

export function resize(columns, rows) {
  return action({ kind: "resize", columns, rows });
}

export function click(row, column) {
  return action({ kind: "click", row, column });
}

export function scrollToTop() {
  return action({ kind: "scroll_to_top" });
}

export function scrollToBottom() {
  return action({ kind: "scroll_to_bottom" });
}

function writeSequence(sequence) {
  return write(sequence);
}

function keys(sequences) {
  return oneOf(...sequences.map(writeSequence));
}

function codePointRange(first, last) {
  const characters = [];
  for (let codePoint = first; codePoint <= last; codePoint += 1) {
    characters.push(String.fromCodePoint(codePoint));
  }
  return characters;
}

const SAFE_CHARACTERS = [
  ...codePointRange(0x20, 0x7e),
  ...codePointRange(0xc0, 0xff),
  ...codePointRange(0x100, 0x17f),
  ...codePointRange(0x391, 0x3a1),
  ...codePointRange(0x3a3, 0x3a9),
  ...codePointRange(0x3b1, 0x3c9),
  ...codePointRange(0x410, 0x44f),
  ...codePointRange(0x4e00, 0x4e2f),
  ...codePointRange(0x6c34, 0x6c3f),
  ...codePointRange(0xac00, 0xac2f),
  ...codePointRange(0xd55c, 0xd56b),
  ...codePointRange(0x1f600, 0x1f64f),
  ...codePointRange(0x1f680, 0x1f6c5),
  ...codePointRange(0x1f330, 0x1f37f),
];

export const safeCharacters = chars(SAFE_CHARACTERS, { min: 1, max: 8 });

export const safeText = write(safeCharacters);

export const controlKeys = keys(["\r", "\t"]);

export const editingKeys = keys(["\u007f", CSI + "3~", CSI + "2~"]);

export const arrowKeys = keys([CSI + "A", CSI + "B", CSI + "C", CSI + "D"]);

export const modifiedArrowKeys = keys(
  [2, 3, 5, 6].flatMap((modifier) =>
    ["A", "B", "C", "D"].map((direction) => CSI + "1;" + modifier + direction),
  ),
);

export const navigationKeys = keys([CSI + "H", CSI + "F", CSI + "5~", CSI + "6~"]);

export const functionKeys = keys([
  ESC + "OP",
  ESC + "OQ",
  ESC + "OR",
  ESC + "OS",
  CSI + "15~",
  CSI + "17~",
  CSI + "18~",
  CSI + "19~",
  CSI + "20~",
  CSI + "21~",
  CSI + "23~",
  CSI + "24~",
]);

function pasted(text) {
  return CSI + "200~" + text + CSI + "201~";
}

export function bracketedPaste(textOrGenerator) {
  if (typeof textOrGenerator === "string") {
    return write(pasted(textOrGenerator));
  }
  return write(textOrGenerator.map(pasted));
}
