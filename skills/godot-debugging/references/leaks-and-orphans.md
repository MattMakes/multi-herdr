Adds leak hunting: finding orphan nodes and growing object counts with the Performance monitors and the orphan-node API, and the usual causes; read it when memory or the node count climbs during play, or a test reports orphans.

# Leaks and Orphan Nodes

> ← Back to [SKILL.md](../SKILL.md). Memory monitors and the `queue_free` / `free` rules are in **godot-optimization** [memory-management.md](../../godot-optimization/references/memory-management.md).

All code targets Godot 4.7. Measured on 4.7.2 headless.

---

## 1. What an Orphan Is

A node that is not in the scene tree and was never freed is an **orphan**.
Nodes are not reference counted: `remove_child()` without `queue_free()`, or
`instantiate()` without `add_child()`, keeps the node and its whole subtree in
memory until the game ends. Resources (`RefCounted`) free themselves; nodes
do not.

## 2. Measure

| Tool | Gives |
|---|---|
| `Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT)` | The number of orphan nodes now |
| `Node.get_orphan_node_ids()` | Their instance ids (static) |
| `Node.print_orphan_nodes()` | Prints each orphan with its class and path (static) |
| `Performance.OBJECT_NODE_COUNT`, `OBJECT_COUNT`, `OBJECT_RESOURCE_COUNT` | Totals that should level off during steady play |

The orphan monitor and the orphan functions work only in debug builds
(editor runs and debug exports). In a release export the monitor reads 0, so
a leak test must run on a debug build.

On 4.7.2 headless, one `Node.new()` that is never added gives a monitor value
of 1 and one id from `get_orphan_node_ids()`.

## 3. A Leak Check Around a Repeated Action

Run the suspect action many times (open and close a menu, spawn and kill a
wave, load and unload a level), let deferred frees run, and compare counts.
A count that grows with the number of repeats is a leak.

```gdscript
extends Node

## Repeats `action` and reports node and orphan growth. Debug builds only.
func check_for_leaks(action: Callable, repeats: int = 20) -> bool:
	await _settle()
	var nodes_before := Performance.get_monitor(Performance.OBJECT_NODE_COUNT)
	var orphans_before := Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT)
	for i in repeats:
		await action.call()
	await _settle()
	var node_growth := Performance.get_monitor(Performance.OBJECT_NODE_COUNT) - nodes_before
	var orphan_growth := Performance.get_monitor(Performance.OBJECT_ORPHAN_NODE_COUNT) - orphans_before
	if orphan_growth > 0:
		push_warning("Leak check: %d new orphan nodes after %d repeats" % [orphan_growth, repeats])
		Node.print_orphan_nodes()
	if node_growth > 0:
		push_warning("Leak check: node count grew by %d" % node_growth)
	return orphan_growth <= 0 and node_growth <= 0


## queue_free() takes effect at the end of the frame; wait two frames.
func _settle() -> void:
	await get_tree().process_frame
	await get_tree().process_frame
```

`await action.call()` also works when the action is not a coroutine. In a
test framework, GUT and gdUnit4 report orphans per test as well; read their
orphan output instead of ignoring it.

## 4. Usual Causes

| Cause | Fix |
|---|---|
| `remove_child(node)` to "hide" or move it, then the reference is dropped | Call `queue_free()`, or keep a reference and re-add it on purpose. |
| A pool that removes nodes from the tree and loses track of them | Keep pooled nodes in the tree and hide them, or free the pool on exit. |
| `instantiate()` in a branch that returns before `add_child()` | Free the instance on every early return. |
| A dictionary or array in an autoload that still holds nodes from an old level | Erase entries on `tree_exiting`, or store instance ids and check them with `is_instance_id_valid()`. |
| Signal connections from an autoload to a freed node | Connections to freed objects are removed with them; a lambda that **captured** a freed node is not. See [signal-tracing.md](signal-tracing.md). |
| A `Resource` that holds a node reference | Resources outlive scenes; never store nodes in shared resources. |

## 5. Growing Object Counts Without Orphans

When `OBJECT_COUNT` grows but orphans do not, the leak is in objects that
are still reachable: nodes in the tree that nobody removes (bullets that fly
forever, hidden UI rows), or `Object`s that are not `RefCounted` and need an
explicit `free()`. Print the tree with `print_tree_pretty()` (see
[scene-tree-debugging.md](scene-tree-debugging.md)) at two moments and
compare, or count nodes per group.

A backtrace captured with local variables
(`Engine.capture_script_backtraces(true)`) stores their values, so it holds
references to them for as long as you keep the backtrace. Leave variables
off in a leak hunt, or the capture itself keeps objects alive.
