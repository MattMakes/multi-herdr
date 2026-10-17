# A task runner for dev commands

Read this when a project has many headless commands and needs 1 list of them.

A project with many headless commands needs 1 list of them, so that each one runs the same way
for every person and agent.

1. Collect the existing commands from the notes and the tools folder.
2. Write a preflight script that checks the Godot version and the export templates.
3. Write a `justfile` or a `Makefile` with 1 recipe per command: import, parse check, tests,
   each platform build, the dist package, the visual gate, bot runs and asset generators.
   Make the default recipe list the others.
4. Put the time limit and the log scan inside the recipes, so that a caller cannot forget them.
5. Run each recipe once and read its failure message. Run destructive recipes, such as
   `clean`, as a dry run.
6. Add a commands section to the project README.
7. Test a `play` recipe with a stop signal: start it, wait a few seconds, and stop it.
