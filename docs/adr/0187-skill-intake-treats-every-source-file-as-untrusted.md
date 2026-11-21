# skill-intake treats every source file as untrusted

skill-intake reads skills from sources it does not trust, so no read may
follow a link out of the source, and no stored name may carry a path out
of the store. The modules under `skills/skill-intake/scripts/intakelib/`
check what a file is from its bytes and decide on markers with fixed
rules.

## Consequences

- `acquire._copy_file` opens the source with `O_NOFOLLOW`. The file was a
  regular file at `lstat` time, and a symlink swapped in afterwards is
  never followed.
- `opaque` reuses `sweep.walk` as the 1 directory walker, so symlinks are
  never followed.
- `os.walk` lists a symlink to a folder under `dirs`, not `files`.
  `domain.take` adds such entries to the names it records and records them
  as links, so a symlinked folder cannot escape the domain snapshot.
- `discover` stores a skill folder path raw in `skills.json` (an
  orchestrator decision), so every path component must match
  `[A-Za-z0-9._-]+` and never be `.` or `..`.
- The sweep strips a U+FEFF at byte 0 before INJ-01. At the start of a
  file it is a byte-order mark, not a hidden invisible character.
- Bytes `CA FE BA BE` start both a Mach-O fat header and a Java class file.
  `opaque._native` tells them apart by the next big-endian u32: a small
  arch count (below 45) is Mach-O universal, 45 or more is a Java class
  version.
- In `licensing`, a clause that forbids analysis (reverse engineer,
  decompile, disassemble, no derivatives) makes any licence text
  restrictive, even with a grant. Weaker markers count only when no known
  grant matches, because grants such as the GPL say "proprietary" and "may
  not ... modify".
