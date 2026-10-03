# Messaging and voice

## Message architecture

Build from the top down. Each level must be true at the level above.

| Level | Question | Form |
|---|---|---|
| Mission | Why do we exist? | We [action] for [audience] by [method] so they can [outcome]. |
| Vision | What changes if we win? | A world where [change]. |
| Value proposition | What do we offer? | For [audience] who [need], [brand] is a [category] that [key benefit]. Unlike [alternative], we [differentiator]. |
| Positioning | Why us? | [Brand] is the [category] for [audience] who want [outcome], because [reason to believe]. |
| Primary message | What is the one thing to remember? | One sentence, no jargon. |
| Supporting messages | Why should each segment care? | 3 to 5 messages, each with a proof point. |
| Proof points | Why believe it? | Facts: numbers, customers, certifications, demos. Mark unknowns `TODO(owner)`. |

Supporting messages table:

| Message | Audience need | Proof point |
|---|---|---|
| ... | ... | ... |

Message by audience:

| Segment | Pain | Key message | Call to action |
|---|---|---|---|
| ... | ... | ... | ... |

Elevator pitches: 10 seconds (one line that sparks interest), 30 seconds (problem, solution, difference), 60 seconds (add proof).

### Message tests

1. Clear: no jargon; a newcomer understands it.
2. Different: swap in a competitor's name; if it still works, rewrite it.
3. Credible: every claim has a proof point.
4. Relevant: it answers a need the audience states.
5. Consistent: it fits the product as it exists today.

## Voice and tone

Voice is the personality; it does not change. Tone is how the voice adapts to the moment.

### Spectrums

Place the brand on each line (1 to 5). Record the number.

| Spectrum | 1 | 5 |
|---|---|---|
| Formality | Formal (legal, banking) | Casual (social, consumer) |
| Complexity | Simple (consumer) | Technical (expert B2B) |
| Character | Serious (finance, health) | Playful (games, kids) |
| Emotion | Reserved (enterprise) | Expressive (lifestyle) |

### Traits

Choose 3 to 5 traits. Write each as "X, not Y" so that the boundary is clear.

| Trait | We are | We are not | Do | Don't |
|---|---|---|---|---|
| Confident | Direct, sure of the facts | Arrogant, overselling | "This report takes 2 minutes." | "The ultimate reporting revolution." |
| Helpful | Practical, next-step focused | Patronizing | "Add a payment method to continue." | "Oops! Looks like you forgot something silly!" |
| Clear | Short sentences, plain words | Vague, wordy | "Your file is too large. The limit is 25 MB." | "An error occurred while processing your request." |
| Warm | Human, kind under pressure | Cute, chummy | "That didn't work. Try again, or contact us." | "Uh-oh, spaghetti-o!" |

### Tone by context

| Context | Tone shift | Example |
|---|---|---|
| Marketing | Benefit first, energetic, concrete | "Close the month in an afternoon." |
| Onboarding | Encouraging, one step at a time | "Connect your bank. It takes about a minute." |
| Documentation | Instructional, neutral, imperative | "Run `npm install`, then restart the server." |
| Error messages | Calm, specific cause, a way out | "We couldn't save your changes. Check your connection and try again." |
| Success | Brief, quiet | "Invoice sent." |
| Destructive confirmation | Plain, name the consequence | "Delete 'Q3 report'? You can't undo this." |
| Support | Empathetic, accountable | "You're right, that's our mistake. Here's what we're doing." |
| Legal and billing | Formal, exact | "Your plan renews on 1 November 2026 at $24." |

### Vocabulary

| Use | Avoid | Reason |
|---|---|---|
| use | leverage, utilize | Plain word |
| sign in | log on, login (verb) | Consistency |
| delete | nuke, trash (in UI) | Clarity |
| <brand term> | <competitor's term> | Ownable language |

Banned by default (remove a line only with a reason): revolutionary, game-changing, best-in-class, world-class, seamless, next-gen, cutting-edge, unleash, elevate, empower, supercharge, synergy, robust, frictionless, "we're excited to announce".

### Mechanics

Decide once and write it down: sentence case or title case for headings and buttons; serial comma; numerals (use digits for numbers in UI); date and time format; contractions (yes or no); exclamation marks (at most one per screen, never in errors); emoji (where allowed, if anywhere); how the product refers to itself and to the user ("we", "you").

### Microcopy rules for UI

- Buttons name the action and the object: "Save draft", not "Submit" or "OK".
- Errors state what happened, why if known, and what to do next. Never blame the user.
- Empty states say what goes here and offer one action.
- Confirmations name the consequence and match the action label ("Delete project" / "Cancel").
- Labels are nouns; helper text is one sentence; placeholders are examples, not instructions.

## Rewrite method

1. Take 3 real strings from the product (a headline, an error, a button or empty state).
2. Mark every word that breaks a trait or the vocabulary list.
3. Rewrite. Keep the meaning; change only voice and clarity.
4. Show before and after in the guidelines.

| Before | After | Trait applied |
|---|---|---|
| "An unexpected error has occurred." | "We couldn't load your invoices. Refresh the page or try again in a minute." | Clear, Helpful |

## Voice tests

1. Does it sound like one person wrote all 3 rewrites?
2. Would a competitor say this? If yes, rewrite.
3. Is every trait visible somewhere, and is no "not" side visible anywhere?
4. Read it aloud: does it sound like a person talking to a person?
