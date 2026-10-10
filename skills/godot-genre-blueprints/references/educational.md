# Educational

Games where the player learns a real skill or fact set: language drills,
maths practice, training simulations. Duolingo and DragonBox are the
reference points.

## Core loop

Learn → apply → get feedback → the game adapts → master the topic → the next
topic opens. The mechanic must be the learning itself. A quiz between
unrelated action scenes teaches badly and plays badly.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Learner profile | mastery per topic, XP, streaks; a Resource, not UI state | `godot-resource-pattern`, `godot-save-load` |
| Lesson flow | question → answer → result → next | `godot-state-machine` |
| Topic map | prerequisite graph; a topic opens when its parents reach mastery | `godot-quest-system` (unlock logic) |
| Readable UI | anchors, containers, large touch targets, focus | `godot-ui`, `godot-responsive-ui` |
| Language and text | translation keys, plurals | `godot-localization` |
| Reward feedback | XP bar tweens, particles on correct answers | `godot-tween-animation`, `godot-particles-vfx` |
| Tablets and low-end laptops | battery, touch, export | `godot-mobile-development`, `godot-optimization` |

## Scene tree (4.7)

```text
App (Node)
├── LearnerProfile (autoload; holds the profile Resource, saves it)
├── Scheduler (Node; review queue, below)
├── TopicMap (Control; the prerequisite graph)
└── Lesson (Control; one per exercise type)
    ├── Prompt (RichTextLabel)
    ├── AnswerArea (Container; drag targets or buttons)
    ├── Feedback (Control; hint, "try again", confetti)
    └── ProgressBar
```

## Genre code

Two rules carry most of the teaching value. Spaced review: an item answered
correctly comes back after a longer gap; an item answered wrongly comes back
soon. Adaptive difficulty: keep the recent success rate near 70 to 80%.

```gdscript
extends RefCounted

const GAPS_SECONDS := [0, 60, 600, 3600, 86400, 259200, 864000]

var box := {}   # item_id -> index into GAPS_SECONDS
var due := {}   # item_id -> unix time when the item is due
var recent: Array[bool] = []

func record(item_id: StringName, correct: bool, now: float) -> void:
	var b: int = box.get(item_id, 0)
	b = mini(b + 1, GAPS_SECONDS.size() - 1) if correct else 1
	box[item_id] = b
	due[item_id] = now + GAPS_SECONDS[b]
	recent.append(correct)
	if recent.size() > 10:
		recent.pop_front()

func difficulty_step() -> int:
	if recent.size() < 5:
		return 0
	var rate := float(recent.count(true)) / recent.size()
	if rate > 0.85:
		return 1    # harder
	if rate < 0.65:
		return -1   # easier; also offer a hint
	return 0
```

Use `Time.get_unix_time_from_system()` for `now`, so review gaps survive an
app restart.

## Pitfalls

- "Game over" on a wrong answer. Give a hint and a retry; failure must be
  safe.
- Long instruction text. Show the mechanic with a short demo or an
  interactive first step.
- Mastery and XP stored in labels or progress bars. The profile Resource is
  the truth; UI listens to its signals.
- Hard-coded strings. Classrooms span languages: use translation keys and
  `tr()` from the start.
- Absolute pixel layouts. Lessons run on tablets and laptops; use anchors
  and containers.
- An invisible overlay with `mouse_filter` set to Stop swallows taps on the
  answer buttons. Set overlays to Pass or Ignore.
- Text-to-speech that starts without consent. Offer a toggle; use
  `DisplayServer.tts_speak()` only when it is on.
- Labels updated every frame. Update on change; turn on
  `OS.low_processor_usage_mode` for static lesson screens to save battery.
- Credentials for a school back end inside the export. Exported files can
  be read; use a server-side proxy.
- Live debug metrics: use `Performance.add_custom_monitor()` instead of a
  custom dashboard.
