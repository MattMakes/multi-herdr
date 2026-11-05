# MOTION.md template

`MOTION.md` is the motion rule book of 1 video project. It sits in the project folder next to `timeline.json`. It has 2 parts:

- The method part says how rules are written, read, checked and updated. It is the same in every project. Copy it as it is.
- The project part holds the rules of this film: tokens, owner rules, rig limits and the change log. Fill it in per project.

The animator named in the task owns `MOTION.md`. Other teammates send a change they need to the orchestrator.

## How to start a project file

1. Copy the block below into `MOTION.md` in the project folder.
2. Fill the tokens from `DIRECTION.md` and `BRAND.md`. Keep the defaults for tokens they do not set.
3. Copy each motion rule from the task or `DIRECTION.md` into "Owner rules", with its source.
4. When the film has a character, add the `## Character` section from `video-character` (`references/rig-and-visemes.md`) and fill "Rig limits".

## The template

```markdown
# MOTION.md

## Method (same in every project, do not edit)

Write a rule:
- 1 rule is 1 line: `- M<n>: <what moves, how> (<token or number>). Source: <who or what>. Check: <how>.`
- A rule names a measurable value: a duration in ms, an ease name, a distance in px, a ratio.
  "Smooth" or "snappy" is not a rule. "Cards enter with spring-ui in 450 ms" is a rule.
- A rule says what it covers: all scenes, 1 scene id, or 1 element role.
- Numbers repeated in 2 places become a token. A rule then names the token.

Read first:
- Read this whole file before you build or change any scene.
- Read it again at the start of every fix round. The last round can have changed it.
- When a task and this file disagree, ask the orchestrator before you build.

Check:
- Every rule has a check. Use the strongest that fits:
  1. a script: `scan_linear_motion.py` for eases, the seek self-check, `compare_frames.py` for a reference;
  2. a frame check: render the named frames and look at them;
  3. a read check: read the scene code against the rule.
- Run every check of every rule before a scene is called done. Name each check and its result in the DONE line.

Update each round:
- After each critic round, add a log line: round number, the note, the rule added or changed.
- Change a rule in place and add `(changed round <n>, was <old value>)`. Do not delete a rule.
  Mark a rule that no longer holds `(dropped round <n>: <why>)`.
- A note that names a value becomes a rule before the fix. Then fix the scenes to the rule.

## Tokens

- ease.enter: cubic-bezier(0.22, 1, 0.36, 1)
- ease.exit: cubic-bezier(0.4, 0, 1, 1)
- ease.ui: spring-ui
- ease.camera: spring-camera
- ease.heavy: spring-heavy
- ease.playful: spring-playful
- dur.enter_ms: 550
- dur.exit_ms: 280
- stagger.each_ms: 60
- stagger.max_items: 5
- dim.context_opacity: 0.35
- hold.step_ms: 600

## Owner rules

- M1: Nothing moves, grows or turns with a linear ease, except lines marked `motion-ok: linear`. Source: video-motion default. Check: scan_linear_motion.py on scenes/.
- M2: <rule>. Source: <operator, DIRECTION.md, critic round n>. Check: <check>.

## Rig limits

- <part>: <limit with unit>, for example `rig-head: rotate -12 to 12 deg`, `squash.max: 0.12`. Check: <frame check at named frames>.

## Change log

- round 0: file created from DIRECTION.md and the task.
```

## Notes on the tokens

- The 4 spring tokens name the presets of `seek-timeline.js`. A tween with a spring ease and no `duration` takes the preset duration: ui 0.45 s, camera 1.2 s, heavy 0.9 s, playful 0.75 s. `SeekTimeline.springs` holds the exact numbers.
- Pick the preset by the role of the element, not by taste per tween:
  - `spring-ui`: buttons, cards, labels, chips.
  - `spring-camera`: pans, zooms and push-ins. It never overshoots, because a camera that bounces makes viewers feel sick.
  - `spring-heavy`: large panels, devices, product shots. It starts slowly and settles with almost no overshoot.
  - `spring-playful`: icons, mascots, stickers. It overshoots by about 23 % and rings out.
- Opacity and colour may stay on `linear` or the enter and exit eases. The scan does not flag them.
