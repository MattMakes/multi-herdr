# Changelog

<!-- On a merge to main with entries under "## Unreleased", the auto-release workflow renames it to "## vX.Y.Z - YYYY-MM-DD", adds a new empty "## Unreleased" above it, and publishes the section as the release notes. -->

## Unreleased

## v0.2.0 - 2026-10-25

- Codex workers and the Codex orchestrator start with `--approve-for-me`:
  a request to act outside the sandbox goes to Codex's automatic reviewer
  instead of failing. Plan panes stay read-only with the same reviewer.
- Worker reports stay queued until `horch inbox` acknowledges them. The
  courier resends them to an idle orchestrator.
- Prebuilt releases for macOS, Linux and Windows, with `horch self-update`
  and a boot-time notice when a newer release exists.
- `horch tell` keeps long messages in a folder per workspace, so one fleet
  can no longer read another fleet's messages.
