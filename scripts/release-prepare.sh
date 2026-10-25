#!/usr/bin/env bash
# Prepares a release for .github/workflows/auto-release.yml.
#
# Usage: scripts/release-prepare.sh patch|minor|major
#
# When CHANGELOG.md has no entries under "## Unreleased", prints nothing and
# changes nothing: the merge was not user-visible, so it is not a release.
# Otherwise it bumps [workspace.package] version in Cargo.toml (and the
# workspace crates in Cargo.lock), renames "## Unreleased" to
# "## vX.Y.Z - YYYY-MM-DD" under a new empty "## Unreleased", and prints
# X.Y.Z.
set -euo pipefail
cd "$(dirname "$0")/.."

bump=${1:?usage: release-prepare.sh patch|minor|major}

unreleased=$(awk '
  /^## Unreleased[[:space:]]*$/ { found = 1; next }
  found && /^## / { exit }
  found { print }
' CHANGELOG.md)
if ! grep -q '[^[:space:]]' <<<"$unreleased"; then
  exit 0
fi

current=$(awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p && /^version[[:space:]]*=/{sub(/^[^"]*"/,"");sub(/".*/,"");print;exit}' Cargo.toml)
IFS=. read -r major minor patch <<<"$current"
case $bump in
  major) next="$((major + 1)).0.0" ;;
  minor) next="$major.$((minor + 1)).0" ;;
  patch) next="$major.$minor.$((patch + 1))" ;;
  *) echo "unknown bump '$bump'" >&2; exit 2 ;;
esac

# Only the version line inside [workspace.package].
awk -v v="$next" '
  /^\[workspace\.package\]/ { p = 1 }
  /^\[/ && !/^\[workspace\.package\]/ { p = 0 }
  p && /^version[[:space:]]*=/ { print "version = \"" v "\""; next }
  { print }
' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml
cargo update --workspace --quiet

today=$(date -u +%Y-%m-%d)
awk -v h="## v$next - $today" '
  /^## Unreleased[[:space:]]*$/ && !done { print "## Unreleased"; print ""; print h; done = 1; next }
  { print }
' CHANGELOG.md > CHANGELOG.md.tmp && mv CHANGELOG.md.tmp CHANGELOG.md

echo "$next"
