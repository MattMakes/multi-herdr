---
name: release
description: Use when the operator types /release, or asks to ship, publish or check a new version of multi-herdr so friends get it through `horch self-update`. Releases are automatic on merge to main; this skill gets a release out (merges a PR with a changelog line, or re-runs auto-release), follows the auto-release and release workflows, fixes forward on failure, and verifies the published assets. For opening a PR, use /push-pr.
---

# /release - follow and verify an automatic release

Releases need no hand steps. On every push to `main`,
`.github/workflows/auto-release.yml`:

1. Does nothing when `CHANGELOG.md` has no entries under `## Unreleased`.
2. Picks the bump: patch, or minor/major when a PR merged since the last tag
   has the label `release:minor` or `release:major`.
3. Runs `scripts/release-prepare.sh`: bumps `[workspace.package] version` in
   `Cargo.toml` and `Cargo.lock`, and renames `## Unreleased` to
   `## vX.Y.Z - YYYY-MM-DD` under a new empty `## Unreleased`.
4. Commits `Release vX.Y.Z` to `main` as github-actions[bot], tags it with
   annotated `vX.Y.Z`, and pushes both in one atomic push.
5. Calls `.github/workflows/release.yml`, which builds 5 targets and publishes
   a GitHub release with `multi-herdr-vX.Y.Z-<target>.tar.gz|.zip`,
   `SHA256SUMS`, and the changelog section as the notes.

Every `horch` older than the release then prints an update notice at boot,
and `horch self-update` installs it. A published tag must never be moved or
deleted: a friend's `horch` may already have seen it. Everything after the
tag is fix-forward.

This skill is for an operator session opened in this repository.

## 1. Get a release started

- **An open PR with a changelog line:** check CI is green
  (`gh pr checks <n>`), set the bump label if it should not be a patch, and
  ask the operator once: "Merge and publish? Every horch on your and your
  friends' machines will offer it." On yes,
  `gh pr merge <n> --squash --delete-branch`.
- **Entries already on `main` but no release** (an earlier run failed before
  tagging): `gh workflow run auto-release.yml --ref main`.
- **Nothing under `## Unreleased` on `main`:** there is nothing to release.
  Say so; a change reaches users through `/push-pr` plus a changelog line.

## 2. Follow the workflows

1. `gh run list --workflow auto-release.yml --limit 3 --json databaseId,headSha,status,conclusion,url`
   and take the run for the merge commit (retry for up to a minute while it
   registers). `gh run watch <id> --exit-status`.
2. The run summary says `Releasing vX.Y.Z (<bump>)` or that there was
   nothing to release. A notice "main moved while preparing" means a newer
   push's run will release; follow that run instead.
3. The `release` job runs `release.yml` inside the same run, so the watch
   above covers the build and publish too.

## 3. On failure

Read `gh run view <id> --log-failed` and report.

- **Before the tag was pushed** (the `prepare` job failed): nothing reached
  users. Fix the cause on a branch through `/push-pr`. Merging it re-runs
  auto-release, which still sees the Unreleased entries.
- **After the tag** (the `release` job failed): re-run the failed jobs
  (`gh run rerun <id> --failed`) for a flaky runner. For a broken workflow,
  merge a fix to `release.yml`, then rebuild the same tag with
  `gh workflow run release.yml -f tag=vX.Y.Z`. A publish that failed
  mid-upload leaves a draft release; a re-run deletes the draft (never the
  tag) and publishes again. A bug in horch's own code is a new release: a
  fix PR with a changelog line.

## 4. Verify what was published

1. `gh release view vX.Y.Z --json url,assets`. Expect 6 assets: 5 archives
   (`aarch64-apple-darwin`, `x86_64-apple-darwin`,
   `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
   `x86_64-pc-windows-msvc`) and `SHA256SUMS`.
2. Download the archive for this machine and `SHA256SUMS` into a temp dir,
   check the digest, extract, and run `./horch --version`. It must print
   `X.Y.Z`. This is the check that a friend's `horch self-update` will work.
3. `git pull --ff-only origin main` so the local checkout has the release
   commit.

## 5. Report

Give the operator: the release URL, the asset count, and the one-liner
friends run to install for the first time (from the README). Remind them
that friends already on a release just run `horch self-update`.
