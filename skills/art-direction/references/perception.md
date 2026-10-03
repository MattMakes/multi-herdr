# Perception: hierarchy, attention, grouping, and measurable floors

A direction sets taste. Perception sets limits that every direction keeps. Use the principles to place things; use the tables as defaults and floors. A direction file may change a default; it may not go below a floor (marked **floor**).

## Visual hierarchy

The eye ranks elements by how much they differ from their surroundings. The levers, strongest first: size, contrast (luminance more than hue), weight, isolation (empty space around one element), position (top and left in left-to-right scripts), color saturation, motion.

- Plan 3 levels per view: one primary (the focal point), a small set of secondary elements, everything else tertiary. Four or more competing levels read as noise.
- Make the step between levels large. A heading at 1.1 times body size is not a level. Use a type scale ratio of at least 1.25 for product UI and 1.333 to 1.618 for marketing and editorial pages.
- Use one lever strongly rather than several weakly. A large light-weight headline is clearer than a medium bold colored underlined one.
- Primary action: one per view. Secondary actions lose contrast (outline or text style), not only size.
- Check: blur a screenshot by about 8 px (the squint test). Exactly one element must still stand out first, and a second level must still be readable as a group.

## Attention

- Viewers scan before they read. Text-heavy pages are scanned in an F shape (down the left edge, across the first lines); sparse pages are scanned in a Z shape (top left, top right, diagonal, bottom). Place the focal point and the primary action on the scan path.
- An isolated element is remembered (isolation effect). Use it for the one thing that must be remembered, and only once per view.
- Motion captures attention before everything else. Every moving element competes with the focal point, which is why the interaction budget exists. Ambient motion must be slow and low in contrast.
- Faces and eyes draw the gaze; a face looking toward the headline leads the eye to it.
- The first view decides whether the viewer stays. It must state what this is and why it matters without a scroll, even on a 390 x 844 screen.
- Check: the 3-second test. Show the first view for 3 seconds; the reviewer names the subject and the focal point.

## Grouping

The eye groups by these cues, strongest first: common region (inside one box or band), proximity, similarity (shape, color, size), continuity (alignment along a line), closure, figure and ground.

- Proximity first, boxes second. Space inside a group is smaller than space between groups. Aim for a ratio of at least 1:2 (for example 16 px inside, 32 px or more between).
- Do not give every group a box. A card around everything removes the contrast that common region gives. Use one cue per grouping level.
- Align to few edges. Every new left edge is a new group in the viewer's mind.
- Items that look the same are read as the same kind. Make different kinds of thing look different.
- Check: in a grayscale screenshot, a reviewer can draw the group outlines without seeing borders.

## Typography

| Property | Default | Note |
|---|---|---|
| Display letter spacing (56 px and up) | -0.03 em | Large counters look open; tighten |
| Heading letter spacing (32 to 48 px) | -0.015 em | |
| Body letter spacing (14 to 18 px) | 0 | Faces are drawn for this size |
| Caption letter spacing (10 to 12 px) | +0.015 em | Small counters close up; open |
| Uppercase labels | +0.05 to +0.1 em | Capitals need air |
| Body line height | **floor** 1.5 | WCAG 1.4.12 |
| Heading line height | 1.0 to 1.2 | Display type may go to 0.85 in dense directions |
| Body line length | 45 to 75 characters, 65 ideal | Use `max-width: 65ch` |
| Smallest text | **floor** 12 px for reading, 10 px for labels only | |

## Color

| Property | Value | Note |
|---|---|---|
| Body text contrast | **floor** 4.5:1 | WCAG AA |
| Large text contrast (24 px, or 18.66 px bold) | **floor** 3:1 | |
| Non-text contrast (icons, borders of inputs, focus rings) | **floor** 3:1 | WCAG 2.2 SC 1.4.11 |
| Disabled opacity | 40% | 50% competes with active elements |
| Hover lightness change | at least 8% | Smaller steps are invisible on many screens |
| Body text on dark grounds | off-white such as `#E2E8F0` or `#EAEAEA` | Pure white body text on near-black halates; headlines may be white |
| Pure black grounds | avoid; use `#050505` to `#121212` | Except where the direction asks for it |
| Accent colors | 1, at most 2 | More accents remove the meaning of accent |

Contrast check: compute the WCAG ratio for each pair in the contract with any contrast tool or formula (relative luminance L = 0.2126 R + 0.7152 G + 0.0722 B on linearized channels; ratio = (L1 + 0.05) / (L2 + 0.05)). Record the ratio next to each pair.

## Motion

| Property | Default | Note |
|---|---|---|
| Shortest perceptible duration | 100 ms | Below this, change state without animation |
| UI feedback (hover, press, toggle) | 150 to 300 ms | **floor** for responsiveness: start within 100 ms of input |
| Menus, popovers, dialogs | 200 to 400 ms | |
| Scene entrances | direction-specific, at most 1200 ms | Never block input while they run |
| Enter easing | `cubic-bezier(0, 0, 0.2, 1)` or a stronger ease-out such as `cubic-bezier(0.16, 1, 0.3, 1)` | Arriving things decelerate |
| Exit easing | `cubic-bezier(0.4, 0, 1, 1)` | Leaving things accelerate |
| Move within the screen | `cubic-bezier(0.4, 0, 0.2, 1)` | |
| Reduced motion | **floor**: honor `prefers-reduced-motion: reduce` | Replace movement with an instant state or a short opacity change |
| Flashing | **floor**: no more than 3 flashes per second | WCAG 2.3.1 |

## Spacing and targets

| Property | Value | Note |
|---|---|---|
| Touch target, mobile | **floor** 44 x 44 px | Extend the hit area, not the icon |
| Pointer target, desktop | **floor** 24 x 24 px | WCAG 2.2 SC 2.5.8 |
| Primary button height | 40 to 48 px | |
| Compact button height | 32 px | Dense UI only |
| Spacing scale | a 4 or 8 px base, used without exceptions | Off-scale values are drift |
| Focus ring | visible, 2 px offset, radius = component radius + 2 px | **floor**: never remove focus without a replacement |
| Card radius in product UI | 8 px or less by default | Directions may change it for marketing surfaces |
| Nested radius | inner radius = outer radius - padding | Keeps curves concentric |

## Icons

| Property | Value |
|---|---|
| Smallest icon | 16 x 16 px |
| Stroke | 1.5 px standard, 2 px emphasis; one stroke weight per set |
| Optical alignment | Move round icons 1 px down from the box center |
| Label | Every icon-only control has an accessible name |
