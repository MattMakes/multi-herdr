# remove-zsh-fleet-function edits the startup file only when safe

`scripts/remove-zsh-fleet-function` removes only a `herdr-fleet()` function
that closes on its own first line. Deleting the first line of a function
that spans several lines would leave a broken shell startup file, so for
any other shape it refuses and asks for manual removal.

## Consequences

- It does not use `sed -i`, whose syntax differs between macOS and GNU. It
  copies the file with `cp -p` into a temp file in the same directory, so
  the replacement keeps the mode of the original and the final `mv` is a
  rename.
- When the startup file is a symlink it writes through it with `cat`,
  because `mv` would replace the symlink with a regular file.
