# Capture queue

Read this when you set up real-window captures for workers that cannot open a window (a
sandbox, a headless container, a remote agent). One runner outside the sandbox does every
windowed run, one at a time, under the window lock.

## Procedure

Use this when a worker runs in a sandbox or headless, or every window lock is busy.

1. Run 1 runner outside the sandbox. It watches a queue folder, takes a free window lock, runs
   each request with a time limit and the requester's scratch save, and writes `<id>.log` and
   `<id>.done` with the exit code.
2. Write the capture script as a `SceneTree` script. It builds the exact state, waits some
   frames, saves 1 PNG per state and quits. Start a quit watchdog first, so a failed capture
   cannot hold the lock. Get autoloads from the tree root at run time, because their global
   names do not resolve in a `-s` script.
3. Parse-check the capture script headless before you queue it.
4. Write 1 request file per job: line 1 the requester name, line 2 the worktree path, line 3
   the Godot arguments with no shell characters. Put every script option after `--`. Use
   absolute script paths.
5. Use a new request id per round (`-2`, `-3`). The runner skips an id that has a `.done` file.
   Delete stale `.done` files before you queue an old id again.
6. Wait on the `.done` file with 1 bounded wait. Do not poll with long `sleep` loops.
7. Read the log for script errors. Fix and queue again.
8. Check each image as in SKILL.md method steps 5 and 8. Repeat for the second renderer.
9. Copy the chosen images to the review folder. Record each path and what it shows.

## Folder layout

```
<queue>/
  <id>.req    request, written by the worker
  <id>.log    stdout and stderr of the Godot run, written by the runner
  <id>.done   exit code on 1 line, written by the runner last
```

The `.done` file is the signal. The runner writes it after the log is complete.

## Request file

3 lines, plain text, no shell characters (`;`, `|`, `&`, `$`, backticks, quotes):

```
<requester name>
<absolute worktree path>
--resolution 1280x720 --rendering-method forward_plus -s <absolute capture script path> -- --state=menu --out=<absolute output folder>
```

- Line 1 names the requester. The runner uses it in the log and for the scratch save slot.
- Line 2 is the project root the runner passes with `--path`.
- Line 3 is the Godot argument list. Put every script option after `--`.

## Runner outline

1. Loop over `<queue>/*.req` whose id has no `.done` file.
2. Reject a request whose line 3 has a shell character. Write `.done` with a non-zero code.
3. Take a free window lock with `mkdir <lock dir>`. Write an owner file. If every lock is busy,
   try again on the next loop.
4. Run Godot with `--path <line 2>` and the words of line 3, as separate arguments. Do not pass
   them through a shell. Set the scratch save location for the requester.
5. Kill the run after a time limit, for example 300 s.
6. Write `<id>.log`, then `<id>.done` with the exit code.
7. Release the lock inside the branch that took it, and only if the owner file names this runner.

## Capture script shape

```gdscript
extends SceneTree
# Usage: godot --path <project> -s <this script> -- --state=<name> --out=<absolute dir>

const WATCHDOG_S := 60.0

func _initialize() -> void:
	# Quit even if a later step fails, so the window lock is released.
	create_timer(WATCHDOG_S).timeout.connect(func(): quit(2))
	_run.call_deferred()

func _run() -> void:
	var args := _parse(OS.get_cmdline_user_args())
	# Get autoloads from the root at run time, for example root.get_node_or_null("Settings").
	# Their global names do not resolve under -s.
	var scene: Node = load("res://<scene>.tscn").instantiate()
	root.add_child(scene)
	# Build the state here: load a scratch save, place actors, drain queues.
	for i in 60:
		await process_frame
	await RenderingServer.frame_post_draw
	var img := root.get_texture().get_image()
	img.save_png(args.out.path_join(args.state + ".png"))
	quit(0)

func _parse(user_args: PackedStringArray) -> Dictionary:
	var out := {"state": "default", "out": "/tmp"}
	for a in user_args:
		var kv := a.trim_prefix("--").split("=", true, 1)
		if kv.size() == 2:
			out[kv[0]] = kv[1]
	return out
```

This is an outline, not a tested file. Adapt the scene path, the state build and the shot
list. Parse-check it headless before you queue it.

## Worker side

1. Parse-check the capture script.
2. Write `<id>.req`. Use a new id per round, for example `menu-1`, `menu-2`.
3. Wait on `<id>.done` with 1 bounded wait (for example a file monitor with a 10 minute limit).
4. Read `<id>.log`. A script error means the images are missing or stale.
5. Check the pixel size and the subject of each image. Then review it as in the main method.
