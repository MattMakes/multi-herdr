# Shared machine, locks and sandboxes

Read this file when several agents and a human share 1 machine, 1 display or 1 checkout, or when a worker runs in a sandbox.

## Shared machine: slot runner and load

Many agents and a human share 1 machine. Unbounded headless runs make it unusable.

1. Start every headless Godot run through 1 slot runner: N slot folders taken with `mkdir`,
   an owner file with role and pid, stale-pid cleanup, and the command under `nice`. The
   slot runner in [script-slot-run.md](script-slot-run.md) has this shape. Copy the block
   into a file.
2. Put the alarm on the Godot command inside the slot, not around the runner. An alarm
   around the runner counts the slot wait, and it can stop the parent but leave the child.
3. Keep at most 1 Godot process per worker. Do not start a second run while one waits.
4. End every run by itself: a frame limit, `quit()`, `--quit-after`, or the alarm.
5. Exempt timing runs (they take the perf lock and skip `nice`) and windowed runs (they take
   a window lock).
6. Before you change N or blame load, trace each process: a real run, a waiting run, or an
   abandoned run. A load figure is not a count of games.
7. Before DONE, list Godot processes by binary and worktree path. Stop only your own and
   report others. Never start or stop the owner's game.

The Godot flags, the log check and the run cleanup are in the `godot-headless-runs` skill.

## Resource locks

Use 1 lock folder per scarce resource: each window slot, the timing machine, the main
checkout. `mkdir` is atomic, so it is a safe lock on any Unix machine.

1. Take and release in 1 shape only:
   `if mkdir "$L" 2>/dev/null; then echo "$ROLE $(date -u +%FT%TZ)" > "$L/owner"; <run>; release; fi`.
   Release means: check that the owner line names you, remove the owner file, `rmdir`.
2. Never write `mkdir "$L" && run; rm -rf "$L"`. When `mkdir` fails, the `rm` still runs and
   deletes another worker's lock.
3. Put the take-wait-run-release loop in 1 helper. The lock helper in [script-with-lock.md](script-with-lock.md) does it. Copy the block into a file.
   Try each window lock in turn and take the first free one.
4. Bound the wait (for example 10 minutes in 15 s steps). Do headless work while you wait.
5. Keep each hold short (for example 3 minutes). Put the visual gate and the captures into
   1 hold. Wait 30 s after a release before a new take.
6. For an ordered lock (timing), write a wait file per role. Do not take the lock while a
   role ahead of you waits.
7. Respect an announced quiet window: let a running windowed job finish and start no new
   one. Headless runs may go on.
8. Take the checkout lock before a commit, merge, cherry-pick, amend or push in a shared
   checkout. Worktree commits need no lock.
9. Report an owner line older than the limit (for example 30 minutes) to its owner. Never
   remove a lock that is not yours or that has no owner file.
10. Before you report, check that you hold no lock.

## macOS shell

Agents bring GNU habits; macOS has zsh and BSD tools.

- There is no `timeout`. Use `perl -e 'alarm N; exec @ARGV' <command>`, `gtimeout`, or the
  tool's own quit flag.
- Use `sed -i ''` with 1 `-e` per expression, or `perl -pi -e`, for 1-line changes only.
- zsh does not split an unquoted variable into words. Never keep a command line in a string
  variable; use an array, a function or a small `sh` script.
- Quote every glob in `grep --include` and `find -name`. zsh stops on an unmatched glob.
- `echo $?` after a pipe gives the status of the last command. Use a log file or `pipefail`.
- Use absolute paths after any `cd` in a compound command.
- `git grep` needs `-E` or several `-e` for "or". Test a search on a known hit before you
  trust an empty result.
- `python3 -I` hides user-site packages. Check that imports load, or use a venv.
- Avoid 1-letter shell function and variable names; an alias can take them.

## Sandboxed workers

Some fleet workers run in a sandbox (often a fallback agent when quota runs out). They cannot
write `.git`, `user://` or the home folder, read the process table, or open a window.

1. Check each new pane for a silent fallback in the session list.
2. Give sandboxed workers code and tests only. Bot runs, calibration, captures and visual
   judgement go to workers that can write saves and open windows.
3. A sandboxed worker never runs `git worktree add`, `git commit` or `git merge`. It sends
   `COMMIT REQUEST: <worktree>, <files>, <message>` after each passed step.
4. Give Godot runs a private home folder, so `user://` is writable, and a log file in `/tmp`.
   Make logger autoloads fail safe: 1 warning, then no writes.
5. Record the sandbox failure classes in the baseline (denied `user://` writes, `nice`
   denied). Group FAIL lines by cause and report exact counts per group.
6. When a message fails, retry once, then write the line in a note and in the notes file.
7. The steward commits by path, runs 1 import to make `.uid` files, and runs the suite
   outside the sandbox. Sandbox results hide real failures.
