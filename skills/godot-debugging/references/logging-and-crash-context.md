Adds structured logging and error capture: a custom `Logger` (Godot 4.5+) that collects engine and script errors, script backtraces with `ScriptBacktrace`, thread-safe logging, and checks that still run in release; read it when errors must reach a file, an in-game console or a crash report, or when logs come from several threads.

# Logging and Crash Context

> ← Back to [SKILL.md](../SKILL.md). `print`, `push_error` and `print_debug` basics are in SKILL.md section 1.

All code targets Godot 4.7. Measured on 4.7.2 headless.

---

## 1. Catch Every Error with a Custom Logger (4.5+)

Extend `Logger` and register an instance with `OS.add_logger()`. The engine
then calls it for every message:

- `_log_message(message, error)` for `print()`-style output (`error` is true
  for `printerr()`).
- `_log_error(function, file, line, code, rationale, editor_notify,
  error_type, script_backtraces)` for errors and warnings: `push_error()`,
  `push_warning()`, engine errors and script errors.

On 4.7.2, a `push_error("boom")` reaches `_log_error()` with one
`ScriptBacktrace` in `script_backtraces`.

```gdscript
extends Logger

## Keeps the last N errors with their GDScript backtraces.
## Register once at startup: OS.add_logger(ErrorCollector.new())
@export var keep: int = 50

var _lines: PackedStringArray = PackedStringArray()
var _mutex := Mutex.new()


func _log_error(function: String, file: String, line: int, code: String, rationale: String,
		_editor_notify: bool, error_type: int, script_backtraces: Array[ScriptBacktrace]) -> void:
	var kind := "WARNING" if error_type == ERROR_TYPE_WARNING else "ERROR"
	var text := "%s: %s %s (%s:%d in %s)" % [kind, code, rationale, file, line, function]
	for bt in script_backtraces:
		text += "\n" + bt.format(2, 4)
	_mutex.lock()
	_lines.append(text)
	if _lines.size() > keep:
		_lines.remove_at(0)
	_mutex.unlock()


func _log_message(_message: String, _error: bool) -> void:
	pass


func snapshot() -> PackedStringArray:
	_mutex.lock()
	var copy := _lines.duplicate()
	_mutex.unlock()
	return copy
```

Rules:

- **Guard shared state with a `Mutex`.** Any thread that logs calls the
  logger on that thread.
- **Do not log from inside the logger.** On 4.7.2 the engine catches the
  loop: a `print()` inside `_log_message()` reaches the console with "While
  attempting to print an error, another error was printed", but it is not
  passed to loggers again. The message is then lost to your logger, so keep
  logger code free of `print()` and `push_error()`.
- **Keep it fast.** Write to a buffer; flush to disk or the network from the
  main loop, not inside the callback.
- Remove it with `OS.remove_logger()` when the owner goes away.

## 2. Backtraces on Demand

`Engine.capture_script_backtraces()` returns the current call stack of each
script language as `ScriptBacktrace` objects. Use it to tag a warning with
where it came from, or to attach a stack to a bug report.

```gdscript
extends Node

## Logs a message with the GDScript call stack that led here.
func trace(message: String) -> void:
	var stacks := Engine.capture_script_backtraces()
	var text := message
	for bt in stacks:
		for i in bt.get_frame_count():
			text += "\n  at %s (%s:%d)" % [bt.get_frame_function(i), bt.get_frame_file(i), bt.get_frame_line(i)]
	print(text)
```

`capture_script_backtraces(true)` also records local variables. That costs
memory and keeps references to those values alive while the backtrace
exists, so use it only around a specific failure.

## 3. Checks That Survive Release Builds

`assert()` is removed from release exports: the condition is not even
evaluated. Use it only for programmer errors you want to catch while
developing. A check that protects the player's data must stay:

```gdscript
extends Node

## Applies damage. The guard stays active in release builds.
var health: int = 100


func apply_damage(amount: int) -> void:
	assert(amount >= 0, "apply_damage: negative amount is a caller bug")
	if amount < 0:
		push_error("apply_damage: negative amount %d ignored" % amount)
		return
	health = maxi(health - amount, 0)
```

Never put a needed side effect inside `assert()` (`assert(load_save())`): in
release the call disappears with the assert.

## 4. Debug-Only Output

Wrap development logging so that it does not ship:

- `OS.is_debug_build()` is true in the editor and in debug exports.
- `OS.has_feature("debug")` / `OS.has_feature("release")` answer the same
  question by feature tag, and custom export feature tags work the same way.
- `print_debug()` adds the calling file and line (`At: res://x.gd:3:_init()`
  on a 4.7.2 debug run). Do not count on it being silent in release; gate it
  like `print()`.

`print()` in release still writes to stdout and the log file. A `print()`
every frame costs time in any build; remove it or put it behind a flag.

## 5. Thread Safety Checks

Most scene tree calls are not thread-safe. In a debug build,
`Thread.set_thread_safety_checks_enabled(false)` turns off the engine's
checks for the current thread, for code that you have proved safe. Leave
the checks on everywhere else: an error such as "This function in this node
can only be accessed from the main thread" points at a real race. Use
`call_deferred()` to hand work back to the main thread. Thread patterns are
in **godot-multithreading**.
