# horch install places a real binary by rename

`horch install` (`crates/horch/src/cmd/install.rs`) puts a real file at the
destination and replaces it by rename, never by writing in place.
`install::replace_file` fills a staged file, makes it executable and
renames it over the target. Overwriting a running binary fails on Windows
and can crash a live process on Unix.

## Consequences

- A running exe cannot be overwritten on Windows, but it can be renamed.
  The install moves the existing target aside to its `.<name>.old` path
  before the final rename, and restores it if that rename fails.
- A process still running from a `.old` file keeps it locked.
  `install::remove_stale_old` ignores removal errors and retries on the
  next install.
- `same_file` follows symlinks, so a link that resolves to this binary
  would count as installed. `install_one` treats the target as installed
  only when it is the same file and not a symlink.
- A symlink at the destination is usually a stale link to a script in
  another checkout. `install::remove_symlink` removes the link itself and
  never the file it points to, and the install places a real file.
- The running binary can be reached through the very symlink the install
  removes, so `install::place_binary` canonicalizes the source first.
- `install::durable_roster` maps a roster dir inside a linked git worktree
  (a gate or task worktree) to the same dir of the main checkout, when
  that dir exists. Any other dir is kept. A linked worktree is deleted once
  its work lands, and a launcher that baked in its path then runs every
  fleet on the built-in roster. This happened after `just install` ran in
  a gate worktree.
- `selfupdate::install_dir` installs into the canonical parent of the
  running horch, else `default_dir`. A horch in a cargo
  `target/{debug,release}` layout is a source build, updated with
  `git pull` and `just install`, and a release must not overwrite it.
- The GitHub API rejects a request without a User-Agent header.
  `selfupdate` sends `horch/<version>` as User-Agent on every release
  lookup and download.
