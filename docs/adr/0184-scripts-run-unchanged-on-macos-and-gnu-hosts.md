# Scripts run unchanged on macOS and GNU hosts

The live checks and the skill-intake tests run on the operator's Mac as
well as on Linux, so they use no tool or path rule that exists on only 1
of them. `scripts/live/harnesses.sh` (`within`, `clean`) and
`scripts/godot/live_csharp.sh` bound a command with
`perl -e 'alarm shift; exec @ARGV'`, because macOS has no `timeout(1)`.
Replacing it with `timeout` breaks the scripts on macOS.

## Consequences

- `scripts/skill-intake/tests/base.py` resolves its temp dir, because on
  macOS `/var` is a symlink to `/private/var`. Unresolved paths fail
  comparisons against the resolved paths of the code under test.
- AF_UNIX socket paths are short on macOS (about 104 bytes).
  `scripts/skill-intake/tests/test_acquire.py` binds its Unix socket with a
  relative path after `chdir`, not with a long absolute temp path.
