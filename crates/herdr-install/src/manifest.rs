//! Herdr's release manifests.
//!
//! Two channels with two different asset shapes, both of which must be handled:
//!
//! * `https://herdr.dev/latest.json` (stable) - `{"version": "0.8.0",
//!   "assets": {"macos-aarch64": "<url>"}}`
//! * `https://herdr.dev/preview.json` (preview) - `{"base_version": "0.8.0",
//!   "build_id": "...", "assets": {"windows-x86_64": {"url": ..., "sha256": ...,
//!   "format": "zip"}}}`
//!
//! Using the same manifests as `herdr update` means installs and updates agree on
//! what "latest" is.

use std::collections::BTreeMap;

use anyhow::{bail, Result};
use serde::Deserialize;

pub const STABLE_MANIFEST_URL: &str = "https://herdr.dev/latest.json";
pub const PREVIEW_MANIFEST_URL: &str = "https://herdr.dev/preview.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Preview,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Preview => "preview",
        }
    }

    pub fn manifest_url(self) -> &'static str {
        match self {
            Channel::Stable => STABLE_MANIFEST_URL,
            Channel::Preview => PREVIEW_MANIFEST_URL,
        }
    }

    /// Windows builds are published only on preview prereleases while native
    /// Windows support is in beta, so that is the only channel that can serve it.
    /// The official PowerShell installer refuses `-Channel stable` for the same
    /// reason.
    pub fn default_for(os: Os) -> Self {
        match os {
            Os::Windows => Channel::Preview,
            Os::MacOs | Os::Linux => Channel::Stable,
        }
    }
}

impl std::str::FromStr for Channel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stable" => Ok(Channel::Stable),
            "preview" => Ok(Channel::Preview),
            other => Err(format!("unknown channel '{other}' (expected stable or preview)")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
}

/// A platform, as keyed in a manifest's `assets` map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub os: Os,
    pub arch: Arch,
}

impl Target {
    /// The platform this binary is running on.
    pub fn detect() -> Result<Self> {
        let os = match std::env::consts::OS {
            "macos" => Os::MacOs,
            "linux" => Os::Linux,
            "windows" => Os::Windows,
            other => bail!("unsupported OS: {other}"),
        };
        let arch = match std::env::consts::ARCH {
            "x86_64" => Arch::X86_64,
            "aarch64" => Arch::Aarch64,
            other => bail!("unsupported architecture: {other}"),
        };
        Ok(Self { os, arch })
    }

    /// The manifest asset key for this target.
    ///
    /// Windows on ARM64 maps to the x86_64 build, which runs under emulation -
    /// there is no native ARM64 Windows asset.
    pub fn asset_key(self) -> &'static str {
        match (self.os, self.arch) {
            (Os::MacOs, Arch::X86_64) => "macos-x86_64",
            (Os::MacOs, Arch::Aarch64) => "macos-aarch64",
            (Os::Linux, Arch::X86_64) => "linux-x86_64",
            (Os::Linux, Arch::Aarch64) => "linux-aarch64",
            (Os::Windows, _) => "windows-x86_64",
        }
    }

    /// The Rust target triple, used to name Windows release directories the same
    /// way the official installer does.
    pub fn triple(self) -> &'static str {
        match (self.os, self.arch) {
            (Os::MacOs, Arch::X86_64) => "x86_64-apple-darwin",
            (Os::MacOs, Arch::Aarch64) => "aarch64-apple-darwin",
            (Os::Linux, Arch::X86_64) => "x86_64-unknown-linux-gnu",
            (Os::Linux, Arch::Aarch64) => "aarch64-unknown-linux-gnu",
            (Os::Windows, _) => "x86_64-pc-windows-msvc",
        }
    }

    pub fn describe(self) -> String {
        let os = match self.os {
            Os::MacOs => "macos",
            Os::Linux => "linux",
            Os::Windows => "windows",
        };
        let arch = match self.arch {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
        };
        format!("{os}/{arch}")
    }
}

/// How an asset is packaged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// A bare executable.
    Binary,
    /// A zip holding `herdr.exe` plus its ConPTY runtime.
    Zip,
}

/// One downloadable build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub url: String,
    /// Present on preview manifests only.
    pub sha256: Option<String>,
    pub format: Format,
}

/// An `assets` entry: a bare URL on stable, an object on preview.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawAsset {
    Url(String),
    Detailed {
        url: String,
        #[serde(default)]
        sha256: Option<String>,
        #[serde(default)]
        format: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    /// Stable channel version.
    #[serde(default)]
    version: Option<String>,
    /// Preview channel version parts.
    #[serde(default)]
    base_version: Option<String>,
    #[serde(default)]
    build_id: Option<String>,
    assets: BTreeMap<String, RawAsset>,
}

impl Manifest {
    pub fn parse(raw: &str) -> Result<Self> {
        Ok(serde_json::from_str(raw)?)
    }

    /// The version string to report and, on Windows, to name the release
    /// directory with.
    pub fn version_identity(&self, channel: Channel) -> Result<String> {
        match channel {
            Channel::Stable => self
                .version
                .clone()
                .ok_or_else(|| anyhow::anyhow!("stable manifest has no version field")),
            Channel::Preview => match (&self.base_version, &self.build_id) {
                (Some(base), Some(build)) => Ok(format!("{base}-preview.{build}")),
                // Fall back to whatever the manifest does carry rather than
                // failing an otherwise valid install.
                _ => self
                    .version
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("preview manifest has no version fields")),
            },
        }
    }

    /// The asset for `target`, or an error naming what the manifest does offer.
    pub fn asset(&self, target: Target) -> Result<Asset> {
        let key = target.asset_key();
        let Some(raw) = self.assets.get(key) else {
            bail!(
                "release manifest has no binary for {key} (it offers: {})",
                self.assets.keys().cloned().collect::<Vec<_>>().join(", ")
            );
        };
        Ok(match raw {
            RawAsset::Url(url) => Asset {
                url: url.clone(),
                sha256: None,
                format: infer_format(url),
            },
            RawAsset::Detailed { url, sha256, format } => Asset {
                url: url.clone(),
                sha256: sha256.clone(),
                format: match format.as_deref() {
                    Some("zip") => Format::Zip,
                    Some(_) => Format::Binary,
                    None => infer_format(url),
                },
            },
        })
    }
}

/// Manifests do not always carry `format`, so fall back to the URL's extension.
fn infer_format(url: &str) -> Format {
    if url.ends_with(".zip") {
        Format::Zip
    } else {
        Format::Binary
    }
}

/// Sanitise a version for use in a directory name, as the official installer does.
pub fn safe_version(version: &str) -> String {
    version
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real shape of https://herdr.dev/latest.json.
    const STABLE: &str = r####"{
      "version": "0.8.0",
      "protocol": 19,
      "notes": "### Added\n- things",
      "assets": {
        "linux-x86_64": "https://github.com/herdrdev/herdr/releases/download/v0.8.0/herdr-linux-x86_64",
        "linux-aarch64": "https://github.com/herdrdev/herdr/releases/download/v0.8.0/herdr-linux-aarch64",
        "macos-x86_64": "https://github.com/herdrdev/herdr/releases/download/v0.8.0/herdr-macos-x86_64",
        "macos-aarch64": "https://github.com/herdrdev/herdr/releases/download/v0.8.0/herdr-macos-aarch64"
      },
      "releases": {"0.8.0": {"notes": "..."}}
    }"####;

    /// The real shape of https://herdr.dev/preview.json.
    const PREVIEW: &str = r####"{
      "schema_version": 1,
      "channel": "preview",
      "base_version": "0.8.0",
      "build_id": "2026-08-04-d78e3d3b5126",
      "commit": "d78e3d3b51266a1ff80ae6858b894e782aac3e9f",
      "protocol": 19,
      "assets": {
        "macos-aarch64": {
          "url": "https://github.com/herdrdev/herdr/releases/download/preview-2026-08-04-d78e3d3b5126/herdr-macos-aarch64",
          "sha256": "d02190962293ccf19186e0ea8a1000000000000000000000000000000000000a"
        },
        "windows-x86_64": {
          "url": "https://github.com/herdrdev/herdr/releases/download/preview-2026-08-04-d78e3d3b5126/herdr-windows-x86_64.zip",
          "sha256": "b1d288118848ecd3ef33532a34506edc53a38a416057aee5b7fe1de4188a16fc",
          "format": "zip"
        }
      },
      "builds": {"2026-08-04-d78e3d3b5126": {"base_version": "0.8.0"}}
    }"####;

    fn target(os: Os, arch: Arch) -> Target {
        Target { os, arch }
    }

    #[test]
    fn parses_the_stable_manifest() {
        let m = Manifest::parse(STABLE).unwrap();
        assert_eq!(m.version_identity(Channel::Stable).unwrap(), "0.8.0");

        let asset = m.asset(target(Os::MacOs, Arch::Aarch64)).unwrap();
        assert!(asset.url.ends_with("/herdr-macos-aarch64"));
        assert_eq!(asset.format, Format::Binary);
        assert_eq!(asset.sha256, None, "stable manifests carry no digests");
    }

    #[test]
    fn parses_the_preview_manifest() {
        let m = Manifest::parse(PREVIEW).unwrap();
        assert_eq!(
            m.version_identity(Channel::Preview).unwrap(),
            "0.8.0-preview.2026-08-04-d78e3d3b5126"
        );

        let asset = m.asset(target(Os::MacOs, Arch::Aarch64)).unwrap();
        assert_eq!(asset.format, Format::Binary);
        assert!(asset.sha256.is_some());
    }

    /// The Windows asset is a zip carrying the ConPTY runtime alongside herdr.exe.
    #[test]
    fn windows_asset_is_a_zip_with_a_digest() {
        let m = Manifest::parse(PREVIEW).unwrap();
        let asset = m.asset(target(Os::Windows, Arch::X86_64)).unwrap();
        assert_eq!(asset.format, Format::Zip);
        assert_eq!(
            asset.sha256.as_deref(),
            Some("b1d288118848ecd3ef33532a34506edc53a38a416057aee5b7fe1de4188a16fc")
        );
    }

    /// Windows ARM64 runs the x86_64 build under emulation.
    #[test]
    fn windows_arm64_maps_to_the_x86_64_asset() {
        assert_eq!(target(Os::Windows, Arch::Aarch64).asset_key(), "windows-x86_64");
        assert_eq!(
            target(Os::Windows, Arch::Aarch64).triple(),
            "x86_64-pc-windows-msvc"
        );
    }

    /// Stable has no Windows build, so asking for one must say so clearly rather
    /// than panicking or installing the wrong thing.
    #[test]
    fn a_missing_target_lists_what_is_available() {
        let m = Manifest::parse(STABLE).unwrap();
        let err = m.asset(target(Os::Windows, Arch::X86_64)).unwrap_err().to_string();
        assert!(err.contains("no binary for windows-x86_64"), "{err}");
        assert!(err.contains("macos-aarch64"), "{err}");
    }

    #[test]
    fn windows_defaults_to_preview_and_others_to_stable() {
        assert_eq!(Channel::default_for(Os::Windows), Channel::Preview);
        assert_eq!(Channel::default_for(Os::MacOs), Channel::Stable);
        assert_eq!(Channel::default_for(Os::Linux), Channel::Stable);
    }

    #[test]
    fn channel_urls_are_the_ones_herdr_update_uses() {
        assert_eq!(Channel::Stable.manifest_url(), "https://herdr.dev/latest.json");
        assert_eq!(Channel::Preview.manifest_url(), "https://herdr.dev/preview.json");
    }

    #[test]
    fn format_falls_back_to_the_url_extension() {
        let m = Manifest::parse(
            r#"{"version":"1.0","assets":{"macos-aarch64":"https://x/herdr.zip"}}"#,
        )
        .unwrap();
        assert_eq!(
            m.asset(target(Os::MacOs, Arch::Aarch64)).unwrap().format,
            Format::Zip
        );
    }

    /// Real version identities must survive untouched, and anything that could
    /// act as a path separator must not.
    #[test]
    fn safe_version_keeps_real_versions_and_removes_separators() {
        assert_eq!(safe_version("0.8.0"), "0.8.0");
        assert_eq!(
            safe_version("0.8.0-preview.2026-08-04-d78e3d3b5126"),
            "0.8.0-preview.2026-08-04-d78e3d3b5126"
        );

        // Dots are legal in a version, so they are kept - as the official
        // installer's [^0-9A-Za-z._-] rule also does. What must go is every
        // separator, which is what stops the result being more than one path
        // component.
        for hostile in ["0.8.0/../evil", r"0.8.0\..\evil", "a:b", "a b", "a\0b"] {
            let safe = safe_version(hostile);
            assert!(!safe.contains('/'), "{safe}");
            assert!(!safe.contains('\\'), "{safe}");
            assert!(!safe.contains(':'), "{safe}");
            assert_eq!(
                std::path::Path::new(&safe).components().count(),
                1,
                "{safe} must be exactly one path component"
            );
        }
    }

    #[test]
    fn the_running_platform_is_supported() {
        let t = Target::detect().expect("this platform should be supported");
        assert!(!t.asset_key().is_empty());
        assert!(!t.describe().is_empty());
    }
}
