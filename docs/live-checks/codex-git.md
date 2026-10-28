# Live check: a Codex worker commits in a linked worktree

Run 2026-10-28 on macOS (Darwin 25.5.0), `codex-cli 0.165.0`, by `opus-1`
for ticket T-10 (fleet-reliability 01). No model session started: every
step used `codex sandbox` (Seatbelt, no model) or `codex execpolicy check`.

## Setup

A scratch repository `main` with a linked worktree `wt` on branch `feat`,
the same shape as a fleet worktree. In `wt`, `.git` is a file:
`gitdir: <main>/.git/worktrees/wt`. The git common dir is `<main>/.git`.

Each sandboxed command ran in the profile a Codex write pane launches in,
`fleet_write` (see `fleet_write_profile_args` in
`crates/horch-core/src/harness/codex.rs`), with `-C <wt>` and a scratch
state root:

    codex sandbox -P fleet_write -C <wt> \
      -c 'permissions.fleet_write.extends=":workspace"' \
      -c 'permissions.fleet_write.filesystem={":project_roots"={"."="write",".git"="write",".codex"="write",".agents"="write",".aws"="write"},":slash_tmp"="read",":tmpdir"="read"}' \
      -c 'permissions.fleet_write.workspace_roots={"<state>"=true<EXTRA>}' \
      -c 'permissions.fleet_write.network.enabled=true' -- <command>

## Results

| Candidate | Command | Exit | Error text |
|---|---|---|---|
| baseline (shipped profile) | `git add a.txt` | 128 | `Unable to create '<main>/.git/worktrees/wt/index.lock': Operation not permitted` |
| baseline | `touch <main>/.git/probe` | 1 | `Operation not permitted` |
| b1: root `<main>/.git/worktrees/wt` | `git add a.txt` | 128 | `unable to create temporary file` (objects live in the common dir) |
| b2: root `<main>/.git` (common dir) | `git add a.txt` | 128 | `Unable to create '<main>/.git/worktrees/wt/index.lock'` |
| b2 | `touch <main>/.git/{probe,objects/probe,refs/probe,worktrees/probe}` | 0 | - |
| b2 | `touch <main>/.git/worktrees/wt/probe` | 1 | denied: Codex resolves the project root's `.git` pointer and keeps that gitdir read-only |
| b4: roots common dir + `<main>/.git/worktrees/wt` | `git add a.txt`, then `git commit -m b4` | 0, 0 | - |
| b4 | `git branch -D victim` (a branch of another checkout) | 0 | - (the sandbox deletes any branch) |
| a: `prefix_rule` allow `["git","add"]`, `["git","commit"]` | `codex execpolicy check -- git add a.txt` | - | `decision: allow` |
| a | `codex execpolicy check -- git commit -m x` | - | `decision: allow` |
| a | the command an `allow` runs: `git add c.txt`, then `git commit -m a-allow` outside the sandbox | 0, 0 | - |
| a | `codex execpolicy check -- git push origin feat` | - | no match: runs sandboxed |
| a | `codex execpolicy check -- git reset --hard`, `git clean -fd`, `git checkout -- a.txt`, `git branch -D feat`, `git worktree remove ../wt` | - | no match for each |
| a | `codex execpolicy check -- git -C . commit -m x` | - | no match |
| a | `codex execpolicy check -- bash -lc "git add a.txt && git commit -m x"` | - | no match |
| a (shipped profile, no rule) | `git push origin feat` to a local bare remote | 1 | `! [remote rejected] feat -> feat (unpacker error)` |

Shipped rule set: `codex execpolicy check -r crates/horch-core/tests/fixtures/harness/codex_rules_worker.rules`
(the rendered rules of every Codex worker pane) after the change:

| Command | Decision |
|---|---|
| `git add a.txt` | allow |
| `git commit -m x` | allow |
| `git push origin feat` | no match: sandboxed, refused as above |
| `git reset --hard` | no match |
| `git branch -D feat` | no match |

The sandbox itself keeps the network on (`fleet_write` sets
`network.enabled=true`): `curl https://example.com` gave HTTP 200, and 000
with `network.enabled=false`. A write to `$HOME` gave `Operation not
permitted`; a write to the state root exited 0.

An execpolicy `allow` lifts the sandbox for the matching command only. This
is the mechanism the existing `horch tell`, `horch note`, `horch done` and
`horch ledger` rules use (see the `prepare` comment in `codex.rs`: a
command with a redirection or a `;` runs sandboxed).

## Decision

Candidate a: 2 `prefix_rule`s with `decision = "allow"`, for
`["git", "add"]` and `["git", "commit"]`, in `teammates/_base/codex-execpolicy.md`.

- It names exact subcommands. `git push`, `git reset`, `git clean`,
  `git checkout --`, `git branch -D` and `git worktree remove` match no rule,
  so they stay in the sandbox, and the sandbox refuses their writes to the
  git metadata. `horch ledger finish` does landing and cleanup.

Rejected:

- b1 and b2: each alone does not let `git add` succeed.
- b4: it works, but it needs 2 extra roots and it opens every local git
  command on the whole common dir. A sandboxed `git branch -D` deleted a
  branch of another checkout.
- A bare `["git"]` prefix: it allows push, reset and every other subcommand.

## Limits that remain

- The rule matches only a plain `git add ...` or `git commit ...`. A
  compound line (`git add x && git commit -m y`), a redirection, or a global
  option before the subcommand (`git -C dir commit`) runs sandboxed and
  fails. Workers run the 2 commands separately.
- An allowed `git commit` runs its hooks outside the sandbox too.
- `git push` is not tested against a network remote: the check pushed to a
  local bare repository only, and the sandbox refused the write.
