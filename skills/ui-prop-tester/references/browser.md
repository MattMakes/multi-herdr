# Browser runs

```
ui-prop-tester browser test [OPTIONS] <ORIGIN>
ui-prop-tester browser test-external --remote-debugger <URL> [--create-target] [OPTIONS] <ORIGIN>
ui-prop-tester browser fuzz [OPTIONS] <ORIGIN>
```

`<ORIGIN>` is an `http://` or `https://` URL, or a local file path (turned into a `file://` URL). The tool
starts Chromium with a fresh temporary profile, drives it over the debugging protocol, and removes the
profile and the download folder when the run ends.

## Options

| option | default | meaning |
|---|---|---|
| `--headless` | off | No window. Use it in agent and CI runs |
| `--no-sandbox` | off | For containers where the Chromium sandbox cannot start |
| `--width`, `--height` | 1024, 768 | Viewport |
| `--device-scale-factor` | 1 | Pixel ratio |
| `--allow-url <MATCH_PATTERN>` | starting origin | Where the walk may go; repeat for more. Replaces the default boundary |
| `--header <KEY=VALUE>` | none | Extra request header; repeat for more |
| `--cookie <SET-COOKIE>` | none | Cookie in Set-Cookie form, for example `sid=abc; Domain=localhost; Path=/`; repeat for more |
| `--chrome-grant-permissions <LIST>` | local network access | Comma-separated permissions granted before the first navigation |
| `--chrome-virtual-time-policy <POLICY>` | real time | `advance`, `pause` or `pauseIfNetworkFetchesPending` |
| `--instrument-javascript` | off | Record JavaScript branch coverage per state |
| `--coverage-symbols <FILE>` | none | Write the covered script symbols to this file |
| `--chrome <FILE>` | found on the system | Chromium executable; also `UI_PROP_TESTER_CHROME` |

## Boundary

By default an `http`/`https` walk stays on the starting scheme, host and port. A `file://` walk stays on that
1 file; the query string may change. Links that leave the boundary are not offered, and a navigation that
leaves it is blocked.

`--allow-url` takes Chrome match patterns (`<scheme>://<host>/<path>`, `*` as wildcard) and replaces the
default:

```
--allow-url 'http://localhost:3000/*' --allow-url 'http://localhost:4000/api/*'
```

If the walk ends with `no actions are available`, the page it reached has nothing to click and no history
to go back to, or the boundary hides every link.

## Unattended behaviour

- `alert`, `confirm`, `prompt` and `beforeunload` dialogs are accepted.
- A `debugger` statement is resumed.
- A download goes to a temporary folder, does not block the walk, and is listed in `state.downloads`. The
  file is deleted at the end and is not stored in the trace.
- A navigation that does not settle in 30 seconds stops the run with exit 1.

## Defaults (`ui-prop-tester/browser/defaults`)

| export | kind | rule or actions |
|---|---|---|
| `noHttpErrors` | property | The latest navigation response has a status below 400 |
| `noUncaughtExceptions` | property | No uncaught synchronous exception on the page |
| `noUnhandledRejections` | property | No unhandled promise rejection |
| `noConsoleErrors` | property | No console entry at error level. Warnings do not count |
| `clicks` | generator, weight 100 | Click a visible, in-viewport control |
| `inputs` | generator, weight 100 | Type into the focused field, or press Backspace, Tab, Enter or Escape |
| `scrolling` | generator, weight 50 | Down by up to half a viewport until the bottom, then back to the top |
| `navigation` | generator, weight 10 | Back (when there is a prior entry), forward (same origin), reload |
| `waiting` | generator, weight 1 | Wait once; not twice in a row |

Click candidates come from the main document, same-origin iframes, open shadow roots and slotted content:
links that do not open a new window, enabled buttons, inputs, text areas, labels of enabled controls, and
elements with an interactive ARIA role. Skipped: disabled controls and their labels, file inputs, a focused
text field that already has text, anything outside the viewport, cross-origin iframes.

Typing matches the field: text for text inputs and text areas, an address for email inputs, 1 to 5 digits
for number inputs.

Console entries belong to the current page and are cleared on navigation.

## Builders (`ui-prop-tester/browser`)

| export | gives |
|---|---|
| `click(candidate)`, `doubleClick(candidate)` | Click an entry of `state.clickables` |
| `type(textOrGenerator)` | Type into the focused element |
| `key(name)` | Press a key: `Enter`, `Tab`, `Escape`, `Backspace`, `ArrowDown`, ... |
| `scrollBy(dy)`, `scrollToTop()` | Scroll the page |
| `back()`, `forward()`, `reload()` | History |
| `wait(ms)` | Let time pass, then capture |
| `drag({ from, to, steps, delayMs })` | Primary-button drag between 2 `{ x, y }` points in `steps` moves |
| `resize(width, height)` | Change the viewport |
| `setFiles(selector, paths)` | Put files on the file input that matches the CSS selector |
| `custom(name, args)` | Run a registered action |
| `defineAction(name, asyncFn)` | Register `asyncFn({ document, window, cells, args })`; it runs in the page |
| `resourceGrowth({ metric, window, minimumGrowth })` | A formula that fails on sustained growth |
| `page` | A cell with the whole captured state |

## State seen by extractors

In the page, `state` has `document`, `window` and:

| field | value |
|---|---|
| `url`, `title`, `mime_type` | The current document |
| `viewport` | `width`, `height`, `device_scale_factor` |
| `navigation` | `status` (or `null`), `kind`: `load`, `same_document`, `non_document`, `bfcache` |
| `history` | `index`, `entries[{ url, title }]` |
| `console` | Warning and error entries of this page: `level`, `text`, `url`, `line` |
| `exceptions` | `kind` (`exception` or `rejection`), `message`, `url`, `line`, `column`, `stack` |
| `resources` | `js_heap_used`, `js_heap_total`, `dom_nodes`, `event_listeners`, `layout_objects` |
| `coverage` | `null`, or `branches_new`, `branches_total` with `--instrument-javascript` |
| `scroll` | `x`, `y`, `max_x`, `max_y` |
| `focused` | `null`, or `tag`, `input_type`, `value_length`, `target` |
| `clickables` | `kind`, `tag`, `target` (fingerprint), `point` |
| `downloads`, `last_action` | Downloads so far; the action that led here |

A fingerprint holds what identifies an element: `test_id`, `id`, `role`, `name`, `href`, `form_name`,
`placeholder`, `input_type`, `text`, `path`. Replay matches a recorded click by the first of these that the
element has, so give important controls a `data-testid` or an `id`.

## Patterns

A domain rule from the DOM:

```js
import { always, extract } from "ui-prop-tester";
export * from "ui-prop-tester/browser/defaults";

const cart = extract((state) => {
  const rows = [...state.document.querySelectorAll("[data-testid=cart-row]")];
  const sum = rows.reduce((total, row) => total + Number(row.dataset.price) * Number(row.dataset.quantity), 0);
  const shown = Number(state.document.querySelector("[data-testid=cart-total]")?.dataset.value ?? 0);
  return { sum, shown };
});

export const cartTotalMatchesRows = always(() => cart.current.sum === cart.current.shown);
```

Aim clicks at 1 part of the page:

```js
import { actions, extract } from "ui-prop-tester";
import { click } from "ui-prop-tester/browser";

const toolbar = extract((state) => state.clickables.filter((candidate) => candidate.target.path?.includes("nav")));
export const toolbarClicks = actions(() => toolbar.current.map((candidate) => click(candidate))).weight(30);
```

A custom action for a step that random clicks rarely reach:

```js
import { custom, defineAction } from "ui-prop-tester/browser";

defineAction("login", async ({ document, args }) => {
  document.querySelector("#user").value = args.user;
  document.querySelector("#password").value = args.password;
  document.querySelector("form").requestSubmit();
});

export const login = custom("login", { user: "test", password: "test" }).when(() => !loggedIn.current);
```

A leak check: fail when DOM nodes grew in each of the last 20 states by 500 or more in total:

```js
import { always } from "ui-prop-tester";
import { resourceGrowth } from "ui-prop-tester/browser";
export const noDomLeak = always(resourceGrowth({ metric: "dom_nodes", window: 20, minimumGrowth: 500 }));
```

Bounded sawtooth allocation passes; only growth in every state of the window fails.

## A running browser or Electron app

Start the app with `--remote-debugging-port=9222`, then:

```
scripts/ui-prop-tester browser test-external --remote-debugger http://127.0.0.1:9222 --time-limit 60s <ORIGIN>
```

It attaches to an existing page; `--create-target` opens a new one. The tool never closes a browser it did
not start.

## Looking at a failure

`browser inspect run.trace` serves a local viewer on port 1073 (`--port`, `--no-open`): action list, state
timeline with violation markers, before and after screenshots, cell values, console entries, resource
charts. In an agent session use `--no-open` and read the trace with `trace` instead; see [traces](traces.md).
