# Tool save isolation

Read this when a tool, a test or a capture starts the real game on a machine where a person also plays it.

Autoloads that read a save or settings file do it at start-up, before any `-s` script runs. A
tool, test or capture that starts the real game can therefore write the player's save, or read
another run's settings.

1. Find each autoload that reads or writes a save or settings file.
2. Add a run-mode check at its start. Script mode (`-s` before a lone `--`), test mode, and a
   start from a worktree path all count as tool runs. Tool runs use tool save and settings
   paths.
3. Read the tool save slot from an environment variable, for example `<GAME>_TOOL_SAVE=<name>`.
   Why: the environment exists before any autoload starts. A safe name (lower-case letters,
   digits and `-`) gives `user://tool-save-<name>.json`. An unsafe name falls back to 1 shared
   tool path.
4. Start every tool run with its own slot name. Give each parallel experiment and each copied
   tool its own slot. Rename the slot when you copy a tool.
5. Give tests a helper that returns a per-process scratch path and deletes it at exit. Add 1
   guard test that builds a separate node, not the autoload, and proves the path switch.
6. Hash the player's save before and after a tool batch or a suite. Equal hashes prove that
   the run did not touch it. Read the player's save for analysis only from a temp copy.
7. Give a human playtest mode and a demo build their own save paths. For a fresh start, move
   the old save aside. Start an exported build with an empty `HOME` to keep it off the real
   save.
8. Pin every setting that changes timing or visuals in the tool's own settings file, for
   example reduced motion or hit-stop. Why: a shared settings file once shifted event ticks by
   18 frames, for example.
9. For a test that fails only on some machines or only in parallel, run it alone. Then make 2
   scratch slots that differ in 1 key and prove the cause.
10. Delete your scratch slot files after the run. Clean tool saves by name only, and change
    nothing near the player's save.
