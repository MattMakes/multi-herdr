# Research: what the studies show and what they do not show

> Back to [SKILL.md](../SKILL.md).

This file gives each source, its method, its result, its limits, and the
rule that the skill takes from it. Each rule has a tag:

- **research**: a study in this file supports the rule directly.
- **standard**: a published standard or industry specification sets the number.
- **practice**: common game-audio practice. No study here tests it. Tune it
  by ear and by measurement.

Do not raise a `practice` rule to `research` in a report. Do not cite a study
for a claim that this file lists under "does not show".

## 1. Koepp et al. 1998: dopamine release during a video game

Koepp MJ, Gunn RN, Lawrence AD, Cunningham VJ, Dagher A, Jones T, Brooks DJ,
Bench CJ, Grasby PM. "Evidence for striatal dopamine release during a video
game." *Nature* 393(6682):266-268, 1998. doi:10.1038/30498. PMID 9607763.
Source check: abstract read (Europe PMC). Full text not read.

**Method.** Positron emission tomography (PET) with 11C-raclopride, a tracer
that binds to dopamine D2 receptors. Less tracer binding means more
endogenous dopamine. The volunteers played a goal-directed video game and
also rested (baseline).

**Result.** Raclopride binding in the striatum fell during the game. The fall
correlated positively with the performance level, and it was largest in the
ventral striatum.

**Does show.** Goal-directed play engages the striatal dopamine system, and
the size of the effect goes with how well the player performs.

**Does not show.** The study did not change the sound. It does not show that
a sound causes dopamine release, that a louder or brighter sound gives more
dopamine, or that any feedback design is better than another. The task was
one game. This file does not record the sample size: read the paper before
you cite one.

**Rule (research, indirect).** Tie reward feedback to performance: the
feedback follows what the player did, at once, and in proportion. Do not use
this study to argue for "more juice".

## 2. Dixon et al. 2014: sound in multiline slot-machine play

Dixon MJ, Harrigan KA, Santesso DL, Graydon C, Fugelsang JA, Collins K. "The
impact of sound in modern multiline video slot machine play." *Journal of
Gambling Studies* 30(4):913-929, 2014 (online 2013).
doi:10.1007/s10899-013-9391-8. PMID 23821220, PMCID PMC4225056 (open access).
Source check: abstract and full text read (Europe PMC).

**Method.** 96 slot-machine players played a 9-line simulator for 2 blocks
of 200 spins: 1 block with sound, 1 block without sound (order
counterbalanced). Each block had 144 losses, 28 wins and 28 "losses disguised
as wins" (LDW). An LDW is a spin that pays back less than the bet but still
shows the win animation and plays the win jingle. The win jingle got longer
for a bigger win. Measures: skin conductance responses (SCR), heart rate,
self-reports, and an estimate of the number of wins.

**Result.**

- SCR to outcomes was higher with sound (main effect of sound,
  F(1,84) = 4.597, p = .035). SCR grew with the size of the win.
- Heart-rate deceleration followed credit gains (wins and LDWs). Sound did
  NOT increase it.
- Most players preferred the game with win sounds.
- Players overestimated their wins: actual 28, estimate 33 without sound
  (+15 %), estimate 36 with sound (+24 %). The authors conclude that sound is
  part of the disguise in an LDW.

**Does show.** Reward sounds raise physiological arousal, players prefer
them, and a reward sound on a net loss makes players remember more wins than
happened.

**Does not show.** The study used gamblers and a gambling machine, not a skill
game. It does not show which sound features (pitch, length, loudness) cause
the effect, and it does not measure long-term behaviour. It does not show
that sound raises heart-rate responses.

**Rules (research).**

1. A reward sound is honest. Its size (tier, length, loudness, brightness)
   is proportional to the real net gain.
2. Never play a reward sound for a net loss or a zero net gain. A "you got
   back part of what you lost" outcome gets a neutral sound or none.
3. A bigger real gain gets a bigger sound. This uses the effect in the honest
   direction: the player's memory then matches the truth.

## 3. Reynolds and Day 2007: sound speeds fast visuomotor processing

Reynolds RF, Day BL. "Fast visuomotor processing made faster by sound." *The
Journal of Physiology* 583(Pt 3):1107-1115, 2007.
doi:10.1113/jphysiol.2007.136192. PMID 17656434.
Source check: abstract read (Europe PMC). Full text not read.

**Method.** Participants stepped onto a lit target. Sometimes the target moved
left or right in mid-step, and the foot path had to change. On some trials a
loud, startling sound played with the target jump. The sound carried no
information about the direction.

**Result.** The sound shortened the mean response time from 134 ms by about
20 ms. The sound alone had no effect: the gain came from auditory-visual
interaction. The gain did not depend on a visible startle response. The
authors estimate that central visuomotor processing became at least 30 %
faster.

**Does show.** A sound that starts with a visual event makes a fast,
visually guided correction faster, even when the sound gives no direction.

**Does not show.** The sound was startle-level loud, the task was stepping,
and the participants were not playing a game. The study gives no latency
threshold: it does not say how late a sound can be and still help. It does
not show that normal game-level sounds speed play.

**Rules.**

1. (research) A sound for a visual event starts together with that event.
   Play it on the frame where the event happens. Do not defer it.
2. (research, indirect) The onset is sharp. A slow attack moves the
   perceived start later.
3. (practice) Do not use startle-level loudness as normal feedback. Keep it
   for rare danger cues, if at all.
4. (standard) Keep audio-visual offset inside the detection window of ITU-R
   BT.1359-1 (1998): sound at most about 45 ms before or 125 ms after the
   picture. For game feedback, aim for much less: see
   [latency.md](latency.md).

## 4. Haehn, Schlittmeier and Böffel 2024: ambient and character sounds

Haehn L, Schlittmeier SJ, Böffel C. "Exploring the impact of ambient and
character sounds on player experience in video games." *Applied Sciences*
14(2):583, 2024. doi:10.3390/app14020583.
Source check: abstract read (Crossref). Full text not read.

**Method.** 2 experiments, N = 32 each. Participants played League of Legends
in 4 sound conditions: character sounds on or off, crossed with ambient
sounds on or off. Character sounds are sounds that the character's actions
make. Ambient sounds describe the game world. Measures: immersion, avatar
identification, fun, perceived competence (and flow).

**Result.**

- Experiment 1: a non-significant trend of character sounds on avatar
  identification. The task kept players in a small part of the map, so the
  ambient sounds were few.
- Experiment 2 (task changed for more ambient sound): character sounds
  significantly raised immersion, avatar identification and fun. Character
  and ambient sounds interacted on fun. Ambient sounds showed a
  non-significant trend on flow.

**Does show.** Sounds tied to the player's own actions raise immersion,
identification and fun. Ambient sound changes how much the character sounds
add to fun.

**Does not show.** The abstract reports no effect on perceived competence.
The study used 1 game, short sessions and small samples. It does not say
which character sounds matter most, and it does not show that ambient sound
alone raises flow (the trend was not significant).

**Rules.**

1. (research) Every player action that changes the world has a character
   sound. Test it first, before the ambient bed.
2. (research, weak) Keep an ambient bed. Tune it together with the character
   sounds, because the 2 interact. Do not claim that it raises flow.

## 5. Practices and standards used in this skill

| Item | Tag | Source or basis |
| --- | --- | --- |
| Integrated loudness about -24 LUFS for console and PC on a TV, about -18 LUFS for handheld and mobile | standard (industry) | Sony Audio Standards Working Group, ASWG-R001 (2013). LUFS: loudness units relative to full scale, per ITU-R BS.1770. |
| True peak at or below -1 dBTP | standard | ITU-R BS.1770 meter; EBU R128 sets -1 dBTP for broadcast. |
| Audio-visual sync window +45 ms / -125 ms | standard | ITU-R BT.1359-1 (1998). |
| Frequency slots (EQ) so that sound groups do not mask each other | practice | Masking is basic psychoacoustics; the slot plan is mix practice. |
| Pitch and volume variation against repetition fatigue | practice | Common game-audio practice; no study here tests it. |
| Voice limits and per-sound caps | practice | Mix clarity and CPU. |
| Ducking with a sidechain compressor | practice | Mix clarity. |
| Reward escalation with a combo | practice, bounded by Dixon rule 1 | The escalation follows the real gain. |
| Silence as contrast before a big reward | practice | Mix practice. |

Plan to measure each `practice` number in your game. Change it when the
measurement or a play test says so.
