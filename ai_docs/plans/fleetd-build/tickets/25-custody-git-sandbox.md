# Each git command of a fleetd job runs in the platform sandbox, so code of the project never runs with the rights of fleetd

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test worktree_jobs && cargo test -p horch-core --lib fleetd && cargo test -p horch-marketplace && cargo test -p horch-platform && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/rescue_git.rs`, `crates/horch-marketplace/src/git.rs`, `crates/horch-core/tests/worktree_jobs.rs`

## Goal

No program that the project names (a hook, a clean or smudge filter, a merge driver, an fsmonitor,
an ssh command, a credential helper, a diff or textconv program) can run with the rights of
fleetd when a fleetd job runs git in a tree of the project. This is true also when a live process
of the project changes `.git/config` or `.gitattributes` between the checks of fleetd and its git
command.

## Context

Review finding F5 of T-286 (the rescue job), and the same window in `worktree_add` and
`worktree_list` of T-285. The fleetd git rule of today scans the config, turns off hooks
(`-c core.hooksPath=/dev/null`) and refuses a project program, then runs git. A live process of
the project (a worker) can add a clean filter to `.git/config` after the scan and before
`git add -A`. Then git starts that program as fleetd. From Phase 5, fleetd holds the operator
token, so this goes around SR-8.

A check before the command cannot close a race. The fix must hold at the time when git runs:

- Way A: run each such git command inside the platform sandbox primitive of T-360
  (`crates/horch-platform/src/sandbox.rs`): it can write only the paths that the job needs (the
  tree, the git folder of the repository, a private index in a fleet-owned temporary folder), it
  cannot read the fleet root `run/` folder, and it cannot write outside those paths. A program of
  the project that git starts then has only those rights.
- Way B: make git unable to start a program at all for that command: a config that the project
  cannot change (`GIT_CONFIG_GLOBAL=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_COUNT` pairs
  that set each program setting to an empty or safe value for this command, `GIT_ATTR_NOSYSTEM=1`,
  and a way to ignore the repository `.gitattributes` filters), proven for each setting.

Way A is the stronger one. Use A where the platform gives a sandbox, and B as the second layer in
each case. Where the platform says NotSupported, the rule of the build holds: the job does not
run, and 1 needs-you item is opened.

## Prior work

- Read the done summaries of T-285, T-286 and T-360 (`horch ledger show T-286`), and the review
  report `.horch/reviews/T286-*/round-*/report.md`, finding F5.
- `GitRunner` in `crates/horch-marketplace/src/git.rs` is the 1 copy of the fleetd git rule.
- Later tickets use the same runner: the remove job (ticket 07 of Phase 3), the push, rebase and
  bundle jobs of Phase 4. Keep 1 runner, so that they get the fix with no change.

## Steps

1. Write the attack tests first, 1 for each program kind above: a repository where a second
   thread or process writes the setting into `.git/config` (or `.gitattributes`) AFTER the scan
   of fleetd and BEFORE the git command (use a test seam in the runner between the scan and the
   start); the program writes a marker file outside the tree. The job runs. Assert: the marker
   does not exist, and the job result says why.
2. Implement Way A in the runner for each custody job, and Way B as the second layer.
3. NotSupported: the job does not run git; it ends with a result that the pass turns into a
   needs-you item. Test it with a fake platform answer.
4. Check that the rescue still saves each dirty and untracked file byte for byte with the filters
   off (a filter that is off must not change what is saved: test a repository that uses a clean
   filter, for example line endings).
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Never run
  `horch self-update` or `cargo install`. Never register a service.
- Each test makes its own temporary git repository and temporary folders. No test and no worker
  runs a rescue, a cleanup or a `git worktree` command on the repository
  `/Users/MASCOTT/projects/mh/multi-herdr` or on its `.worktrees/` folder.
- OS-specific code is only in `crates/horch-platform`. If the sandbox primitive needs a change,
  send QUESTION first.
- A rescue never loses a file: if the sandbox or the safe config makes a rescue fail, the tree
  stays and the job says why. Never remove a tree on a failed rescue.
- Run `horch ledger progress` each 5 min during a long run.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, each attack test fails without the change, and the done summary lists each
program kind, the way that stops it, and what happens on an OS with no sandbox.
