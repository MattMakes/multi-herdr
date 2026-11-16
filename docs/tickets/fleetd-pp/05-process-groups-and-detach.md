# 05: Platform concern: process groups, job objects and the detach of a child

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform --test group && cargo clippy -p horch-platform --all-targets -- -D warnings`
**Own:** `crates/horch-platform/src/group.rs`, `crates/horch-platform/src/mac/group.rs`, `crates/horch-platform/src/linux/group.rs`, `crates/horch-platform/src/windows/group.rs`, `crates/horch-platform/src/unix/group.rs`, `crates/horch-platform/tests/group.rs`

Spec: `docs/specs/fleetd/design.md`, PL-2 (the concerns "process groups (stop and kill of a
group)" and "detach of a child process"), and
`docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6, rows "Process groups" and
"Service or daemon start".

## Goal

`horch_platform::group` puts a child in its own group (a process group on macOS and Linux, a job
object where Windows uses one today), kills a whole group, tells if a child exited without a reap,
and detaches a child from the session. The behavior is the same as today on each OS. A fault here
leaves processes alive or kills the wrong ones, so this ticket has the hardening persona and a
full review.

## Requirements

- **PL-2** (part): the concerns "process groups" and "detach of a child process" are in the
  platform interface.
- **PL-6** (part): the gaps of today are explicit `NotSupported` values.

## Context

Today's code that this ticket copies (do not edit these files):

| Today | What it does | Platform function (proposed name) |
| --- | --- | --- |
| `crates/horch-core/src/procid.rs:65` `is_same_group` | the group leader has its start time, or the group has a member | `group::is_same_group(pgid, started)`; Windows gives `NotSupported` |
| `procid.rs:81` `group_members` | members of a group | use `process::group_members` of ticket 03 |
| `crates/horch-core/src/runtime/process.rs:62`, `:68` `kill_pgid` | `killpg(SIGKILL)`; `false` elsewhere | `group::kill_group_id(pgid)`; Windows gives `NotSupported` |
| `runtime/process.rs:76`, `:81` `kill_group` | kill the group of a child; Windows kills the child only | `group::kill_child_group(&mut Child)`; Windows kills the child only and states it in the doc (06 row "Process groups") |
| `runtime/process.rs:280` `detach` | `setsid` in `pre_exec` | `group::detach(&mut Command)`; Windows gives `NotSupported` (06 row "Service or daemon start") |
| `crates/horch-core/src/harness/update.rs:525`, `crates/horch-core/src/vcs/network.rs:155`, `crates/horch-core/src/taskledger/finish/land.rs:99`, `crates/horch/src/cmd/taskledger_finish.rs:168` | `CommandExt::process_group(&mut cmd, 0)` | `group::new_group(&mut Command)`; Windows: no change to the command, stated in the doc |
| `crates/horch-core/src/workspace/herdr.rs:676` `struct Job`, `:679` `Job::contain` and `terminate`, `:721` `Drop`, `:748` `resume_main_thread` | a Windows job object that holds a herdr call and its children | `group::Contained` (Windows: the job object; macOS and Linux: the process group) |
| `herdr.rs:793` `kill_and_reap` (`:796`, `:799`) | kill the group, then reap | `group::kill_child_group` |
| `herdr.rs:833` `has_exited` (`:834` `waitid` with `WNOWAIT`; `:851`, `:853` pid field; `:857` Windows `try_wait`) | test for an exit without a reap | `group::exited_without_reap(&mut Child)` |
| `herdr.rs:867` `output_within` (`:876`, `:887`, `:893`, `:907`, `:936`, `:953`) | start the herdr call in a new group or job, wait with a deadline, kill the whole group at the deadline | the functions above; `output_within` keeps its logic in `herdr.rs` |

## Steps

1. Write the tests first, in `crates/horch-platform/tests/group.rs`. Each fails before the
   functions exist:
   - `kill_child_group_ends_a_grandchild` (`#[cfg(unix)]`): a `sh` child in a new group starts a
     `sleep` grandchild; after the kill both are gone.
   - `contained_child_is_killed_with_its_children` (`#[cfg(windows)]`, run by CI).
   - `kill_child_group_on_windows_kills_the_child` (`#[cfg(windows)]`).
   - `exited_without_reap_leaves_the_child_reapable` (`#[cfg(unix)]`): after `true`, the call
     says yes and `wait` still gives the status.
   - `detach_gives_a_new_session` (`#[cfg(unix)]`): the child's session id differs from ours.
   - `detach_and_kill_group_id_are_not_supported_on_windows` (`#[cfg(windows)]`).
   - `is_same_group_is_false_for_a_wrong_start_time` (`#[cfg(unix)]`).
2. Write `src/group.rs` and the 3 OS modules. Shared unix code goes in `src/unix/group.rs`. Use
   `crate::process` of ticket 03 for each start time.
3. Run the Check.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

Rules of this phase (tickets 03 to 09):

- Copy the OS code of today with no change of behavior: the same system calls in the same order,
  the same timeouts, the same return values and the same error kinds. PL-6 forbids a change of
  behavior. Do not fix an old fault here; send it as a NOTE to the orchestrator.
- Do not edit a file of another crate. The callers keep their own copy until the move tickets 10
  to 15 switch them to this crate. A short time with 2 copies is expected.
- Give core code a fact or an action. Never add a function that returns the name of the OS for
  core code to branch on.
- Where an OS has no equivalent today, its module returns `NotSupported` (ticket 01), or a type
  that holds it. Core code decides what to do. The doc comment of the function names the row of
  `docs/specs/fleetd/preservation-06-wiring-and-platform.md`, section 6, that states the gap.
- A helper that a test of another crate calls today (for example a parser) becomes a `pub`
  function here, so the move ticket can keep that test with its name and its place.
- Each `unsafe` block has a `// SAFETY:` note. Keep the OS modules small, and write the Linux
  and Windows modules with care: this Mac compiles only the macOS module.
- Make the code compile for Linux and Windows as far as this Mac can check. When
  `rustup target list --installed` shows the target, also run
  `cargo check -p horch-platform --target x86_64-unknown-linux-gnu` and
  `cargo check -p horch-platform --target x86_64-pc-windows-msvc`. Today this Mac has Homebrew
  Rust with only `aarch64-apple-darwin`. CI on the draft pull request of `fleet/fleetd-build`
  compiles and tests Linux and Windows at each push. A CI fault on Linux or Windows after you land
  comes back as a fix task.

## Done when

- The Check passes on this Mac.
- Each row of the table has its platform function. Each Windows gap is `NotSupported` or is
  stated in the doc of its function with the row of section 6.
