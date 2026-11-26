# 09: A sandboxed worker creates its worktree, commits and finishes

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `backend-developer` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `cargo test -p horch-core && HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`
**Own:** `crates/horch-core/src/harness/`, `teammates/skill-importer.md`, `teammates/spec-writer.md`, `teammates/ticket-writer.md`, `docs/security/sandboxed-pane-horch-access.md`

## Goal

A worker whose persona has a sandbox runs `horch ledger worktree new`, `git add`, `git commit` and `horch ledger finish` with no help from the orchestrator. The sandbox opens no path that the 4 commands do not need.

## Context

- Bug, seen 3 times on 2026-11-24 in run external-imports (horch 0.5.3, backend hc):
- `skill-importer-1` (claude harness, sandbox block in `teammates/skill-importer.md`): `git worktree add` fails with "could not create directory of .git/worktrees/T5-...: Operation not permitted".
- `spec-writer-1` (agy harness): `horch ledger worktree new` fails on `.git/refs/heads/fleet/external-imports-T2-...lock`.
- `spec-writer-1`: `git add` fails on `.git/worktrees/T2-.../index.lock: Operation not permitted`.
- The orchestrator made both worktrees and 1 commit by hand. That is a workaround, not a fix.
- A failed `git worktree add` also left the new branch behind, so the next attempt failed with "a branch named ... already exists".
- `teammates/skill-importer.md` says: "horch adds its own state root, ledger folder and sockets at launch". The git common directory of the project and the `.worktrees/` folder are not in that set, or the hc launch path does not add them. Find which.
- The Claude sandbox settings are built in `crates/horch-core/src/harness/claude.rs`. Find the agy harness file in the same folder.
- Security contract: `docs/security/sandboxed-pane-horch-access.md`.

## Steps

1. Reproduce: write a test that builds the sandbox settings of a sandboxed persona for a project path and asserts that the git common directory and `.worktrees/` are writable. Check: the test fails.
2. Find the root cause for each harness (claude, agy). Write it in the ledger with `horch ledger progress`.
3. Fix the launch path so that horch adds the project's git common directory (`git rev-parse --git-common-dir`) and the `.worktrees/` folder to the writable set of each sandboxed worker in a ledger run. Do not add them by hand in each persona file, unless the harness gives no other way. Check: the test passes.
4. Make `horch ledger worktree new` remove the branch it created when `git worktree add` fails. If that code is outside Own, send `horch tell orchestrator "QUESTION: ..."` with the file path and end your turn.
5. State in `docs/security/sandboxed-pane-horch-access.md` which paths a sandboxed worker can write now, and why.
6. Run the Check line.

## Constraints

- WARNING: do not widen a sandbox to the home folder, to `~/.claude` or to another project.
- Do not change `disallowed_tools` or `spawn_only` of a persona.
- Do not reinstall horch and do not restart a pane. The operator installs the fix after the merge.
- Add no code comments in lines you add.
- Never read, print or use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes.
- The DONE summary states the root cause for each harness in 1 sentence, and each file changed.

## Report

Run `horch ledger finish`, then `horch ledger done`.
