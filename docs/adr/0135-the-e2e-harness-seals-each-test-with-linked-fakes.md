# The e2e harness seals each test in its own root with linked fakes

The e2e tests run horch inside a `Harness` (`crates/horch-e2e/src/harness.rs`)
with its own root, a git project and a `bin/` dir of fake programs, and a
sealed environment that is cleared of the operator's variables. The
`bin/` entries are links to the fakes Cargo built next to `horch`, not
copies. macOS scans every new executable on its first run, and under load
many fresh copies at once take seconds to start. A link to the built file
is scanned once. On unix the link is a symlink, not a hard link:
syspolicyd scans an exec late under load and reads the file by a path it
finds from the inode. With hard links that can be another harness's link,
already deleted, and syspolicyd then kills the process with SIGKILL
("Malware rejection"). The built file is never deleted.

## Consequences

- Nothing may write through a `bin/` name. `Harness::write_bin` removes an
  existing entry first, never writes through it, because it can be a link
  to a built fake.
- `sqlite3` is the one real program the tests use, on a fixture database
  only. It is a symlink, not a copy: macOS kills a copied system binary
  (exit 137).
- The opencode fixture SQL goes to `sqlite3` on stdin. It starts with
  `--`, which `sqlite3` reads as an option when it is an argument.
- `horch_e2e::bin_dir` finds the fakes from the test executable. How far
  below them it sits depends on the Cargo version:
  `target/<profile>/deps/<test>-<hash>` up to Cargo 1.99, and
  `target/<profile>/build/<package>/<hash>/out/<test>-<hash>` from Cargo
  1.100. It walks up to the first dir that contains a fake. If none exists
  it uses the older layout, so a caller's error names a useful path.
- The sealed environment keeps `SYSTEMROOT`, `SystemRoot`, `WINDIR` and
  `COMSPEC`. Windows needs them to start any process at all.
- The project's first branch is set with `git symbolic-ref HEAD
  refs/heads/main`, not `git init -b`: older git has no `-b`.
- `harness::DEADLINE` (120 s) is the one deadline of every e2e wait. A
  passing run never reaches it, so it is generous: under the parallel full
  suite on a 4-CPU Linux VM, one step of a debug build can take 10 times as
  long as it takes alone.
- Git 2.50 prints strict ISO UTC dates (`%cI`, `%aI`) in the `Z` form
  (`...T12:00:00Z`), git 2.39 in the `+00:00` form. Tests that compare git
  output rewrite the `Z` form to `+00:00`, only in a whole word that is a
  date, so a `Z` in a name stays.
- A flock stays held while any copy of the file descriptor is open, and a
  process that another test thread spawns holds a copy of the file until
  it execs. A test that probes or takes a courier lock (`courier_holds` in
  `tests/mail.rs`, and `tests/done_budget.rs`) unlocks it before it closes
  the file. Otherwise a courier that starts while the flock stays held
  exits at once. The probe returns `None` before the lock file exists.
