extends SceneTree
## godot-quest-system scenarios: "Prove it" in SKILL.md and the "Checks"
## lists of references/branching-and-timing.md and references/ui-and-world.md,
## on the skill's own blocks in res://skill/.

const Expect = preload("res://expect.gd")

var e := Expect.new()


func _initialize() -> void:
	e.watch(self)
	# The root enters the tree after _initialize starts; _ready needs it.
	await process_frame
	_chain_and_save()
	_rewards()
	_escort()
	_burst()
	_boot_check()
	await _timers()
	await _tracker()
	e.finish(self)


func _objective(id: StringName, event: StringName, subject: StringName, required: int,
		hidden: bool = false) -> ObjectiveDef:
	var o := ObjectiveDef.new()
	o.id = id
	o.event = event
	o.subject = subject
	o.required = required
	o.hidden = hidden
	o.text_key = "OBJ_%s" % id
	return o


func _quest(id: StringName, objectives: Array[ObjectiveDef], needs: Array[StringName] = []) -> QuestDef:
	var q := QuestDef.new()
	q.id = id
	q.title_key = "TITLE_%s" % id
	q.objectives = objectives
	q.prerequisites = needs
	return q


func _new_log() -> QuestLog:
	var log := QuestLog.new()
	root.add_child(log)
	return log


func _chain_and_save() -> void:
	var log := _new_log()
	var a := _quest(&"a", [_objective(&"slimes", &"kill", &"slime", 2), _objective(&"herb", &"collect", &"herb", 1)])
	log.register(a)
	log.register(_quest(&"b", [_objective(&"bats", &"kill", &"bat", 3)], [&"a"] as Array[StringName]))
	var completed: Array[StringName] = []
	log.quest_completed.connect(func(id: StringName, _def: QuestDef) -> void: completed.append(id))
	e.check(log.status(&"b") == QuestLog.Status.LOCKED and not log.accept(&"b"), "B needs A: B is LOCKED")
	e.check(log.accept(&"a") and not log.accept(&"a"), "a quest is accepted once")
	log.notify(&"kill", &"bat")
	log.notify(&"kill", &"slime")
	e.check(log.count(&"a", &"slimes") == 1 and log.count(&"a", &"herb") == 0,
		"a kill event advances only the matching objective")
	log.notify(&"collect", &"herb")
	log.notify(&"kill", &"slime")
	log.notify(&"kill", &"slime")
	e.check(completed == [&"a"] and log.status(&"a") == QuestLog.Status.COMPLETED,
		"the completion signal fires once even if more events arrive")
	e.check(log.status(&"b") == QuestLog.Status.AVAILABLE, "B is AVAILABLE after A completes")
	log.accept(&"b")
	log.notify(&"kill", &"bat")
	var text: String = JSON.stringify(log.to_save())
	var loaded := _new_log()
	loaded.register(a)
	loaded.register(_quest(&"b", [_objective(&"bats", &"kill", &"bat", 3)], [&"a"] as Array[StringName]))
	loaded.from_save(JSON.parse_string(text))
	e.check(loaded.status(&"a") == QuestLog.Status.COMPLETED and loaded.status(&"b") == QuestLog.Status.ACTIVE
		and loaded.count(&"b", &"bats") == 1 and loaded.count(&"a", &"slimes") == 2,
		"a save through JSON and a load into a new log restores the status and the counts")
	log.queue_free()
	loaded.queue_free()


func _rewards() -> void:
	var log := _new_log()
	var q := _quest(&"r", [_objective(&"wolf", &"kill", &"wolf", 1)])
	q.reward_currency = {&"gold": 25} as Dictionary[StringName, int]
	log.register(q)
	var purse_script := GDScript.new()
	purse_script.source_code = "extends Node\nvar got: Array = []\nfunc grant(c, a, r):\n\tgot.append([c, a, r])\n\treturn a\n"
	purse_script.reload()
	var purse: Node = purse_script.new()
	var bridge: Node = load("res://skill/SKILL_4.gd").new()
	bridge.quests = log
	bridge.wallet = purse
	root.add_child(purse)
	root.add_child(bridge)
	log.accept(&"r")
	bridge.on_enemy_killed(&"wolf")
	e.check(purse.got == [[&"gold", 25, &"quest"]], "the event bridge advances the quest and grants the reward once")
	for n: Node in [log, purse, bridge]:
		n.queue_free()


func _escort() -> void:
	var log := _new_log()
	log.register(_quest(&"escort_mara", [_objective(&"reach", &"reach", &"gate", 1)]))
	log.register(_quest(&"escort_tom", [_objective(&"reach", &"reach", &"bridge", 1)]))
	var fail_rule := func(npc: StringName) -> void:
		var quest_id := StringName("escort_%s" % npc)
		if log.status(quest_id) == QuestLog.Status.ACTIVE:
			log.fail(quest_id)
	log.accept(&"escort_mara")
	log.accept(&"escort_tom")
	fail_rule.call(&"mara")
	log.notify(&"reach", &"bridge")
	fail_rule.call(&"tom")
	e.check(log.status(&"escort_mara") == QuestLog.Status.FAILED
		and log.status(&"escort_tom") == QuestLog.Status.COMPLETED,
		"escort: the NPC's death fails it; a death after completion does not")
	log.queue_free()


func _burst() -> void:
	var log := _new_log()
	log.register(_quest(&"cull", [_objective(&"kills", &"kill", &"rat", 10)]))
	# A lambda captures a local int by value: count in an array.
	var completions: Array[StringName] = []
	log.quest_completed.connect(func(id: StringName, _def: QuestDef) -> void: completions.append(id))
	log.accept(&"cull")
	for i: int in 20:
		log.notify(&"kill", &"rat")
	e.check(log.count(&"cull", &"kills") == 10 and completions.size() == 1,
		"20 kill events in one frame for kill 10: the count is 10 and completion fires once")
	log.queue_free()


func _boot_check() -> void:
	var log := _new_log()
	log.register(_quest(&"x", [], [&"no_such_quest"] as Array[StringName]))
	var unknown: Array[StringName] = []
	for quest_id: StringName in log.quest_ids():
		for pre: StringName in log.definition(quest_id).prerequisites:
			if log.definition(pre) == null:
				unknown.append(pre)
	e.check(unknown == [&"no_such_quest"], "a prerequisite that names a missing quest id is found at boot")
	log.queue_free()


func _timers() -> void:
	var log := _new_log()
	var slow := _quest(&"slow", [_objective(&"o", &"reach", &"far", 1)])
	slow.time_limit_sec = 0.5
	var quick := _quest(&"quick", [_objective(&"o", &"reach", &"near", 1)])
	quick.time_limit_sec = 0.5
	log.register(slow)
	log.register(quick)
	var timers := QuestTimers.new()
	timers.quests = log
	root.add_child(timers)
	log.accept(&"slow")
	log.accept(&"quick")
	await physics_frame
	log.notify(&"reach", &"near")
	var counting: bool = timers.seconds_left(&"slow") > 0.0
	await create_timer(0.8).timeout
	e.check(counting and log.status(&"slow") == QuestLog.Status.FAILED
		and log.status(&"quick") == QuestLog.Status.COMPLETED,
		"a timed quest with 0.5 s fails after the time; a quest completed in time is not failed")
	log.queue_free()
	timers.queue_free()


func _texts(box: Node) -> Array[String]:
	var out: Array[String] = []
	for child: Node in box.get_children():
		if not child.is_queued_for_deletion():
			out.append(child.text)
	return out


func _tracker() -> void:
	var log := _new_log()
	log.register(_quest(&"hunt", [
		_objective(&"boars", &"kill", &"boar", 3),
		_objective(&"secret", &"collect", &"tusk", 1, true),
	]))
	var tracker: VBoxContainer = load("res://skill/ui-and-world_1.gd").new()
	tracker.quests = log
	root.add_child(tracker)
	log.accept(&"hunt")
	e.check(_texts(tracker) == ["TITLE_hunt", "OBJ_boars 0/3"],
		"accept a quest: the tracker shows it with 0/N; a hidden objective at 0 is not shown")
	var line: Label = tracker._lines[&"hunt/boars"]
	log.notify(&"kill", &"boar")
	e.check(line.text == "OBJ_boars 1/3" and tracker._lines[&"hunt/boars"] == line,
		"an event updates the line to 1/N without a rebuild")
	log.notify(&"collect", &"tusk")
	e.check(_texts(tracker) == ["TITLE_hunt", "OBJ_boars 1/3", "OBJ_secret 1/1"],
		"a hidden objective is shown after its first event")
	log.notify(&"kill", &"boar")
	log.notify(&"kill", &"boar")
	await process_frame
	e.check(_texts(tracker).is_empty() and tracker.get_child_count() == 0, "a completed quest leaves the tracker")
	tracker.queue_free()
	log.queue_free()
