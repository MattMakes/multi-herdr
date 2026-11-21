# Specifications: properties, cells and generators

A specification is an ES module: `.js`, `.mjs`, or `.ts` (type annotations are stripped; keep TypeScript
simple: annotations, interfaces, type aliases, `as` casts). With no `--specification`, the target's defaults
module is the specification.

The tool reads the module's exports:

- an exported formula is a **property**, named by its export identifier;
- an exported generator is an **action category**, named by its export identifier;
- any other export is ignored.

## Modules

| import from | holds |
|---|---|
| `ui-prop-tester` | `extract`, formulas, generators |
| `ui-prop-tester/browser` | browser action builders, `defineAction`, `resourceGrowth`, the `page` cell |
| `ui-prop-tester/browser/defaults` | default browser properties and generators |
| `ui-prop-tester/terminal` | terminal action builders and key sets |
| `ui-prop-tester/terminal/defaults` | default terminal properties and generators |

Keep all defaults: `export * from "ui-prop-tester/browser/defaults";`. Keep some:
`export { noConsoleErrors, clicks } from "ui-prop-tester/browser/defaults";`. A default you do not re-export
is off.

## Cells

`extract(fn)` returns a cell. `fn(state)` runs once per captured state and must be pure and return a value
that JSON can hold. `cell.current` is the value in the current state.

- An extractor that reads another cell throws `an extractor cannot read another extractor's cell`. Compute
  from `state` only.
- An extractor that throws ends the run with exit 1 and prints its JavaScript stack.
- Terminal extractors run in the tool. Browser extractors run in the page: the function is sent as text, so
  it cannot use variables or imports from the module around it.
- Export a cell to see it by name in traces, reports and the `cells` of custom actions. A cell that is not
  exported shows as `cell#N`.

## Formulas

| formula | holds when |
|---|---|
| `now(fn)` | `fn()` is true in this state. `fn` may return a formula; it is then checked from this state on |
| `always(f)` | `f` holds in this state and every later one. No counterexample by the end of the run means it held |
| `eventually(f)` | `f` holds in this state or a later one. Still waiting at the end of the run means it failed |
| `next(f)` | `f` holds in the next captured state. With no next state (the run ended) it is not a violation |
| `always(f).within(n, unit)` | `f` holds in every state for the next `n` units |
| `eventually(f).within(n, unit)` | `f` holds in some state before `n` units pass |
| `and(...fs)`, `or(...fs)`, `not(f)`, `implies(a, b)` | the usual logic |

`unit` is `"milliseconds"` or `"seconds"`, measured on state timestamps. A plain function where a formula is
expected is `now(fn)`.

A state is captured after each action, so "time" passes only when actions happen. To give a slow UI time to
settle, include a wait action (browser `waiting` default, or `wait(ms)`).

### Patterns

Safety (never):

```js
export const noStackTraceOnScreen = always(() => !screenText.current.includes("Traceback"));
```

Response (if A then later B, with a deadline):

```js
export const submitGivesFeedback = always(
  implies(() => submitted.current, eventually(() => feedbackShown.current).within(3, "seconds")),
);
```

Relating a past value to a later state. The outer function runs now and keeps `before`; the inner one runs
later:

```js
export const totalNeverDrops = always(now(() => {
  const before = total.current;
  return next(() => total.current >= before);
}));
```

Stability (once B, B stays):

```js
export const loginSticks = always(implies(() => loggedIn.current, always(() => loggedIn.current)));
```

A violation report names the property, the state index, the path through the formula (for example
`always > now > next > now`) and the value of every cell the failing path read. A value kept in a closure
from an earlier state shows with its state index, for example `total@4`.

## Generators

Every generator has `.map(fn)`, `.filter(fn)`, `.flatMap(fn)`, `.when(condition)` and `.weight(n)`.
`condition` is a boolean, a cell (its `.current` is read) or a function. `weight` is a nonnegative integer
(default 1) used wherever the generator sits in a choice.

Values:

| generator | gives |
|---|---|
| `constant(value)` | that value |
| `from(array)` | 1 element |
| `integer(min, max)`, `float(min, max)` | a number in the range, ends included |
| `regex(patternOrRegExp)` | a string that matches |
| `email()` | an address-shaped string |
| `text({ min, max })` | Unicode text with a length in the range |
| `chars(characters, { min, max })` | text from those characters only |

Actions:

| generator | gives |
|---|---|
| `action(template)` | 1 action. `template` is an object with a string `kind`; any field may be a generator |
| `oneOf(...generators)` | 1 of them, by weight |
| `weighted([[weight, generator], ...])` | 1 of them, by the given weights |
| `actions(fn)` | `fn()` runs in every state and returns a generator, an array of them, or nothing |

The driver modules give builders for every action kind (`click`, `type`, `write`, ...), so `action(...)` is
rarely needed directly.

All exported generators form 1 weighted choice. A generator that offers nothing in a state (a false `when`,
an `actions` that returned nothing, weight 0) is left out. If nothing is left, the run stops with exit 1:
`no actions are available in state N`.

### Patterns

Offer an action only when it is valid:

```js
export const submit = actions(() => click(submitButton.current)).when(() => formValid.current);
```

Bias the walk:

```js
export const editing = weighted([
  [8, type(text({ min: 1, max: 12 }))],
  [1, key("Backspace")],
  [1, key("Enter")],
]).weight(50);
```

Inputs with a shape:

```js
export const postcode = type(regex("[A-Z]{1,2}[0-9]{1,2} [0-9][A-Z]{2}")).when(() => focusedName.current === "postcode");
export const quantity = type(integer(0, 999).map(String)).when(() => focusedName.current === "qty");
```

Every random choice comes from the run's seed.

## Checklist before you run

1. Each property is exported, and its name says the rule.
2. Each extractor reads only `state`.
3. Each `eventually` has a `.within`, or you accept that it fails only at the end.
4. Guards (`when`) cannot all be false in a reachable state, or the run ends with the no-actions error.
5. The defaults you want are re-exported.
