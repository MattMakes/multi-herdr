# Releases build from the tip of main and lint the release profile

Users install the release build, so the release path is checked as it
ships. `.github/workflows/auto-release.yml` checks out the tip of main
(`ref: main`), not the pushed sha, because a queued run must see the
release commit that an earlier auto-release run pushed. Checking out the
pushed sha would release again from stale state.

## Consequences

- `.github/workflows/release.yml` is also a reusable `workflow_call`.
  `auto-release.yml` calls it after it pushes the tag, because a tag pushed
  with `GITHUB_TOKEN` does not trigger GitHub's push event, so the
  tag-push trigger alone never fires.
- The packaging step sets `COPYFILE_DISABLE=1`. macOS tar would otherwise
  add AppleDouble `._*` entries for extended attributes to the tarball.
- CI runs a separate clippy job on the release profile
  (`.github/workflows/ci.yml`), and `scripts/phase-gate.sh` runs clippy a
  second time on `--release` bins. Code under `cfg(debug_assertions)` hides
  release-only warnings from the debug build, and `just install` builds
  release.
