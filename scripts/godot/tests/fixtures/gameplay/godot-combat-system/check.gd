extends SceneTree
## godot-combat-system scenarios: the "Checks" lists of references/real-time.md
## and references/turn-based.md, on the skill's own blocks in res://skill/.

const Expect = preload("res://expect.gd")

var e := Expect.new()


func _initialize() -> void:
	e.watch(self)
	# The root enters the tree after _initialize starts; _ready needs it.
	await process_frame
	_damage()
	_swing()
	_hurtbox()
	_turn_order()
	_timeline()
	_budget()
	_gauges()
	_grid()
	await _knockback_and_death()
	await _hit_stop()
	e.finish(self)


func _hit(amount: int, types: int) -> DamageInfo:
	return DamageInfo.new(amount, types)


func _stats(defense: int, resistances: Dictionary) -> DefenseStats:
	var s := DefenseStats.new()
	s.defense = defense
	for t: int in resistances:
		s.resistances[t as DamageInfo.Type] = resistances[t]
	return s


func _damage() -> void:
	var fire_half := _stats(2, {DamageInfo.Type.FIRE: 0.5})
	e.check(DamageResolver.resolve(_hit(10, DamageInfo.Type.FIRE), fire_half) == 4,
		"10 fire, defense 2, fire resistance 0.5: final 4")
	var crit := _hit(10, DamageInfo.Type.FIRE)
	crit.is_critical = true
	e.check(DamageResolver.resolve(crit, fire_half) == 7, "the same hit critical: (15 - 2) * 0.5 = 6.5 rounds to 7")
	var immune := _stats(2, {DamageInfo.Type.FIRE: 0.0})
	e.check(DamageResolver.resolve(_hit(10, DamageInfo.Type.FIRE), immune) == 0, "a fire immunity: final 0")
	var mixed := _stats(2, {DamageInfo.Type.FIRE: 0.5, DamageInfo.Type.ICE: 1.5})
	e.check(DamageResolver.resolve(_hit(10, DamageInfo.Type.FIRE | DamageInfo.Type.ICE), mixed) == 12,
		"fire and ice: the strongest multiplier (1.5) applies")
	var pierce := _hit(10, DamageInfo.Type.FIRE)
	pierce.pierces_defense = true
	e.check(DamageResolver.resolve(pierce, fire_half) == 5, "pierces_defense skips defense, keeps resistance")
	e.check(DamageResolver.resolve(_hit(1, DamageInfo.Type.PHYSICAL), _stats(5, {})) == 1,
		"defense above the amount: MIN_DAMAGE 1")
	var rng := RandomNumberGenerator.new()
	rng.seed = 7
	var always := _hit(1, DamageInfo.Type.PHYSICAL)
	DamageResolver.roll_critical(always, 1.0, rng)
	var never := _hit(1, DamageInfo.Type.PHYSICAL)
	DamageResolver.roll_critical(never, 0.0, rng)
	e.check(always.is_critical and not never.is_critical, "roll_critical with chance 1 and 0")


func _swing() -> void:
	var swing := SwingTracker.new()
	var target := RefCounted.new()
	swing.begin_swing()
	var first: bool = swing.try_hit(target)
	var second: bool = swing.try_hit(target)
	swing.begin_swing()
	e.check(first and not second and swing.try_hit(target),
		"try_hit twice in one swing: true, then false; a new swing hits again")


func _hurtbox() -> void:
	var hurtbox: Area2D = load("res://skill/real-time_4.gd").new()
	hurtbox.defense_stats = _stats(2, {DamageInfo.Type.FIRE: 0.5})
	var got: Array[int] = []
	hurtbox.hit_resolved.connect(func(_hit_info: DamageInfo, final_amount: int) -> void: got.append(final_amount))
	hurtbox.receive_attack(_hit(10, DamageInfo.Type.FIRE))
	e.check(got == [4], "the hurtbox receive_attack emits hit_resolved with the resolved amount")
	hurtbox.free()


func _turn_order() -> void:
	var a1 := TurnActor.new(1, "slow", 10)
	var a2 := TurnActor.new(2, "fast", 30)
	var a3 := TurnActor.new(3, "fast too", 30)
	var queue := RoundQueue.new()
	var rounds: Array[int] = []
	queue.round_started.connect(func(n: int) -> void: rounds.append(n))
	queue.setup([a1, a2, a3] as Array[TurnActor])
	var order: Array[int] = []
	for i: int in 3:
		order.append(queue.next_turn().id)
	e.check(order == [2, 3, 1], "speeds 10, 30, 30 (ids 1, 2, 3): order 2, 3, 1")
	# Round 2: actor 3 dies after actor 2 acts.
	var first: TurnActor = queue.next_turn()
	a3.alive = false
	var second: TurnActor = queue.next_turn()
	var third: TurnActor = queue.next_turn()
	e.check(first.id == 2 and second.id == 1 and third.id == 2 and rounds == [1, 2, 3],
		"an actor that dies mid-round does not act; the round still ends")
	# Round 3: actor 2 has acted; actor 1 gets faster than nobody left: re-sort keeps the rest.
	a1.speed = 50
	queue.resort_remaining()
	e.check(queue.next_turn().id == 1, "resort_remaining orders the actors who have not acted")
	a1.alive = false
	a2.alive = false
	e.check(queue.next_turn() == null, "next_turn returns null when no actor is alive")


func _timeline() -> void:
	var fast := TurnActor.new(1, "fast", 50)
	var slow := TurnActor.new(2, "slow", 25)
	var line := TimelineQueue.new()
	line.add(fast)
	line.add(slow)
	var preview: Array[TurnActor] = line.preview(6)
	var fast_turns: int = preview.filter(func(a: TurnActor) -> bool: return a == fast).size()
	e.check(preview.size() == 6 and fast_turns == 4, "speeds 50 and 25: the fast actor has 4 of 6 turns")
	var real: Array[TurnActor] = []
	for i: int in 6:
		real.append(line.next_actor())
	e.check(real == preview, "the preview equals the real order and does not change the state")


func _budget() -> void:
	var budget := TurnBudget.new()
	e.check(not budget.spend(1), "spend outside the MAIN phase is refused")
	budget.advance_phase()
	var paid: bool = budget.spend(2)
	var refused: bool = budget.spend(1)
	e.check(paid and not refused and budget.points == 0, "spend with too few points returns false and keeps the points")
	budget.advance_phase()
	budget.advance_phase()
	e.check(budget.phase == TurnBudget.Phase.START and budget.points == budget.max_points,
		"the END phase resets the points")


func _gauges() -> void:
	var atb: Node = load("res://skill/turn-based_4.gd").new()
	var hero := TurnActor.new(1, "hero", 50)
	var imp := TurnActor.new(2, "imp", 20)
	atb.actors = [hero, imp] as Array[TurnActor]
	var ready: Array[TurnActor] = []
	atb.actor_ready.connect(func(a: TurnActor) -> void: ready.append(a))
	for i: int in 10:
		atb._physics_process(0.25)
	e.check(ready == [hero] and not atb.running, "the gauge stops every actor when the first is full")
	atb.command_done(hero)
	e.check(atb.running and atb.gauges[hero.id] == 0.0, "command_done empties the gauge and restarts time")
	atb.free()


func _grid() -> void:
	var board: Node = load("res://skill/turn-based_6.gd").new()
	var walls: Array[Vector2i] = [Vector2i(2, 0), Vector2i(2, 1), Vector2i(2, 2), Vector2i(2, 3)]
	board.build(Vector2i(5, 5), walls)
	var path: Array[Vector2i] = board.path_cells(Vector2i(0, 0), Vector2i(4, 0))
	var through_wall: bool = walls.any(func(c: Vector2i) -> bool: return c in path)
	e.check(not path.is_empty() and not through_wall and Vector2i(2, 4) in path,
		"AStarGrid2D: the path goes around the wall")
	board.set_occupied(Vector2i(2, 4), true)
	e.check(board.path_cells(Vector2i(0, 0), Vector2i(4, 0)).is_empty(),
		"set_occupied blocks the last gap without update()")
	board.free()


func _knockback_and_death() -> void:
	var body: CharacterBody3D = load("res://skill/real-time_7.gd").new()
	var shape := CollisionShape3D.new()
	shape.shape = SphereShape3D.new()
	body.add_child(shape)
	root.add_child(body)
	body.apply_knockback(Vector3(6, 0, 0))
	# 6 m/s decays at 12 m/s per second: zero after 0.5 s (30 frames at 60 Hz).
	for i: int in 40:
		await physics_frame
	e.check(body.position.x > 1.0 and body.velocity.length() < 0.01,
		"knockback moves the body, then decays to zero")
	body.queue_free()

	var dying: CharacterBody3D = load("res://skill/real-time_8.gd").new()
	var body_shape := CollisionShape3D.new()
	body_shape.shape = SphereShape3D.new()
	var hurt_shape := CollisionShape3D.new()
	hurt_shape.shape = SphereShape3D.new()
	dying.add_child(body_shape)
	dying.add_child(hurt_shape)
	dying.body_shape = body_shape
	dying.hurtbox_shape = hurt_shape
	root.add_child(dying)
	dying._on_died()
	await physics_frame
	e.check(body_shape.disabled and hurt_shape.disabled and not dying.is_physics_processing(),
		"_on_died disables both shapes (deferred) and the physics process")
	dying.queue_free()


func _hit_stop() -> void:
	var stopper: Node = load("res://skill/real-time_6.gd").new()
	root.add_child(stopper)
	stopper.hit_stop(0.1)
	var slowed: bool = is_equal_approx(Engine.time_scale, 0.05)
	await create_timer(0.3, true, false, true).timeout
	e.check(slowed and Engine.time_scale == 1.0, "hit_stop slows time, then restores 1.0 on a real-time timer")
	# Overlap: a longer stop during a short one; only the last timer restores.
	stopper.hit_stop(0.05)
	stopper.hit_stop(0.4)
	await create_timer(0.2, true, false, true).timeout
	var still_slow: bool = is_equal_approx(Engine.time_scale, 0.05)
	await create_timer(0.4, true, false, true).timeout
	e.check(still_slow and Engine.time_scale == 1.0, "an overlapping longer stop extends the stop")
	stopper.queue_free()
