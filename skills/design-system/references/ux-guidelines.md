# UX and accessibility guidelines

Priority-ordered rules for building and reviewing UI, merged and deduplicated from a 119-rule guideline set, a 10-category quick reference, and an app pre-delivery checklist. Each rule has a check. Fix CRITICAL and HIGH items before any polish.

| # | Category | Priority |
|---|---|---|
| 1 | Accessibility | CRITICAL |
| 2 | Touch and interaction | CRITICAL |
| 3 | Performance | HIGH |
| 4 | Layout and responsive | HIGH |
| 5 | Navigation | HIGH |
| 6 | Style consistency | HIGH |
| 7 | Typography and color | MEDIUM |
| 8 | Motion | MEDIUM |
| 9 | Forms and feedback | MEDIUM |
| 10 | Content, data, and charts | LOW to MEDIUM |

## 1. Accessibility (CRITICAL)

| Rule | Check |
|---|---|
| Text contrast 4.5:1 (large text 3:1); meaningful icons, control boundaries, and focus rings 3:1 | Computed contrast table per mode |
| Visible focus on every interactive element; use `:focus-visible`; never remove an outline without a replacement | Tab through the page; every stop shows a ring |
| Focus is not hidden by sticky headers, banners, or chat widgets | Tab near sticky UI; set `scroll-padding-top` to the header height |
| Tab order follows visual order; no keyboard traps; every action works without a pointer | Complete the main task with the keyboard only |
| Icon-only controls have an accessible name; decorative icons next to text are `aria-hidden="true"` | Accessibility tree or a screen reader lists a name for each control |
| Meaningful images have alt text; decorative images have `alt=""` | Grep `<img` without `alt` |
| Headings go in order (h1 to h6) without skips; landmarks (`header`, `nav`, `main`, `footer`) exist | Heading outline tool or a manual list |
| Skip link to main content on pages with long navigation | First Tab shows "Skip to content" |
| Color is never the only signal (errors, status, chart series) | Grayscale view still conveys the state |
| Honor `prefers-reduced-motion`: remove parallax, scroll-jacking, and non-essential animation; show the final state | Toggle the OS setting and reload |
| Text resizes to 200% and reflows at 320 px width without loss; no clipped text in fixed-height boxes | Zoom 200% and check at 320 px |
| Every drag action has a single-pointer or keyboard alternative | Reorder or resize without dragging |
| Auto-rotating or moving content has pause, stop, and previous/next; it stops on focus and with reduced motion | Carousels have controls |
| Authentication allows paste and password managers; offer a non-cognitive method (passkey, magic link) | Paste into password and code fields works |
| Dynamic changes are announced: one polite live region per concern, full phrases ("3 items in cart") | Screen reader announces without moving focus |
| Help mechanisms (contact, chat) stay in the same place across pages | Compare 3 pages |
| Do not ask users to re-enter information they already gave in the same flow | Review multi-step forms |
| After a route change in a single-page app, move focus to the main heading or region | Navigate with keyboard; focus lands on new content |

## 2. Touch and interaction (CRITICAL)

| Rule | Check |
|---|---|
| Touch targets 44 x 44 pt (iOS) or 48 x 48 dp (Android); web pointer targets at least 24 x 24 CSS px; extend the hit area beyond small icons | Measure in devtools |
| At least 8 px between adjacent targets | Measure |
| Primary actions work by click or tap; never hover-only | Use a touch device or emulation |
| Visual feedback within 100 ms of a press | Watch the press state |
| Buttons disable or show progress during async work; double submits are blocked | Click twice quickly |
| Use `cursor: pointer` on clickable non-button elements; `touch-action: manipulation` removes tap delay | Inspect styles |
| Do not override system gestures (back swipe, pull to refresh where it matters); prefer vertical scroll on main content | Test gestures on a device |
| Swipe actions have a visible affordance and a button alternative | Find each swipe action's button |
| Press states do not shift layout | Press and watch neighbors |

## 3. Performance (HIGH)

| Rule | Check |
|---|---|
| Images: WebP or AVIF, `srcset`/`sizes`, `loading="lazy"` below the fold, explicit `width`/`height` or `aspect-ratio` | Lighthouse image audits |
| Reserve space for async content; CLS under 0.1 | Lighthouse or Web Vitals |
| Fonts: `font-display: swap`, preload only critical files | Network panel |
| Split code by route; lazy-load heavy components and dialogs | Bundle report |
| Third-party scripts `async` or `defer`; remove unused ones | Network panel |
| Virtualize lists over about 50 complex items; debounce or throttle scroll, resize, and input handlers | Profile scroll at 60 fps |
| Skeletons for waits over about 1 s; no spinner flash for near-instant work | Throttle network to "Slow 4G" |
| Offline and slow-network states have messaging and a retry | Devtools offline mode |
| Autoplay video: muted, paused off-screen, with controls and captions; prefer click-to-play | Scroll the video out of view |

## 4. Layout and responsive (HIGH)

| Rule | Check |
|---|---|
| `<meta name="viewport" content="width=device-width, initial-scale=1">`; never disable zoom | View source |
| Mobile first; systematic breakpoints; test 375, 768, 1024, 1440 px (and 320 px for reflow) | Screenshots at each width |
| No horizontal scroll on mobile | `document.documentElement.scrollWidth <= innerWidth` |
| Full-height sections use `min-height: 100dvh`, not `100vh` | Grep `100vh`, `h-screen` |
| Text measure 60 to 75 characters on desktop, 35 to 60 on mobile | Measure a paragraph |
| Consistent max content width (for example 1280 px) and gutters that grow with width | Compare pages |
| A defined z-index scale; no arbitrary large values | Grep `z-index` and `z-[` |
| Fixed headers and bottom bars reserve space for content and respect safe areas | Scroll to page ends |
| Wide tables scroll inside a container or become cards | Check at 375 px |
| Long tokens (URLs, IDs) wrap with `overflow-wrap: anywhere`; flex and grid text children have `min-width: 0` | Paste a 60-character URL |
| Badges and chips keep labels on one line; the collection wraps; hidden overflow is an operable "+n" | Add many chips |

## 5. Navigation (HIGH)

| Rule | Check |
|---|---|
| Navigation stays in the same place on every page; the current location is highlighted with `aria-current` | Compare pages |
| Back behavior is predictable and restores scroll, filters, and input | Navigate away and back |
| Every key screen has a URL (deep link); state that matters lives in the URL | Copy a URL into a new tab |
| Bottom navigation: at most 5 top-level items with icon and label | Count |
| Large screens (1024 px and up) prefer a sidebar; small screens a bottom or top bar; never mix patterns at one level | Review layout per width |
| Breadcrumbs for hierarchies 3 or more levels deep | Check deep pages |
| Modals are not used for primary navigation; every modal and sheet has a visible close | Review flows |
| Dangerous actions (delete account, sign out) are separated from normal items | Review menus |
| Search is reachable from the top bar or a tab; no-results states suggest next steps | Search for nonsense |

## 6. Style consistency (HIGH)

| Rule | Check |
|---|---|
| One style across the product; effects (radius, shadow, blur) follow it | Review 3 screens side by side |
| One icon family and stroke width; no emoji as icons; icon sizes are tokens | Grep emoji ranges in components |
| One primary action per view; secondary actions visibly weaker | Count filled buttons per view |
| Hover, pressed, focus, disabled, and selected states are distinct in both themes | State matrix screenshots |
| One elevation scale | Grep `box-shadow` and `shadow-[` |
| Platform idioms respected on native targets (iOS HIG, Material) | Compare with platform controls |
| Blur only to signal a layer behind a modal or sheet, not as decoration | Review blur usage |
| Official brand assets only, at correct proportions and clear space | Compare with the brand file |

## 7. Typography and color (MEDIUM)

| Rule | Check |
|---|---|
| Body 16 px minimum, line height 1.5 to 1.75 | Inspect computed styles |
| One type scale; headings clearly differ from body in size and weight | Render the scale |
| Tabular figures for numbers in columns, prices, timers | Inspect `font-variant-numeric` |
| Semantic color tokens in components; no raw hex | Raw-value grep (SKILL.md step 8) |
| Dark mode uses tuned tints, not inversion; tested on its own | Contrast table for dark |
| Prefer wrapping to truncation; when truncating, show the full text on focus and hover | Keyboard-focus a truncated item |
| Balanced wrapping (`text-wrap: balance`) for short headings; no forced non-breaking spaces | Resize the window |

## 8. Motion (MEDIUM)

| Rule | Check |
|---|---|
| Animate `transform` and `opacity` only; never `width`, `height`, `top`, `left` | Grep transitions and keyframes |
| Motion shows cause and effect; animate 1 or 2 key elements per view | Review each animation's purpose |
| Shared duration and easing tokens; ease-out to enter, ease-in to exit; exit about 60 to 70% of enter | Token file lists motion values |
| Stagger list entrances by 30 to 50 ms per item | Watch a list mount |
| Animations are interruptible and never block input | Click during an animation |
| Rapid state changes cancel prior motion and set the final state directly; never depend on `animationend` for correctness | Toggle fast 10 times |
| Modals and sheets animate from their trigger; navigation direction is consistent | Open and close overlays |
| Fades do not linger below 0.2 opacity | Watch a fade |
| Continuous animation only for loading indicators, unless the brief asks for ambient motion and a reduced-motion path exists | List looping animations |

## 9. Forms and feedback (MEDIUM)

| Rule | Check |
|---|---|
| Visible label for every input; placeholder is never the label | Grep inputs without `<label>` or `aria-label` |
| Errors appear below the field, say what is wrong and how to fix it, and link with `aria-describedby` | Submit an invalid form |
| After a failed submit with several errors, focus a summary at the top that links to each field | Submit with 3 errors |
| Validate on blur, not on each keystroke | Type in a field |
| Correct `type`, `inputmode`, and `autocomplete` on every field | Inspect fields on mobile |
| Required fields are marked in text | Review labels |
| Submit shows loading, then success or error | Submit on a slow network |
| Destructive actions ask for confirmation or offer undo | Delete something |
| Toasts: polite, do not steal focus, 3 to 5 s, pause on hover and focus | Trigger a toast with a screen reader |
| Empty states explain and offer one action; error states offer recovery | Clear all data |
| Multi-step flows show progress and allow going back; long forms save drafts | Walk a flow |
| Disabled and read-only look different and are announced differently | Inspect both |
| Confirm before closing a sheet or dialog with unsaved changes | Edit and press Esc |

## 10. Content, data, and charts (LOW to MEDIUM)

| Rule | Check |
|---|---|
| Realistic sample content, not lorem ipsum or generic names | Read the screen |
| Locale-aware dates, numbers, and currency | Switch locale |
| Label AI-generated content; stream long AI responses | Review AI surfaces |
| Match chart to data: trend line, comparison bar, part-to-whole bar or donut (5 categories or fewer) | Review each chart |
| Charts have a legend near them, tooltips on hover and focus, and a data table or text summary | Keyboard through a chart |
| Series differ by more than hue (pattern, line style, direct label) | Grayscale view |
| Data marks 3:1 against the background; labels 4.5:1; gridlines subtle | Contrast table |
| Charts have loading, empty, and error states | Break the data source |
| Aggregate or sample over about 1000 points | Load a large set |

## Pre-delivery pass

1. Keyboard-only run of the main task.
2. Contrast table complete for every mode.
3. Screenshots at 375, 768, 1024, and 1440 px in each mode.
4. Reduced motion on: nothing essential lost.
5. Zoom 200%: no clipped or overlapping text.
6. Raw-value and focus-ring greps clean.
7. On native targets: small phone, large phone, tablet, landscape, largest system text size, safe areas respected.
