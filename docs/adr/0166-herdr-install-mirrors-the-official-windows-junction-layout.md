# herdr-install mirrors the official versioned junction layout on Windows

On Windows, `crates/herdr-install/src/place.rs` puts each Herdr version in
its own release directory and points a `current` junction plus a visible
bin directory at it, exactly as the official installer does. An update then
never has to overwrite a `herdr.exe` that is running, and `herdr update`
keeps working against an install made by herdr-install. `windows_layout`
computes `~/.herdr/packages/standalone/releases/<version>-<triple>`, the
`current` junction beside it, and the visible bin junction that goes on
PATH. Unix is simple: 1 binary at `~/.local/bin/herdr`, matching
`install.sh`. The Windows complexity is load-bearing.

## Consequences

- Release directories are named with the sanitised version and the Rust
  target triple (`manifest::Target::triple`), the same way the official
  installer names them.
- The Windows asset is a zip that carries an app-local ConPTY runtime next
  to the binary. The whole archive is extracted, not just the executable.
- `extract_zip` resolves every entry through zip's `enclosed_name`, which
  rejects `../` and absolute paths. A hostile archive cannot write outside
  the release directory.
- Path computation lives in pure functions so it can be verified off
  Windows.
- `win::set_junction` makes directory junctions with `mklink /J`, not
  symlinks. Creating a directory symlink on Windows needs Developer Mode or
  elevation, and a junction does not. `mklink` is a cmd built-in, so no
  extra tooling is needed.
- `win::clear_link` removes an existing junction with `std::fs::remove_dir`,
  never `remove_dir_all`. `remove_dir_all` follows the junction and deletes
  the release directory it points at.
- `win::ensure_user_path` writes the user PATH directly to
  `HKCU\Environment` rather than using `setx`, because `setx` truncates
  PATH at 1024 characters.
- `place_binary` writes a bare binary to a staged file and renames it over
  the target. Overwriting a running binary in place fails on Windows and can
  crash the live process on Unix.
