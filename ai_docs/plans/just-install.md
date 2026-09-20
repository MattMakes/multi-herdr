# Brief: `just install` puts the current horch on the operator's PATH as the `herdr-fleet` startup command

Repo: /Users/mascott/projects/multi-herdr, branch `main` (confirm with `git branch --show-current`; create branch `just-install` for this work).

## The operator's current state (verified 2026-09-19)
- `~/.local/bin/horch` is a STALE SYMLINK to an old shell script: `/Users/mascott/projects/matts-robot-skills/plugins/herdr/skills/herdr-orchestrator/scripts/horch`. In a normal shell `horch` resolves to that, not to this repo's binary. (Inside fleet panes `cargo run` puts `target/debug` first on PATH, which hides the problem.)
- `~/.zshrc` line 218 defines a shell function: `herdr-fleet() { just --justfile "/Users/MASCOTT/projects/multi-herdr/justfile" herdr-fleet "$@"; }`. It runs the debug build via `cargo run` every time. A shell function shadows any `herdr-fleet` binary on PATH, so the function must go for an installed launcher to work.
- `~/.local/bin` is on PATH. `just` is installed. `horch install` (crates/horch/src/cmd/install.rs) copies the running binary to `~/.local/bin/horch`; check how it treats an existing symlink at the destination (`std::fs::copy` onto a symlink follows it and overwrites the TARGET script, which would corrupt the operator's other repo). Fix that in install.rs if needed: remove an existing symlink at the destination before copying, and print that it did so.

## Goal
`just install` (from the repo root) leaves the operator with:
1. `~/.local/bin/horch` = a real file, the release build of this checkout.
2. `~/.local/bin/herdr-fleet` = an executable launcher that runs `horch fleet` for the current directory with this repo's roster.
3. No `herdr-fleet` shell function in `~/.zshrc`.
4. A printed summary of what changed and a reminder to open a new shell.
And `just update` = `git pull --ff-only` on main then `just install`.

## Steps
1. Read `crates/horch/src/cmd/install.rs`. Make `horch install` safe against a symlink at the destination (remove it first, say so). Add a unit test if the file has tests; otherwise a short manual check in step 6. Also make it print the installed version (`horch --version` output) at the end.
2. Add `scripts/herdr-fleet` to the repo (mode 755, `#!/bin/sh`):
   ```sh
   #!/bin/sh
   # Start a herdr fleet in the current directory with the multi-herdr roster.
   # Installed by `just install`. Usage: herdr-fleet [cc|codex]
   HORCH_TEAMMATES_DIR="__TEAMMATES_DIR__"
   export HORCH_TEAMMATES_DIR
   exec horch fleet "$@" --cwd "$PWD"
   ```
   `just install` copies it to `~/.local/bin/herdr-fleet` with `__TEAMMATES_DIR__` replaced by the absolute `{{justfile_directory()}}/teammates` (use `sed` in the recipe; the justfile already uses shell). Verify `horch fleet` accepts the flavor positional before `--cwd` (see `crates/horch/src/main.rs` fleet args); if the order matters, put `--cwd` first.
3. Add to the justfile, after `default`:
   ```
   # Build the release binary, install it as ~/.local/bin/horch, install the
   # herdr-fleet launcher next to it, and remove the old herdr-fleet shell
   # function from ~/.zshrc (a backup is written first).
   install:
       cargo build --release --bin horch
       ./target/release/horch install
       sed "s|__TEAMMATES_DIR__|{{justfile_directory()}}/teammates|" scripts/herdr-fleet > ~/.local/bin/herdr-fleet
       chmod 755 ~/.local/bin/herdr-fleet
       ./scripts/remove-zsh-fleet-function ~/.zshrc
       @echo "Installed: $(~/.local/bin/horch --version). Open a new shell, then run: herdr-fleet"

   # Pull main and reinstall.
   update:
       git pull --ff-only
       just install
   ```
   Keep the existing recipes untouched; they still run from source via `cargo run`, which is useful for development. Update the justfile header comment to say `just install` exists for day-to-day use.
4. Add `scripts/remove-zsh-fleet-function` (mode 755, `#!/bin/sh`): takes a path; if the file contains a line starting with `herdr-fleet() {`, copy the file to `<path>.bak-horch-<YYYYMMDD-HHMMSS>` and delete only lines that start with `herdr-fleet() {` (use `grep -v` into a temp file, then `mv`; do not use `sed -i`, which differs between macOS and GNU). Print "removed the herdr-fleet shell function from <path>; backup at <bak>" or "no herdr-fleet shell function in <path>". Exit 0 either way; exit 0 also when the file does not exist.
5. `cargo build --release --bin horch`, `cargo test -p horch`.
6. Run `just install` for real. Then, in a FRESH shell (`zsh -lic 'type herdr-fleet; which horch; horch --version'`), confirm: `herdr-fleet` is `~/.local/bin/herdr-fleet` (not a function), `horch` is `~/.local/bin/horch` (a regular file, `ls -la`), and the version matches this build. Confirm the old script at `.../matts-robot-skills/.../scripts/horch` is unchanged (`git -C /Users/mascott/projects/matts-robot-skills status --short` shows nothing for it). Do NOT start a fleet.
7. README.md: in "Getting started", add a two-line "Day to day: `just install` once, then `herdr-fleet` from any project. `just update` pulls and reinstalls."
8. Commit on branch `just-install` (include this brief), message:
   ```
   Add just install and a herdr-fleet launcher

   just install builds the release horch, installs it to ~/.local/bin
   (replacing a stale symlink safely), installs a herdr-fleet launcher
   that pins this repo's roster, and removes the old herdr-fleet shell
   function from ~/.zshrc with a backup. just update pulls and reinstalls.
   ```
   Push, open a PR against main with `gh pr create --fill`, and report the URL with `horch done` in Simplified Technical English, including the fresh-shell check output.

## Out of scope
Do not change how existing recipes run. Do not edit any other line of `~/.zshrc`. Do not touch the matts-robot-skills repo. Do not start a fleet.
