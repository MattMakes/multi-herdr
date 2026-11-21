# Integration tests reproduce host and outside-program quirks

The horch-core integration tests run the same on every host and leave the
host untouched. Where a host or an outside program behaves in a way the code
depends on, the test reproduces that behaviour in a temp dir or a stub
instead of relying on the machine it runs on. The shared helpers live in
`crates/horch-core/tests/common/mod.rs`.

## Consequences

- The fake `herdr` in `tests/context_prime.rs` appends each argv to
  `<tmp>/herdr.log`, 1 line per call. Like herdr 0.8.2, it fails when the
  pane id does not follow the subcommand directly
  (`pane release-agent --source x ...` exits 2). horch puts the pane id
  right after the subcommand.
- `build_opencode_db` feeds the SQL fixture to `sqlite3` on stdin. The SQL
  starts with `--`, which `sqlite3` reads as an option when it is an
  argument.
- `assert_decoy_untouched` (D20 item 5) runs test `child` of the same test
  binary in a new process with `GIT_DIR` and `GIT_WORK_TREE` aimed at a
  decoy repository. It asserts that the child passed and the decoy did not
  change. The child process gets the variables, not the test process: the
  other tests of the binary run in parallel and must not see them.
- macOS temp dirs are spelled `/var/...` but harnesses record the canonical
  `/private/var/...`. Session discovery compares canonical workdirs, so a
  workdir in the temp dir's own spelling matches a session recorded under
  its canonical path. A temp dir that is already canonical, such as a Codex
  pane's TMPDIR, gets a symlinked dir in its place in the test.
- Root reads anything. A test that makes a directory unreadable with mode
  000 (`operator_skills_unreadable_dir_is_an_error`) checks readability
  first, and has nothing to test when the suite runs as root.
- The pi compaction extension is copied as a `.mts` file, so node loads it
  as an ES module whatever `package.json` is above it.
- The hand-off files `video-direction` writes (brief, direction,
  storyboard) are named in their exact case. A lowercase name works on
  case-insensitive macOS file systems and breaks on Linux.
  `HANDOFF_NAMES` and `names_file_lowercase` in
  `tests/video_pipeline_wave.rs` guard it.
