---
name: game-juice
description: Use when asked to study a game (its code, a build, captures) or a game design document and propose "juice" - the audiovisual and tactile feedback that makes actions feel good - that is plentiful, balanced and tasteful. Audits each core action for input feel, hit-stop, deformation, directional feedback and sound variance, then writes a ranked idea list with concrete starting values.
---

# Game juice

Juice is feedback that does not change the rules but changes how the rules
feel: tweens, squash and stretch, hit-stop, screen kick, particles, debris,
pitch variation, rumble. The job is more juice where actions feel flat, less
where it buries the game, and every effect in proportion to what happened.

You produce ideas, not code. Each idea names where it hooks into the game, a
starting value, and how to check it worked.

## Sources this skill draws on

- **"Juice it or lose it"** - Martin Jonasson and Petri Purho (2012),
  https://www.youtube.com/watch?v=Fy0aCDmgnxg. A plain Breakout clone becomes
  lively through tweening, elastic overshoot, squash and stretch, camera kick
  and pitch changes alone. Lesson: the rules can stay; the response changes.
- **Game Feel** - Steve Swink (2008). Real-time control, simulated space and
  polish together make "virtual sensation". Input latency and response curves
  matter as much as the effects; a control should feel like an instrument.
- **"The Art of Screenshake"** - Jan Willem Nijman, Vlambeer (2013),
  https://www.youtube.com/watch?v=AJdEqssNZ-U. About 30 small steps on a
  top-down shooter: camera kick along the shot, hit-stop, knockback, bullet
  spread, permanent debris and casings.
- **"Pushing Buttons"** - Mick West (Game Developer, May 2005),
  https://cowboyprogramming.com/2007/01/02/pushhing-buttons/. Input
  buffering, telling similar inputs apart, and forgiveness, so the character
  does what the player meant.

## Workflow

### 1. Find out what you were given

- **A codebase**: find the engine and its tick rate (physics FPS, fixed
  step). Every frame count below is at 60 FPS; convert to the game's own
  rate. Find where input is read, where hits, pickups, jumps and deaths are
  resolved, and what already exists for camera, tweens, particles and audio.
- **A design document**: work from the verbs it describes. Mark each value
  you could not ground in the document as an assumption.
- **Captures or a build you can run**: watch the core action frame by frame
  if you can. Note what happens on the frame of impact and the 10 frames
  after it.

Read the game's tone before proposing anything: art style, pace, audience,
genre. A cosy farming game and an arcade shooter want different amounts.

### 2. List the core actions

Name the 3 to 8 verbs the player does most (jump, land, attack, hit, get
hit, collect, buy, die, level up, menu confirm). For each, record:

- how often it happens (per second, per minute, per session),
- how much it matters (routine, notable, rare climax),
- the feedback it has now on each channel: animation, camera, particles,
  sound, UI, time (hit-stop or slow motion), haptics.

Frequency and importance set the budget in step 4.

### 3. Audit each core action, in this order

Earlier layers carry later ones: polish on a sluggish control still feels
sluggish.

1. **Input responsiveness and buffering**
   - The visible response starts on the frame the input lands, or the next.
   - An input buffer of 4 to 6 frames (65 to 100 ms), so a press just before
     an animation ends is not eaten.
   - 3 to 5 frames of coyote time after walking off a ledge, where the game
     has jumping.
   - Similar inputs resolve to what the player meant (West): a quick tap and
     a hold, a jump pressed while landing.
2. **Hit-stop (a micro-pause)**
   - Freeze attacker and target for 2 to 4 frames (33 to 66 ms) on impact,
     while sound and shake keep running. Scale it with the hit: light hits
     short, heavy and final hits longer.
   - Never freeze the player's input; buffer it through the pause.
3. **Deformation curves**
   - Squash and stretch that keeps the area: about X 1.2, Y 0.8 for 2 frames
     on impact, then an elastic return to 1.0 with a small overshoot.
   - Stretch along velocity when moving fast; squash on landing.
4. **Directional feedback**
   - Camera kick and shake point along the attack or recoil vector, not
     random noise in every direction. Prefer trauma-based shake (amplitude
     from trauma squared, decaying over time) to fixed shakes.
   - Particles and debris spray along the impact normal. Leave marks that
     stay where it fits the game: scorch, smoke, casings, dents.
   - Knockback pushes the target away from the source.
5. **Acoustic variance**
   - Randomise pitch by 5 to 8 percent on repeated sounds so they do not
     tire the ear; rotate 2 to 4 sample variants for the most frequent ones.
   - Keep a low-frequency layer (below about 120 Hz) for high-tier or
     finishing impacts only, so it still means something.
   - Raise pitch up a combo or a pickup chain to reward streaks.

For each layer write: present, missing, or overdone, with the file and line
or the design section it comes from.

### 4. Balance the juice

More is not always better. Check every proposal against these:

- **Hierarchy**: rank events into 3 or 4 tiers. Each tier up gets visibly
  and audibly more (longer hit-stop, more shake, more particles, the
  low-frequency layer). A routine pickup must never feel like a boss kill.
- **Frequency budget**: the more often an event happens, the shorter and
  quieter its feedback. Something that happens 5 times a second gets a
  small pop and pitch variance, not screen shake.
- **Readability**: effects never hide threats, bullets, the player's
  position or the information needed for the next decision. Shake and
  flashes stay off during precise aiming.
- **Stacking**: when 20 hits land at once, cap shake, cap hit-stop to the
  largest single one, and limit identical sounds playing at the same time.
- **Tone**: every effect matches the art direction and pace. Tasteful
  juice looks like it belongs in that game.
- **Comfort and accessibility**: an option to reduce or turn off screen
  shake and flashes; no full-screen flashing faster than 3 times a second;
  haptics and audio carry the same information for players who turn
  visuals down.
- **Silence and contrast**: quiet moments make loud ones land. Leave some
  actions plain on purpose and say why.

### 5. Write the ideas

For each idea:

- **Action and tier** it applies to.
- **What changes**, in one or two sentences.
- **Starting values** (frames, scale, amplitude, pitch range, particle
  count), converted to the game's tick rate.
- **Where it hooks in**: file and function, or design section.
- **Cost**: small, medium or large.
- **Risk**: what could go wrong (readability, fatigue, motion sickness,
  performance) and how to dial it back.
- **How to verify**: what to watch or measure (frame-step the impact,
  compare a before-and-after capture, count simultaneous sounds).

Rank the list by feel gained for the cost. Put input and timing fixes
first: they help every effect after them.

## Output

Write one Markdown report to the path your task names. With no path, use
`ai_docs/juice/<YYYY-MM-DD>-<slug>.md`. Sections, in order:

1. **What I studied** - files, documents or captures, and the tick rate.
2. **Core actions** - table: action, frequency, importance, tier, current
   feedback per channel.
3. **Audit** - per action, the 5 layers: present, missing or overdone, with
   evidence.
4. **Ideas** - ranked, in the format of step 5.
5. **Leave alone** - what already works, and what should stay plain.
6. **Balance notes** - the tier table, the stacking caps and the
   accessibility options this game needs.

Reply with the report path. Do not paste the report into a message.

## Rules

- Change nothing in the game. Your only write is the report.
- Never present an assumed value as measured. Mark design-document guesses
  and values you could not check.
- The numbers above are starting points, not rules. When the game's own
  feel argues for another value, say which and why.
