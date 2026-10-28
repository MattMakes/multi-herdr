# Live check: a Codex worker commits in a linked worktree

Run 2026-10-28 on macOS (Darwin 25.5.0), `codex-cli 0.165.0`, by `opus-1`
for ticket T-10 (fleet-reliability 01, decision 01b). No model session
started: every step used `codex sandbox` (Seatbelt, no model) or
`codex execpolicy check`.

## How to run

    cargo build -p horch
    scripts/live/codex-git.sh

The script makes a scratch repository with a linked worktree under
`/private/tmp`, with its own ledger file and state folder, and no remote. It
unsets every `HORCH_*` variable first: a worker pane sets
`HORCH_PROJECT_DIR`, and without the unset `horch ledger` makes its run
branch and worktree in that project. The script stops if the worktree is not
under its scratch folder. Each check prints `PASS`, `FAIL` or `SKIP`; the
exit code is 1 when any check fails.

The sandbox is the profile a Codex write pane launches in, `fleet_write`
(`fleet_write_profile_args` in `crates/horch-core/src/harness/codex.rs`):

    codex sandbox -P fleet_write -C <worktree> \
      -c 'permissions.fleet_write.extends=":workspace"' \
      -c 'permissions.fleet_write.filesystem={":project_roots"={"."="write",".git"="write",".codex"="write",".agents"="write",".aws"="write"},":slash_tmp"="read",":tmpdir"="read"}' \
      -c 'permissions.fleet_write.workspace_roots={"<state>"=true}' \
      -c 'permissions.fleet_write.network.enabled=true' -- <command>

## Result: shipped design (scripts/live/codex-git.sh, exit 0)

| Check | Result |
|---|---|
| `codex execpolicy check` on the shipped worker rules: `git add a.txt`, `git commit -m x`, `git -c core.hooksPath=/dev/null commit -m x`, `git push origin HEAD` | no match for each: they run sandboxed |
| `codex execpolicy check`: `horch ledger commit -m x` | allow (the existing `["horch", "ledger"]` rule) |
| sandboxed `git add a.txt` in the linked worktree | refused: `Unable to create '<main>/.git/worktrees/T1-probe/index.lock': Operation not permitted` |
| sandboxed `git push <local bare repo> HEAD` | refused: `! [remote rejected] HEAD -> fleet/codexgit-T1-probe (unpacker error)` |
| nested repository with `filter.x.clean = "touch <scratch>/ESCAPED; cat"` in its `.git/config`: plain `git add` | the filter ran: the attack on a raw `git add` allow rule is real |
| the same nested `git add`, sandboxed | the filter could not write outside the sandbox |
| `horch ledger commit -m x -- nested/f.txt` | refused: `nested repository` |
| `horch ledger commit -m probe -- a.txt` in the worktree | exit 0; prints the commit id; HEAD on `fleet/codexgit-T1-probe` moved to it |

The network is on in `fleet_write`: sandboxed `curl https://example.com` gave
HTTP 200, and 000 with `network.enabled=false`. A sandboxed write to `$HOME`
gave `Operation not permitted`; a write to the state root exited 0.

## Push to a network remote: the sandbox does not stop it

An earlier run of the script, before it unset `HORCH_PROJECT_DIR`, used the
real `multi-herdr` repository by mistake. Its sandboxed
`git push origin HEAD` reached GitHub over SSH and exited 0. It created the
branch `fleet/codexgit-T1-probe` on `origin` at the main commit, and
`horch ledger run new` pushed `fleet/codexgit`. No new commit left the
machine. The script now has no remote and pushes only to a local bare
repository by path.

So: the exec policy does not lift the sandbox for `git push`, but with the
network on, the `fleet_write` sandbox does not stop a push to a network
remote when SSH credentials are reachable. The sandbox refuses a push only
when the remote is a local path outside its writable roots. The Codex
teammate descriptions say this.

## Decision (01b)

- No `prefix_rule` allows a raw `git` command. A Codex worker commits with
  `horch ledger commit -m "<message>" [-- <path>...]`, which the existing
  `["horch", "ledger"]` rule runs outside the sandbox.
- `horch ledger commit` (`crates/horch/src/cmd/taskledger_commit.rs`):
  - takes the worker from `HORCH_WORKER_ID`, and refuses a worker with no
    claimed task or with a review task;
  - takes the worktree from the ledger row of that task, and refuses when
    the current folder is outside it;
  - runs git with `-C <worktree>`, every `GIT_*` variable removed except the
    author and committer name, email and date, `GIT_CONFIG_NOSYSTEM=1` and
    `GIT_CONFIG_GLOBAL=/dev/null`; `user.name` and `user.email` come from the
    operator's global config as values;
  - passes `-c core.hooksPath=/dev/null -c core.fsmonitor=false
    -c commit.gpgsign=false -c gpg.program=/usr/bin/false` to every call;
  - refuses a path outside the worktree, inside `.git`, or in a nested
    repository (a folder below the worktree root with its own `.git`);
  - stages with `git add -- <paths>` (or `git add -A`), commits with
    `git commit -m <message> -- <paths>` (only the named files) or
    `git commit -m <message>`, prints the commit id, and records the
    progress event `commit <short id>`.
- End-to-end tests: `crates/horch-e2e/tests/ledger_commit.rs`.

## Earlier candidates (rejected)

From the first runs of this check, sandboxed in `fleet_write`:

| Candidate | Result |
|---|---|
| extra root: per-worktree gitdir `<main>/.git/worktrees/wt` | `git add` 128: objects live in the common dir |
| extra root: git common dir `<main>/.git` | `git add` 128: Codex keeps the worktree's gitdir read-only |
| both roots | `git add` and `git commit` 0, but a sandboxed `git branch -D` deleted a branch of another checkout |
| `prefix_rule` allow `["git", "add"]`, `["git", "commit"]` | works, but a plain `git commit` ran worktree-controlled hooks outside the sandbox |
| `prefix_rule` allow `["git", "-c", "core.hooksPath=/dev/null", "commit"]` | no hooks, but a raw `git add` in a nested repository runs that repository's filters outside the sandbox (round-2 Critical finding) |

## Limits that remain

- A Codex worker can push to a network remote from its sandbox (see above).
  Only the briefing and `horch ledger finish` keep pushes in the fleet flow.
- `git add -A` (no paths) stages every change in the worktree. A nested
  repository there is staged as a gitlink (mode 160000). Checked with the
  same git flags: the nested repository's clean filter and
  `core.fsmonitor` did not run.
- Commits skip the repository's own hooks. `horch ledger finish` runs the
  ticket's check before it lands.
