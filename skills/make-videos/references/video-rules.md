# Video rules

Every script, cut, title and thumbnail follows these rules. They are the
operator's rules. A ticket or a production plan may quote them. It may not
loosen them.

## What makes these videos work

Most viewers meet these videos in a feed, often with the sound off. They
decide within a second or two whether to keep watching. Every video makes
complete sense on mute: captions carry the narration and the footage carries
the story.

The game is the star and the narrator is the guide. Most of the runtime is
gameplay, with narration laid over footage, not in place of it. When you are
tempted to describe something, show it. If a line of narration only repeats
what the footage already shows, cut the line.

Every clip gets about a second to make sense. Choose moments with one clear
focal point: the player character is easy to find, the action reads at a
glance and nothing else pulls focus. A spectacular moment the viewer cannot
follow is worse than a simple one they get instantly. Use crops, zooms and
slow motion to steer the eye to what matters.

Rhythm turns footage into hype. Cut on action (the swing, the jump, the
impact) and on the music's beats. Fast sections can use shots of 1 s to 2 s
while the payoff gets room to breathe, and that contrast makes the climax feel
big. Game audio does half the work of making play feel rewarding, so let hits,
pickups and level-up sounds cut through the music at key moments.

Put the viewer in the game. Say what "you" can do far more than what "I"
built. Every beat shows something the player gets to experience, and builds
anticipation for playing it.

These videos are episodes of a show, so they need a recognizable identity:
the same logo intro, the same end card and the same voice. A returning viewer
knows what they are watching within seconds. Every episode still works as
someone's first, with nothing that assumes they saw the last one.

Packaging decides whether anyone watches at all. On YouTube, the title and the
thumbnail matter as much as the first 10 seconds. Treat them as part of the
script, with the same single purpose as the video.

## Audiences and logos

A cut is 1 video for 1 audience. There are 2 audiences: social feeds and the
Steam store page. The `Cuts` decision in `devblog/00-index.md` says which of
the 3 cuts every episode gets.

The social main cut and the Steam cut share 1 script body and 1 hook. Only the
opening and the ending change. The social vertical cut is the vertical
cutdown: its own script of 15 s to 30 s with its own hook, always `short`.

| Cut | Plays on | Opens with | Ends with |
|---|---|---|---|
| Social main, `16:9` | YouTube | The studio logo intro, then the hook | Hero moment, then the end card: game logo and the call to action |
| Social vertical, `9:16` | Shorts, TikTok, Reels, X | Gameplay in motion in the first second. No logo, title card or face | Hero moment, then the end card: game logo and the call to action |
| Steam, `16:9` | The Steam store page | Gameplay in motion in the first frame. No logo, title card or face | Hero moment, then the end card: game logo, the call to action, and the studio logo small and subdued |

- The studio is Leviathan Labs unless the operator names another.
- The studio logo intro is the same clip in every social main cut. The first
  shot after it is gameplay in motion and proves the hook.
- A Steam cut never opens with a studio logo. Its first shot is the hook. Its
  studio logo is a still: small and quiet next to the game's logo, on the end
  card only.
- No video has time-of-day cards, a clock or a "day of development" frame.

## Story

- **One purpose.** Each video makes the viewer want one specific thing about
  the game, for example "the new grappling hook makes movement feel
  incredible". State the purpose at the top of the script. Every beat, shot and
  line serves it. Cut anything that does not, even if it is true and
  interesting.
- **Every line has a job.** Each line of narration and each on-screen caption
  sets up the purpose, shows it or pays it off. No filler. No throat-clearing
  ("so today I..."). No generic hype words: say something concrete.
- **Build hype.** Escalate through the video so the hero moment lands last.
  End with the viewer wanting to play, and wanting the next episode.
- **Make it exciting and rewarding.** Show the game at its most satisfying:
  wins, combos, unlocks, discoveries, crisp feedback. The viewer feels the
  payoff of playing, not only sees that a feature exists.
- **Viewer first.** Use "you" for what the player can do. Use "I" only for a
  short aside from the developer, and keep it rare.
- **Every episode stands alone.** Never rely on the viewer having seen an
  earlier episode.
- **One call to action.** Each cut asks for exactly 1 thing.

## Craft

- **No rough edges.** No placeholder art, debug overlays, unfinished menus,
  crashes or awkward animation. Film only the polished parts of the game. If
  something is not ready to look great, leave it out or tease it without
  showing it.
- **No development details in the video.** Commit hashes, dates and how long
  the work took never appear in narration, captions or shots. They are for the
  operator only.
- **Mostly gameplay.** Between the opening and the end card, the runtime is
  gameplay footage. Any face-cam shot runs under 5 seconds.
- **Narration budget.** At most 20 seconds of narration per minute of runtime.
- **Captions.** Burn in captions for all narration. Keep on-screen callouts to
  6 words or fewer, up long enough to read twice. Narration is every line the
  narrator says. With captions only, each narration line is a burned-in
  caption and still counts against the budget. A callout is a label on the
  footage, not a narration line.
- **No developer jargon.** No "refactor", "shader", "commit", "build",
  "branch", "pipeline" or engine names in narration or text, unless the term
  is part of the game's own content.
- **Capture specs.** Capture at maximum settings and 60 fps. Keep the mouse
  cursor, OS notifications and debug text out of frame.
- **HUD.** Hide the HUD unless it tells the story, such as damage numbers, XP
  or an unlock pop-up.
- **Music.** Music cues describe mood, tempo and energy only. Never name
  commercial songs. All music is original or properly licensed.
- **Endings.** End on the hero moment and then the end card, never on a
  talking head.
- **Fresh hooks.** Never reuse a clip from an earlier episode as a hook.

## Accuracy

- No fiction. Hype only works when it is true, so the history has to support
  every feature and claim. This applies to titles, thumbnails and captions
  too.
- Do not invent features, player counts, reviews, quotes or release dates.
- Use specific numbers ("12 new weapons") only when the history confirms them.
- Avoid superlatives you cannot back up. Skip comparisons to other games
  unless the operator supplies them.
- If a claim does not come from a commit message, because you worked it out
  from a diff or it still needs a look at the game, flag it `[inferred]` in
  the production notes so the operator can check it.
