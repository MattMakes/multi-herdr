# F4 usage-doctor: the judge counted twice, and doctor checks Blender

Plan: `ai_docs/plans/finish/f4-usage-doctor.md`. Worker: opus-64.

## Item 1: the judge counted 2 times

### What the failure was

`exp_07_candidate_and_judge_in_usage` failed 1 time under load
(`ai_docs/reports/finish/spec-b-events.md`). `horch cost` showed 4 sessions
and 9000 input tokens. The test expects 3 sessions and 6000 input tokens
(1000 + 2000 + 3000). The extra 3000 is a second judge session.

`horch cost` does not double-count. It makes 1 row for each ledger record,
and the fake judge overwrites its transcript (`fs::write`), so 1 session
can never give 6000. A judge record exists for each attempt, and each
attempt mints its own session id (`competition/judging.rs`,
`judge_execution`). So 4 sessions means that the product ran a second judge
attempt. The fake judge answers `valid` every time, so attempt 1 gave a
valid answer and the coordinator still failed it. The cost report was
correct: 2 judge runs cost 2 times. The defect is the spurious retry.

### The 2 causes (both are in the product, both widen under load)

1. **`job_facts` read the files before the pid**
   (`crates/horch-core/src/evaluation/scheduler.rs`). It read `exit.json`
   and `output.json` first, then checked if the heartbeat pid was alive.
   The job writes `output.json`, then `exit.json`, then exits. When the job
   did all 3 between the file reads and the pid check, the coordinator saw
   no output, no exit file, and a dead pid. `decide` returns `Lost` for
   that. `poll` then calls `fail(Lost)`, kills nothing, and schedules
   attempt 2.
2. **`fsx::create_immutable` made the file visible before its bytes**
   (`crates/horch-core/src/fsx.rs`). It opened `output.json` with
   `create_new`, then wrote, then ran `sync_all`. A poll between the create
   and the write read an empty answer. `parse_judgment` refused it as
   `Malformed`, and the coordinator scheduled attempt 2. The same window
   applies to every immutable file (judgment, bundles).

I cannot tell from the 1 report which cause fired. Both give exactly the
seen numbers, so I fixed both.

### Fixes

- `job_facts` looks at liveness first, then reads the files. A job that was
  alive at the look and has ended since wrote its files before it ended, so
  the reads see them. `job_facts_with(dir, between)` is the same function
  with a hook between the 2 steps, for the test.
- `create_immutable` writes a temp file `.<name>.new-<pid>-<nonce>` in the
  same directory, runs `sync_all`, then `hard_link`s it to the path. The
  link is atomic and fails when the path exists, so the write-once and
  identical-bytes rules stay the same. The temp file is always removed.
  `create_immutable_with(..., written)` has a hook after the bytes and
  before the link, for the test.

### Tests (hooks, no sleeps)

- `evaluation::scheduler::tests::a_job_that_answers_and_exits_mid_look_is_not_lost`:
  a real `sleep` child is the job. The hook writes `output.json` and
  `exit.json`, then kills and reaps the child. The facts give
  `OutputPresent`. Red check: with the old read order the test fails
  (`Lost`). I ran this check and then restored the fix.
- `fsx::tests::create_immutable_shows_no_partial_file`: the hook sees that
  the path does not exist while the bytes are written. No temp file is left.
  A second write with other bytes is still a `Conflict`.
- `usage_dataset.rs` now also checks that the fake judge ran 1 time
  (`<log>.judge.count` is `1`), so a retry fails with a clear cause.

## Item 2: doctor checks Blender

- `Requirement::Blender` (`blender`) is in `crates/horch-core/src/roster/offer.rs`.
  The plan says `roster/teammate.rs`, but the enum is in `offer.rs`.
- `teammates/blender-artist.md` has `requires: [blender]`. Its body has 1
  note: port 9876 is the default of both the Blender Lab add-on and the
  `ahujasid` add-on, so running both clashes; the agent reports `BLOCKED:`.
- `crates/horch/src/cmd/doctor.rs` has `blender_problem`. It uses
  `BLENDER_PATH` when it is set and not empty, else `blender` on PATH. This
  is the order of the Blender Lab MCP server. It runs `--version`. It warns
  when Blender is missing, when `--version` fails (with the first stderr
  line), and when the version is older than 5.1 (the live MCP tools need
  5.1; only the `*_for_cli` tools work). Each warning names the fix and the
  macOS path `/Applications/Blender.app/Contents/MacOS/Blender`.
- `requirement_problems` has a 4th parameter, `blender_path`. `doctor`
  reads `BLENDER_PATH` with `std::env::var_os`, because
  `runtime::context::Inherited` has no field for it and is not my file.
- 6 new doctor tests: on PATH, missing, `BLENDER_PATH` wins over PATH (also
  a path that does not run), too old, version parse, and the built-in
  blender-artist offered on a `*.blend` project. The 3 Xcode tests now keep
  only the `xcode` line, because the unfiltered built-in roster also offers
  the blender-artist.
- Live run on this host: Blender 3.5.1 is in `/Applications`, not on PATH.
  `horch doctor` in a scratch project with `ship.blend` warns "blender not
  found on PATH and BLENDER_PATH is not set". With `BLENDER_PATH` set, it
  warns "is Blender 3.5. The live MCP tools need 5.1 or later".

## Checks

- `cargo test -p horch-core --lib -- fsx:: evaluation::scheduler roster::`: green.
- `cargo test -p horch --bin horch -- cmd::doctor`: 11 of 11.
- `cargo test -p horch-e2e --test usage_dataset --test judge --test dataset --test promotion --test brief`: green.
- `cargo clippy -p horch-core --lib --tests` and `-p horch --bin horch --tests`, `-D warnings`: clean.
- `rustfmt --check` on my files: clean.

## Not done, outside my files

- `teammates/_template.md` says `requires` values are `xcode` only. It needs
  `blender` (`blender` on PATH or BLENDER_PATH, `--version` runs). Another
  worker had uncommitted edits in it (a `sandbox` field), so I did not touch it.
- `docs/recipes/add-teammate.md:40` names only `requires: [xcode]`.
  `docs/skills-and-teams.md:199` says a missing Blender gives `BLOCKED:`; it
  can add that `horch doctor` warns first. Line 200 already notes the port clash.
- `BLENDER_PATH` belongs in `runtime::context::Inherited`, next to `PATH`.
- At my check time, other workers' uncommitted edits broke 2
  `harness::launch` tests, `crates/horch-core/tests/preflight.rs`, and
  `the_template_documents_exactly_the_teammate_fields`. They were not from
  my commits.
