# herdr-install is a Rust port that reads the manifests of herdr update

`crates/herdr-install` installs the latest Herdr CLI. It is the Rust port of
<https://herdr.dev/install.sh> and <https://herdr.dev/install.ps1>, as 1
binary that needs no shell, no curl and no PowerShell. It reads the same
release manifests as `herdr update` (`crates/herdr-install/src/manifest.rs`),
so installs and updates agree on what "latest" means. Fetching uses ureq
with rustls (`crates/herdr-install/src/net.rs`), so there is no OpenSSL to
find at build or run time on any platform. Avoiding that system dependency
is the reason the installer is a Rust binary rather than a shell script
plus curl.

## Consequences

- The 2 channels use 2 different asset shapes, and `manifest::RawAsset`
  accepts both. `https://herdr.dev/latest.json` (stable) is
  `{"version": "0.8.0", "assets": {"macos-aarch64": "<url>"}}`, with each
  platform key mapping to a bare URL.
  `https://herdr.dev/preview.json` (preview) is
  `{"base_version": "0.8.0", "build_id": "...", "assets":
  {"windows-x86_64": {"url": ..., "sha256": ..., "format": "zip"}}}`.
- The platform differences are deliberate. macOS and Linux get 1 binary at
  `~/.local/bin/herdr` from the stable channel. Windows gets the preview
  channel.
- `manifest::Channel::default_for` picks preview on Windows because Herdr
  publishes Windows builds only on preview prereleases while native Windows
  support is in beta. It is the only channel that can serve Windows. The
  official PowerShell installer refuses `-Channel stable` for the same
  reason.
- `manifest::Target::asset_key` maps Windows on ARM64 to the
  `windows-x86_64` asset. There is no native ARM64 Windows asset, and the
  x86_64 build runs under emulation.
- The asset SHA-256 is verified only when the manifest publishes a digest
  (`crates/herdr-install/src/main.rs`). Stable manifests publish no
  digests, so on the stable channel TLS to the release host is the only
  integrity guarantee.
