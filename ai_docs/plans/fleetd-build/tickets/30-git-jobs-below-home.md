# A fleetd git job works for a repository below HOME: the sandbox hides HOME but not the path to the repository

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-platform --test sandbox && cargo test -p horch-core --test worktree_jobs && cargo test -p horch-core --test worktree_remove && cargo test -p horch-core --test worktree_rescue && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/sandbox.rs`, `crates/horch-platform/src/mac/sandbox.rs`, `crates/horch-platform/src/linux/sandbox.rs`, `crates/horch-platform/tests/sandbox.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-core/tests/worktree_jobs.rs`, `crates/horch-core/tests/worktree_remove.rs`, `crates/horch-core/tests/worktree_rescue.rs`

## Goal

Each custody git job (`worktree_add`, `worktree_list`, rescue, `worktree_remove`) passes for a
repository that is below the home folder of the user, on macOS and on Linux. The rule of T-691
stays: a program that the project controls cannot read the files of the user below HOME.

## Context

The worker of T-371 found the fault. The `worktree_add` job fails for each repository below HOME
on macOS:

`fatal: could not create leading directories of <repo>/.worktrees/t1/.git: Operation not permitted`

Cause: `CliWorktreeGit` (T-691) hides HOME. git cannot `stat` the parent folders of the tree, and
`mkdir` gives EPERM. The tests of today pass only because their repository is in the system temp
folder. Nearly each real repository is below HOME, so the product cannot make a worktree.

Repro: `TMPDIR=$PWD/target/tmp/x cargo test -p horch-core --test worktree_jobs worktree_add_job_makes_the_tree_on_its_branch_at_the_base_tip`

Read the done summary of T-691 first (`horch ledger show T-691`): `sandboxed_hiding(cmd, &Reach {
writable, readable, hidden, network })`, the longest listed path wins.

## Steps

1. Make the repro a test that stays: 1 test for each job kind with a repository below HOME (use a
   folder below the real HOME that the test makes and removes, or set HOME of the job host to a
   parent of the test repository; choose the way that tests the real rule). Check: the tests are
   red before the change.
2. Correct the rule in the platform sandbox, not in each caller. For each path that is `readable`
   or `writable` and is below a `hidden` path, the command may read the metadata of each parent
   folder between the hidden path and that path (stat, and the lookup of the next path part), and
   nothing more: it may not list those folders and may not read a file in them. On macOS use
   literal rules for the parent folders (metadata only if Seatbelt allows that split; if it does
   not, name exactly what a parent folder leaks). On Linux check what bwrap already gives (a bind
   below a tmpfs makes the parent folders; they are empty) and add a test.
3. Keep each attack test of T-691 green, and add 1: a raced driver in a job of a repository below
   HOME cannot read `~/.ssh` or a file beside the repository, and cannot list the parent folders.
4. Run the job tests on Linux in Docker with bubblewrap if Docker starts a container. If Docker
   does not start after 2 tries, write "Linux: not run, Docker did not start" in the done summary
   and go on.

## Constraints

- Do not remove HOME from the hidden list, and do not make the hidden rule weaker for a path that
  is not a parent of a listed path.
- No second sandbox mechanism. 1 function, `sandboxed_hiding`.
- Simplified Technical English. No em dash.
- Run only the Check and the guard tests before finish (speed rule of the operator).

## Done when

- The new tests pass and the Check passes.
- The done summary says what a parent folder still shows to the command on each OS.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
