---
name: make-videos
description: "Use when the operator asks for promotional or devblog videos of a game made from its git history: a new episode series, new episodes after more commits, or a change to episodes that exist. Symptoms: 'make videos', 'devblog', 'milestone video', 'update the videos', a devblog/ folder in a game project."
---

# Make videos: a game's history as a show for players

Turn the game's git history into a series of short promotional videos. Each
video shows 1 big milestone as 1 finished piece, even when the real commits
span weeks. The audience is potential players, not developers. They care about
what the game feels like to play, not how it was built. Every video stays on
gameplay and builds anticipation for how fun the game is.

You plan the series and hold 2 operator gates: the plan table and the scripts.
Workers write the scripts. The `video-*` teammates render them.

| File | What it holds |
|---|---|
| `devblog/00-index.md` | The series decisions and the plan table, with a status per episode |
| `devblog/<NN>-<slug>.md` | 1 episode: script, packaging, production notes |
| [references/video-rules.md](references/video-rules.md) | The operator's rules for every video: audiences and logos, story, craft, accuracy |
| [references/script-format.md](references/script-format.md) | The script structure, the cues, the packaging and the 2 file formats |

Read both reference files before step 3.

## 0. New series or update

Look for `devblog/00-index.md` in the game project.

- Absent: this is a new series. Run steps 1 to 6.
- Present: this is an update. Read it, then go to step 7.

## 1. Understand the game

1. Read the README, the docs, the design notes and any store-page copy. Learn
   what the game is, who it is for and what makes it fun.
2. Read how the game grew. Run
   `git log --first-parent --reverse --date=short --stat` on the main branch.
   Check `git tag` and the merge commits.
3. When the first-parent history has too few commits to show the milestones,
   because the work sits inside merged branches, read every commit reachable
   from main in date order. Say so with the plan table.
4. When a commit message is vague, read the diff to work out what changed for
   the player.
5. For a history too long for your own context, queue 1 `researcher` task: a
   commit catalog with 1 player-facing line per commit. Group the milestones
   yourself.

## 2. Group commits into milestones

1. A milestone is something a player would notice and want, such as:
   - the first playable version
   - a new mechanic, mode, level, enemy or boss
   - a big visual, audio or game-feel overhaul
   - a major gain in performance or platform support
   - a release
2. Internal refactors, tooling and small fixes do not earn an episode of their
   own. Fold each into the episode before it. Those before the first milestone
   join episode 1.
3. Every commit belongs to exactly 1 episode, and each episode covers a
   contiguous range.
4. Aim for 4 to 8 episodes for a new series, depending on the size of the
   history.

## 3. Operator gate 1: the plan table

Before any script exists, show the operator the plan as 1 table with these
columns:

- episode number
- working title
- first and last commit hashes
- commit count
- real date range
- the video's purpose, in 1 sentence
- the hero moment: the single most exciting thing in the video, which becomes
  the climax and the thumbnail

Mark a purpose or a hero moment `[inferred]` when no commit message states it.

In the same message, name the studio (Leviathan Labs unless the operator names
another) and ask for each series decision that `devblog/00-index.md` does not
hold yet:

| Decision | Choices |
|---|---|
| Format | `short` (15 s to 90 s) or `long` (180 s to 900 s, `16:9`, captions only). No film runs 91 s to 179 s |
| Cuts | Any of: social main (YouTube, `16:9`), social vertical (Shorts, TikTok, Reels, X, `9:16`, always 15 s to 30 s), Steam (`16:9`). Every episode gets the same cuts. Say how many productions that makes |
| Call to action | 1 action per audience and where it leads, for example wishlist, follow or join the Discord |
| Voice | voice-over or captions only. `long` is always captions only |

Then end your turn. Write no script before the operator's OK. After the OK,
write `devblog/00-index.md` in the format of the script-format file. Each
episode starts as `planned`.

## 4. Write the scripts

1. Queue 1 ticket per episode for `opus`. Each ticket owns 1 file,
   `devblog/<NN>-<slug>.md`, so all episodes run at the same time.
2. Put in each ticket: the whole plan table with this episode's row marked,
   the commits of its range, the series decisions, the paths of the episode
   files that exist, and these 2 commands for the rules:
   `horch skills read make-videos references/video-rules.md` and
   `horch skills read make-videos references/script-format.md`.
3. Read each finished script against these checks. Send a failed script back
   as a fix ticket.
   - The purpose is at the top, and every beat serves it.
   - The hero moment comes last, then the end card.
   - Each main cut has its own opening and ending from "Audiences and logos".
     Each cut has exactly 1 call to action.
   - No time-of-day card, clock or "day of development" frame.
   - Narration is 20 s or less per minute of runtime. Callouts are 6 words or
     fewer.
   - No commit hash, date, development detail or developer jargon in
     narration, in a caption or on screen.
   - No 2 episodes share a hook, and no hook reuses a clip of an earlier
     episode. Compare the `Hook:` lines of all episode files.
   - The history supports every feature, number and claim. Every `[inferred]`
     flag is in the production notes.
4. Set the episode to `scripted`.

## 5. Operator gate 2: the scripts

Show the operator 1 table: episode, script file, runtime of each cut, hook,
hero moment and the first title option. List every `[inferred]` flag below it.
For a changed script, add 1 line that says what changed. Then end your turn.
No production unit starts before the operator's OK. After the OK, set each
approved episode to `approved`.

## 6. Render

Follow `references/video-pipeline.md` of the `orchestrate` skill. That recipe
and the `video-*` skills own every production rule. This step adds only what
the recipe cannot know:

1. 1 cut is 1 production. An episode with a social main, a social vertical and
   a Steam cut is 3 productions. The social vertical is produced from the
   vertical cutdown script. Use the `batch` workflow for 2 or more.
2. Put in each production request: the script path, the cut, the `Format`, the
   aspect, the call to action and the voice. The producer then asks the
   operator nothing twice.
3. Gate 2 is the script approval that the recipe needs. Its claims check still
   runs: an `[inferred]` flag is not evidence, so verify the claim or cut it.
4. Build the studio logo intro once, as its own unit, before the first social
   main cut. A Steam cut needs the studio logo as a still. When
   `devblog/00-index.md` names an approved intro or still, reuse that file.
   Record a new one there.
5. A `short` social main cut opens with the studio logo intro by operator
   decision. Before critic round 1, write 1 line yourself in that production's
   `qa/accepted-exceptions.md`: the operator's rule replaces the check that a
   short opens on content. A social vertical cut and a Steam cut get no such
   line: they open on gameplay.
6. After the critic passes a cut, add it to `Rendered cuts` of its episode.
   Deliver the masters as the recipe says.

## 7. Update a series

Read `devblog/00-index.md` and the top block of every episode file. Then take
the first row that matches the request.

| The operator asks for | Do this |
|---|---|
| A change to 1 episode | Change that episode's script. When its purpose, hero moment or commit range changes, show the changed row and hold gate 1. Hold gate 2. Render only that episode's cuts |
| A new cut for episodes that exist, for example "also on Steam" | Add it to `Cuts`. Add the new opening and ending, or the vertical cutdown, to each script. Hold gate 2. Render only the new cuts |
| A rule change for the whole series | List the episodes that the change touches. Change each script. Hold gate 2. Render the touched cuts |
| Anything else, for example "update the videos" or "new videos" | Run step 1 for the commits after the last episode's last commit. Group them as in step 2. New episodes continue the numbering. When the new commits hold no milestone, report that and stop: they stay unassigned until a later run. Else run steps 3 to 6 for the new rows only |

Rules for every update:

- Before a script changes, ask for each series decision that the update needs
  and the index does not hold, for example the call to action of a new
  audience. Then end your turn. Ask for nothing that the index holds.
- A script change is a ticket for `opus`, as in step 4. Set the episode back
  to `scripted`.
- Never renumber an episode. Never move the commit range of an episode unless
  the operator asks.
- An episode that you touch meets the current video rules in full. For
  example, an older script with time-of-day cards loses them.
- When the body of a script changes, the masters in `Rendered cuts` no longer
  match it. Name them at gate 2 and ask whether to render them again.
- In your next message to the operator, name each untouched episode that you
  saw break a current rule. Do not change it unasked.
- A new master replaces an old one only by the recipe's blind rank of old
  against new.

## Done means

- `devblog/00-index.md` holds the series decisions and 1 row per episode with
  a true `Status` and true `Rendered cuts`.
- Every commit up to the last episode's last commit is in exactly 1 episode,
  and the ranges are contiguous.
- Every episode file holds the script, the packaging and the production
  notes, and passes the checks of step 4.
- The operator approved the plan table and the scripts before any production
  unit started.
- Each requested cut has a master that passed the critic, or your report names
  the cut that did not and why.
