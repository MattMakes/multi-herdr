class_name BenchAdapter
extends RefCounted
## The base of the adapter of a game. The adapter is the only code of the bench that knows the game.
##
## The game writes `res://bench/adapter.gd`. That script extends this script by path:
##     extends "res://addons/tuning_bench/bench_adapter.gd"
## The path works in a headless run of a project that the editor did not import. The class name does not.
##
## All times are game seconds. Override each required method. The optional methods have defaults.
##
## Before you write `setup`, do this checklist. The simulation process runs the whole game, with each autoload:
## 1. List each autoload of `project.godot`.
## 2. For each one that writes a file, reads a setting or reacts to an economy signal, switch it off in `setup()`
##    (stop its autosave, disconnect its handlers, stop its `_process`).
## 3. Run the self-check. Its row "No file written" lists `user://` before and after the simulation and names
##    each file that is new or changed.

## The bench reads this list. A required method that the game did not write adds a line here.
var bench_errors: Array[String] = []

## The bench sets these 2 values before `setup`: the content of `bench.json` and the seed of `sim.seed`.
## An adapter of a game with random effects passes the seed to the game.
var bench_config: Dictionary = {}
var bench_seed: int = 1

## The bench reads this value. The default `away` sets it: the game then has no offline rule for the bench.
var bench_away_missing: bool = false


## Optional. The bench calls this 1 time after it made the adapter. Return a text when the adapter cannot work.
func bench_prepare() -> String:
	return ""


## Required. Start a new game from the start. `data` maps each file of `sources` to its parsed content
## with the controls applied. Read no save file. Write no save file. Switch off each autoload that writes a
## file, reads a setting or reacts to an economy signal (the checklist at the top of this script).
func setup(_data: Dictionary) -> void:
	_missing("setup")


## Required. Advance the economy by `dt` seconds. Use no frame and no real clock.
func step(_dt: float) -> void:
	_missing("step")


## Required. Return each thing that the player can do or see now.
## An action is {"id", "kind", "label", "cost", "affordable"} with the optional keys "element", "gain" and
## "currency". `kind` is "buy", "once", "advance", "reset" or "other".
## Give "currency" (a name) when the cost is not in the main currency. The bot compares the cost of 2 actions
## only when they have the same currency, and `wealth()` is compared only with a cost in the main currency.
## Give "bot": false to a free choice of the player that can repeat with no end (a selector, a switch that goes
## on and off, a click). The view shows it. The bot never does it, and meter 8 does not count it as "never used".
## Without the key such an action stops the run with the error "the bot did 5000 actions at 1 decision".
func actions() -> Array:
	_missing("actions")
	return []


## Required. Do 1 action. Return false when the game refuses it.
func do_action(_id: String) -> bool:
	_missing("do_action")
	return false


## Required. Return the main income for each second now, with no manual action.
func rate() -> float:
	_missing("rate")
	return 0.0


## Required. Return 0 at the start and 1 when the game is complete.
func progress() -> float:
	_missing("progress")
	return 0.0


## Optional. Return the largest `dt` that `step` computes correctly.
func max_step() -> float:
	return 1.0


## Optional. Return the amount of the main currency. A negative value means "not known".
## With this value the bot skips the checks while nothing is affordable.
func wealth() -> float:
	return -1.0


## Optional. The player does the manual action for this time.
## The bench calls this 1 time for each decision, after the steps of that decision, with
## `decision_s x active_share` seconds. It does not call it before the first step and not while the player is away.
## A manual action that is a hold (more income while a button is down): keep the time as a credit here and use
## the credit in `step`. The head of core_adapter.gd has the pattern.
func active(_seconds: float) -> void:
	pass


## Optional. The player closes the game for this time, and the game applies its offline rule.
## The bench calls this in place of `step` for a player with `away_every_min` and `away_min` in `bench.json`.
## Do not call this default from an adapter that has an offline rule: the default says "not supported".
func away(_seconds: float) -> void:
	bench_away_missing = true


## Optional. Return the things that occurred with no action since the last call.
## An event is {"kind": "unlock" or "advance" or "reward" or "bought", "id": String, "label": String}.
## Connect the purchase signal of the game: a purchase that the bot did not ask for with `do_action` (an
## auto-buy) is an event {"kind": "bought", "id": <action id>, "count": n} ("count" is 1 when it is absent).
## Do not report a purchase that `do_action` made: the bench counts that one itself.
func events() -> Array:
	return []


## Optional. Push the values into the game that runs. Return true when the game took them.
## With the default the panel says "restart to apply".
func apply_live(_data: Dictionary) -> bool:
	return false


## Optional. Return true when `setup` touches no autoload and no scene.
func isolated() -> bool:
	return false


func _missing(method: String) -> void:
	var line: String = "the adapter does not have the required method %s()" % method
	if not bench_errors.has(line):
		bench_errors.append(line)
