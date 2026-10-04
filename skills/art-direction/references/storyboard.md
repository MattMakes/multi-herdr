# Storyboard: plan pages as sequences of shots

A page is a sequence of shots with a rhythm, not a list of sections. Write the storyboard in Markdown before any markup. Order of work: site grammar, then each page's scene, then each page's signature composition, and only then the shared system (nav, footer, cards, utilities). Shared parts decided first flatten every page into the same shell.

## 1. Site grammar (write once per site)

| Field | What to decide |
|---|---|
| Page shell | How a page is built: full-bleed stage, centered column, sticky rail plus scroll, horizontal track, archive wall, framed viewport. |
| Navigation posture | Hidden until scroll, edge-anchored rail, floating island, plain top bar, index overlay. One choice for the site. |
| Framing discipline | Margins, max widths, bleed rules, which elements may break the frame. |
| Density cadence | The pattern of dense and quiet sections, for example "dense, quiet, dense, spectacle, quiet". |
| Recurring materials | Grain, rules, frames, glows, paper, glass. At most 2 for the site. |
| Allowed composition families | 2 or 3 families from section 5 that fit the direction. |
| What may repeat | Elements allowed on every page (type scale, nav, footer, one motif). |
| What must vary | Elements each page owns (hero posture, signature composition, arc). |

## 2. Page scene (write once per page)

- **One big idea**: the single visual idea a viewer remembers after 3 seconds (monumental type, one object on a stage, a wall of evidence, light through fog).
- **Restraint statement**: what this page deliberately does not do.
- **Hero dominance statement**: one sentence on why the first view commands attention without a generic gradient or a product screenshot in a tilted card.
- **Signature composition**: the one layout move this page owns.
- **Grid fallback test**: what breaks if this page became a generic card grid. If the answer is "not much", redesign the composition.
- **Arc**: 5 to 9 beats from section 3.

Interior pages are new scenes in the same film, not reduced copies of the home page. Each major page role gets its own signature composition.

## 3. Beats

Pick beats, then resolve each beat to a section function (what the section does) and a composition (how it looks). Directions list preferred arcs and forbidden beats.

| Beat | Purpose | Typical functions |
|---|---|---|
| Cold open | Impact with no context | Single number, provocative line, data punch |
| Establishing shot | Mood before message | Full-bleed atmosphere, landscape, world |
| The promise | What the viewer gets if they stay | Question hook, single word, manifesto |
| Prologue | Short context before the story | Origin line, date, short timeline |
| Open from the end | Show the outcome, then rewind | Result, before/after |
| World exploration | Let the viewer browse breadth | Category map, collection, filter |
| The encounter | First deep contact with one subject | Featured story, case study, person |
| Evidence wall | Many proofs at once | Masonry of results, logo wall, data grid |
| Data bombardment | Precision as force | Stats, dashboard, comparison |
| Deep dive | Narrow to one subject | Sticky visual plus scrolling text |
| The tutorial | How it works | Steps, tabs, FAQ |
| Montage | Fast variety | Carousel, marquee, gallery |
| Flashback | History and origin | Timeline, changelog, about |
| The pivot | Tonal shift | Visual break, color change, quote |
| Parallel stories | Several threads at once | Tabs, split columns, multi-panel |
| The authority | Credentials | Awards, press, numbers |
| Community voice | Other people speak | Testimonials, reviews |
| The confrontation | Challenge a belief | Contrarian quote, comparison |
| Quiet moment | Air, no push | Empty band, single image, single line |
| The invitation | Gentle call to act | Newsletter, contact, soft CTA |
| The mission | Urgent call to act | Pricing, waitlist, deadline |
| The farewell | Closing credits | Footer with personality |
| The loop | End echoes the start | Hero motif returns in a new form |
| Black screen | Abrupt stop | Minimal footer, empty space |

Rules: do not default to `hero, features, stats, testimonials, CTA`. If the content lacks a beat's material (no testimonials, no data), replace the beat with another allowed beat; do not invent proof. Keep at least 5 beats on a full page.

## 4. Shots: one entry per section

```markdown
### Shot 3 - Deep dive
- Beat / function: Deep dive / product explainer
- Framing: medium; sticky visual left 5/12, text scrolls right 7/12
- Focal point: the product render, lit from top left
- Supporting elements: 3 numbered annotations, a thin rule
- Motion intent: visual stays pinned; annotations reveal with clip wipe; no hover effects
- Content: 3 real feature statements from the brief
- Why it exists: proves the promise from shot 1 before asking for anything
```

Framing vocabulary: **extreme wide** (full-bleed, subject small in a large field), **wide** (full-bleed with inset text column), **medium** (split or asymmetric, subject and text share the frame), **close-up** (one element fills the frame: one number, one word, one object), **insert** (a detail at small scale between large shots), **letterbox** (wide strip with bars of empty space above and below), **over-the-shoulder** (content seen through a frame: device, window, card edge).

Focal point rule: one per shot. Name it. It must win on size, contrast or isolation (see `perception.md`). Hero shots have at least 3 visual elements beyond plain text; most other shots have 1 or 2. Empty space counts only as a deliberate quiet beat.

## 5. Composition families

| Family | Moves |
|---|---|
| Symmetric | Center-weighted monument, bilateral mirror, centered stack, title card |
| Asymmetric | Rule of thirds, offset column, weighted split (8/4, 7/5) |
| Full-bleed | Edge-to-edge image or color, full-bleed with inset text, panorama strip |
| Split | Clean bisect, diagonal clash, shot/reverse-shot (alternating sides) |
| Layered | Background atmosphere, mid frame, foreground content; text behind object |
| Broken grid | Mosaic, scatter with slight rotation, overlap across columns |
| Horizontal | Tracking scroll (vertical scroll moves a horizontal track), strip carousel |
| Sticky | Sticky rail plus scrolling content, pinned visual with stepping text |
| Typographic | Giant letter background, text as divider, monumental headline, manifesto block |

## 6. Entrance vocabulary

Choose an entrance per shot and write the entrance map before specifying code. Values are starting points; take timing from the direction's motion character.

| Entrance | Implementation sketch | Character |
|---|---|---|
| Fade from black | `opacity 0 to 1`, 1 to 2 s, on a dark ground | Slow, philosophical |
| Push in | `scale(0.92)` to `scale(1)` plus opacity | Arrival, product reveal |
| Rack focus | `filter: blur(16px)` to `blur(0)` plus slight scale | Text sections, dreamy |
| Curtain wipe | `clip-path: inset(0 100% 0 0)` to `inset(0)` | Chapter change |
| Iris | `clip-path: circle(0%)` to `circle(75%)` | Opening, single object |
| Split open | two halves `clip-path` open from the center | Comparison, two columns |
| Tilt up | `perspective(800px) rotateX(5deg) translateY(60px)` to rest | Lists, steady content |
| Crane down | from `translateY(-8vh)` with long ease-out | Headers, establishing |
| Whip pan | fast `translateX(-120%)` to 0, overshoot ease | Energy, action |
| Venetian blind | striped mask that closes | Noir, dramatic |
| Stagger cut | children appear 50 to 80 ms apart | Grids, lists |
| Crossfade | outgoing and incoming overlap 0.5 to 1.5 s | Mood, memory |
| Smash cut | no transition, instant state change | Comedy, shock, brutalist |
| Fade up | `opacity` plus `translateY(12 to 24px)` | Neutral; budget at most 2 per page |

Scroll behaviours (count toward the heavy-interaction budget when pinned or scrubbed): parallax depth layers, pinned subject with moving background, tracking scroll, slow zoom on a full-bleed image, fade to black at section end.

Implementation notes: animate only `transform`, `opacity`, `clip-path` and `filter`; trigger reveals with `IntersectionObserver` or a scroll library, never a raw `scroll` listener; apply blur and grain only to fixed or small layers. If the `motion-gsap` skill is available, use it to implement pinned and scrubbed scenes.

## 7. Reference decomposition

Classify each reference first. Social platforms and feeds (Instagram, Dribbble, Medium-style feeds) are high risk: their surface turns the page into a feed. From high-risk references take only rhythm, density and navigation posture. Brand and campaign sites are medium risk: take framing and restraint. Editorial publications and cultural institutions are low risk.

Assign each reference one role and write it down:

```markdown
- Reference A (rhythm and pacing) contributes: ...
- Reference B (material and surface) contributes: ...
- Reference C (type attitude) contributes: ...
- Will not be copied: section order, hero composition, grid skeleton, palette of ...
```

If a reference conflicts with the chosen direction, say so to the orchestrator and recommend which to keep. Do not blend them silently.

## 8. Storyboard checks

- [ ] Site grammar is written before any page scene.
- [ ] Each page has one big idea, a restraint statement, a hero dominance statement and a signature composition.
- [ ] Each page's grid fallback test names something specific that would break.
- [ ] Beat count is 5 to 9 per full page and no beat is forbidden by the direction.
- [ ] Each shot names framing, one focal point and a motion intent (or an intentional `none`).
- [ ] Entrance map: no two adjacent shots share an entrance; at least 4 distinct entrances on pages with 6 or more shots, unless the direction declares a single quiet entrance; `fade up` at most 2 times.
- [ ] Interaction budget: at most 1 heavy interaction and at most 2 showy reveals per page.
- [ ] At least 2 shots per page are structurally different from default marketing layouts.
- [ ] Home and interior pages do not share a shell with only surface changes.
- [ ] The wireframe differs from every item on the shell-ban list.
- [ ] Image needs are listed per shot (type, subject, treatment, aspect ratio, placeholder label) or marked "no image".
