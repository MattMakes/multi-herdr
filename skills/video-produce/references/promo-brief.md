# Promo brief and claims gate

Use this gate for a product promo, trailer, or devlog. Complete it before
the orchestrator starts a production unit.

## Write for the viewer

Read `Format:` in `PRODUCTION.md`. Use the list of that format.

Both formats:

- Name the audience as the buyer or player who can act on the CTA.
- Use words that this audience uses. Do not show team jargon, commit hashes,
  tool names, internal file names, or production details.
- End with 1 CTA. Name 1 action and its destination.

`short`:

- Put 1 hook in the first 2 s. The first shot must prove the hook line.
- Pick 1 hero moment for the episode. Save it for the main payoff.

`long`:

- Make the first 30 s prove the promise of the title and the thumbnail.
- Write 3 or more chapters. Give each chapter 1 payoff and 1 hook caption.
- Put the main hero moment in the final chapter.
- Write 3 title options, 1 thumbnail concept and the chapter timestamps.

A devlog series with shorts is 2 productions for each episode: 1 `long` and 1 `short`.
The `short` has its own script. It is not a cut of the `long` master.

Write the complete script before production starts. Include every caption,
the hook, the hero moment, and the CTA. The operator must approve this script.
No production unit starts before this approval.

## Verify every caption claim

Create `claims.md` next to the script. Add 1 table row for every caption
that makes a factual claim.

```markdown
# Claims

| Caption claim | Verdict | Evidence |
| --- | --- | --- |
| The app finishes the task in 4 s. | true | Measurement: 4.0 s in the release build. |
| The product removes every manual step. | partly true | Play test: 2 setup steps remain before the run. |
```

Use only these verdicts:

- `true`: the evidence proves the complete claim.
- `partly true`: the evidence proves part of the claim and states its limit.
- `false`: the evidence disproves the claim. Remove or rewrite the caption.

Evidence must name a code path, a play test, or a measurement. Configuration,
metadata, memory, and `[inferred]` text are not evidence. An empty evidence
cell is unverified. An unknown verdict is also unverified.

Run the gate from the video project folder:

```bash
python3 "<skill-dir>/scripts/check_claims.py" claims.md
```

The gate blocks a `false` or unverified claim. Fix the caption and its ledger
row, then run the gate again. Give the verified script and `claims.md` to the
operator. Production starts only after the gate passes and the operator
approves the script.
