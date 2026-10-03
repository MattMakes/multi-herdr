# Sections: page patterns, section patterns, layout rules

Load at workflow step 3. Pick one page pattern, then build its sections from the section catalog. Adapt the order to the brief; the pattern is a start, not a template.

## 1. Section jobs

Every section does exactly one of these jobs. A section without a job is cut.

| Job | Answers the visitor's question | Typical sections |
|---|---|---|
| Hook | What is this, and is it for me? | Hero |
| Proof | Why should I believe you? | Logo wall, testimonials, results, ratings, press |
| Explain | How does it work, what do I get? | Features, how it works, demo, use cases, comparison |
| Objection | What could go wrong? | FAQ, security, guarantee, pricing detail, migration |
| Convert | What do I do now? | Pricing, final CTA, form, waitlist |
| Orient | Where am I, where else can I go? | Nav, footer |

Default order: Hook, Proof, Explain, Proof, Objection, Convert. Put proof directly before each conversion point.

## 2. Page patterns

Choose by the primary action and the proof you have. "CTA" is where the primary CTA appears.

| Pattern | Section order | CTA | Use when | Watch for |
|---|---|---|---|---|
| Hero, features, CTA | Hero, value strip, 3-5 features, CTA, footer | Hero and end | General SaaS or product with a clear single action | Do not make the features 3 identical cards |
| Social-proof led | Hero, problem, solution, testimonials, CTA | Hero and after proof | Trust is the main barrier (health, finance, services) | Verified quotes only, with name and role |
| Product demo | Hero, demo or video, feature breakdown, comparison (optional), CTA | Beside and after the demo | The product explains itself when seen | Captions, poster frame, no autoplay under reduced motion |
| Minimal single column | Headline, short description, 3 benefit bullets max, CTA, footer | Center, large | One simple offer, newsletter, indie product | No nav clutter; real visual still needed |
| Problem, solution, action | Hero, problem, solution, action, CTA | A small CTA per step, main CTA last | A considered purchase that needs reasoning | Do not label steps "Step 1, 2, 3"; name them |
| Comparison led | Hero, problem, comparison table, pricing (optional), CTA | Under the table | Buyers compare you with named alternatives | Factual claims only; highlight your column |
| Lead magnet | Benefit headline, magnet preview, form with minimal fields, submit | The form | Ebook, checklist, report, course | Ask only for fields needed to deliver |
| Pricing led | Value hero, 2-4 plan cards, feature comparison, FAQ, CTA | Every card and the end | Pricing is the decision | Show real monthly and annual totals |
| Scroll story | Hook, problem chapter, journey chapter, solution chapter, CTA | End of each chapter and the climax | Brand, agency, mission or cause pages | Must read completely with motion off |
| Waitlist or launch | Hero with form, teaser, benefits of early access, form again | Form above the fold, sticky form | Pre-launch | No fake scarcity; show a dated, verified count or none |
| Event or webinar | Hero (date, time zone, place), speakers, agenda, sponsors, register | Sticky register, after speakers, end | Conference, meetup, webinar | Deadline as text; pause any countdown motion |
| App download | Hero with device frame, screenshots, features, ratings, store buttons | Store buttons throughout | Mobile app | Real screenshots; current verified ratings only |
| Trust and authority | Mission hero, proof (logos, certifications), solutions by role or industry, contact sales | Contact sales, nav | Enterprise, legal, B2B services | Path selection ("I am a ..."); low-friction form |
| Portfolio | Name and role, project grid, about, contact | Project cards, footer | Personal or studio work | Visuals first; fast image loading |

### Starting pattern by product family

| Product family | Start with |
|---|---|
| B2B SaaS, productivity, dev tools | Hero, features, CTA; or product demo |
| Indie or micro SaaS | Minimal single column plus demo |
| AI product | Product demo with a live "try it" input |
| Fintech, insurance, banking, legal | Trust and authority, with social proof |
| Healthcare, clinics, care, childcare | Social-proof led, with trust |
| E-commerce, DTC product | Hero-led product showcase with ratings |
| Agency, studio, nonprofit, brand | Scroll story |
| Restaurant, hotel, venue, local service | Hero-led with one booking or call CTA |
| Course, bootcamp, community | Feature showcase plus social proof |
| Event, conference, webinar | Event or webinar |
| Newsletter, creator | Minimal single column with sample issue link |
| Public sector | Minimal single column; the design system of the service if it has one |

## 3. Section catalog

### Nav
- One line at 1024 px and wider. At most 80 px high (64-72 px default).
- Logo, 3-5 links, the primary CTA. On mobile, the CTA stays visible outside the menu.
- A sticky nav must not cover a focused element or an anchor target (`scroll-margin-top`).

### Hero
See `hero.md`. The hero holds the value proposition and the CTA only.

### Logo wall
- Directly under the hero. Real logos you are allowed to show, as SVG, in one color that works on the page theme.
- Logos only: no category labels under them. Brand name goes in `alt`.
- Invented brand for a demo: a simple generated mark, marked `TODO`.
- 4-8 logos. A marquee only when there are more than 8, with a pause control.

### Features
- 3-6 items. Each item: a benefit headline (at most 8 words), one sentence (at most 25 words), one visual.
- Do not use a row of 3 identical icon cards. Use one of: asymmetric bento, alternating split rows (at most 2 in a row), a tabbed demo, a sticky visual with scrolling text, a numbered narrative without "Step" labels.
- Bento: exactly as many cells as items. At least 2 cells carry a real visual (image, product crop, pattern, tinted fill).

### How it works
- 3-4 stages named by the action ("Connect", "Configure", "Ship").
- Each stage shows the product state at that stage, not an icon.

### Testimonials
- 1-3 quotes. Each quote at most 3 lines, with name, role, company and a real photo or none.
- One strong quote beats a carousel. If you use a carousel: previous, next and pause controls, keyboard access, no autoplay under reduced motion.

### Results and stats
- Numbers from the brief or a dated source only. State the unit and the period ("38% fewer tickets in 90 days").
- 2-4 numbers. Plain layout, no filled progress tracks.

### Comparison
- Your product plus 2-3 named alternatives or "the old way". Highlight your column.
- Rows are outcomes, not internal feature names. At most 8 rows; link to the full table.
- On mobile: a stacked card per competitor or a horizontally scrollable table with a sticky first column.

### Pricing
- 2-4 plans. Recommend one plan for the main audience. Each card has its own CTA.
- Monthly/annual toggle shows the real price and the real saving. Taxes and limits in plain text.
- Put the FAQ directly after pricing.

### FAQ
- 4-8 real objections from the brief read, answered in at most 3 sentences each.
- Use `<details>`/`<summary>` or an accessible accordion. The first answer may be open.

### Final CTA
- Restate the value proposition in new words, the same CTA label, and one risk reducer (free trial length, cancel terms, guarantee) if true.

### Form
- Labels above inputs, never placeholder-as-label. Helper text under the label, error text under the input.
- Only the fields you need. Correct `type`, `autocomplete` and `inputmode`.
- Loading, success and error states are built and visible in the review.

### Footer
- Links the visitor expects: product, pricing, docs or help, legal, contact. One contact path.
- No version strings, build numbers, locale or weather strips.

## 4. Layout rules (checkable)

| Rule | Check |
|---|---|
| Layout variety | At least 4 layout families on a page with 7+ sections; each family at most once (hero excluded). |
| Split-row cap | At most 2 consecutive image-text split sections. |
| Eyebrow restraint | Small uppercase labels above section headings: at most 1 per 3 sections. Count them. |
| No number eyebrows | No "01 /", "002 ·", "Step 1" labels. |
| Section header | Headline above body, body at most 65ch. No small paragraph floating in the opposite column. |
| Content shape | Default section: headline at most 8 words, body at most 25 words, one visual or one CTA. |
| Long lists | More than 5 items: group, use a grid, tabs, or a disclosure. Not a long divided list. |
| Theme lock | One page theme. Tints within the theme are fine; inverted sections are not, unless the brief asks for one deliberate theme switch. |
| Cards | Use a card only when elevation shows real hierarchy; otherwise group with space or a single rule. |
| Shadows | Tinted to the background hue; never pure black on light. |
| Mobile | Every multi-column section states its below-768 px layout. No horizontal scroll at 320 px. |
| Container | One max content width (for example 1200-1280 px) with consistent side padding. |
| Spacing | One spacing scale (4 or 8 px base). Section vertical padding from the same scale. |
