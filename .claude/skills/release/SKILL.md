---
name: release
description: Use when the operator types /release [patch|minor|major], or asks to cut, tag, ship or publish a new version of multi-herdr so friends get it through `horch self-update`. Bumps the version, rolls the changelog, opens a release PR whose description tracks every step, merges it once CI is green, tags the merge commit, follows the release workflow that builds the prebuilt binaries, and verifies the published assets. For a change that should not trigger an update notice, use /push-pr instead.
---

# /release - a tagged release with prebuilt binaries

A release is a tag `vX.Y.Z` on `main` whose version equals
`[workspace.package] version` in `Cargo.toml`. Pushing the tag runs
`.github/workflows/release.yml`, which builds 5 targets and publishes a GitHub
release with `multi-herdr-vX.Y.Z-<target>.tar.gz|.zip` archives and
`SHA256SUMS`. Every `horch` older than the release then prints an update
notice at boot, and `horch self-update` installs it. The full contract is in
`ai_docs/specs/self-update/design.md` when that file exists locally.

Releasing reaches the operator and their friends at once, and a published tag
must never be moved. So this skill confirms once, at the point of no return
(step 7), and treats everything after the tag push as fix-forward.

This skill is for an operator session opened in this repository.

## The living PR description

Keep the release PR's description in a file outside the tree:
`BODY="$(git rev-parse --git-dir)/pr-body-release-vX.Y.Z.md"`. After every
step below, tick its box, rewrite the file and run
`gh pr edit <n> --body-file "$BODY"`. A merged PR can still be edited, so keep
updating it through publish. The PR then tells anyone who opens it exactly
how far the release got.

```markdown
> **Release vX.Y.Z** - status: <one line, kept current>

- [x] Version bumped 0.2.0 -> 0.3.0
- [x] CHANGELOG rolled
- [x] Local tests passed
- [ ] CI green
- [ ] Merged
- [ ] Tagged `vX.Y.Z` at <sha>
- [ ] Release workflow: <run link>
- [ ] Published: <release link>, <N> assets verified

## What's in vX.Y.Z

<the CHANGELOG section, verbatim>
```

End it with the PR attribution line your session requires.

## 1. Preflight

1. `git fetch origin --tags`. The tree must be clean (`git status --short`
   empty). If it is not, ask whether to commit the work into this release
   (then do it as `/push-pr` steps 1-4 would, on the release branch) or stop.
2. `gh auth status` must succeed.
3. `.github/workflows/release.yml` must exist on `origin/main` (`git show
   origin/main:.github/workflows/release.yml`). Without it a tag publishes
   nothing; stop and say so.
4. Find the last release: `git tag --list 'v*' --sort=-v:refname | head -1`
   (none means this is the first release).
5. There must be something to release: commits in `<last tag>..HEAD`, or a
   non-empty `## Unreleased`. Otherwise stop.

## 2. Choose the version

Current version: `[workspace.package] version` in `Cargo.toml`.

- With an argument (`patch`, `minor`, `major`), bump that part.
- Without one, propose a bump from the Unreleased entries and the commit
  subjects since the last tag: a new feature or a changed behavior is
  `minor`, fixes only are `patch`. Before 1.0, a breaking change is `minor`,
  not `major`. Ask the operator to confirm the number, with the 3 options.

The new tag `vX.Y.Z` must not exist locally or on origin
(`git ls-remote --tags origin vX.Y.Z` prints nothing).

## 3. Branch, bump, roll the changelog

1. Create `release/vX.Y.Z` from the current HEAD (normally an up-to-date
   `main`; if HEAD is a feature branch, its commits ship in this release, so
   say so).
2. Set `[workspace.package] version = "X.Y.Z"` in `Cargo.toml`. Then run
   `cargo update --workspace` so `Cargo.lock` follows. Check:
   `cargo run -q --bin horch -- --version` prints `X.Y.Z`.
3. In `CHANGELOG.md`, rename `## Unreleased` to `## vX.Y.Z - YYYY-MM-DD`
   (today) and add a new empty `## Unreleased` above it. If the section is
   thin, fill it from `git log <last tag>..HEAD --format='%s'`, rewritten for
   users. The release workflow fails on an empty section, and these lines are
   what friends read in the update notice's link.

## 4. Test, commit, push, open the PR

1. `cargo test --workspace`. On a failure, stop and show it.
2. Commit `Release vX.Y.Z` (body: the changelog section; add the attribution
   lines your session requires). `git push -u origin release/vX.Y.Z`.
3. Write `$BODY` from the template above and run
   `gh pr create --base main --title "Release vX.Y.Z" --body-file "$BODY"`.

## 5. CI

`gh pr checks <n> --watch --interval 30`. Wait for the first check to appear
before you trust an empty result. On red: read `gh run view <id>
--log-failed`, fix on the release branch, push, repeat. Tick "CI green".

## 6. Confirm the point of no return

Ask the operator once: "Merge and publish vX.Y.Z? Every horch on your and
your friends' machines will offer it." Show the changelog section. Skip the
question only when the operator said "yes" or passed `--yes` with the
command. On "no", leave the PR open and stop; update the status line to say
the release is waiting.

## 7. Merge and tag

1. `gh pr merge <n> --squash --delete-branch`. Tick "Merged".
2. `git switch main && git pull --ff-only origin main`. The merge commit is
   `HEAD`. Check that its `Cargo.toml` version is `X.Y.Z`; if not, stop
   (something else merged in between) and report.
3. `git tag -a vX.Y.Z -m "vX.Y.Z" HEAD && git push origin vX.Y.Z`.
   Tick "Tagged" with the sha.

## 8. Follow the release workflow

1. Find the run: `gh run list --workflow release.yml --limit 5 --json
   databaseId,headBranch,status,url` and take the one for `vX.Y.Z` (retry for
   up to a minute while it registers). Put its link in the PR.
2. `gh run watch <id> --exit-status`.
3. On failure: read `--log-failed` and report. Do not delete or move the tag:
   a friend's `horch` may already have seen it. Fix forward: either re-run the
   failed jobs (`gh run rerun <id> --failed`) for a flaky runner, or fix the
   workflow on a branch, merge it, and run
   `gh workflow run release.yml -f tag=vX.Y.Z` to rebuild the same tag. A fix
   to horch's own code is a new patch release instead.

## 9. Verify what was published

1. `gh release view vX.Y.Z --json url,assets`. Expect 6 assets: 5 archives
   (`aarch64-apple-darwin`, `x86_64-apple-darwin`,
   `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
   `x86_64-pc-windows-msvc`) and `SHA256SUMS`.
2. Download the archive for this machine and `SHA256SUMS` into a temp dir,
   check the digest, extract, and run `./horch --version`. It must print
   `X.Y.Z`. This is the check that a friend's `horch self-update` will work.
3. Tick "Published" with the release link and the asset count. Set the status
   line to `published`.

## 10. Report

Give the operator: the release URL, the PR URL, the asset count, and the
one-liner friends run to install for the first time (from the README). Remind
them that friends already on a release just run `horch self-update`.
