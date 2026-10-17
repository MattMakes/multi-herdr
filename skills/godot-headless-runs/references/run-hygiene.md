# Run hygiene

Read this before you start a batch of headless runs, and before you report done.

1. Give every run an end of its own: a frame limit, `quit()`, `--quit-after <frames>`, or a
   hard alarm. On macOS there is no `timeout` command (exit 127), so use
   `perl -e 'alarm N; exec @ARGV' godot ...` or `gtimeout`. The run wrapper in [script-godot-run.md](script-godot-run.md) does this; copy the block into a file.
2. Pick the alarm from a timed smoke run, and keep it shorter than your shell's call limit.
   Split long batches, for example 2 or 3 seeds per call.
3. Put the alarm on the Godot command only. A wait for a free slot or a lock outside the alarm
   must not eat the run's time, and an alarm around a wrapper can kill the parent but leave
   the Godot child alive.
4. Write output to a log file with an absolute path. A relative path breaks in a loop that
   changes folders.
5. Read the exit code from the run, not from a pipe. `echo $?` after `godot ... | tail` gives
   the code of `tail`. Use a log file or `set -o pipefail`.
6. Grep each log for `SCRIPT ERROR`, `Parse Error`, `FAIL`, the summary line and "leaked at
   exit". Treat an empty result or a missing summary line as a failure.
7. Pass 1 test filter per call when the runner does not split lists. Check that the test count
   is not 0, because a filter that matches nothing looks like a pass.
8. For deterministic bot runs, pass `--fixed-fps 60` and `--delta-smoothing disable` as engine
   flags before `--`. The engine consumes `--fixed-fps`, so the tool cannot read it from its
   user arguments. Prove the rate from ticks per frame.
9. Lower the priority of batch runs with `nice -n 10`. Do not lower it for timing runs.
10. Run 1 Godot process per agent unless a plan gives more. Do not start a second run while
    one waits or runs in the background.
11. Do not start a run with a bare shell `&`. Use a tracked background task with an end
    marker, or run it in the foreground.
12. In zsh, a variable that holds several flags does not split. Use an array or a shell
    function.
13. Before you report done, list Godot processes with `godot-procs.sh <worktree path>`. Copy the block in [script-godot-procs.md](script-godot-procs.md) into a file.
    Match the binary and the path, not the text of your own command. Stop only your own
    processes. Report the others. Never start or stop the player's game.
