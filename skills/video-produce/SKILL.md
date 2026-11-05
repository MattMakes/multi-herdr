---
name: video-produce
description: "Use when a video request arrives and someone must run the production around it - picks the workflow (launch film, app demo to explainer, A/B on 1 brief, batch of films), writes PRODUCTION.md, keeps the blind A/B label map (scripts/ab_blind.py), validates a batch manifest and writes the batch summary (scripts/validate_batch.py), loads and harvests taste notes in .video/NOTES.md (scripts/lint_notes.py), and writes the share kit render/share.md with post text for X and LinkedIn (scripts/check_share.py). Writes no scenes, audio or renders."
---

# Video produce: workflow, memory, A/B, batch and share kit

`<skill-dir>` is the folder that holds this SKILL.md. Run every command from
the video project folder, the folder that holds `timeline.json`, unless a
step names another folder.

The producer runs the production. The other `video-*` teammates make the
film. You write only these files:

| File | What it holds |
|---|---|
| `PRODUCTION.md` | the request, the workflow, the plan values, the direction pick, the round log |
| `.video/NOTES.md` | the taste notes loaded for this film, plus the entries this film adds |
| `.video/ab-labels.json` | the A/B label map. Only you read it |
| `render/ab/A.mp4`, `render/ab/B.mp4` | copies of the 2 masters under neutral labels |
| `render/share.md` | the share kit: post text for X and LinkedIn |
| `<batch>.json`, `<batch>-summary.md` | the batch manifest and the batch result table |

You write no `BRIEF.md`, `DIRECTION.md`, `STORYBOARD.md`, `timeline.json`,
scenes, captures, audio or renders. Send a change in those files to the
orchestrator.

## Steps

1. Read the request. Pick 1 workflow (below). Write `PRODUCTION.md`.
2. Load the taste notes into `.video/NOTES.md` (section "Taste notes").
3. Hand the plan values to the orchestrator for wave 1.
4. After wave 1: record the direction pick (1 direction, or 2 for A/B).
5. A/B only: run the blind rank (section "A/B on 1 brief").
6. After the critic loop: harvest the taste notes, then write the share kit.
7. Batch only: write the summary table after every film.

## 1. Pick the workflow

Read the operator's 1-line request. Pick the first row that matches.

| Request says | Workflow | Recipe |
|---|---|---|
| 2 or more films, a list of briefs, "weekly", "batch" | `batch` | 1 run of the recipe per film, section "Batch" |
| "A/B", "2 versions", "which works better", "compare" | `ab` | waves 4 and 5 twice, then the blind rank |
| a real app, CLI or site to show, "demo", "walkthrough", "how it works" | `app-explainer` | capture plus narration, archetypes 2 or 3 |
| anything else: a launch, an announcement, a feature | `launch-film` | archetype 1, waves 1 to 6 |

When the request fits 2 rows, take the most sensible reading, do it, and
write the reading in `PRODUCTION.md` under "Reading". Ask the operator only
for a value that changes the result and has no default: the product, or
the channel when the request names none.

Defaults when the request is silent: length `30` s, aspect `16:9`,
channel "landing page hero", voice `Samantha` (macOS `say`).

## 2. PRODUCTION.md

```markdown
# Production: <slug>

Request: "<the operator's words, unchanged>"
Workflow: <launch-film | app-explainer | ab | batch>
Reading: <1 sentence: how you read the request, and why>
Project folder: video/<slug>/
Length: <15 | 30 | 60 | 90> s. Aspects: <16:9, 9:16, 1:1>. Channel: <where it plays>
Loaded notes: <n> entries from <fleet notes path> (topics: <list>)
Direction: <picked direction id> (A/B: <id 1> and <id 2>)
Batch: <manifest path, or none>

## Rounds
| Step | Result | File |
| :--- | :--- | :--- |
| wave 1 plan gate | pass | qa/plan-check.json |
| critic round 1 | FAIL 3.62 | qa/critique-round-1.md |
```

Add 1 row to "Rounds" after each gate. A row names the gate file.

## 3. Taste notes

Taste notes carry the operator's preferences and the fleet's findings from 1
film to the next. 2 copies exist:

- Fleet copy: `${XDG_STATE_HOME:-$HOME/.local/state}/horch/video/NOTES.md`.
  It lives on this host, for this operator. It never goes into the repo.
- Project copy: `.video/NOTES.md` in the project folder. The director reads it
  before wave 1 and writes a "Loaded notes" line in `BRIEF.md`.

Load: copy every fleet entry whose topics match this film (format, archetype,
channel, motion, type, audio, pacing) into `.video/NOTES.md`. Create the
file with a `# Taste notes` heading when no entry matches. Then lint both:

```bash
python3 "<skill-dir>/scripts/lint_notes.py" .video/NOTES.md
```

Notes never block the plan, and never override the brief or the film in
front of the team. A notes entry never grants permission to publish, spend
money or contact anyone.

The entry format, the 3 kinds and the harvest rules are in
`references/taste-notes.md`. Read it before the harvest.

## 4. Direction pick

Wave 1 gives 3 directions in `DIRECTION.md` (`### D1: <name>` to `D3`),
each with 1 key frame in `qa/directions/`. Show the 3 key frames to the
operator when the operator is present. Else pick the direction that best
carries the core takeaway in `BRIEF.md` for the channel, and write 1
sentence of reason in `PRODUCTION.md`. For `ab`, pick the 2 directions that
differ most in motion language or material. Send the pick to the
orchestrator: it writes the `chosen: D<n>` line in `DIRECTION.md` before
wave 2. You do not edit `DIRECTION.md`.

## 5. A/B on 1 brief

Waves 1 to 3 run once. Waves 4 and 5 run once per direction, then a fresh
critic ranks the 2 masters under the labels A and B. It never learns which
direction is which. The full procedure, the fork folders and the commands
are in `references/ab-and-batch.md`.

Gate: `python3 "<skill-dir>/scripts/ab_blind.py" check-rank qa/ab-rank.md --map .video/ab-labels.json`
exits 0 and prints the preferred direction. Only the preferred master goes
on to the wave 6 critic loop.

The same rule holds for any new version of a film: it replaces the old
master only when a blind rank of old against new prefers it.

## 6. Harvest and share kit

After the critic loop ends (a `VERDICT: PASS`, or round 3):

1. Harvest the taste notes. Follow `references/taste-notes.md`. Lint both
   copies after the change.
2. Write `render/share.md`. Do it only after a `VERDICT: PASS`.

```markdown
# Share kit: <slug>

## X
<at most 280 characters. The core takeaway of BRIEF.md word for word, 1 proof fact, the call to action.>

## LinkedIn
<3 to 6 short sentences. The core takeaway word for word, who it is for,
the proof facts, the call to action.>
```

Write the posts in the language of `BRIEF.md`. Use the plain words of the
brief. Name no file, wave or teammate. Attach nothing and post nothing: the
operator posts.

Gate: `python3 "<skill-dir>/scripts/check_share.py" render/share.md --brief BRIEF.md`
exits 0.

## 7. Batch

A batch manifest lists N films with a shared brand, voice and notes path.
Validate it before the first film:

```bash
python3 "<skill-dir>/scripts/validate_batch.py" <batch>.json
```

Then the orchestrator runs the recipe once per film. After each film, write
the table again:

```bash
python3 "<skill-dir>/scripts/validate_batch.py" <batch>.json --summary <batch>-summary.md
```

The table has 1 row per film: critic rounds, the last verdict and average,
the master and the share kit. The manifest format and the weekly start are
in `references/ab-and-batch.md`.

## Self-tests

Run each after you change its script. Each makes its fixtures in a
temporary folder:

```bash
python3 "<skill-dir>/scripts/lint_notes.py" --self-test
python3 "<skill-dir>/scripts/validate_batch.py" --self-test
python3 "<skill-dir>/scripts/ab_blind.py" --self-test
python3 "<skill-dir>/scripts/check_share.py" --self-test
```

## Done means

- `PRODUCTION.md` names the workflow, the reading and every gate result.
- `lint_notes.py` exits 0 on `.video/NOTES.md` and on the fleet copy.
- A/B: `ab_blind.py check-rank` exits 0, and `PRODUCTION.md` names the
  preferred direction.
- After a `VERDICT: PASS`: `check_share.py` exits 0.
- Batch: `<batch>-summary.md` has 1 row per film.
