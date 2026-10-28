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
- `horch ledger commit` (`crates/horch/src/cmd/taskledger_commit.rs`) keeps
  these invariants. Its module and function comments give the detail, and
  `crates/horch-e2e/tests/ledger_commit.rs` tests each one:
  - Only the worker that owns a running ledger task commits, and only in
    that task's open worktree, from inside it. A review task never commits.
  - git is absolute and 2.41 or later (`HORCH_GIT_BIN`, or a fixed list),
    and never from the worktree, the main checkout or the state root.
  - The git dir comes from the main checkout's metadata, never from the
    worktree's `.git` file, which must point at it.
  - No program that a worker can steer runs: every `GIT_*` variable is
    cleared, system and global config are off, hooks, fsmonitor and signing
    are off, and attributes come from the empty tree. A path under a
    required filter (Git LFS) is refused, and so is a failed lookup.
  - Paths are literal, inside the worktree, outside `.git` and nested
    repositories, and not folders.
  - Every git child has a 120 s deadline, at most 10000 output records of
    at most 4099 bytes, and 4 KiB of stderr. git is killed past any bound.
    1 commit stages at most 10000 paths, 100 MiB per file and 1 GiB in
    all, by length (a sparse file counts in full).
  - No git starts in a gitlink: submodules are ignored, and no
    maintenance runs after the commit.
  - What is committed is what was checked: after git add, the staged
    index is compared with HEAD, and a new gitlink or a path that was not
    checked refuses the commit. git commit takes no pathspec, so it never
    stages from the worktree again.
  - The index changes only when the commit succeeds, under git's own
    `index.lock`. The lock holds horch's pid, so a lock that a killed run
    left is refused as stale, with its path.
- SHA-256: `scripts/live/codex-git.sh`, run with both scratch repositories
  made by `git init --object-format=sha256`, passed every check (exit 0).
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
- A Codex worker cannot commit outside a ledger task with a worktree, and
  cannot rebase: on a land conflict it sends `BLOCKED:` to the orchestrator.
- With no paths, the command stages exactly the changes `git status`
  lists, never the whole tree: `git add -A` of the whole tree starts a git
  in each gitlink, and that git ran the nested repository's clean filter
  (git 2.55, checked with `GIT_TRACE`). An untracked nested repository is
  refused, and a moved gitlink is not staged.
- Commits skip the repository's own hooks and attribute filters. A path
  under a required filter (Git LFS) is refused, not committed raw.
  `horch ledger finish` runs the ticket's check before it lands.
- The empty-tree attribute source replaces `.gitattributes` files only.
  git still reads `$GIT_DIR/info/attributes` and a repository
  `core.attributesFile`, so a filter that the operator sets there still
  runs. A Codex pane cannot write either one: the main checkout's `.git` is
  read-only in `fleet_write` (rows above), and the global config is off.
