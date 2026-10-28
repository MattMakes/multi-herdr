# Live check: a Codex worker commits in a linked worktree

Run 2026-10-28 on macOS (Darwin 25.5.0), `codex-cli 0.165.0`, git 2.55.0,
by `opus-1` for ticket T-10 (fleet-reliability 01, decisions 01b and 01c).
No model session started: every step used `codex sandbox` (Seatbelt, no
model) or `codex execpolicy check`.

## How to run

    cargo build -p horch
    scripts/live/codex-git.sh

The script makes a scratch repository under `/private/tmp` in the fleet
layout (a main checkout and `.worktrees/<name>`), with its own ledger file
and state folder, and no remote. It unsets every `HORCH_*` variable first: a
worker pane sets `HORCH_PROJECT_DIR`, and without the unset `horch ledger`
makes its run branch and worktree in that project. The script stops if the
worktree is not under its scratch folder. Each check prints `PASS`, `FAIL`
or `SKIP`; the exit code is 1 when any check fails.

The sandbox is the profile a Codex write pane launches in, `fleet_write`
(`fleet_write_profile_args` in `crates/horch-core/src/harness/codex.rs`),
after decision 01c:

    codex sandbox -P fleet_write -C <pane folder> \
      -c 'permissions.fleet_write.extends=":workspace"' \
      -c 'permissions.fleet_write.filesystem={":project_roots"={"."="write",".agents"="write"},":slash_tmp"="read",":tmpdir"="read"}' \
      -c 'permissions.fleet_write.workspace_roots={"<state>"=true}' \
      -c 'permissions.fleet_write.network.enabled=true' -- <command>

## What a fleet_write pane can write

Before 01c, `fleet_write` also granted `".git"="write"`, `".codex"="write"`
and `".aws"="write"` under the project roots (added on main in `2b5af80`).
A pane's project root is its own folder: the worktree, or the main checkout
for a worker that runs in the shared checkout.

| Pane folder | Path | Before 01c | After 01c |
|---|---|---|---|
| worktree | `<main>/.git/hooks/pre-commit` | no | no |
| worktree | `<main>/.git/config` | no | no |
| worktree | `<main>/.git/worktrees/<name>/` files | no | no |
| worktree | `<main>/.codex/` files | no | no |
| worktree | the worktree's `.codex/` files | yes | no |
| worktree | the worktree's `.git` file | yes | no |
| worktree | the worktree's `.agents/` files | yes | yes |
| worktree | the worktree's `.aws/` files | yes | no |
| shared checkout | `<main>/.git/hooks/pre-commit` | **yes** | no |
| shared checkout | `<main>/.git/config` | **yes** | no |
| shared checkout | `<main>/.codex/` files | **yes** | no |

The 3 rows in bold were a sandbox escape on main: a pane in the shared
checkout could plant a hook or a config entry that runs later, outside any
sandbox, in a Claude worker's shell or the operator's.

Does Codex read a planted `.codex/`? `codex sandbox -P probe` did not load a
`[permissions.probe]` table from a trusted project's `.codex/config.toml`
(`default_permissions requires a [permissions] table`); the same table in
`CODEX_HOME/config.toml` loaded (exit 0). An interactive Codex launch was
not tested: it needs a model session. The grant is removed either way.

Kept and removed:

- `.git`, `.codex`, `.aws`: removed. `:workspace` keeps them read-only.
  Nothing in the repository needs `.aws`.
- `.agents`: kept. `godot-tech-lead` and `ue-tech-lead` fall back to
  `codex-sol` and write `.agents/<engine>-project-context.md`.

The cost: with `.git` read-only, a sandboxed `git worktree add` exits 255
(`cannot lock ref`), and a sandboxed `git add` in the shared checkout exits
128 (`index.lock`). So a Codex worker commits only in a ledger task, through
`horch ledger commit`, and is not a primary for non-ledger work that
commits. The Codex teammate descriptions say this.

## Result: shipped design (scripts/live/codex-git.sh, exit 0, 22 PASS)

| Check | Result |
|---|---|
| `codex execpolicy check` on the shipped worker rules: `git add a.txt`, `git commit -m x`, `git -c core.hooksPath=/dev/null commit -m x`, `git push origin HEAD` | no match for each: they run sandboxed |
| `codex execpolicy check`: `horch ledger commit -m x` | allow (the existing `["horch", "ledger"]` rule) |
| the 11 grant rows above | as in the "After 01c" column |
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
machine. The operator deletes the 2 branches. The script now has no remote
and pushes only to a local bare repository by path.

So: the exec policy does not lift the sandbox for `git push`, but with the
network on, the `fleet_write` sandbox does not stop a push to a network
remote when SSH credentials are reachable. The sandbox refuses a push only
when the remote is a local path outside its writable roots. The Codex
teammate descriptions say this.

## Decision (01b, hardened by 01c)

- No `prefix_rule` allows a raw `git` command. A Codex worker commits with
  `horch ledger commit -m "<message>" [-- <path>...]`, which the existing
  `["horch", "ledger"]` rule runs outside the sandbox.
- `horch ledger commit` (`crates/horch/src/cmd/taskledger_commit.rs`):
  - takes the worker from `HORCH_WORKER_ID`, and refuses a worker with no
    claimed task or with a review task. It also refuses unless the worker is
    `working`, the task names it as its current worker, the task is
    `running` or `landing`, and the worktree is `open`: a reaped or
    requeued worker keeps its old task id;
  - takes the worktree from the ledger row of that task, and refuses when
    the current folder is outside it;
  - runs an absolute git only: `HORCH_GIT_BIN` when it is absolute, else
    the first of `/usr/bin/git`, `/opt/homebrew/bin/git` and
    `/usr/local/bin/git` at 2.40 or later. Never a lookup through `PATH`:
    a worker-writable folder on it would run a fake git. It refuses a git
    inside the worktree or the state root, and every git child gets
    `PATH=/usr/bin:/bin`. `horch doctor` warns when no such git exists;
  - takes the git dir from trusted metadata only: the 1 entry under the
    common dir (`git rev-parse --path-format=absolute --git-common-dir` in
    the main checkout the ledger recorded) whose `gitdir` file names the
    recorded worktree. Not the worktree's folder name: git appends a number
    when that admin name is taken. It refuses on 0 or 2 or more entries, and
    unless `<worktree>/.git` is a file that says exactly
    `gitdir: <that entry>`;
  - runs every git call with `GIT_DIR=<that git dir>` and
    `GIT_WORK_TREE=<worktree>`, every other `GIT_*` variable removed except
    the author and committer name, email and date, `GIT_CONFIG_NOSYSTEM=1`,
    `GIT_CONFIG_GLOBAL=/dev/null`, `GIT_ATTR_SOURCE=<empty tree>` (so no
    `.gitattributes` file selects a filter), `GIT_PAGER=cat`,
    `GIT_EDITOR=true` and `GIT_TERMINAL_PROMPT=0`. It refuses to run on git
    older than 2.40, which ignores `GIT_ATTR_SOURCE`. `user.name` and
    `user.email` come from the operator's global config as values;
  - passes `-c core.hooksPath=/dev/null -c core.fsmonitor=false
    -c commit.gpgsign=false -c gpg.program=/usr/bin/false` to every call;
  - refuses a path outside the worktree, inside `.git`, or in a nested
    repository (a folder below the worktree root with its own `.git`). It
    checks the path as given and its resolved target, and gives git the
    path as given, so a symlink is committed as the link itself;
  - stages with `git add -- <paths>` (or `git add -A`), commits with
    `git commit -m <message> -- <paths>` (only the named files) or
    `git commit -m <message>`, prints the commit id, and records the
    progress event `commit <short id>`. A failed ledger write after the
    commit prints a warning and still exits 0: the commit exists.
- End-to-end tests (`crates/horch-e2e/tests/ledger_commit.rs`, 9 tests): a
  commit lands without hooks; refused paths; refused for a review task or no
  task; a rewritten `.git` file is refused and its config never runs; a
  `* filter=evil` attribute with `filter.evil.clean` in the repository
  config does not run the filter, and the commit lands; a symlink is
  committed as the link, and links that lead outside are refused; a stopped
  worker cannot commit; a fake git first on `PATH`, or named inside the
  worktree, never runs; a worktree whose admin name is taken still commits.
- The caller cannot steer the command through its environment.
  `codex execpolicy check` on the shipped rules gives no match for
  `HORCH_GIT_BIN=x horch ledger commit -m x`, for
  `env HORCH_GIT_BIN=x horch ledger commit -m x`, and for the same line in
  `bash -lc`. So an allowed `horch ledger` call runs with the environment
  horch set at launch. The ledger file is in `~/.horch`, which is outside
  the sandbox's writable roots.

## Earlier candidates (rejected)

From the first runs of this check, sandboxed in `fleet_write` (with the
pre-01c grants):

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
- A Codex worker cannot commit outside a ledger task.
- `git add -A` (no paths) stages every change in the worktree. A nested
  repository there is staged as a gitlink (mode 160000). Checked with the
  same git flags: the nested repository's clean filter and
  `core.fsmonitor` did not run.
- Commits skip the repository's own hooks and attribute filters (Git LFS
  too). `horch ledger finish` runs the ticket's check before it lands.
