# Linux proof of the tests that need git 2.41 or newer (T-377)

Date: 2026-11-21. Tree: `fleet/fleetd-finish` at `05ff4081`. Host: macOS aarch64, Docker
Desktop, load average 25 to 53 during the runs.

The rescue job of fleetd needs git 2.41 or newer (`--attr-source`). The Linux test image
`t250-linux:local` has git 2.39.5 (Debian bookworm). This report gives the Linux proof of each
test that needs the newer git, and the places where the product checks the git version.

## Image

Name: `t-git241-linux:local` (image id `sha256:d332ee3c...`). It is built FROM
`t250-linux:local` and adds git 2.51.2. `t250-linux:local` is not changed. The orchestrator
removes `t-git241-linux:local` at the end of the run.

Option 1, a Debian trixie package pinned for git only, is rejected (verified). The simulated
install `apt-get install -s -t trixie git` with trixie at pin priority 100 replaces about 40
packages of bookworm, among them `libc6` 2.36 to 2.41, `libgnutls30`, `libssl-dev` and
`base-files`. That is not a change of git only. The apt lines of the simulation:

```
Types: deb
URIs: http://deb.debian.org/debian
Suites: trixie
Components: main
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg

# /etc/apt/preferences.d/trixie
Package: *
Pin: release n=trixie
Pin-Priority: 100

apt-get update && apt-get install -s -t trixie git
```

Option 2, a build from the source tarball of kernel.org, is used. The sha256 comes from
`https://mirrors.edge.kernel.org/pub/software/scm/git/sha256sums.asc`. The build checks the
tarball against it. The GPG signature of `sha256sums.asc` is not checked (not verified).

Dockerfile:

```dockerfile
FROM t250-linux:local
ARG GIT_VERSION=2.51.2
ARG GIT_SHA256=233d7143a2d58e60755eee9b76f559ec73ea2b3c297f5b503162ace95966b4e3
RUN apt-get update \
 && apt-get install -y --no-install-recommends make gcc libc6-dev libcurl4-gnutls-dev libexpat1-dev zlib1g-dev libssl-dev xz-utils curl ca-certificates \
 && cd /tmp \
 && curl -fsSLO https://mirrors.edge.kernel.org/pub/software/scm/git/git-${GIT_VERSION}.tar.xz \
 && echo "${GIT_SHA256}  git-${GIT_VERSION}.tar.xz" | sha256sum -c - \
 && tar -xJf git-${GIT_VERSION}.tar.xz \
 && cd git-${GIT_VERSION} \
 && make -j4 prefix=/usr/local NO_GETTEXT=1 NO_TCLTK=1 NO_PERL=1 NO_PYTHON=1 all \
 && make prefix=/usr/local NO_GETTEXT=1 NO_TCLTK=1 NO_PERL=1 NO_PYTHON=1 install \
 && for b in git git-upload-pack git-receive-pack git-upload-archive git-shell scalar; do ln -sf /usr/local/bin/$b /usr/bin/$b; done \
 && cd / && rm -rf /tmp/git-${GIT_VERSION} /tmp/git-${GIT_VERSION}.tar.xz /var/lib/apt/lists/* \
 && git --version && /usr/bin/git --version
```

Build: `docker build -t t-git241-linux:local <folder of the Dockerfile>`. The links in
`/usr/bin` replace the Debian git, so a PATH of `/usr/bin:/bin` (the PATH of the container
script) also finds git 2.51.2. Verified: `git --version` and `/usr/bin/git --version` print
`git version 2.51.2`, and `git --attr-source=HEAD version` runs.

How the tests ran (the way of T-319 and T-332). Each container has its own copy of the
worktree at `/src`, a shared `CARGO_TARGET_DIR=/target` and a shared cargo home, both in
`<worktree>/.dev-sandbox/linux`, not the macOS `target/`:

```sh
docker run --rm --name t377-lx-<n> --privileged --init \
  -v $W/src-<n>:/src -v $W/target:/target -v $W/registry:/cargo-home \
  -w /src t-git241-linux:local sh -c "chown runner /target /cargo-home /src; \
  exec su runner -s /bin/sh -c 'export PATH=/usr/local/cargo/bin:/usr/bin:/bin \
  RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/cargo-home CARGO_TARGET_DIR=/target \
  HOME=/home/runner; cd /src; git init -q . 2>/dev/null; git --version; <command>'"
```

`<command>` is `scripts/dev-sandbox.sh cargo test --locked ...`. At most 4 containers ran at the
same time, with 1 test process in each.

Gotcha (verified): 2 containers must not share 1 `/src`. `scripts/dev-sandbox.sh` makes
`<root>/.dev-sandbox/run.<pid>`, and the PIDs of 2 containers repeat. In the first try 2
rounds failed with `ln: failed to create symbolic link '/src/.dev-sandbox/run.11/tmp': File
exists`, and the cleanup of 1 container can remove the folder of another container. Those
rounds are not counted. Each run below used its own copy.

## Result of each test

Image `t-git241-linux:local`, git 2.51.2. Each result is verified by the test output.

| Test | File | Runs | Passed | Failed | Message |
|---|---|---|---|---|---|
| `fm39_cancelled_task_does_not_block_its_idea` (T-332) | horch-e2e `p6_features.rs` | 3 | 3 | 0 | 24.3 s, 13.4 s, 13.0 s |
| `a_critical_finding_fails_until_a_fix_is_reviewed_again` (T-336) | horch-e2e `taskledger.rs` | 3 | 3 | 0 | round 1 ran 71.3 s (over 60 s), rounds 2 and 3 about 34 s |
| `a_push_review_task_ends_done_by_its_findings_file_and_its_task_lands` (T-336) | horch-e2e `taskledger.rs` | 3 | 3 | 0 | |
| `a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it` (T-336) | horch-e2e `taskledger.rs` | 3 | 3 | 0 | |
| `hung_fetch_process_past_deadline_is_killed_and_releases_git_writer_for_queued_rescue` (T-319) | horch-core `worktree_jobs.rs` | 3 | 3 | 0 | |
| `race_clean_filter_does_not_run_in_a_rescue_job` (T-319) | horch-core `worktree_jobs.rs` | 3 | 3 | 0 | |
| `race_filter_in_a_rescue_writes_only_its_own_index_folder_of_the_shared_tmp` (T-319) | horch-core `worktree_jobs.rs` | 3 | 3 | 0 | |
| `rescue_runs_no_filter_of_the_global_config` (T-319) | horch-core `worktree_jobs.rs` | 3 | 3 | 0 | |
| `rescue_saves_each_file_byte_for_byte_with_the_filters_off` (T-319) | horch-core `worktree_jobs.rs` | 3 | 3 | 0 | |
| `failed_rescue_verification_keeps_the_tree` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `fm_killed_worker_dirty_untracked_files_on_verified_rescue_ref` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `ignored_files_are_not_in_the_rescue_ref` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `rescue_job_at_its_limit_is_not_rescued_and_runs_again_after_its_backoff` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `rescue_job_runs_no_hook_or_program_of_the_project` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `rescue_job_saves_the_tree_of_a_repository_below_home` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `rescue_keeps_the_branch_and_the_index_of_the_tree` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `rescue_ref_is_read_back_before_the_tree_is_rescued` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `rescue_writes_its_manifest_with_ref_head_count_size_and_attempt` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| `rr16_second_rescue_of_1_tree_does_not_overwrite_the_first_ref` (T-319) | horch-core `worktree_rescue.rs` | 3 | 3 | 0 | |
| Full file `gate_jobs` | horch-core | 1 | 12 | 0 | |
| Full file `worktree_jobs` | horch-core | 1 | 34 | 0 | |
| Full file `worktree_jobs_stop` | horch-core | 1 | 5 | 0 | |
| Full file `worktree_rescue` | horch-core | 1 | 17 | 0 | |

Which tests are "the 13 rescue tests" of T-319 and "the 2 tests" of T-336 (not verified). No
commit message of T-319 names its 13 tests. The table has the 15 tests of `worktree_jobs.rs` and
`worktree_rescue.rs` that failed in the T-319 transcript and that make a rescue, so the 13 are
among them. The commit `a8d376a1` of T-336 names 3 tests and says that "the 2 tests with a
rescue" time out. It does not say which 2, so the table has all 3.

Control in the old image `t250-linux:local`, git 2.39.5 (verified): 2 of the tests fail with the
version text, 2 of 2 runs:

```
test failed_rescue_verification_keeps_the_tree ... FAILED
test rescue_writes_its_manifest_with_ref_head_count_size_and_attempt ... FAILED
thread 'failed_rescue_verification_keeps_the_tree' (31) panicked at crates/horch-core/tests/worktree_rescue.rs:630:5:
the rescue of t1: git version 2.39.5: a snapshot with the filters off needs git 2.41 or newer (`--attr-source`), so the job saved no file; install a newer git
```

## Where the product checks the git version

Each line below is read in the code at `05ff4081` (verified by reading).

1. `crates/horch-core/src/fleetd/worktree_custody/adapters/rescue_git.rs:296-303`: `snapshot`
   runs `git version`. When `has_attr_source` (line 318) finds a git older than 2.41, it fails
   with `<git version text>: a snapshot with the filters off needs git 2.41 or newer
   (--attr-source), so the job saved no file; install a newer git`. This is the only check of
   the git version in the product.
2. `snapshot` has 3 callers: the rescue (`rescue_git.rs:100`), the verification of the rescue
   (`rescue_git.rs:114`) and the removal (`remove_git.rs:181`, `disk_tree`). A removal runs
   only after a verified rescue, so on git 2.39 the rescue fails first.
3. `crates/horch-core/src/fleetd/worktree_custody/application/rescue.rs:108-119`: a failed
   rescue job opens 1 needs-you item, key `rescue_failed:<worktree id>`, text `The rescue of
   worktree <id> failed, so its tree stays: <reason>`. The reason is the text of item 1.
4. `crates/horch/src/cmd/doctor.rs:322-465`: the doctor runs the git of the fleet only for
   `git lfs version`, and only for a teammate with the GitLfs requirement. It reads no git
   version.
5. `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs:158`: the only
   `--version` of the conformance runner is `horch --version`, as a test MCP server. The
   conformance reads no git version.
6. `crates/horch/src/cmd/fleetd.rs:833`: the start of fleetd makes `CliWorktreeGit::new(&ctx.bins.harness.git, ...)`
   and runs no `git version`. The adapters of the jobs (`exec_fetch.rs:59`, `executors.rs:47`
   and the others) take `git_bin(...)` and run no version check.
7. Docs: `docs/specs/fleetd/gates/fleetd-p3.md:126` says that git 2.41 or newer is needed.

What the operator sees on a host with git 2.39:

- `horch doctor`: no line about git. Verified: the build of this worktree
  (`/target/debug/horch doctor`) in a container of `t250-linux:local`, with HOME,
  HORCH_STATE_DIR, HORCH_LEDGER_DB and FLEET_ROOT in `/tmp/t377-doc`, and the e2e `fake-herdr`
  on PATH as `herdr` (without it the doctor stops at `horch: herdr CLI not found on PATH.`).
  The repository has no `horch-dev` binary: the dev build is the `horch` binary of the worktree.
  Exit 0. The output, with 16 MCP `npx` lines cut:

  ```
  git version 2.39.5
  herdr is installed and its server is reachable.
  warning: delivery test failed: herdr did not answer inside the push budget
  warning: herdr resumes agent panes by itself after a herdr server restart: [session] resume_agents_on_restore is not false in /tmp/t377-doc/home/.config/herdr/config.toml. ...
  warning: backend-developer: MCP server context7 command npx is missing from PATH
  ... (15 more MCP lines of the same form)
  roster: 107 teammates, 55 offered to the orchestrator.
  warning: antigravity (needed by audio-reviewer, spec-writer, ticket-writer, antigravity): agy not found. ...
  warning: reviewer codex-reviewer cannot run on this host: the codex binary is not installed. Code-sweep reviews for it cannot start.
  warning: reviewer architect-reviewer cannot run on this host: the claude binary is not installed. Code-sweep reviews for it cannot start.
  warning: harness antigravity has no timer format and no transcript lookup, so the watchdog cannot see the progress of its workers.
  stale state: none
  doctor_exit=0
  ```

  The first line `git version 2.39.5` comes from the container script, not from the doctor.
- Start of fleetd: no message about git. From the code (item 6). Not verified by a run.
- First rescue: the rescue job fails, the tree stays on disk, and 1 needs-you item
  `rescue_failed:<id>` opens with the text of item 1. The job text is verified by the control
  run above. The needs-you text is from the code (item 3), not verified by a run.

Gap list: each place where the operator learns of the old git only by a failed job.

1. G1: `horch doctor` does not check the git version (verified by the run above).
2. G2: the start of fleetd does not check the git version (verified by reading).
3. G3: the conformance does not check the git version (verified by reading).
4. G4: so the first message is the failed rescue job and its needs-you item. A tree to rescue
   stays on disk, so no work is lost, but no worktree reaches `rescued`, and no rescued
   worktree is removed. The cleanup of each worktree that needs a rescue stops on that host
   (verified by reading items 2 and 3).

## For the operator

1. All 19 named tests and the 4 full files are green on Linux with git 2.51.2. The T-319 and
   T-332 reds in `t250-linux:local` come from the git of the image, not from the product.
   No test is red, so this report has no test finding.
2. Image `t-git241-linux:local` stays on the host. Remove it at the end of the run with
   `docker rmi t-git241-linux:local`.
3. The product tells the operator about an old git only at the first rescue (G1 to G4). A
   ticket can add a git version check to `horch doctor` and to the start of fleetd, with the
   same `has_attr_source` rule.
