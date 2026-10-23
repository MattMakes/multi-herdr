# Changelog

<!-- /release renames "## Unreleased" to "## vX.Y.Z - YYYY-MM-DD" and adds a new empty "## Unreleased" above it. -->

## Unreleased

- Worker reports stay queued until `horch inbox` acknowledges them. The
  courier resends them to an idle orchestrator.
- Prebuilt releases for macOS, Linux and Windows, with `horch self-update`
  and a boot-time notice when a newer release exists.
- `horch tell` keeps long messages in a folder per workspace, so one fleet
  can no longer read another fleet's messages.
