# Direction contract template

Copy the block below to the project as `DESIGN-DIRECTION.md` and replace every `<placeholder>`. Keep the filled file under about 150 lines: builders read it before every page. Put the full storyboard in a separate file (for example `design/storyboard.md`) and link it.

````markdown
# Design direction: <project name>

Status: <draft | active | superseded> - Revision <n> - <date>
Owner: <role that holds the direction>
Storyboard: <path to the storyboard file>

## Brief
<At most 10 lines: subject, audience, the one feeling to leave, page roles, missing content, hard constraints.>

## Direction
- Direction: <direction id from the catalog, and one borrowed trait if any>
- Why: <one sentence naming a concrete visual quality that fits this brief>
- Research: <film, references or sources used; "inferred, no web access" if so>
- Big idea for the site: <one sentence>

## Shell-ban list
- <trait from previous work or common defaults that must not appear>
- <...at least 3>

## Tokens
```css
:root {
  /* color roles */
  --color-bg: <hex>;
  --color-surface: <hex>;
  --color-text: <hex>;
  --color-text-muted: <hex>;
  --color-border: <hex>;
  --color-accent: <hex>;
  /* type */
  --font-display: <stack>;
  --font-body: <stack>;
  --font-mono: <stack>;
  --step-0: <body size>;
  --type-ratio: <ratio>;
  /* space, shape */
  --space-unit: <4px | 8px>;
  --radius: <value>;
  --border: <value>;
  --shadow: <value or none>;
  /* motion */
  --dur-ui: <ms>;
  --dur-scene: <ms>;
  --ease-enter: <cubic-bezier>;
  --ease-exit: <cubic-bezier>;
}
```

Contrast pairs:
| Text | Background | Ratio | Use |
|---|---|---|---|
| <token> | <token> | <n.n>:1 | <body / large / non-text> |

## Typography
- Display: <family, weights, sizes, tracking, case>
- Body: <family, size, line height, max width>
- Labels and data: <family, size, tracking, case>

## Imagery
- Type: <photography / illustration / 3D / none>
- Treatment: <filter, crop, aspect ratios, color grade>
- Placeholder rule: <how placeholders are labelled until real assets exist>

## Motion character
- Entrance vocabulary: <entrances allowed in this direction>
- Heavy interaction: <the one allowed per page, or none>
- Hover and press: <behavior>
- Reduced motion: <fallback>

## Site grammar
- Page shell: <...>
- Navigation posture: <...>
- Density cadence: <...>
- Recurring materials: <at most 2>
- May repeat: <...>
- Must vary per page: <...>

## Pages
| Page | Big idea | Signature composition | Arc (beats) | Heavy interaction |
|---|---|---|---|---|
| <page> | <...> | <...> | <beat, beat, ...> | <... or none> |

## Do
- <3 to 6 concrete moves builders must make>

## Do not
- <3 to 8 banned moves, each checkable>

## Checks
- <command or test, for example: rg -n '#[0-9a-fA-F]{6}' src/ shows only token files>
- <grayscale wireframe test against the shell-ban list>
- <contrast pairs above all pass>
- <entrance map: no adjacent repeats>

## Change log
| Rev | Date | Change | Reason | Approved by |
|---|---|---|---|---|
| 1 | <date> | Initial direction | <brief> | <role> |
````

## Filling rules

- Use real values only. A token without a value, or a value of "TBD", fails the contract check.
- Copy token defaults from the direction file, then adapt to the brand. Record the reason for any change to a direction default in the change log.
- "Do not" items must be checkable by a search, a measurement or a screenshot comparison. "Avoid generic design" is not checkable; "no `border-radius` above 0 on any element" is.
- When the direction changes after builds started, add a revision and list the pages that must be updated.
