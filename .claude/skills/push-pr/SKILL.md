---
name: push-pr
description: Use when the operator types /push-pr, or asks to push the current work, open a PR, or "ship this". Commits, pushes and opens a pull request against main with a description that is kept up to date as checks run. A user-visible change gets a CHANGELOG line, and merging it then publishes a release automatically; a change without one ships quietly. To follow or check a published release, use /release.
---

# /push-pr - a PR; merging it releases when it has a changelog line

Releases are automatic. When a merge to `main` leaves entries under
`## Unreleased` in `CHANGELOG.md`, the `auto-release` workflow bumps the
version, tags it and publishes a release, and every `horch` shows an update
notice. A merge without a changelog line (docs, CI, internal tooling) lands
quietly and ships with the next release. So the changelog line in step 3 is
the decision to release.

This skill is for an operator session opened in this repository. Pushing and
opening a PR are outward-facing: the operator asked for them by invoking the
skill, so do them, but do nothing past opening the PR. Merging stays the
operator's call.

## 1. Check the tree

1. Run `git status --short` and `git branch --show-current`.
2. Nothing to commit and no commits ahead of `origin/main`: say so and stop.
3. On `main`: create a branch first. Name it `<kind>/<short-slug>` from the
   change, kind one of `feat`, `fix`, `docs`, `chore`, `ci`. Never push to
   `main` directly.
4. Run `git diff origin/main -- Cargo.toml`. If `[workspace.package] version`
   changed, stop and ask the operator to revert it: the `auto-release`
   workflow sets the version, and a hand bump makes it skip a number.
5. Look at what is about to be committed. Leave out build output, scratch
   files and anything that looks like a credential. Ask when unsure.

## 2. Run the gates

Run `cargo test --workspace` (and `cargo clippy --workspace --all-targets`
when Rust changed). On a failure, stop and show the failing output. A PR that
is red on arrival wastes the reviewer's time and CI minutes.

## 3. Record it for the next release

When the change is visible to a user (a command, a flag, an output, a fix
they would notice), add one line under `## Unreleased` in `CHANGELOG.md`, in
plain words, written for the user, not the developer. Skip it for internal
refactors, tests and CI. The line becomes the release notes, and it is what
makes the merge a release: no line, no release.

The bump is a patch by default. When the change adds a feature or changes a
behavior, add the label `release:minor` after step 5
(`gh pr edit <n> --add-label release:minor`); before 1.0 a breaking change is
also `release:minor`. Use `release:major` only when the operator asks.

## 4. Commit and push

1. Commit with a message in the repo's style: a short imperative subject, a
   body that says what changed and why. Add the attribution lines your
   session's instructions require.
2. `git push -u origin <branch>`.

## 5. Open the PR with a living description

Keep the description in a file outside the tree so each update rewrites it in
one call. Branch names hold `/`, so flatten it into the file name:
`BODY="$(git rev-parse --git-dir)/pr-body-$(git branch --show-current | tr '/' '-').md"`.

Write it in the style of the repo's recent PRs (`gh pr view <n> --json body`
on a recent merged PR shows it): an opening paragraph that says what the PR
does, then sections per part with Why / Changes. Start it with this status
block, then the prose:

```markdown
> **Status:** checks running
> **Release:** <"patch on merge" / "minor on merge" / "none: no changelog line, ships with the next release">

- [x] Local tests: `cargo test --workspace` passed
- [ ] CI
```

Then:

```
gh pr create --base main --head <branch> --title "<subject>" --body-file "$BODY"
```

End the body with the PR attribution line your session requires.

## 6. Follow CI and keep the description true

1. `gh pr checks <n> --watch --interval 30`. When there are no checks yet
   (the workflow has not started), wait for the first one to appear before
   reading the result.
2. Update the status block to the result: `CI passed`, or `CI failed: <job>`
   with a link to the failing run. Rewrite the file and run
   `gh pr edit <n> --body-file "$BODY"`.
3. On a failure, read the failing log (`gh run view <run-id> --log-failed`),
   say what broke, and offer to fix it. A fix is a new commit on the same
   branch; then repeat this step.

## 7. Report

Give the operator the PR URL, the CI result, and one line on what merging
does: "Merging publishes vX.Y.Z (patch|minor)" when there is a changelog line,
or "Not a release: users get it with the next release" when there is none.
