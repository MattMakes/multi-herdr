# Copy

Words are part of the design. Tight copy in an average layout reads as considered; stock copy makes a strong layout look generated.

## Rules

- **Specific over general.** Name the feature, the user, the number, the place, the date. "Open the trace, find the span, fix the regression" beats "Powerful observability for modern teams".
- **Do not invent specifics.** If the brief gives no metric, customer, or quote, do not make one up. Use a labeled slot (`metric to confirm`) or ask: `horch tell orchestrator "[<role>] QUESTION: ..."`. Treat source documents (PDFs, briefs, READMEs) as material to adapt, not text to paste, unless the user asks for verbatim copy.
- **One register per page.** Do not mix terminal-style metadata, literary prose, and marketing punch unless the brand voice calls for it.
- **Active voice, present tense.** "We couldn't find your account", not "Your account could not be found".
- **One term per thing.** Choose "Sign in" or "Log in", "Delete" or "Remove", and use it everywhere.
- **Sentence case** for headings and buttons. Title Case On Every Header reads as a template.
- **Plain beats clever.** Re-read every visible string. Rewrite anything grammatically broken, with an unclear referent, a forced metaphor, or mock-humble "craftsman" phrasing. Boring and clear is better than cute and wrong.

## Lengths

- Hero headline: 7 words and 50 characters or fewer when you write it.
- Hero supporting line: 20 words or fewer.
- Section: headline of 8 words or fewer, body of 25 words or fewer, unless the section's job needs more.
- Primary button: 1 to 3 words. If a label wraps, shorten it ("Get started free" to "Start free", "Read the documentation" to "Read docs").
- Quote: 3 lines or fewer; attribution has name and role (and company), never a first name alone.

## Buttons and links

- The verb of the action: `Save changes`, `Create account`, `Send invite`, `Copy link`. Not `OK`, `Submit`, `Click here`.
- Link text stands alone: "View pricing plans", not "click here".
- One label per intent across the page.

## Errors, empty states, loading

- Error: what happened (past tense), why if known, what to do (imperative). "That card was declined. Your bank flagged the charge. Try another card or contact your bank." No "Oops!", no exclamation marks, no humor on frustration paths (payment failed, locked account).
- Empty state: what is empty, why it matters, one action. "No projects yet. Projects group your tasks and team. Create a project."
- Success: silent when the result is visible. No exclamation marks.
- Loading over 10 s: an honest label ("Compiling. This can take a minute.").

## Banned phrases

These appear in generated copy because they say nothing. Replace each with a concrete claim.

| Phrase | Problem |
| --- | --- |
| Unleash, supercharge, empower, elevate, revolutionize | energy with no mechanism |
| Seamless, effortless, next-gen, cutting-edge, innovative, game-changing | no antonym; every product claims it |
| Built for the modern team / In today's digital landscape | vague and dated |
| Where X meets Y / Reimagine the way you... | false synthesis |
| Experience the power of... / Delve / Tapestry | empty |
| Quietly trusted by / Field notes / Currently on the bench | performative labels; say "Customers", "Writing", "Now" |
| Stage 1, Step 01, Phase One as labels | the step's verb is the label ("Install", "Configure", "Ship") |

## Names and data

- No Jane Doe, John Smith, Acme, Nexus, Lorem ipsum. Use plausible, locale-fitting names for demo people and concrete demo brand names ("Ridgeline Inventory").
- Demo data looks real: organic numbers (`47.2%`, `$1,284`), different dates, a different avatar per person. Mark demo data as sample data in the code.
- Never fake engineering precision ("5.8 mm", "4.1x") that the brand does not claim.

## Punctuation

- Curly quotes and apostrophes: `“ ” ‘ ’`. Ellipsis `…`, not `...`.
- No em dashes in headings, labels, buttons, captions, nav, or quote attribution. In prose, at most one per paragraph; a period, comma, colon, or parentheses usually reads better. Ranges: en dash in prose (`10–20`), hyphen in compact UI.
- The middle dot `·` at most once per line of metadata.
- Non-breaking space before units (`5 min`).

## Voice by tone

Imitate the kind of specificity, not the words.

| Tone | Pattern | Example shape |
| --- | --- | --- |
| Editorial | date or place anchor, named deliverables | "Type and identity for publishers since 2009." |
| Technical | data first, the real command, refusal of the alternative | "Drop-in OTLP. No agent, no sidecar." |
| Brutalist | flat declaration, definition by refusal | "We answer the email ourselves." |
| Soft | short claim, then proof | "Soft on the surface. Exact underneath." |
| Luxury | named scale, restraint | "By appointment." |
| Playful | useful analogy, anticipated reaction | "Playlists, but for ideas." |
| Austere | the decision, stated plainly | "This page does not move." |

If the brief gives nothing to write an opening line from, ask one question that gets a specific noun, verb, or place.
