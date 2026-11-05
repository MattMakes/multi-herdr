# Taste notes: format, kinds and harvest

## Entry format

1 entry per `## <title>` heading. The fields are `- <key>: <value>` lines.

```markdown
# Taste notes

## Hold the logo 0.5 s longer at the end
- kind: preference
- topics: ending, pacing
- evidence: "the logo leaves too fast" (operator, launch-v2 round 1)
- films: launch-v2
- date: 2026-11-04

## Hook text at 96 px or more reads on a phone
- kind: verified
- topics: typography, hook
- evidence: critic axis 3 scored 5 in both films at 96 px, 3 at 64 px
- films: launch-v2, cli-demo
```

| Field | Rule |
|---|---|
| `kind` | `preference`, `observed` or `verified`. Never 2 kinds in 1 entry. |
| `topics` | 1 or more words, separated by commas. Use the load topics: format, archetype, channel, motion, type, audio, pacing, hook, ending, brand. |
| `evidence` | Where the entry comes from. A file, a round, a number, or a quote. |
| `films` | The film slugs that gave the evidence. |
| `date` | Optional. The day the entry was written. |

`lint_notes.py` rejects an entry with no kind, no topics or no evidence, an
unknown field, a repeated field, a repeated title, a preference with no
quote, and a verified entry with fewer than 2 films and no measurement. It
also rejects a notes file inside a `skills/` folder.

## The 3 kinds

- `preference`: the operator's own words, in double quotes. It is taste. It
  holds for this operator only.
- `observed`: something 1 film showed. It is a lead, not a rule.
- `verified`: the same result in 2 or more films, or a measurement. The
  evidence starts with `measured` when it is a measurement.

## Harvest, after the critic loop

Sources: every `qa/critique-round-<N>.md`, `qa/ab-rank.md`, the operator's
notes on the film, and the gate files in `qa/`.

1. Fix first. When this film contradicts an entry, change or delete that
   entry, in the project copy and in the fleet copy.
2. Keep a new entry only when all 3 are true: it is not obvious, it saves
   time on the next film, and this film produced the evidence.
3. Prefer a check over a note. When a finding can be measured, send it to
   the orchestrator as a proposed check with a test. Do not add an entry
   for it.
4. Else add the entry to `.video/NOTES.md` and to the fleet copy.
5. An `observed` entry that a 2nd film confirms becomes `verified`. Add the
   2nd film to `films`.
6. Lint both files.

## What never happens

- What 1 operator likes never turns into a rule that ships to others. A `preference` stays in
  the fleet copy on this host. It never goes into `skills/`, `teammates/`
  or any file in the repo.
- A `verified` finding that holds for every operator goes to the
  orchestrator as a proposed check or skill change. The orchestrator
  decides. A fact that holds for every task can also go to
  `horch note --fact`.
- A notes entry never blocks a gate. It gives no authority to post, pay for
  anything or reach out to anyone.
