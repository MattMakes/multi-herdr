extends SceneTree
## godot-gameplay-loops scenarios: the "Checks" list of each reference, on the
## skill's own blocks in res://skill/. The 3 three-dimensional nodes
## (CollectiblePickup, HarvestNode, a GhostTrack recorder and player) run in
## the scene tree with real physics frames.

const Expect = preload("res://expect.gd")

var e := Expect.new()


func _initialize() -> void:
	e.watch(self)
	# The root enters the tree after _initialize starts; _ready needs it.
	await process_frame
	_collection()
	_harvest_rules()
	_secrets()
	_time_trial()
	await _pickup_in_scene()
	await _harvest_node_in_scene()
	await _ghost_in_scene()
	await _look_spot()
	await _revival()
	await _waves()
	e.finish(self)


func _frames(n: int) -> void:
	for i: int in n:
		await physics_frame


func _shape(radius: float) -> CollisionShape3D:
	var shape := CollisionShape3D.new()
	shape.name = "CollisionShape3D"
	var sphere := SphereShape3D.new()
	sphere.radius = radius
	shape.shape = sphere
	return shape


# collection.md
func _collection() -> void:
	var t := CollectionTracker.new()
	var done: Array[StringName] = []
	t.set_completed.connect(func(id: StringName) -> void: done.append(id))
	t.define_set(&"gems", PackedStringArray(["a", "b", "c"]))
	var first: bool = t.collect(&"gems", &"a")
	var again: bool = t.collect(&"gems", &"a")
	e.check(first and not again and t.remaining(&"gems").size() == 2,
		"collect an item twice: the second call returns false, and the count does not change")
	e.check(not t.collect(&"gems", &"z"), "collect an id outside the set: the call returns false")
	t.collect(&"gems", &"b")
	var loaded := CollectionTracker.new()
	loaded.from_save(JSON.parse_string(JSON.stringify(t.to_save())))
	loaded.define_set(&"gems", PackedStringArray(["a", "b", "c"]))
	e.check(loaded.remaining(&"gems") == PackedStringArray(["c"]),
		"save after 2 of 3, load into a new tracker: remaining() has 1 id")
	t.collect(&"gems", &"c")
	t.collect(&"gems", &"c")
	e.check(done == [&"gems"], "collect the last item: set_completed fires once")
	t.free()
	loaded.free()


# harvest.md: the rules without a scene
func _harvest_rules() -> void:
	var reg := HarvestRegistry.new()
	reg.mark_depleted(&"oak_1", 60)
	var past: int = int(Time.get_unix_time_from_system()) - 10
	var back := HarvestRegistry.new()
	back.from_save(JSON.parse_string(JSON.stringify({"back_at": {"oak_1": past}})))
	e.check(not reg.is_available(&"oak_1") and reg.seconds_left(&"oak_1") > 55 and back.is_available(&"oak_1"),
		"mark_depleted with 60 s: not available; a registry loaded with a past time: available")
	reg.free()
	back.free()
	var passive: Node = load("res://skill/harvest_5.gd").new()
	for i: int in 60:
		passive._physics_process(1.0 / 60.0)
	var after_one_second: int = passive.stored
	for i: int in 70:
		passive._physics_process(1.0 / 60.0)
	e.check(after_one_second == 0 and passive.stored == 1, "a 0.5 per second rate pays 1 whole unit after 2 s")
	passive.free()


# secrets.md: the code matcher and the meta unlocks
func _secrets() -> void:
	var m := SequenceMatcher.new()
	m.codes = {&"up_up_down": PackedStringArray(["ui_up", "ui_up", "ui_down"])} as Dictionary[StringName, PackedStringArray]
	var hits: Array[StringName] = []
	m.sequence_matched.connect(func(id: StringName) -> void: hits.append(id))
	for step: Array in [[&"ui_up", 0], [&"ui_up", 300], [&"ui_down", 600]]:
		m.push_action(step[0], step[1])
	e.check(hits == [&"up_up_down"], "a code with gaps under max_gap_msec: sequence_matched fires")
	hits.clear()
	for step: Array in [[&"ui_up", 1000], [&"ui_up", 1700], [&"ui_down", 1800]]:
		m.push_action(step[0], step[1])
	e.check(hits.is_empty(), "the same code with one gap over the limit: no match")
	for step: Array in [[&"ui_left", 3000], [&"ui_up", 3100], [&"ui_up", 3200], [&"ui_down", 3300]]:
		m.push_action(step[0], step[1])
	e.check(hits == [&"up_up_down"], "a wrong press, then the full code: the match fires")
	m.free()
	MetaUnlocks.unlock(&"x")
	e.check(MetaUnlocks.is_unlocked(&"x") and not MetaUnlocks.is_unlocked(&"y"),
		"MetaUnlocks.unlock(&\"x\"), then is_unlocked(&\"x\") is true in a new call")


# time-trial.md: the clock and the track math
func _time_trial() -> void:
	var clock := RaceClock.new()
	clock.lap_count = 1
	var laps: Array = []
	var finished: Array = []
	clock.lap_completed.connect(func(lap: int, usec: int) -> void: laps.append([lap, usec]))
	clock.race_finished.connect(func(total: int, best: bool) -> void: finished.append([total, best]))
	clock.start(1_000_000)
	var early_finish: bool = clock.cross_finish_line(1_500_000)
	var wrong_order: bool = clock.pass_checkpoint(2, 1_600_000)
	for i: int in [1, 2, 3]:
		clock.pass_checkpoint(i, 1_000_000 + i * 2_000_000)
	clock.cross_finish_line(9_000_000)
	e.check(not wrong_order, "checkpoint 2 before 1: pass_checkpoint returns false")
	e.check(not early_finish, "the finish line before all checkpoints: no lap")
	e.check(laps == [[1, 8_000_000]] and finished == [[8_000_000, true]],
		"checkpoints 1, 2, 3, then the finish line: one lap with the expected lap_usec")
	clock.free()
	var track := GhostTrack.new()
	track.sample_interval = 0.1
	track.add_sample(Transform3D(Basis(), Vector3(0, 0, 0)))
	track.add_sample(Transform3D(Basis(), Vector3(2, 0, 4)))
	e.check(track.sample_at(0.05).origin.is_equal_approx(Vector3(1, 0, 2)),
		"sample_at halfway between two samples returns the middle position")
	var fmt: GDScript = load("res://skill/time-trial_3.gd")
	e.check(fmt.format_race_time(83_456_789) == "01:23.456", "format_race_time shows minutes, seconds, milliseconds")


# collection.md: CollectiblePickup in a scene, picked up by a moving body
func _pickup_in_scene() -> void:
	var tracker := CollectionTracker.new()
	tracker.name = "Collection"
	root.add_child(tracker)
	tracker.define_set(&"shards", PackedStringArray(["s1", "s2"]))
	tracker.collect(&"shards", &"s2")
	var found_before := CollectiblePickup.new()
	found_before.set_id = &"shards"
	found_before.item_id = &"s2"
	found_before.add_child(_shape(1.0))
	var pickup := CollectiblePickup.new()
	pickup.set_id = &"shards"
	pickup.item_id = &"s1"
	pickup.add_child(_shape(1.0))
	var body := CharacterBody3D.new()
	body.add_child(_shape(0.5))
	body.position = Vector3(10, 0, 0)
	root.add_child(found_before)
	root.add_child(pickup)
	root.add_child(body)
	await _frames(3)
	var waiting: bool = is_instance_valid(pickup) and not tracker.is_found(&"shards", &"s1")
	body.position = Vector3.ZERO
	await _frames(5)
	e.check(not is_instance_valid(found_before), "a pickup whose item is already found frees itself in _ready")
	e.check(waiting and tracker.is_found(&"shards", &"s1") and not is_instance_valid(pickup),
		"a body that enters the pickup's Area3D collects the item, and the pickup frees itself")
	body.queue_free()
	tracker.queue_free()
	await process_frame


# harvest.md: HarvestNode in a scene
func _harvest_node_in_scene() -> void:
	var source := HarvestSource.new()
	source.needs_kind = HarvestTool.Kind.AXE
	source.min_tier = 2
	source.hits_to_break = 3
	source.yield_item = &"wood"
	source.yield_min = 2
	source.yield_max = 4
	var node := HarvestNode.new()
	node.source = source
	node.add_child(_shape(1.0))
	root.add_child(node)
	var refused: Array[StringName] = []
	var landed: Array[int] = []
	var harvested: Array = []
	node.hit_refused.connect(func(r: StringName) -> void: refused.append(r))
	node.hit_landed.connect(func(left: int) -> void: landed.append(left))
	node.harvested.connect(func(item: StringName, n: int) -> void: harvested.append([item, n]))
	var pickaxe := HarvestTool.new()
	pickaxe.kind = HarvestTool.Kind.PICKAXE
	pickaxe.tier = 3
	var weak_axe := HarvestTool.new()
	weak_axe.tier = 1
	node.hit(pickaxe)
	node.hit(weak_axe)
	e.check(refused == [&"wrong_tool", &"tier_too_low"] and landed.is_empty(),
		"wrong tool and low tier: hit_refused fires, and the hit count does not change")
	var axe := HarvestTool.new()
	axe.tier = 2
	for i: int in 4:
		node.hit(axe)
	await physics_frame
	var shape: CollisionShape3D = node.get_node(^"CollisionShape3D")
	e.check(landed == [2, 1, 0] and harvested.size() == 1 and harvested[0][0] == &"wood"
		and harvested[0][1] >= 2 and harvested[0][1] <= 4,
		"hits_to_break hits with power 1: harvested fires once with an amount in range")
	e.check(not node.visible and shape.disabled, "a depleted node hides and disables its collision")
	node.restore()
	await physics_frame
	e.check(node.visible and not shape.disabled, "restore shows the node and enables its collision")
	node.queue_free()


# time-trial.md: the recorder and the player of a GhostTrack, on nodes
func _ghost_in_scene() -> void:
	var racer := Node3D.new()
	var ghost := Node3D.new()
	root.add_child(racer)
	root.add_child(ghost)
	var track := GhostTrack.new()
	track.sample_interval = 0.05
	var since: float = 0.0
	var recorded: Array[Vector3] = []
	# Recorder: the racer moves 1 m and turns 0.02 rad per physics frame.
	for i: int in 30:
		await physics_frame
		var dt: float = get_root().get_physics_process_delta_time()
		racer.position.x += 1.0
		racer.rotate_y(0.02)
		since += dt
		if since >= track.sample_interval - 0.0001:
			since -= track.sample_interval
			track.add_sample(racer.global_transform)
			recorded.append(racer.global_position)
	# Player: the ghost follows the track at play time.
	var play_time: float = 0.0
	var worst: float = 0.0
	for i: int in 24:
		await physics_frame
		play_time += get_root().get_physics_process_delta_time()
		ghost.global_transform = track.sample_at(play_time)
		var expected: Vector3 = track.sample_at(play_time).origin
		worst = maxf(worst, ghost.global_position.distance_to(expected))
	var last: int = recorded.size() - 1
	ghost.global_transform = track.sample_at(last * track.sample_interval)
	e.check(recorded.size() >= 9 and worst < 0.001 and ghost.global_position.is_equal_approx(recorded[last])
		and is_equal_approx(ghost.global_basis.get_euler().y, racer.global_basis.get_euler().y),
		"a GhostTrack recorded from a moving node plays back on a ghost node")
	racer.queue_free()
	ghost.queue_free()


# secrets.md: the look-at spot
func _look_spot() -> void:
	var camera := Camera3D.new()
	root.add_child(camera)
	var spot: Node3D = load("res://skill/secrets_2.gd").new()
	spot.camera = camera
	spot.hold_sec = 0.1
	spot.position = Vector3(0, 0, -3)
	var behind: Node3D = load("res://skill/secrets_2.gd").new()
	behind.camera = camera
	behind.hold_sec = 0.1
	behind.position = Vector3(0, 0, 3)
	var revealed: Array[Node] = []
	spot.revealed.connect(func() -> void: revealed.append(spot))
	behind.revealed.connect(func() -> void: revealed.append(behind))
	root.add_child(spot)
	root.add_child(behind)
	await create_timer(0.3).timeout
	e.check(revealed == [spot], "a spot in front of the camera for hold_sec is revealed; one behind it is not")
	for n: Node in [camera, spot, behind]:
		n.queue_free()


# revival.md
func _revival() -> void:
	var d := RespawnDirector.new()
	d.respawn_delay_sec = 0.05
	root.add_child(d)
	var spawn := Transform3D(Basis(), Vector3(5, 0, 0))
	d.offer_checkpoint(&"c1", 1, Transform3D.IDENTITY)
	d.offer_checkpoint(&"c3", 3, spawn)
	var lower: bool = d.offer_checkpoint(&"c2", 2, Transform3D.IDENTITY)
	e.check(not lower and d.checkpoint_id == &"c3", "checkpoints with progress 1, then 3, then 2: the active one stays 3")
	var player := CharacterBody3D.new()
	player.add_child(_shape(0.5))
	root.add_child(player)
	player.position = Vector3(9, 9, 9)
	player.velocity = Vector3(3, 0, 0)
	var deaths: Array[StringName] = []
	var respawns: Array[StringName] = []
	d.player_died.connect(func(cause: StringName) -> void: deaths.append(cause))
	d.player_respawned.connect(func(id: StringName) -> void: respawns.append(id))
	d.on_player_died(player, &"lava")
	d.on_player_died(player, &"spikes")
	await create_timer(0.3).timeout
	e.check(deaths == [&"lava"] and respawns == [&"c3"], "a death during the pause of another death is ignored")
	e.check(player.velocity == Vector3.ZERO and player.global_transform.is_equal_approx(spawn)
		and player.is_physics_processing(), "after the respawn, velocity is zero and the transform is the spawn")
	d.set_flag(&"bridge_down")
	var copy := RespawnDirector.new()
	copy.from_save(JSON.parse_string(JSON.stringify(d.to_save())))
	e.check(copy.checkpoint_id == &"c3" and copy.checkpoint_progress == 3 and copy.spawn_transform.is_equal_approx(spawn)
		and copy.world_flags.has(&"bridge_down"), "to_save() then from_save() keeps the checkpoint and the flags")
	copy.free()
	d.queue_free()
	player.queue_free()


# waves.md
func _waves() -> void:
	var enemy := Node3D.new()
	var scene := PackedScene.new()
	scene.pack(enemy)
	enemy.free()
	var wave1 := WaveDefinition.new()
	var entry := WaveEntry.new()
	entry.enemy_scene = scene
	entry.count = 3
	entry.interval_sec = 0.0
	wave1.entries = [entry] as Array[WaveEntry]
	wave1.break_after_sec = 0.05
	var wave2 := WaveDefinition.new()
	var entry2 := WaveEntry.new()
	entry2.enemy_scene = scene
	entry2.count = 1
	entry2.interval_sec = 0.0
	wave2.entries = [entry2] as Array[WaveEntry]
	wave2.break_after_sec = 0.05
	var parent := Node3D.new()
	var point := Marker3D.new()
	point.position = Vector3(4, 0, 0)
	root.add_child(parent)
	root.add_child(point)
	var dir := WaveDirector.new()
	dir.waves = [wave1, wave2] as Array[WaveDefinition]
	dir.spawn_points = [point] as Array[Marker3D]
	dir.enemy_parent = parent
	root.add_child(dir)
	var started: Array[int] = []
	var cleared: Array[int] = []
	var all_clear: Array[bool] = []
	dir.wave_started.connect(func(n: int, _total: int) -> void: started.append(n))
	dir.wave_cleared.connect(func(n: int) -> void: cleared.append(n))
	dir.all_waves_cleared.connect(func() -> void: all_clear.append(true))
	dir.start_next_wave()
	dir.start_next_wave()
	await create_timer(0.1).timeout
	e.check(started == [1] and dir.alive == 3 and parent.get_child_count() == 3,
		"start_next_wave() twice in a row starts one wave; alive reaches 3")
	e.check(parent.get_child(0).global_position.is_equal_approx(Vector3(4, 0, 0)), "an enemy spawns at a spawn point")
	parent.get_child(0).queue_free()
	parent.get_child(1).queue_free()
	await process_frame
	var not_yet: bool = cleared.is_empty() and dir.alive == 1
	parent.get_child(0).queue_free()
	await process_frame
	e.check(not_yet and cleared == [1], "wave_cleared fires after the third enemy is freed, not before")
	await create_timer(0.2).timeout
	e.check(started == [1, 2] and dir.alive == 1, "after the break the next wave starts")
	parent.get_child(0).queue_free()
	await create_timer(0.3).timeout
	e.check(cleared == [1, 2] and all_clear == [true], "after the last wave clears, all_waves_cleared fires once")
	var picker: Node = load("res://skill/waves_4.gd").new()
	picker.enemy_scenes = [PackedScene.new(), scene] as Array[PackedScene]
	picker.weights = PackedFloat32Array([0.0, 1.0])
	e.check(picker.pick_enemy() == scene, "the weighted picker never picks a weight of 0")
	picker.free()
	for n: Node in [dir, parent, point]:
		n.queue_free()
