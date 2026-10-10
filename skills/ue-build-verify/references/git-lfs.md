# Git LFS in an Unreal project

The project keeps its binary assets in Git LFS, and the assets are *lockable*. A `.uasset` or `.umap` cannot be merged, so only one person changes it at a time: the one who holds its lock. This file gives the setup, the recommended `.gitattributes`, and the lock rules for a fleet pane.

## Setup

- `git lfs install` runs once per machine and user. It adds the LFS filters to the global git config. Check: `git lfs env` prints the filter lines. If `git lfs` is not installed, send `QUESTION:`; do not install it yourself.
- A new worktree needs `git lfs pull` before you build or start the editor. Without it, each asset is a small text pointer, and the editor fails to load it. Check: `git lfs ls-files` marks each file with `*` (the object is present) and not `-` (only the pointer).
- LFS objects live in the common `.git/lfs` directory of the clone, and every worktree of that clone shares it. A second worktree downloads only the objects that are missing.

## Recommended `.gitattributes`

The operator or the orchestrator owns this file. Do not change it unless the orchestrator assigned that change. Recommend it in `QUESTION:` or in the project context when a project has no LFS rules.

```
# Unreal assets: binary, cannot be merged
*.uasset filter=lfs diff=lfs merge=lfs -text lockable
*.umap   filter=lfs diff=lfs merge=lfs -text lockable

# Source art, audio, video and fonts that a project imports
*.fbx    filter=lfs diff=lfs merge=lfs -text lockable
*.obj    filter=lfs diff=lfs merge=lfs -text lockable
*.abc    filter=lfs diff=lfs merge=lfs -text lockable
*.blend  filter=lfs diff=lfs merge=lfs -text lockable
*.psd    filter=lfs diff=lfs merge=lfs -text lockable
*.png    filter=lfs diff=lfs merge=lfs -text lockable
*.tga    filter=lfs diff=lfs merge=lfs -text lockable
*.exr    filter=lfs diff=lfs merge=lfs -text lockable
*.hdr    filter=lfs diff=lfs merge=lfs -text lockable
*.wav    filter=lfs diff=lfs merge=lfs -text lockable
*.ogg    filter=lfs diff=lfs merge=lfs -text lockable
*.mov    filter=lfs diff=lfs merge=lfs -text lockable
*.mp4    filter=lfs diff=lfs merge=lfs -text lockable
*.ttf    filter=lfs diff=lfs merge=lfs -text lockable
*.otf    filter=lfs diff=lfs merge=lfs -text lockable
```

Add any other binary type the project imports (for example `.mp3` audio or `.webm` video) with the same attributes.

Do not put `Binaries/`, `Intermediate/`, `Saved/`, `DerivedDataCache/` or cooked output (`.uexp`, `.ubulk`, `.pak`) in LFS. They are build output and belong in `.gitignore`.

Check a path: `git check-attr filter lockable -- "<path>"` prints `filter: lfs` and `lockable: set`.

## Locks

`lockable` makes Git LFS check the file out read-only when the current user does not hold its lock. Locks live on the Git LFS server of the remote, not in the clone. If `lfs.setlockablereadonly` is `false`, lockable files stay writable, so a writable file does not prove a lock: check with `git lfs locks`.

1. **Change a lockable file only when the orchestrator assigned that asset to you.** The assignment names the path.
2. **Lock it before you change it:** `git lfs lock "<path>"`. The file must exist in the working copy. Check: `git lfs locks --path="<path>"` lists the lock.
3. **A lock held by someone else is `BLOCKED:`.** Report the path and the owner from `git lfs locks --path="<path>"`. Never run `git lfs unlock --force`, and never ask another pane to unlock.
4. **Panes can share one git user.** Then a lock "owned by you" can be another pane's lock. The orchestrator's assignment, not the owner name, says who may change the file.
5. **Keep your locks.** List each lock you hold in `DONE:`. Run `git lfs unlock "<path>"` only when the orchestrator tells you to. Unlock needs a clean `git status` for the file, so commit first.
6. **If `git lfs lock` fails because the remote has no locking support, or there is no remote,** report `BLOCKED:` with the error. Do not change the asset without a lock.

## A read-only file

A read-only lockable file is the Git LFS sign that you do not hold its lock. The editor fails to save it, and a build or a script fails with "Permission denied" or "access denied". Report `BLOCKED:` with the path and the output of `git lfs locks --path="<path>"`. Never `chmod` it, never `attrib -r` it, and never copy it to a new name to get around the lock.

## Before you commit

- `git lfs status` lists the LFS files in the commit.
- A new binary file that `git check-attr` does not show as `filter: lfs` would go into git as a normal blob. Do not commit it. Send `QUESTION:` with the path and the missing pattern.

## Sources

- Git LFS manual pages, checked 2026-10-04: `git-lfs-lock` (lock against the LFS server; the file must exist), `git-lfs-unlock` (clean status needed; `--force` removes another user's lock), `git-lfs-ls-files` (`*` full object, `-` pointer), `git-lfs-config` (`lfs.storage` default `.git/lfs`; `lfs.setlockablereadonly` default `true`): https://github.com/git-lfs/git-lfs/tree/main/docs/man
- Community Unreal setups with `git lfs track "*.uasset" --lockable` and read-only checkout: https://www.stevestreeting.com/2020/08/09/my-unreal-engine-vcs-setup-gitea--git--lfs--locking/ and https://miltoncandelero.github.io/unreal-git
- Live check `docs/live-checks/unreal.md` (git-lfs 3.4.0, 2026-10-06): with the `.gitattributes` above, a `.uasset` saved by UE 5.8.1 gives `filter: lfs` and `lockable: set`, `git lfs ls-files` marks it `*`, a fresh checkout of it is read-only, and `git lfs lock` in a clone with no remote fails with `missing protocol: ""` (rule 6).
- No Epic 5.8 page documents Git LFS locks. The editor's own Git source control provider is not covered here; the fleet uses the `git lfs` commands above.
