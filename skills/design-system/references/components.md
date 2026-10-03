# Component specs

Spec each component as: purpose, anatomy, variants, sizes, a state matrix, tokens, and ARIA. Component tokens point at semantic tokens.

## States

| State | Trigger | Visual change | Semantics |
|---|---|---|---|
| default | none | base | - |
| hover | pointer over (pointer devices only) | one step of lightness or a `/90` opacity fill | - |
| active / pressed | pointer down, key down | one more step; optional `scale(0.98)` | `aria-pressed` for toggles |
| focus-visible | keyboard focus | ring 2 px, offset 2 px, `ring` color, 3:1 | - |
| disabled | not available | 0.5 opacity or `muted` fill, `cursor: not-allowed`, no action | `disabled` or `aria-disabled="true"` |
| loading | async work running | spinner or skeleton in place, width kept, input blocked | `aria-busy="true"`, live status text |
| error | invalid value | `destructive` border, message below, icon | `aria-invalid="true"`, `aria-describedby` |
| selected / checked | chosen | `accent` fill or check mark, not color alone | `aria-selected`, `aria-checked`, `aria-current` |
| read-only | visible, not editable | normal text, no input chrome | `readonly` |

Priority when states combine (highest first): disabled, loading, error, active, focus-visible, hover, default. A focused element in error shows the error ring color.

Transitions: color, background, border, and shadow 150 ms ease-in-out; transform and shadow lift 200 ms ease-out. A state change must never change layout size.

## Button

| Variant | Fill | Text | Border | Use |
|---|---|---|---|---|
| default (primary) | `primary` | `primary-foreground` | none | The one main action per view |
| secondary | `secondary` | `secondary-foreground` | none | Secondary actions |
| outline | transparent / `background` | `foreground` | `input` | Tertiary actions |
| ghost | transparent | `foreground` | none | Toolbars, low emphasis |
| link | transparent | `primary` | none, underline on hover | Inline navigation |
| destructive | `destructive` | `destructive-foreground` | none | Delete, remove; separate it from primary |

| Size | Height | Padding x | Font | Icon |
|---|---|---|---|---|
| sm | 32 px | 12 px | 14 px | 16 px |
| default | 40 px | 16 px | 14 px | 16-18 px |
| lg | 48 px | 24 px | 16 px | 20 px |
| icon | 40 x 40 px | 0 | - | 18 px |

- Anatomy: optional leading icon, label, optional trailing icon. Label in sentence case.
- Icon-only buttons need an accessible name (`aria-label` or visually hidden text).
- Loading: keep the width, replace the icon or overlay a spinner, set `aria-busy`, block repeat submits.
- On touch screens make the hit area at least 44 x 44 px even when the visual is smaller.
- Use `<button>` for actions and `<a href>` for navigation. Never a clickable `<div>`.

## Input

| Size | Height | Padding | Font |
|---|---|---|---|
| sm | 32 px | 8 x 12 px | 14 px |
| default | 40 px | 8 x 12 px | 14-16 px (16 px on mobile to stop iOS zoom) |
| lg | 48 px | 12 x 16 px | 16 px |

| State | Border | Fill | Ring |
|---|---|---|---|
| default | `input` | `background` | none |
| hover | one step darker | `background` | none |
| focus-visible | `ring` | `background` | `ring` 2 px |
| error | `destructive` | `background` | `destructive` 2 px on focus |
| disabled | `border` | `muted` | none |

- Anatomy: visible label above, field, helper text, error text below the field. Placeholder is an example, never the label.
- Use the right `type` and `inputmode`, plus `autocomplete`. Allow paste and password managers.
- Validate on blur, not on every keystroke. After a failed submit, focus an error summary at the top that links to each field, and keep inline errors.
- Mark required fields with text or an asterisk plus visually hidden "(required)".
- Variants with the same states: textarea, select, checkbox, radio, switch. Checkbox and radio hit areas reach 24 px minimum on web.

## Card

| Variant | Elevation | Border | Use |
|---|---|---|---|
| default | `sm` | 1 px `border` | Group related content |
| elevated | `lg` | none | Hierarchy needs lift |
| outline | none | 1 px `border` | Quiet container, dense layouts |
| interactive | `sm` to `md` on hover | 1 px `border` | Whole card is one link or action |

- Anatomy: header (title, description), content, footer (actions). Padding 24 px (16 px compact), gap 16 px.
- An interactive card has one primary link; extend its hit area with a pseudo-element instead of nesting interactive elements.
- In dense layouts prefer dividers or spacing over cards.

## Badge, tag, chip

| Variant | Fill | Text |
|---|---|---|
| default | `primary` | `primary-foreground` |
| secondary | `secondary` | `secondary-foreground` |
| outline | transparent | `foreground`, border `border` |
| destructive / success / warning | status color | status foreground |

Sizes: sm 20 px high, 11-12 px text; default 24 px, 12 px; lg 28 px, 14 px. Radius `full` or `sm` by style.

- Pick the element by meaning: a static status is a `<span>`; a filter is a toggle `<button aria-pressed>`; a removable value has a remove button with a name ("Remove tag: Design").
- Keep a label on one line; wrap the collection, not the label. Replace hidden overflow with an operable "+3" disclosure.
- A changing count is announced as a full phrase ("3 items in cart") in one polite live region.

## Alert and toast

| Variant | Icon | Fill | Border |
|---|---|---|---|
| default / info | info | `muted` or info tint | `border` |
| success | check | success tint | success |
| warning | warning | warning tint | warning |
| destructive | alert | destructive tint | destructive |

- An alert is inline and persistent; use `role="alert"` only for urgent messages. A toast is transient.
- Toasts: polite live region, do not steal focus, 3 to 5 s, pause on hover and focus, include an action such as Undo when one exists.

## Dialog, sheet, alert dialog

| Size | Max width | Use |
|---|---|---|
| sm | 384 px | Confirmations |
| default | 512 px | Standard forms |
| lg | 640 px | Complex forms |
| xl | 768 px | Data-heavy content |
| full | 100% - 32 px | Mobile |

- Anatomy: header (title, description), scrollable body, footer (Cancel left of the confirm action). Close button with a name.
- Focus moves into the dialog, is trapped, Esc closes, and focus returns to the trigger.
- Every dialog has a title (visually hidden if needed). Link a description with `aria-describedby` when there is one.
- Use an alert dialog for destructive confirmation; name the action ("Delete project"), not "OK".
- Sheet: side panel for navigation, filters, settings; set the side explicitly.
- Overlay: `rgb(0 0 0 / 0.5)` or stronger; measure foreground legibility on the real page.

## Table

| Element | Value |
|---|---|
| Cell padding | 12 x 16 px |
| Row height | compact 40 px, default 48 px, comfortable 56 px |
| Header | `muted` fill or bold text, sticky when the table scrolls |
| Hover row | `muted` |
| Selected row | `primary` at 10% plus a checkbox |
| Alignment | text left, numbers right with tabular figures, status center, actions right |

- Use `<table>` with `<thead>`, `<tbody>`, `<th scope>`. Sortable headers are buttons and set `aria-sort`.
- On narrow screens scroll the table inside its container, or switch to stacked cards.
- Provide empty, loading (skeleton rows), and error states.

## Loading, empty, error patterns

- Under about 1 s: no indicator. 1 to 10 s: skeleton that matches the final layout, or an inline spinner for small actions. Over 10 s: progress with text.
- Empty state: what this is, why it is empty, and one action to fill it.
- Error state: what happened, and a recovery action (retry, edit, contact).

## ARIA quick reference

```html
<button disabled>Save</button>
<button aria-busy="true"><span class="sr-only">Saving</span>...</button>
<button aria-pressed="true">Bold</button>
<input id="email" aria-invalid="true" aria-describedby="email-error">
<p id="email-error">Enter an email address like name@example.com.</p>
<div role="status" aria-live="polite">3 items in cart</div>
<a href="/settings" aria-current="page">Settings</a>
```

## Spec template

```markdown
### <Component>
Purpose: <one line>
Anatomy: <parts>
Variants: <name: fill / text / border / use>
Sizes: <name: height / padding / font / icon>
States: default | hover | active | focus-visible | disabled | loading | error | selected
Tokens: --<component>-bg: var(--primary) ...
ARIA and keyboard: <roles, attributes, keys>
Do / Don't: <2 to 4 lines>
```
