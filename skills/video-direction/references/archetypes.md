# Narrative archetypes

5 archetypes cover launch and explainer videos. Pick 1. Its duration range
must contain the target duration from `BRIEF.md`. Its format label must
match `Format` in `BRIEF.md`.

Each archetype has 1 format label: `short` or `long`. Archetypes 1 to 4 are
`short`. Archetype 5 is `long`. Never use a `short` archetype for a `long`
film. Never use a `long` archetype for a `short` film.

Each archetype gives a beat list. A beat here is a story step, not a music
beat. The time column is a share of the total duration. For a `short`
archetype, scale it to 15 s, 30 s, 60 s or 90 s. For the `long` archetype,
scale it to 180 s to 900 s. Round each beat to whole scenes of 2 s to 8 s
(the plan checker limit) and snap cuts to the music beat grid.

Scene kinds in the tables are the `timeline.json` kinds: `hook`, `capture`,
`motion`, `character`, `diagram`, `callout`, `cta`.

## Hook patterns

Every archetype opens with a hook scene that starts at 0.0 s. Motion and a
headline land before 3.0 s. Pick 1 pattern. The patterns serve the `short`
archetypes. The `long` archetype uses a cold open and the opening rules
below.

| Pattern | First 3 s | Fits |
|---|---|---|
| Pain first | Show the problem as the viewer meets it: a red CI run, a 40-step setup, a wall of logs. Headline names the pain in 3 to 5 words. | Launch, walkthrough |
| Result first | Show the end state before the steps: the finished dashboard, the green build, the 1-line output. Headline states the gain. | Launch, social |
| Number | 1 large number that moves (counts up, collapses, splits). Headline says what it measures. | Launch, explainer |
| Question | 1 question the audience asks already, as kinetic text on a clean plate. Answer arrives in the next scene. | Explainer, social |
| Contrast cut | 2 states side by side or as a hard cut: before and after, slow and fast. | Launch, social |
| Live command | A terminal types 1 command and prints a short, striking result. | Walkthrough |

Hook rules (`short`):

- No logo intro. The logo appears in the CTA scene, or as a small fixed
  mark.
- No dark or empty first frame unless `meta.allow_dark_intro` is `true`.
- The headline is the scene `caption` and has 7 words or fewer.
- Set `motion.reveal_sec` to the second when the motion and headline have
  both landed. It must be below 3.0 s.

Opening rules (`long`):

- Start with a cold open in motion. The first frame moves. The cold open
  shows the best moment or the core promise of the title.
- A studio intro of 5 s or less may follow the cold open. It never comes
  first.
- The first 30 s prove the promise of the title and the thumbnail. The
  viewer sees the thing the title names within 30 s.
- No dark or empty first frame unless `meta.allow_dark_intro` is `true`.
  This rule applies to both formats.

## 1. SaaS product launch (`short`, 30 s to 60 s)

Goal: the viewer knows what the product does and wants to try it.
Best hooks: pain first, result first, contrast cut.

| Beat | Share | Kind | Content |
|---|---|---|---|
| Hook | 0% to 8% | `hook` | Pain or result in 1 image and 1 headline. |
| Problem | 8% to 20% | `motion` or `character` | Why the old way hurts. 1 concrete cost (time, money, errors). |
| Reveal | 20% to 30% | `callout` | Product name and 1-line promise. |
| Feature 1 | 30% to 48% | `capture` | The core action, live in the product. |
| Feature 2 | 48% to 64% | `capture` or `diagram` | The 2nd reason to care. Change framing from feature 1. |
| Proof | 64% to 82% | `callout` or `motion` | 1 number, a quote or a before/after. |
| CTA | 82% to 100% | `cta` | 1 action, 1 URL, logo. Hold for 2 s or more. |

At 30 s, merge feature 2 and proof. At 60 s, add a 3rd feature beat only if
it supports the core takeaway.

## 2. CLI or developer tool walkthrough (`short`, 30 s to 90 s)

Goal: the viewer can picture running the tool on their own project.
Best hooks: live command, pain first.

| Beat | Share | Kind | Content |
|---|---|---|---|
| Hook | 0% to 7% | `hook` | 1 command and its striking output, or the pain it removes. |
| Install | 7% to 17% | `capture` | The install line. 1 line only. Show it finish. |
| First run | 17% to 37% | `capture` | The smallest real use. Zoom on the output that matters. |
| Explain | 37% to 52% | `diagram` or `callout` | What happened under the hood, in 1 picture. |
| Power use | 52% to 77% | `capture` | A flag, a config or an integration. New framing. |
| Recap | 77% to 87% | `motion` | 2 to 3 short lines: install, run, result. |
| CTA | 87% to 100% | `cta` | Repo URL or docs URL, plus the install command. |

Terminal rules:

- Font size 36 px or more at 1080p. Crop the terminal so the text fills the
  frame. A full desktop shows text too small to read.
- Type at a natural pace with the cursor visible. Cut long waits. Show a
  short "time passes" cue instead of a frozen screen.
- Highlight the 1 output line that proves the point with a `callout`
  region, not the whole screen.

## 3. Concept or architecture explainer (`short`, 45 s to 90 s)

Goal: the viewer can draw the idea from memory.
Best hooks: question, number.

| Beat | Share | Kind | Content |
|---|---|---|---|
| Hook | 0% to 6% | `hook` | The question or the surprising number. |
| Context | 6% to 18% | `motion` or `character` | The world before the idea: who has the problem, where it hurts. |
| Core model | 18% to 42% | `diagram` | Build the model piece by piece. 1 new element per 2 s to 3 s. |
| Walkthrough | 42% to 66% | `diagram` or `motion` | Follow 1 request, record or user through the model. |
| Edge case | 66% to 80% | `callout` or `diagram` | 1 failure mode and how the model handles it. |
| Takeaway | 80% to 92% | `callout` | The core takeaway sentence on screen, read aloud. |
| CTA | 92% to 100% | `cta` | Where to read more. |

Diagram rules:

- Build, do not reveal: add parts in the order the narration names them.
- 7 labeled parts or fewer on screen at once. Group or zoom when you need
  more.
- Keep 1 color per role across every diagram scene (for example, accent for
  the thing that moves, surface for containers).
- Change framing between adjacent diagram scenes: zoom in, pan, or move the
  focal point by 25% of the frame or more. 2 identical framings in a row
  fail the visual variety rule.

## 4. Short social hook cut (`short`, 15 s to 30 s)

Goal: stop the scroll and get 1 tap. Plays muted by default.
Best hooks: result first, contrast cut, question.

| Beat | Share | Kind | Content |
|---|---|---|---|
| Hook | 0% to 15% | `hook` | Full motion plus headline before 1.5 s when you can. |
| Turn | 15% to 40% | `capture` or `motion` | The surprise: how fast, how small, how easy. |
| Payoff | 40% to 75% | `capture`, `callout` or `character` | The result on screen, large. |
| CTA | 75% to 100% | `cta` | 1 short action. Text on screen, not only in the voice. |

Social rules (`short`):

- Captions carry the whole story. A viewer with the sound off must get the
  takeaway.
- Cut every 2 s to 3 s. Do not repeat the same kind more than 2 scenes in a
  row.
- In `9:16`, keep critical content out of the right 15% (platform buttons)
  and the bottom 18% (caption band and platform text).
- The last frame works as a still: product name and action readable.

## 5. Long-form episode (`long`, 180 s to 900 s)

Goal: the viewer clicked a title and a thumbnail, and the film keeps the
promise of both. Format: `16:9` only. Captions carry the story, with no
voice-over. Game or product audio and music play under the captions.
Best hooks: cold open in motion (see "Opening rules (`long`)").

| Beat | Share | Kind | Content |
|---|---|---|---|
| Cold open | 0% to 5% | `hook` | The best moment or the core promise, in motion. |
| Promise | 5% to 10% | `callout` or `motion` | The title's promise in 1 caption. A studio intro of 5 s or less may sit here. |
| Chapters | 10% to 90% | `capture`, `motion`, `diagram` or `character` | 3 chapters or more. Each chapter has a hook, a build and a payoff. The hero chapter comes last. |
| CTA and end screen | 90% to 100% | `cta` | 1 CTA, then the end screen of 5 s to 20 s. |

Chapter rules (`long`):

- Each chapter runs 30 s to 180 s. Each chapter has 1 payoff.
- Each chapter opens with its own hook caption. List each chapter in
  `meta.chapters` with `id`, `title`, `start_sec` and `hook`.
- The hero chapter comes last. It holds the main hero moment.
- The music bed changes between chapters. A single song never loops for the
  whole film.
- Shot length varies. Use a fast montage to build, then room to breathe at
  each payoff.
- The end screen runs 5 s to 20 s. Keep clear space for the end-screen
  elements. Set `meta.end_screen_sec` to its length.
- The film has no loop.
- Scale the beat shares to the film length. Keep 3 chapters or more.

## Pick an archetype

| Need | Archetype |
|---|---|
| A new product or a major release, for buyers | SaaS product launch |
| A tool developers install and run | CLI or developer tool walkthrough |
| An idea, a protocol or a system design | Concept or architecture explainer |
| A cut-down for a feed, from any of the above | Short social hook cut |
| A YouTube episode or devlog of 3 min to 15 min | Long-form episode |

Pick from the archetypes whose label matches `Format` in `BRIEF.md`.

When 2 `short` archetypes fit, pick the one whose hook pattern shows the
product fastest. A social cut of a long film is a separate production with
`format: short`. Plan it as its own storyboard with its own `timeline.json`,
not as a crop of the long master.
