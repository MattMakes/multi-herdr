# The Linux live check tests a committed snapshot as a normal user

`scripts/live/linux.sh` runs `cargo test --workspace` and Godot headless in
an aarch64 Linux container under colima. By default
(`HORCH_LINUX_SRC=head`) it mounts a `git archive HEAD` snapshot, not the
working checkout, so other workers' uncommitted edits in the shared
checkout do not break the build. Its result file is
`docs/live-checks/linux.md`.

## Consequences

- Steps: `colima` (started, and stopped again at exit if this script
  started it), `cargo-test` (in the official rust image, the source mounted
  read-only, the target dir and HOME container-local), `godot-sha`
  (download Godot 4.7.2 linux.arm64 and check SHA-512 against the release's
  `SHA512-SUMS.txt`), `godot-version` (`godot --headless --version`) and
  `godot-xdg` (import and 1 headless script with `XDG_*` set to scratch
  paths; every file Godot writes must be under a scratch path).
- Env: `HORCH_LINUX_IMAGE` (default `rust:1-bookworm`), `HORCH_LINUX_STEPS`
  (default `"cargo-test godot"`, pick a subset), `HORCH_LINUX_SRC`
  (`worktree` mounts the checkout as it is, any other value than `head` or
  `worktree` is a source directory to mount).
- It writes only under `.worktrees/_scratch/live-linux/` (the logs, the
  snapshot), builds no image, and runs every container with `--rm`.
- `cargo test` runs as a normal user, not root. Root ignores file modes,
  and some tests make a directory read-only to force a failure.
- The container installs bubblewrap and socat, because Claude Code's
  sandbox needs them on Linux and a test checks the host for them.
  `/bin/sh` stays dash, the Debian and Ubuntu default.
