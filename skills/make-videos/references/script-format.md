# Script and packaging format

One file per episode: `devblog/<NN>-<slug>.md`. `<slug>` is the working title
in lowercase with hyphens. The file holds the top block, then 3 sections:
`## Script`, `## Packaging` and `## Production notes — not for viewers`. Read
[video-rules.md](video-rules.md) first: every line here must pass it.

## Voice and length

Write as the developer talking to the viewer: warm, confident and genuinely
excited.

- Lead with how the game feels, not how it works.
- Never walk through code.
- Length comes from the series `Format` in `devblog/00-index.md`. `short`: 15 s
  to 90 s. `long`: 180 s to 900 s, `16:9`, captions only. The length includes
  the logo intro and the end card. Shorter is better once the purpose is fully
  delivered.

## Top of the file

```markdown
# Episode <N>: <working title>

Purpose: <1 sentence: the one thing this video makes the viewer want>
Hero moment: <the single most exciting thing in the video>
Hook: <the first 3 seconds of gameplay, in 1 sentence>
Format: <short | long>. Target length: <seconds> s for each cut
Cuts: <the `Cuts` of the series decisions>
```

## Script structure

1. **Opening.** Write 1 opening for each main cut, from the "Audiences and
   logos" table. The hook is the first 3 seconds of gameplay: open mid-action
   on a striking moment. Make it strong, but save the hero moment for the
   showcase. Follow it with a single line or caption that says what is new.
2. **Build-up.** A fast montage of what the milestone adds, 1 beat per thing
   the player gets to do. Use a sentence or two per beat to build momentum.
   Each beat raises the anticipation for the next one.
3. **Showcase.** The new thing in action, escalating to the hero moment. Give
   the hero moment a line of narration or a music breakdown just before it: a
   dip in the music, never silence.
4. **Call to action.** The 1 ask of this cut, shown on the end card for at
   least 5 seconds. When the plan table has a next episode, the end card also
   teases that milestone in 1 line, with no second ask. With no next episode,
   there is no tease.
   Write 1 ending for each main cut, from the same table.

For `long`, the build-up and the showcase become 3 chapters or more. Each
chapter has its own hook caption and 1 payoff, and the hero moment sits in the
last chapter.

## Production cues

Add bracketed cues to the script:

- `[SHOT: what is on screen, e.g. a gameplay clip, a slow-mo hit, a before/after split]`
- `[DEMO: exact gameplay to capture, recordable from the episode's last commit]`.
  The capture seat may record a later build when it looks better. It still
  shows only this episode's features.
- `[TEXT: on-screen caption or callout]`
- `[SFX: game sound to feature]`
- `[MUSIC: mood, tempo or energy change]`
- `[CUT: pacing note, e.g. "cut on the beat" or "hold for 2 seconds"]`

## Packaging

After the script, add a section headed `## Packaging`:

- **Titles:** 3 options for the social main cut, each under 60 characters and
  built on the video's purpose. No clickbait the video does not deliver.
- **Thumbnail:** 1 concept built on a single frame from the hero moment. Use at
  most 3 words of text, readable at phone size.
- **Vertical cutdown:** a 15 s to 30 s vertical (`9:16`) script for Shorts,
  TikTok, Reels and X. It is the social vertical cut. Write it only when
  `Cuts` holds that cut.
  - Hook in the first second, then the hero moment, then the call to action.
  - Same purpose as the main video.
  - Its hook is its own, not the main video's hook.
  - Nothing that needs sound to make sense.

## Production notes

End the file with a section headed `## Production notes — not for viewers`.
It holds:

- the short hashes the episode covers
- the estimated runtime and the seconds of narration
- a capture list grouped by commit: which build to check out and what to
  record from it, so each build is captured in one sitting
- any `[inferred]` flags

## The index: devblog/00-index.md

```markdown
# Devblog: <game>

## Series decisions

| Decision | Value |
|---|---|
| Format | <short or long> |
| Cuts | <any of: social main, social vertical, Steam> |
| Call to action | <per audience: the 1 action and where it leads> |
| Voice | <voice-over or captions only> |
| Studio logo | <intro clip: path or "not built">. <still for the Steam end card: path or "not built"> |

## Episodes

| # | Working title | First commit | Last commit | Commits | Real dates | Purpose | Hero moment | Status | Rendered cuts |
|---|---|---|---|---|---|---|---|---|---|
| 1 | <title> | <hash> | <hash> | <n> | <first> to <last> | <1 sentence> | <1 sentence> | <planned, scripted or approved> | <cuts whose master passed the critic, or "none"> |
```

`Status` is the state of the script: `planned` after the plan OK, `scripted`
when the script file exists or changed, `approved` after the script OK.
`Rendered cuts` names only a master that matches the approved script.
