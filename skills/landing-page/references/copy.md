# Copy: formulas, section shapes, self-audit

Load at workflow step 4. Write all visible copy before styling. Copy is the design's content; layout serves it.

## 1. Message hierarchy (from the brief read)

Fill this block first and keep it in your notes or the report.

```
Audience:        <who, in their words>
Problem:         <what hurts, in their words>
Differentiator:  <the one thing only this product does or does best>
Value prop:      <one sentence, at most 12 words>
Benefits:        <3 outcomes, each at most 8 words>
Objections:      <top 3, each with the proof that answers it>
Primary CTA:     <2-3 words, verb first>  -> <what happens after the click>
Secondary CTA:   <optional, a lower-commitment action>
Voice:           <3 adjectives, and 1 thing the voice never does>
```

## 2. Headline formulas

Pick the formula that fits the differentiator. Write 5 candidates, then keep the clearest, not the cleverest.

| Formula | Shape | Example |
|---|---|---|
| Outcome | Get <outcome> <without pain> | Ship reports without opening a spreadsheet |
| Category plus difference | The <category> for <audience> who <need> | The CRM for agencies that bill by the hour |
| Before and after | From <old state> to <new state> | From 40 tabs to one inbox |
| Name the enemy | <Pain> ends here / Stop <pain> | Stop chasing invoices |
| Direct statement | <Product> <does X> for <audience> | Lumen checks every contract clause for you |
| Number (only if true) | <Verified number> <unit> <outcome> | Deploys in 9 seconds, measured on a 2 GB repo |

Rules:
- At most 2 lines at 1440 px; usually 4-10 words.
- Concrete nouns and verbs. The headline must survive without the visual.
- If it needs a subhead to make sense, rewrite it. The subhead adds detail, not meaning.

## 3. Subhead, body and section copy

- Hero subtext: at most 20 words. Say who it is for and how it works, or remove the top objection.
- Section headline: at most 8 words, states a benefit or answers a question.
- Section body: at most 25 words by default. Longer only when the section's job is to explain.
- Feature copy: benefit first, mechanism second ("Find any clause in seconds. Search reads every PDF you upload.").
- Problem copy: use the visitor's words. Name the situation, not their feelings about it.
- Proof copy: specific, attributable, dated. "Cut onboarding from 3 weeks to 4 days" beats "Game-changing".
- FAQ: the question as the visitor asks it; the answer starts with "Yes", "No" or the fact.

## 4. CTA rules

- Verb first, 2-3 words, one line at every width: "Start free trial", "Book a demo", "Get the guide".
- The label says what happens after the click. Do not use "Submit", "Learn more" or "Click here" for the primary action.
- One label per intent on the whole page. "Get started", "Try free" and "Sign up" are the same intent: pick one.
- Next to the primary CTA, one risk reducer if it is true: "No card needed", "Cancel anytime", "14-day trial".
- Secondary CTA: lower commitment ("See how it works", "View pricing"), visually lighter.

## 5. Short frameworks for longer sections

Use one per section; do not chain all of them.

- **Problem, agitation, solution**: state the problem, show its cost with one fact, present the product as the fix.
- **Before, after, bridge**: the current state, the improved state, the product as the bridge.
- **Feature, advantage, benefit**: what it is, what it does better, what the visitor gains.
- **Objection, answer, proof**: the doubt in the visitor's words, the direct answer, the evidence.

## 6. Names, numbers and placeholders

- Real names, logos, quotes and numbers come only from the brief or a source the owner supplied.
- When the brief has none, write a clearly marked placeholder: visible text such as `[Customer quote, name, role]` and a `TODO` comment in the code. Do not invent a plausible person, company or statistic.
- For invented demo brands, choose a specific, plausible name that fits the domain. Avoid "Acme", "Nexus", "Cloudly", "SmartFlow" and similar.
- Dates, prices and time zones are written in full ("Tuesday 14 October 2026, 17:00 CET").

## 7. Banned patterns (search the built page for them)

| Pattern | Why | Replace with |
|---|---|---|
| `—` and `–` in visible text | The most common machine-writing tell | A period, comma, colon or hyphen |
| elevate, seamless, unleash, empower, revolutionize, next-gen, supercharge, game-changing, cutting-edge, unlock | Filler verbs and adjectives that say nothing | The concrete action |
| "Quietly trusted by", "Field notes", "On our desks" | Performative-craft labels | "Trusted by", "Testimonials", or no label |
| Micro-meta sentences under headings ("This list will stay short on purpose.") | Clutter | Delete |
| "Step 1 / Stage 2 / Phase 03" | Generic labels | Name the action |
| Version labels in a hero ("v2.0", "BETA") | Noise unless the page is a launch | Delete |
| "Scroll to explore" and scroll cues | Visitors know how to scroll | Delete |
| Fake precision ("99.99%", "4.1x faster") with no source | Unverifiable | A sourced number or no number |

A quick search over the built files (adapt the path):

```
grep -rnE '—|–' <build-or-src>
grep -rniE 'elevate|seamless|unleash|empower|revolutioni[sz]e|next-gen|supercharge|game-chang|cutting-edge|unlock' <build-or-src>
```

## 8. Copy self-audit (do this before styling and again before handoff)

Read every visible string: nav, headings, body, buttons, labels, alt text, captions, errors, footer. Flag and rewrite each string that:

1. Is grammatically broken or has an unclear referent ("we plan to stay that way").
2. Is wordplay or a metaphor that does not hold up when read literally.
3. Mixes registers (terminal-style metadata next to marketing prose) without a brand reason.
4. Claims something the brief does not support.
5. Could appear unchanged on a competitor's page.

If you are not sure a string works, replace it with a plain functional sentence. Plain copy beats clever copy that fails.
