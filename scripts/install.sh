#!/bin/sh
# Install multi-herdr from a GitHub release. No Rust toolchain needed.
#   HORCH_VERSION      tag to install, for example v0.2.0 (default: latest)
#   HORCH_INSTALL_DIR  directory for the binaries (default: horch's own default)
set -eu

REPO="MattMakes/multi-herdr"

detect_target() {
  os=$(uname -s)
  arch=$(uname -m)
  case "$arch" in
    arm64 | aarch64) arch=aarch64 ;;
    x86_64 | amd64) arch=x86_64 ;;
  esac
  case "$os-$arch" in
    Darwin-aarch64) echo aarch64-apple-darwin ;;
    Darwin-x86_64) echo x86_64-apple-darwin ;;
    Linux-aarch64) echo aarch64-unknown-linux-gnu ;;
    Linux-x86_64) echo x86_64-unknown-linux-gnu ;;
    *)
      echo "install.sh: unsupported platform: $os $arch" >&2
      exit 1
      ;;
  esac
}

target=$(detect_target)

if [ "${1:-}" = "--print-target" ]; then
  echo "$target"
  exit 0
fi

tag=${HORCH_VERSION:-}
if [ -z "$tag" ]; then
  tag=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" |
    sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)
fi
if [ -z "$tag" ]; then
  echo "install.sh: could not find the latest release tag" >&2
  exit 1
fi

name="multi-herdr-$tag-$target.tar.gz"
base="https://github.com/$REPO/releases/download/$tag"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM
cd "$tmp"

echo "Downloading $name"
curl -fsSL -o "$name" "$base/$name"
curl -fsSL -o SHA256SUMS "$base/SHA256SUMS"

grep "  $name\$" SHA256SUMS > expected.sum || {
  echo "install.sh: $name is not listed in SHA256SUMS" >&2
  exit 1
}
if command -v sha256sum > /dev/null 2>&1; then
  sha256sum -c expected.sum > /dev/null
else
  shasum -a 256 -c expected.sum > /dev/null
fi || {
  echo "install.sh: checksum mismatch for $name" >&2
  exit 1
}

tar -xzf "$name"

if [ -n "${HORCH_INSTALL_DIR:-}" ]; then
  ./horch install --dir "$HORCH_INSTALL_DIR"
else
  ./horch install
fi

if ! command -v herdr > /dev/null 2>&1; then
  echo "herdr is not on PATH: running herdr-install"
  ./herdr-install
fi

echo "Done. Run: horch doctor"
