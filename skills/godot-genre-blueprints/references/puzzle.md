# Puzzle

Logic games where the player tests ideas against clear rules: push-block,
match-3, physics and connection puzzles. Baba Is You, Sokoban and The
Witness are the reference points.

## Core loop

Look → form an idea → make a move → see the result at once → undo or reset
if wrong → solve → the next puzzle adds one new rule.

## Required systems

| system | what this genre needs | skill |
|---|---|---|
| Grid model | `Vector2i` cells in a Dictionary or array; the model is the truth | `godot-gdscript-patterns` |
| Undo and redo | every move reversible, unlimited steps | this reference (`UndoRedo`) |
| Level data | one Resource per level: layout, start state, par moves | `godot-resource-pattern` |
| Piece motion | tween to the cell after the model changes | `godot-tween-animation` |
| Input | one move per press, repeat on hold, grid snapping | `godot-input-handling` |
| Hints and solvers | path search on the grid | `godot-ai-navigation` (`AStarGrid2D`) |
| Progress | solved levels, best move counts | `godot-save-load` |

## Scene tree (4.7)

```text
Puzzle (Node2D)
├── Board (Node; grid model, rules, win check, UndoRedo)
├── Floor (TileMapLayer; static tiles only)
├── Pieces (Node2D; one Sprite2D per movable piece, view only)
├── Goals (Node2D)
└── HUD (CanvasLayer; moves, undo, reset, level name)
```

## Genre code

Godot's `UndoRedo` stores both directions of each move. The do and undo
methods only change the model and then refresh the view.

```gdscript
extends Node

signal changed

var cells: Dictionary[Vector2i, StringName] = {}   # cell -> piece kind
var undo := UndoRedo.new()

func try_push(from: Vector2i, dir: Vector2i) -> bool:
	var to := from + dir
	if cells.get(to, &"") != &"":
		return false   # blocked; chain pushes go here
	undo.create_action("push")
	undo.add_do_method(_move.bind(from, to))
	undo.add_undo_method(_move.bind(to, from))
	undo.commit_action()   # runs the do method
	return true

func _move(a: Vector2i, b: Vector2i) -> void:
	cells[b] = cells[a]
	cells.erase(a)
	changed.emit()

func _exit_tree() -> void:
	undo.free()
```

Check the win condition after each commit, undo and redo, not in
`_process()`. Bind undo, redo and reset to keys the player can always use.

## Pitfalls

- No undo. Experiments become punishment. Undo is required, and it must be
  unlimited within a level.
- Two undo systems (a custom command stack and `UndoRedo`). Pick one.
- Do and undo logic in one function. Keep them separate and symmetric.
- `Vector2` grid positions. Rounding drifts; use `Vector2i`.
- Node positions as the truth. Tweens end late or get killed; the model
  decides, the nodes follow.
- A new tween while one runs on the same piece. Kill the old tween first,
  or finish it.
- An unsolvable state with no notice. Detect it when you can, or make
  reset obvious.
- Rules the player cannot see. A powered wire glows; a blocked move bumps
  and plays a sound.
- Pixel-precise input. Snap to the grid.
- Tutorials in text. Introduce each rule alone in a small level, then
  combine it.
- Modifying a Dictionary while iterating over it. Iterate over
  `keys()` copies, or collect changes and apply them after.
- `AStarGrid2D` used after changes without `update()`. Call it after
  changing `region` or `cell_size`; set solids with `set_point_solid()`.
  Set `diagonal_mode` to match the rules.
