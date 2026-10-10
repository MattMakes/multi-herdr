# The event-to-sound map

> Back to [SKILL.md](../SKILL.md). The event bus itself is in
> `godot-event-bus`.

Gameplay code emits events on an event bus and never plays a sound. One
sound director listens to the bus and plays sounds from a data table. This
gives 3 things:

1. **One place to tune.** Every stream, bus, level, variation, cap and
   cooldown is a row in 1 resource.
2. **Same-frame playback.** The director's handler runs inside `emit()`, so
   the sound starts on the frame of the event ([latency.md](latency.md)).
3. **A checkable contract.** A test lists the bus signals and the map rows,
   and fails on a gap.

## 1. One row: `SoundCue`

```gdscript
class_name SoundCue
extends Resource

## One row of the event-to-sound map.
enum Kind { ACTION, FREQUENT, REWARD, BAD_NEWS, UI, LOOP }

@export var stream: AudioStream
@export var kind: Kind = Kind.ACTION
@export var bus: StringName = &"SFX"
@export var volume_db: float = 0.0
## Random pitch range in semitones (+/-). Keep 0 for a cue that carries a value.
@export_range(0.0, 2.0, 0.1) var pitch_semitones: float = 0.0
## Random volume range in dB (+/-).
@export_range(0.0, 3.0, 0.1) var volume_jitter_db: float = 0.0
@export var cooldown_ms: int = 0
@export var max_voices: int = 3
## Higher wins when the pool is full: danger 50, reward 40, hit 30,
## action 20, frequent 10, UI 5.
@export var priority: int = 20
```

## 2. The table: `SoundMap`

```gdscript
class_name SoundMap
extends Resource

## Event id -> cue. The event id is usually the bus signal name.
@export var cues: Dictionary[StringName, SoundCue] = {}


func cue(event_id: StringName) -> SoundCue:
	return cues.get(event_id, null)


## Rows that break a rule of the skill. Empty means the map passes.
func problems() -> PackedStringArray:
	var out := PackedStringArray()
	for event_id: StringName in cues:
		var row: SoundCue = cues[event_id]
		if row == null or row.stream == null:
			out.append("%s: no stream" % event_id)
			continue
		if row.kind == SoundCue.Kind.REWARD and row.pitch_semitones > 0.0:
			out.append("%s: a reward cue must not vary its pitch" % event_id)
		if row.kind == SoundCue.Kind.FREQUENT and row.pitch_semitones <= 0.0 \
				and not row.stream is AudioStreamRandomizer:
			out.append("%s: a frequent cue needs variation" % event_id)
		if row.kind == SoundCue.Kind.FREQUENT and row.cooldown_ms <= 0 and row.max_voices > 3:
			out.append("%s: a frequent cue needs a cooldown or a cap of 3" % event_id)
	return out
```

## 3. The director

The director is an autoload. It owns a small voice pool, applies the row,
and enforces the cap and the cooldown. Simple events map 1 to 1 with
`Callable.unbind()`. An event that carries a value (a reward) gets its own
handler, so the value chooses the sound.

```gdscript
class_name SoundDirector
extends Node

## Plays cues for event-bus signals. Add as an autoload after the event bus.
@export var sound_map: SoundMap
@export var reward_tiers: RewardTiers
@export var voice_count: int = 24

var _voices: Array[AudioStreamPlayer] = []
var _voice_cue: Array[StringName] = []
var _voice_priority: PackedInt32Array = PackedInt32Array()
var _last_ms: Dictionary[StringName, int] = {}


func _ready() -> void:
	for i: int in voice_count:
		var player := AudioStreamPlayer.new()
		add_child(player)
		_voices.append(player)
		_voice_cue.append(&"")
		_voice_priority.append(0)


## Connects every map row whose id is a signal of `bus` with no special handler.
func connect_simple(bus: Object, special: Array[StringName]) -> void:
	for info: Dictionary in bus.get_signal_list():
		var signal_name: StringName = info["name"]
		if special.has(signal_name) or sound_map.cue(signal_name) == null:
			continue
		var arg_count: int = (info["args"] as Array).size()
		var handler: Callable = play_cue.bind(signal_name)
		# unbind(0) is an error, so drop arguments only when the signal has some.
		bus.connect(signal_name, handler.unbind(arg_count) if arg_count > 0 else handler)


## Plays a cue now. Returns false when a cap, a cooldown or the pool stops it.
## `stream` replaces the row's stream for this call (a reward tier).
func play_cue(event_id: StringName, extra_db: float = 0.0, pitch: float = 1.0,
		stream: AudioStream = null) -> bool:
	var row: SoundCue = sound_map.cue(event_id)
	if row == null:
		return false
	var chosen: AudioStream = stream if stream != null else row.stream
	if chosen == null:
		return false
	var now: int = Time.get_ticks_msec()
	if row.cooldown_ms > 0 and now - _last_ms.get(event_id, -100000) < row.cooldown_ms:
		return false
	if _active_count(event_id) >= row.max_voices:
		return false
	var slot: int = _pick_slot(row.priority)
	if slot < 0:
		return false
	_last_ms[event_id] = now
	var player: AudioStreamPlayer = _voices[slot]
	player.stream = chosen
	player.bus = row.bus
	player.volume_db = row.volume_db + extra_db + randf_range(-row.volume_jitter_db, row.volume_jitter_db)
	var semitones: float = randf_range(-row.pitch_semitones, row.pitch_semitones)
	player.pitch_scale = pitch * pow(2.0, semitones / 12.0)
	player.play()
	_voice_cue[slot] = event_id
	_voice_priority[slot] = row.priority
	return true


## Honest reward: the net gain picks the tier; a net loss plays nothing.
func play_reward(event_id: StringName, net_gain: int) -> bool:
	var tier_stream: AudioStream = reward_tiers.stream_for(net_gain)
	if tier_stream == null:
		return false
	return play_cue(event_id, 0.0, 1.0, tier_stream)


func _active_count(event_id: StringName) -> int:
	var count: int = 0
	for i: int in _voices.size():
		if _voices[i].playing and _voice_cue[i] == event_id:
			count += 1
	return count


func _pick_slot(priority: int) -> int:
	var best: int = -1
	for i: int in _voices.size():
		if not _voices[i].playing:
			return i
		if _voice_priority[i] <= priority and (best < 0 or _voice_priority[i] < _voice_priority[best]):
			best = i
	return best
```

## 4. Wiring

```gdscript
extends Node

## Example wiring in the director's owner. "Events" is the event-bus autoload.
@onready var director: SoundDirector = $SoundDirector


func _ready() -> void:
	var events: Node = get_node(^"/root/Events")
	var special: Array[StringName] = [&"coins_banked"]
	director.connect_simple(events, special)
	events.connect(&"coins_banked", _on_coins_banked)


func _on_coins_banked(gross: int, cost: int) -> void:
	director.play_reward(&"coins_banked", gross - cost)
```

## 5. The contract test

Run it headless. It needs no audio device.

```gdscript
extends SceneTree

## Fails when a bus signal has no cue, a cue has no signal, or a row breaks
## a rule. Edit the 2 paths and the list of silent signals for your game.
const MAP_PATH: String = "res://audio/sound_map.tres"
const BUS_SCRIPT: String = "res://systems/events.gd"
const SILENT: Array[StringName] = [&"run_saved"]


func _initialize() -> void:
	var failures: PackedStringArray = PackedStringArray()
	var sound_map: SoundMap = load(MAP_PATH)
	var bus_script: GDScript = load(BUS_SCRIPT)
	var names: Array[StringName] = []
	for info: Dictionary in bus_script.get_script_signal_list():
		names.append(info["name"])
	for signal_name: StringName in names:
		if not SILENT.has(signal_name) and sound_map.cue(signal_name) == null:
			failures.append("%s: no cue and not listed as silent" % signal_name)
	for event_id: StringName in sound_map.cues:
		if not names.has(event_id):
			failures.append("%s: cue for a signal that does not exist" % event_id)
	failures.append_array(sound_map.problems())
	for line: String in failures:
		push_error(line)
	quit(1 if failures.size() > 0 else 0)
```

`get_script_signal_list()` lists only the signals that the bus script
declares, not the inherited `Node` signals. The director's `connect_simple()`
uses `get_signal_list()`, but it skips every signal without a map row, so the
inherited signals do no harm there.
