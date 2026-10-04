# Branching, failure and time

These extend the `QuestLog` in `SKILL.md`. Keep each rule in the log or in
a small node beside it, never in enemy or NPC scripts.

## Branching outcomes

A quest with choices ("give the amulet to the priest or to the thief") has
one objective per branch, all `optional`, plus a required objective that the
choice completes. Record the choice as a world flag, so later quests and
dialogue can read it:

1. The dialogue emits an event, for example `notify(&"choice", &"amulet_priest")`.
2. The matching optional objective advances.
3. A listener of `objective_progressed` sets the flag `&"amulet_to_priest"`
   in the save data and sends `notify(&"choice_made", &"amulet")`, which
   completes the required objective.

Follow-up quests list the flag as a condition. Add a `required_flags:
Array[StringName]` field to `QuestDef` and check it in `_prerequisites_met`
next to the prerequisites. Mutually exclusive quests ("join the guild" or
"join the rebels") each fail the other on accept: `fail(&"join_rebels")`
when `join_guild` is accepted.

## Failure

A quest fails when a condition it depends on breaks: an escort target dies,
a timer runs out, the player picks the other side. Rules:

- The thing that breaks sends an event; a small fail-rule node maps events
  to `fail(quest_id)`. Example: an escort NPC emits `died(&"mara")`; the rule
  says "if `escort_mara` is active, fail it".
- `fail` works only on an active quest, so a late event after completion
  does nothing.
- Decide whether a failed quest can restart. To allow it, add a `retry`
  method that sets the status back to `AVAILABLE` and clears its counts.

## Timed quests

```gdscript
class_name QuestTimers
extends Node

@export var quests: QuestLog

var _left: Dictionary[StringName, float] = {}


func _ready() -> void:
	quests.quest_accepted.connect(_on_accepted)


func _on_accepted(quest_id: StringName) -> void:
	var def: QuestDef = quests.definition(quest_id)
	if def.time_limit_sec > 0.0:
		_left[quest_id] = def.time_limit_sec


func seconds_left(quest_id: StringName) -> float:
	return _left.get(quest_id, 0.0)


func _physics_process(delta: float) -> void:
	for quest_id: StringName in _left.keys():
		if quests.status(quest_id) != QuestLog.Status.ACTIVE:
			_left.erase(quest_id)
			continue
		_left[quest_id] -= delta
		if _left[quest_id] <= 0.0:
			_left.erase(quest_id)
			quests.fail(quest_id)
```

- The timer counts game time, so it stops when the tree pauses (set the
  node's `process_mode` to pausable, the default). A real-time limit
  ("within 24 hours") stores a Unix deadline from
  `Time.get_unix_time_from_system()` instead.
- Save `_left` with the quest log's data, or a reload gives a full timer.
- A countdown on the HUD reads `seconds_left` on a `Timer` tick, a few times
  a second, not every frame.

## Hidden objectives

An objective with `hidden = true` exists from the start but the tracker does
not show it until it has progress (or until a flag reveals it). The log
treats it like any other objective; only the UI hides it. Use it for
surprise bonus goals and for the second half of a twist.

## Chains

A chain is quests linked by `prerequisites`. Keep the graph valid:

- Every prerequisite id exists. Check at boot after all `register` calls,
  and `push_error` on an unknown id.
- No cycles. Run a depth-first search over the prerequisites at boot in a
  debug build; a cycle locks every quest in it forever.
- A test loads every `QuestDef` in the project and runs both checks. A data
  error then fails the test, not the play session.

## Repeatable quests

Daily or bounty quests return to `AVAILABLE` after completion. Add a
`repeatable: bool` and a cooldown to `QuestDef`; on completion, store the
Unix time and set the status back when the cooldown passes. Grant the reward
on each completion; the reward handler is still called once per completion.

## Many updates at once

Area damage can kill 20 enemies in one physics frame. `notify` handles that
(it clamps each count at `required`), and `quest_completed` fires once
because the status changes on the first completion. If quest events come
from threads (for example a simulation on `WorkerThreadPool`), do not call
the log from the thread: `quests.notify.call_deferred(event, subject, n)`
moves the call to the main thread.

## Checks

- An escort quest: the NPC's death event fails it; a death after completion
  does not.
- A timed quest with 0.5 s: it fails after the time; a quest completed in
  time is not failed.
- A prerequisite that names a missing quest id is reported at boot.
- 20 kill events in one frame for a "kill 10" objective: the count is 10 and
  completion fires once.
