Adds gameplay events fired from lines, a graph validator that finds broken links before a player does, persistent dialogue state (seen lines, flags), and choice analytics with a custom `Logger`. Read it when dialogue changes the game world, when writers edit the graph, or when the game saves mid-story.

> ← Back to [SKILL.md](../SKILL.md)

# Line Events, Graph Validation and Dialogue State

## Events from lines

A line often changes the game: give an item, start a quest, open a door. Put the event on the **line data** and let the dialogue manager emit it. The UI only shows text, and the NPC only starts the conversation.

| Where the side effect lives | Result |
|---|---|
| In the UI's button handler | A second UI (gamepad, VR) forgets it. Hard to test. |
| In the NPC script after `dialogue_ended` | Fires at the end, not at the line that promised it. |
| **On the line, emitted by the manager** | One place, testable without UI, fires exactly once per visit. |

Add two fields to the line Resource: `@export var events: Array[StringName] = []` and, if events need data, `@export var event_args: Dictionary = {}`. The manager emits them when the line is shown:

```gdscript
# dialogue_event_router.gd
class_name DialogueEventRouter
extends Node

## Emitted for every event name on a shown line. Gameplay systems connect here.
signal dialogue_event(event_name: StringName, args: Dictionary)

var _handlers: Dictionary[StringName, Callable] = {}


## Systems register what they handle: router.register(&"give_item", inventory.give_from_dialogue)
func register(event_name: StringName, handler: Callable) -> void:
    _handlers[event_name] = handler


func fire(events: Array[StringName], args: Dictionary) -> void:
    for event_name in events:
        dialogue_event.emit(event_name, args)
        var handler: Callable = _handlers.get(event_name, Callable())
        if handler.is_valid():
            handler.call(args)
        else:
            push_warning("DialogueEventRouter: no handler for '%s'" % event_name)
```

- **Fire on enter, not on advance.** Call `fire()` where the manager emits `line_displayed`. A player who skips the typewriter still gets the item.
- **Guard one-shot events with state.** "Give 100 gold" must not repeat when the player talks again. Check a flag (below) in the condition of the line, or let the handler record that it ran.
- An unknown event name is a content bug. Warn in development; the validator below catches it before release.

## Validate the graph

A typo in `next_line_id` soft-locks the player in the middle of a conversation. Writers make these typos. Check every dialogue file in a test, not by playing.

The validator takes the plain data the manager already uses: line id -> next id, and line id -> choice targets.

```gdscript
# dialogue_validator.gd
class_name DialogueValidator
extends RefCounted


## `nexts`: line id -> next line id ("" ends the dialogue).
## `choice_targets`: line id -> Array of next ids its choices jump to.
## `known_events`: every event name a handler exists for.
## `line_events`: line id -> Array of event names on that line.
## Returns one message per problem. Empty means valid.
static func validate(start_id: String, nexts: Dictionary, choice_targets: Dictionary,
        line_events: Dictionary, known_events: Array[StringName]) -> PackedStringArray:
    var problems := PackedStringArray()
    if not nexts.has(start_id):
        problems.append("start line '%s' does not exist" % start_id)
        return problems
    var targets_of := func(id: String) -> Array:
        var out: Array = []
        var choices: Array = choice_targets.get(id, [])
        if choices.is_empty():
            out.append(str(nexts[id]))
        else:
            out.append_array(choices)
        return out
    for id: String in nexts:
        for target in targets_of.call(id):
            if target != "" and not nexts.has(target):
                problems.append("line '%s' jumps to missing line '%s'" % [id, target])
        for event_name in line_events.get(id, []):
            if not known_events.has(StringName(event_name)):
                problems.append("line '%s' fires unknown event '%s'" % [id, event_name])
    # Reachability: walk from the start line.
    var seen := {start_id: true}
    var queue: Array[String] = [start_id]
    var ends := false
    while not queue.is_empty():
        var id: String = queue.pop_back()
        for target in targets_of.call(id):
            if target == "":
                ends = true
            elif nexts.has(target) and not seen.has(target):
                seen[target] = true
                queue.append(target)
    for id: String in nexts:
        if not seen.has(id):
            problems.append("line '%s' is unreachable from '%s'" % [id, start_id])
    if not ends:
        problems.append("no path from '%s' reaches an end" % start_id)
    return problems
```

- Run it over every dialogue file in a GUT or gdUnit4 test, and fail the test on any message. That puts the check in the build, where a fleet worker's `DONE:` already runs tests.
- "No path reaches an end" catches a loop with no exit. A deliberate hub loop (a shop menu) has an exit choice, so it passes.
- Conditions are strings evaluated with `Expression` at run time. Parse each one in the test too: `Expression.new().parse(condition, ["GameState"])` returns an `Error` for a syntax error. This finds a typo that would otherwise only fail when the line is reached.

## Dialogue state that survives a save

The UI node is freed on a scene change, so it must not own progress. Keep dialogue state in one autoload object and save it with the rest of the game (see `godot-save-load`).

```gdscript
# dialogue_state.gd
class_name DialogueState
extends RefCounted

## Lines the player has seen, by "<dialogue id>/<line id>".
var seen: Dictionary[String, bool] = {}
## Story flags set by events or choices.
var flags: Dictionary[StringName, Variant] = {}


func mark_seen(dialogue_id: String, line_id: String) -> void:
    seen["%s/%s" % [dialogue_id, line_id]] = true


func has_seen(dialogue_id: String, line_id: String) -> bool:
    return seen.has("%s/%s" % [dialogue_id, line_id])


func to_dict() -> Dictionary:
    var flag_data := {}
    for key in flags:
        flag_data[String(key)] = flags[key]
    return {"seen": seen.keys(), "flags": flag_data}


func from_dict(data: Dictionary) -> void:
    seen.clear()
    flags.clear()
    for key in data.get("seen", []):
        seen[str(key)] = true
    var flag_data: Dictionary = data.get("flags", {})
    for key in flag_data:
        flags[StringName(str(key))] = flag_data[key]
```

- "Seen" lets the UI show a read line in a dimmer color, offer a fast-forward through seen text, and lets a condition say "only the first time".
- Save between lines, never in the middle of a typewriter reveal. If the game saves while a conversation is open, save the dialogue id and the current line id, and on load restart from that line.
- Flags from JSON come back with JSON types: numbers are floats. Compare with `int()` where a count matters.

## Choice analytics without I/O in the dialogue code

To learn which choices players pick, log them, then collect the log in playtests. Since 4.5, `OS.add_logger()` registers a custom `Logger` that receives every engine and script message. The dialogue code only prints a tagged line; the logger filters and stores it.

```gdscript
# choice_logger.gd
class_name ChoiceLogger
extends Logger

const TAG := "[CHOICE] "

var lines: PackedStringArray = []
var _mutex := Mutex.new()


func _log_message(message: String, _error: bool) -> void:
    if not message.begins_with(TAG):
        return
    # The engine can call a logger from any thread.
    _mutex.lock()
    lines.append(message.trim_prefix(TAG).strip_edges())
    _mutex.unlock()


func _log_error(_function: String, _file: String, _line: int, _code: String,
        _rationale: String, _editor_notify: bool, _error_type: int,
        _script_backtrace: Array[ScriptBacktrace]) -> void:
    pass
```

Register it once at start-up: `OS.add_logger(choice_logger)`. In the manager's `choose()`: `print("[CHOICE] %s/%s -> %d" % [dialogue_id, line_id, index])`. Log ids, never the player's name or other personal data.
