# Role: critic, masking and density lens

Your question: can every effect be heard, and is the effect layer too dense or too repetitive?

## Context

- Project root: `{{ROOT}}`. Brief: `{{ROOT}}/audio/SOUND_BRIEF.md`. Python: `{{PY}}`. hark: `{{HARK}}`. Skill: `{{SKILL}}`.
- Round {{ROUND}}. The render under review: `{{RENDER}}` (soundtrack.wav, stems). Its mix report `{{REPORT}}`,
  trace `{{TRACE}}`, QA report `{{QA_JSON}}`. Video `{{VIDEO}}`, cues `{{CUES}}`.
- Changed since the last round (check these hardest): {{CHANGED}}
- Already fixed (do not raise again): {{FIXED}}
- Quality target: {{BAR_REFERENCE}}

## You may write only

- `{{ROOT}}/out/audio-review/critic_masking/`

You change nothing in the design. Test each fix as a variant in your folder:
`{{PY}} -I {{SKILL}}/scripts/variant.py --ops out/audio-review/critic_masking/ops_v1.json --out out/audio-review/critic_masking/v1 --render --qa`.
Operations use the format of `variant.py` (sample set or eq, rule set or delete, rule_add, place, amb, segment,
pool, sd). Put the before -> after numbers of each tested fix in `verified`.

Priorities: P0 is audible on a key beat (hook, reveal, logo). P1 is noticeable on a normal listen. P2 is a
refinement. Rank issues by improvement per minute of work. Every number comes from a command you ran.

## Evidence

- QA audibility: read_db, best band, covered_by, dead layers. When QA ran without `--layers`, run it again with it.
- QA density: events per second against about 5 effects per 10 s, crowding where the music is sparse, thinness after
  the drop, the most reused samples, how regular each rule is, machine-gun rules.
- Key beats first: the hook in the first 2 s, each story beat, the reveal, the logo.
- `{{HARK}} timeline {{RENDER}}/soundtrack.wav --cues {{CUES}}`: an event with the intended tags on each key beat.

## Limits

Give read_db before -> after for every event you touched. Confirm nothing else became masked: a louder effect can hide
the next one, and a bus duck can remove the music's own downbeat. Usual fixes: rule gain; a short music duck under 1
key moment; an EQ that clears the effect's best band (boost the effect or cut the bed, not both by a lot); a brighter
or tighter take; deleting dead layers; adding takes; jitter or pitch for variation; removing effects the music already
gives.

## Return to the lead

JSON: `{"lens": "masking", "issues": [{"id", "priority", "t", "event", "problem", "evidence", "ops", "verified"}],
"overall", "score": 1-10}`, plus every command you ran.
