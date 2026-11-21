# 72: Fix R4-F3: implement Windows ACL-based protected-file checks for request spool and untrusted files (SR-5)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-platform --lib && scripts/dev-sandbox.sh cargo test -p horch-core --test protected_files --test fleetd_request_files && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/fs.rs`, `crates/horch-platform/src/unix/fs.rs`, `crates/horch-platform/src/windows/fs.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`, `crates/horch-core/tests/protected_files.rs`, `crates/horch-core/tests/fleetd_request_files.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p5.md` (finding R4-F3, section "Tests that do not prove their rule" row 3, and operator decision OD-2) and `docs/specs/fleetd/design.md` (rule SR-5).

## Goal

On Windows, protected-file and request-folder creation and verification use an owner-SID and DACL check (equivalent to Unix mode `0600` / `0700`) instead of failing unconditionally on `platform::mode_of` returning `NotSupported`, so a Windows operator can queue a request file when fleetd is down and fleetd can ingest it under SR-5 while still failing closed on links, non-regular files, path traversal, foreign owners, wide DACLs, and oversized files.

## Requirements

- **R4-F3 (Important — SR-5)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs:116`; `crates/horch-platform/src/windows/fs.rs:102`. Every request file is refused on Windows because the protected reader requires a mode, while the Windows platform adapter always returns `NotSupported` for `mode_of`. When fleetd is down, a Windows operator can queue a request file but fleetd can never ingest it. The SR-5 fallback channel is therefore not implemented on a supported platform. The smallest fix is a Windows protected-file rule based on owner SID and ACLs, with the same fail-closed link, type, containment, and size checks. If Windows fleetd is not a release target, state that product limit in the design and service command instead.
  - **Design rule**:
    - **SR-5** (`docs/specs/fleetd/design.md:181`): "The command channel shall accept only the user that owns the ledger. The socket or named pipe shall check the peer user through the platform layer. The request folder shall have mode 0700 and each request file 0600, or an equal access rule on Windows, and fleetd shall check the owner of a file before it reads it."

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Write the failing tests first in `crates/horch-core/tests/protected_files.rs` and `crates/horch-core/tests/fleetd_request_files.rs`:
   - Enable cross-platform tests in `crates/horch-core/tests/protected_files.rs` and `crates/horch-core/tests/fleetd_request_files.rs` (gating only Unix-specific `mkfifo` / raw octal mode tests with `#[cfg(unix)]`):
     - Add test function `valid_owner_only_file_and_request_spool_succeed_on_host_platform`:
       - Create a protected folder and file using the platform owner-only creation helpers (or `channel_client` request queueing when fleetd is down), verify `read_protected` accepts the valid file and rejects a file whose owner does not match `rule.owner` or whose permissions/DACL grant access to other users (`Everyone` / group / other SIDs).
       - In `crates/horch-core/tests/fleetd_request_files.rs`, verify that queueing a request file and running `request_spool::scan` results in `Handled::Archived(Outcome::Applied(_))` on both Unix and Windows.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test protected_files --test fleetd_request_files`.
2. In `crates/horch-platform/src/windows/fs.rs`, `crates/horch-platform/src/unix/fs.rs`, and `crates/horch-platform/src/fs.rs`:
   - Add or implement owner-only DACL support on Windows for protected files and directories:
     - Implement `set_mode` / `create_mode` / `create_dir_mode` (or a dedicated platform function `check_owner_only(file: &File, meta: &Metadata, owner: &User, rule_mode: u32) -> Result<(), String>` and owner-only ACL application in `windows/fs.rs` using `ConvertStringSecurityDescriptorToSecurityDescriptorW` / `GetSecurityInfo` with `DACL_SECURITY_INFORMATION`, the same SDDL pattern already used in `crates/horch-platform/src/windows/channel.rs` for the owner-only named pipe).
     - Ensure `sync_dir` on Windows treats directory sync as a no-op (`Ok(())`) or that callers (`request_spool.rs`, `channel_client.rs`) handle Windows directory sync cleanly after atomic file rename.
     - On Windows, verify that the security descriptor of the open handle has `owner_of(file) == owner` and a protected DACL that grants access only to the owner SID (and `SYSTEM` / `Administrators` if required by Windows kernel defaults, refusing any ACE for `Everyone`, `Users`, or other user SIDs).
3. In `crates/horch-core/src/fleetd/fleet_runtime/adapters/protected_files.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/request_spool.rs`, and `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`:
   - Use the platform owner-and-access check in `protected_files::check_owner_and_mode` and `request_spool::check_folder` so Unix continues to enforce exact mode bits (`mode & 0o077 == 0`) with the existing error messages (`"mode {:04o} is wider than {:04o}"`) while Windows enforces owner SID and owner-only DACL (and `channel_client::queue` / `request_spool::make_folder` create owner-only directories and files on Windows).
4. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Implementing a Windows worker process sandbox (Windows worker profiles remain governed by `NotSupported` under SR-8).

## Done when

`valid_owner_only_file_and_request_spool_succeed_on_host_platform` passes, `protected_files` and `request_spool` support owner-only ACL checks on Windows while preserving exact Unix mode checks and error strings on Unix, and the ticket Check passes.
