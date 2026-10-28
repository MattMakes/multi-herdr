# Live check: a Codex worker commits in a linked worktree

Run 2026-10-28 on macOS (Darwin 25.5.0), `codex-cli 0.165.0`, by `opus-1`
for ticket T-10 (fleet-reliability 01). No model session started: every
step used `codex sandbox` (Seatbelt, no model) or `codex execpolicy check`.

## How to run

No script: the steps are manual and take about 2 minutes. Use a scratch
folder `$S` under `/private/tmp` (the shipped profile keeps `/tmp`
read-only, so the main repository is outside every writable root, as in a
real fleet).

    git init -q $S/main && git -C $S/main commit -q --allow-empty -m init
    git -C $S/main worktree add -q $S/wt -b feat
    git init -q --bare $S/remote.git && git -C $S/wt remote add origin $S/remote.git
    mkdir $S/state

1. Run each "sandboxed" row below with the `codex sandbox` line under Setup.
   For candidate b, add the roots to `workspace_roots` as `<EXTRA>`.
2. Run each "execpolicy" row with
   `codex execpolicy check -r crates/horch-core/tests/fixtures/harness/codex_rules_worker.rules -- <command>`.
   Put `--` before the command, or codex reads `-m` and `--hard` as its own
   options.
3. Run each "outside the sandbox" row with plain `git` in `$S/wt`: an
   execpolicy `allow` runs the command exactly so.

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

Shipped rule set, after the code sweep (round 1, finding F10):
`codex execpolicy check -r crates/horch-core/tests/fixtures/harness/codex_rules_worker.rules`
(the rendered rules of every Codex worker pane):

| Command | Decision |
|---|---|
| `git add a.txt` | allow |
| `git -c core.hooksPath=/dev/null commit -m x -- d.txt` | allow |
| `git commit -m x` | no match: sandboxed, refused as in the baseline row |
| `git push origin feat` | no match: sandboxed, refused as above |
| `git reset --hard` | no match |
| `git branch -D feat` | no match |

Hooks and staged paths, outside the sandbox in `$S/wt`, with
`core.hooksPath=.hooks` holding a `pre-commit` and a `post-commit` hook that
each touch a marker file, and `d.txt` and `e.txt` both staged:

| Command | Exit | Result |
|---|---|---|
| `git commit -m base -- e.txt` | 0 | both markers written: a plain commit runs worktree-controlled hooks outside the sandbox |
| `git -c core.hooksPath=/dev/null commit -m f10 -- d.txt` | 0 | no marker written; the commit holds `d.txt` only; `e.txt` stays staged |

The sandbox itself keeps the network on (`fleet_write` sets
`network.enabled=true`): `curl https://example.com` gave HTTP 200, and 000
with `network.enabled=false`. A write to `$HOME` gave `Operation not
permitted`; a write to the state root exited 0.

An execpolicy `allow` lifts the sandbox for the matching command only. This
is the mechanism the existing `horch tell`, `horch note`, `horch done` and
`horch ledger` rules use (see the `prepare` comment in `codex.rs`: a
command with a redirection or a `;` runs sandboxed).

## Decision

Candidate a, narrowed: 2 `prefix_rule`s with `decision = "allow"` in
`teammates/_base/codex-execpolicy.md`:

- `["git", "add"]`.
- `["git", "-c", "core.hooksPath=/dev/null", "commit"]`. A plain
  `["git", "commit"]` ran hooks outside the sandbox, and a worker can write
  hooks that `core.hooksPath` names inside its worktree (for example
  `.husky/`). With the hooks path fixed to `/dev/null`, no hook runs.
- `_base/fleet-worker.md` tells every worker to commit with
  `git -c core.hooksPath=/dev/null commit -m "<message>" -- <files>`. The
  `-- <files>` form commits only the named files, so paths staged by an
  earlier attempt stay out of the commit.
- `git push`, `git reset`, `git clean`, `git checkout --`, `git branch -D`
  and `git worktree remove` match no rule, so they stay in the sandbox, and
  the sandbox refuses their writes to the git metadata. `horch ledger finish`
  does landing and cleanup.

Rejected:

- b1 and b2: each alone does not let `git add` succeed.
- b4: it works, but it needs 2 extra roots and it opens every local git
  command on the whole common dir. A sandboxed `git branch -D` deleted a
  branch of another checkout.
- A bare `["git"]` prefix: it allows push, reset and every other subcommand.
- A plain `["git", "commit"]` prefix: it runs hooks outside the sandbox.

## Limits that remain

- The rules match only a plain `git add ...` or
  `git -c core.hooksPath=/dev/null commit ...`. A compound line, a
  redirection, or another option before the subcommand (`git -C dir commit`)
  runs sandboxed and fails. Workers run the 2 commands separately.
- The rules do not check the working directory. An allowed `git add` or
  commit can write the git metadata of any repository the worker names by
  `cd`. A check of the task worktree needs a `horch` commit command, which
  this ticket does not add.
- Commits skip the repository's own hooks. `horch ledger finish` runs the
  ticket's check before it lands.
- `git push` is refused only by the sandbox's filesystem limits. The check
  pushed to a local bare repository. It did not push to a network remote,
  and the network is on in `fleet_write`, so a push to a network remote with
  reachable credentials is not proven refused. The teammate descriptions do
  not claim that a push is impossible.
