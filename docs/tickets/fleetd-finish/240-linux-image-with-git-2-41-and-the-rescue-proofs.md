# Linux proof of each test that needs git 2.41 or newer: a test image with a new git, and where the product checks the git version

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** report
**Review:** none
**Check:** `test -s docs/specs/fleetd/gates/linux-git-241-proof.md && grep -q '^## Result of each test' docs/specs/fleetd/gates/linux-git-241-proof.md && grep -q '^## Where the product checks the git version' docs/specs/fleetd/gates/linux-git-241-proof.md && cargo fmt --all --check`
**Own:** `docs/specs/fleetd/gates/linux-git-241-proof.md`

## Why

The rescue job of fleetd needs git 2.41 or newer (`--attr-source`). The Linux test image of this run, `t250-linux:local`, has git 2.39.5 (Debian bookworm; bookworm-backports has no newer git). So these tests have no Linux proof on this host: `fm39_cancelled_task_does_not_block_its_idea` (task T-332), 2 tests of `taskledger.rs` (task T-336: read its commit messages), and 13 rescue tests of `worktree_jobs.rs` and `worktree_rescue.rs` (task T-319, among them `hung_fetch_process_past_deadline_is_killed_and_releases_git_writer_for_queued_rescue`). Also open: where does the product tell the operator that git is too old?

## Steps

1. Build 1 new image `t-git241-linux:local` FROM `t250-linux:local` that adds a git of 2.41 or newer. Ways, in this order: a Debian trixie package pinned for git only (say the exact apt lines), or a build of git from its source tarball of kernel.org with a checksum check. Write the Dockerfile in your scratch folder, not in the repository. Do not change or remove `t250-linux:local`. Docker rules: the operator has services in Docker (`litellm-postgres`, `phoenix`, `buzz-prod-*`): never stop, restart, remove or prune them; never any `docker ... prune`; `docker run --rm --name t<task number>-lx-<n> --privileged --init`; remove by exact name only what you made. Keep the new image at the end and name it in the report (the orchestrator removes it at the end of the run).
2. In a container of the new image, with the worktree mounted and a `CARGO_TARGET_DIR` inside the container (not the macOS `target/`), run each test named above 3 times, and the full files `worktree_jobs`, `worktree_rescue`, `worktree_jobs_stop`, `gate_jobs` of `horch-core` 1 time. Follow the way that tasks T-319 and T-332 ran their containers (read their commit messages; the sandbox needs `--privileged`). At most 4 test processes at the same time. 1 `horch ledger progress` line each 5 minutes.
3. Read the code: where does the product read the version of git (the doctor, the conformance, the start of fleetd, the rescue job)? What does the operator see on a host with git 2.39: at `horch doctor`, at the start of fleetd, at the first rescue? Run `horch-dev doctor` (the built binary of your worktree by its path, never installed, with HOME, HORCH_STATE_DIR, HORCH_LEDGER_DB and FLEET_ROOT inside your sandbox folder) in a container of the OLD image and paste the lines.
4. Write `docs/specs/fleetd/gates/linux-git-241-proof.md`: `## Image` (the exact lines to build it); `## Result of each test` (a table: test, runs, passed, failed, message); `## Where the product checks the git version` (code lines, the output of step 3, and a gap list: each place where the operator learns it only by a failed job); `## For the operator`. Label each claim verified or not verified. A red test is a finding: report it, do not fix it.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. Never install a tool on the host. Never install the build.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `/fleet`.
3. Never run pkill or killall. Never touch a process or a container of another fleet or of the operator.
4. Before DONE: no container of yours runs (`docker ps`), no process of the unit is alive, the sandbox folder and your `target/` are removed.

## Done when

The Check passes and each named test has a row.
