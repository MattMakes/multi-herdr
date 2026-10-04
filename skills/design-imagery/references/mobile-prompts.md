# Mobile screen prompts

Use this file to plan and write one image prompt per mobile screen. The images must look like real app screens, not a website squeezed into a phone. Prompts produce images only; implementation starts at step 6 of the skill.

## 1. Choose the platform mode first

Pick one and keep it for the whole set. Do not mix iOS and Android patterns.

| Mode | Bias |
|---|---|
| iOS-native | calm top area, clear tab bar, native sheets, restrained chrome, elegant spacing |
| Android-native | firmer app bar, bottom navigation, stronger list and card rhythm, explicit states |
| Cross-platform neutral | universal navigation, clear safe areas, little platform ornament |

## 2. Count and order the screens

- N screens requested: generate N images.
- Onboarding: 3 or more distinct screens, not one slide repeated.
- Auth: separate sign in, sign up, and recovery when the task needs them.
- App concept with no count: a meaningful flow of 4 to 6 screens.

The order must be a believable journey. For each screen, write the action that leads to the next one. Examples:

- onboarding, auth, home
- home, browse, detail
- cart, checkout, confirmation
- dashboard, activity, detail
- profile, settings, edit profile
- welcome, permissions, personalized home

If a detail is unclear, generate a fresh detail render of that screen. Do not crop an earlier image.

## 3. Lock the app design bible

Write it once; every prompt repeats it. It holds: platform mode, device frame and scale, palette with hex values, type character and scale, spacing unit, corner radius, icon style (one stroke or fill logic), imagery treatment, texture level, navigation model, button style, shadow language.

Direction menu (choose one per line):

- Theme: pristine light; deep dark; soft wellness neutral; premium monochrome; rich accent; editorial luxe; playful consumer color; calm productivity minimal.
- Type: system-like sans; refined grotesk; expressive display with clean body; soft humanist sans; sharp product sans.
- Structure: list-led utility; card-led modular; dashboard overview; media-led story; profile-led; commerce browse and detail; chat-led; calm wellness blocks.
- Imagery: editorial photography; cinematic lifestyle; soft illustration; tactile abstract; product imagery; photo with vector accents; atmospheric backdrops; light collage.
- Texture: subtle grain; matte paper; gradient fog; noise wash; blurred image haze; flat with one textured hero area; tactile monochrome; low-opacity pattern.
- Palette logic: monochrome plus one accent; warm neutral with dark contrast; cool mineral with one highlight; cream, charcoal, muted accent; dark base with warm accent; soft wellness, low saturation; bright consumer, balanced; desaturated with one bold hit.
- Signature components (choose 4): hero metric card, stat strip, collection grid, media carousel, layered profile header, segmented control, bottom action sheet, product card stack, progress ring, message bubbles, settings group cells, photo card strip, sticky mini player, collection shelf, habit tracker, checkout summary, journal card, achievement row.
- Decorative assets (choose 2, keep restrained): line icon cluster, orbit lines, dotted arcs, starburst motif, sticker accent, arrow system, fine grid, waveform line, badge glyphs, geometric markers.

Category bias:

| Category | Push toward |
|---|---|
| Fintech | trust, calm spacing, clear numbers, transaction clarity, no chart spam |
| Health, fitness | metric hierarchy, readable progress, airy spacing, optimistic imagery |
| Productivity | list and card discipline, simple navigation, task hierarchy |
| Social | feed and profile rhythm, media moments, clear create versus browse |
| Commerce | browse, detail, cart clarity; strong product imagery; stable card ratios |
| Wellness, lifestyle | soft materials, calm type, tactile backgrounds, soft fades |

## 4. Screen rules

- First screen: one focal point, 1 to 3 short lines of headline, one clear next action, no chips, stats, or pills.
- Safe areas: keep critical UI out of the status bar, home indicator, and gesture zones. Leave room for the tab bar and sheet docking.
- Navigation: tab bar for top-level sections, stack feel for drill-down, sheets for secondary tasks, segmented controls for local switching. Do not overload the bottom bar.
- Layout: fewer, clearer containers; no card in a card in a card; no 12 widgets fighting on a home screen.
- Text: if it looks small, the screen is not finished. Split content to another screen before you shrink type. Body text must read at normal phone viewing size.
- Image behind text: use a bottom-to-top fade, side mask, or soft scrim. Raw images under text fail.
- Media frames: stable aspect ratios and one radius logic for repeated media.
- Icons: one consistent custom-feeling set; not a default line-icon pack with mixed weights.
- Vary across screens: top composition, image-to-text balance, density, CTA placement. Keep identity, design system, and spacing logic fixed.

## 5. Device frame

- Default: a clean, visible phone frame (iPhone-style for iOS or neutral, Android-style for Android). The content stays the hero.
- Use one device model and one scale across the set.
- Keep even margins on all 4 sides of the canvas. The phone never touches an edge. Shadows stay soft.
- Several phones in one image: same scale, equal gutters, aligned, no random overlap.
- For extraction images (step 5 of the skill), render the screen flat without a frame at a known width (390 px for iOS, 412 px for Android) so measurements are direct.

## 6. Prompt template

```
Mobile app screen design reference, screen N of M, one screen only.
Screen: <name> - reached from <previous screen> by <action>; leads to <next screen>.
Platform: <iOS-native | Android-native | cross-platform>. Canvas: portrait, <framed in a clean <device> mockup with even margins | flat screen 390 x 844 px>.
Design bible (same for every screen): palette <hex list>; type <character>, readable mobile scale; spacing unit <n> px; radius <value>; icons <style>; imagery <treatment>; texture <level>; navigation <model>.
Layout: <top area>, <main content>, <primary action placement>, <navigation bar state>.
Copy (render exactly, large and readable): title "<text>"; body "<text>"; button "<text>".
Imagery: <subject, frame, fade or mask treatment>.
Respect status bar, safe areas, and home indicator.
Exclude: purple-blue fintech gradients, random glass cards, blobs, fake chart dashboards, pills and badges, tiny labels, placeholder brands, website-style hero sections.
```

## 7. Regenerate when

Text is small; navigation looks fake; the screen reads as a website; cards nest; the first screen is noisy; onboarding screens repeat; image framing or device framing is uneven; the palette is muddy or generic; the set drifts into a different design system.
