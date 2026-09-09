//! `herdr-install` - installs the latest Herdr CLI.
//!
//! The Rust port of https://herdr.dev/install.sh and https://herdr.dev/install.ps1,
//! as one binary that needs no shell, no curl, and no PowerShell. It reads the same
//! release manifests as `herdr update`, so installs and updates agree on what
//! "latest" means.
//!
//! Platform differences, all of them deliberate:
//!   * macOS and Linux get one binary at `~/.local/bin/herdr` from the stable
//!     channel.
//!   * Windows gets the preview channel - the only one that publishes a Windows
//!     asset while native Windows support is in beta - unpacked with its ConPTY
//!     runtime into a versioned directory, with `current` and the PATH directory
//!     pointed at it by junction so an update never overwrites a running
//!     `herdr.exe`.

mod manifest;
mod net;
mod place;
mod win;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use clap::Parser;

use manifest::{Channel, Format, Manifest, Os, Target};

#[derive(Parser)]
#[command(
    name = "herdr-install",
    version,
    about = "Install the latest Herdr CLI",
    long_about = "Install the latest Herdr CLI (https://herdr.dev).\n\n\
                  Downloads the right release for this platform and puts it on your PATH.\n\
                  Re-run at any time to update."
)]
struct Cli {
    /// Release channel. Defaults to stable, or preview on Windows.
    #[arg(long)]
    channel: Option<Channel>,

    /// Where to install. Defaults to ~/.local/bin, or
    /// %LOCALAPPDATA%\Programs\Herdr\bin on Windows.
    #[arg(long, value_name = "DIR", env = "HERDR_INSTALL_DIR")]
    install_dir: Option<PathBuf>,

    /// Fetch the manifest from somewhere else. For testing.
    #[arg(long, value_name = "URL", env = "HERDR_MANIFEST_URL")]
    manifest_url: Option<String>,

    /// How many previous Windows releases to keep on disk.
    #[arg(long, default_value_t = 2)]
    retain: usize,

    /// Report what would happen without writing anything.
    #[arg(long)]
    dry_run: bool,
}

const BANNER: &str = r"
      ,ww
     wWWWWWWW_)  herdr installer
     `WWWWWW'    herdr.dev
      II  II
";

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("\n  \x1b[31mx\x1b[0m {e:#}\n");
            std::process::ExitCode::FAILURE
        }
    }
}

fn step(message: &str) {
    println!("  \x1b[32m>\x1b[0m {message}");
}

fn warn(message: &str) {
    println!("  \x1b[33m!\x1b[0m {message}");
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    println!("{BANNER}");

    let target = Target::detect()?;
    let channel = cli.channel.unwrap_or_else(|| Channel::default_for(target.os));
    if target.os == Os::Windows && channel == Channel::Stable {
        bail!(
            "Windows builds are preview-only for now. Omit --channel, or pass \
             --channel preview."
        );
    }
    step(&format!("detected {} ({} channel)", target.describe(), channel.as_str()));

    let url = cli
        .manifest_url
        .clone()
        .unwrap_or_else(|| channel.manifest_url().to_string());
    step("fetching the latest release manifest...");
    let manifest = Manifest::parse(&net::fetch_text(&url)?)
        .with_context(|| format!("parsing the release manifest from {url}"))?;
    let version = manifest.version_identity(channel)?;
    let asset = manifest.asset(target)?;

    if cli.dry_run {
        step(&format!("would install herdr {version} from {}", asset.url));
        step(&format!(
            "would install to {}",
            describe_destination(target, cli.install_dir.as_deref(), &version)?.display()
        ));
        return Ok(());
    }

    step(&format!("downloading herdr {version}..."));
    let bytes = net::download(&asset.url)?;
    match &asset.sha256 {
        Some(expected) => {
            net::verify_digest(&bytes, expected)?;
            step("checksum verified");
        }
        // Stable manifests publish no digests; TLS is the integrity guarantee there.
        None => step("no checksum published for this channel; skipping verification"),
    }

    let installed = match target.os {
        Os::Windows => install_windows(&bytes, &asset.format, cli.install_dir.as_deref(), &version, target, cli.retain)?,
        Os::MacOs | Os::Linux => install_unix(&bytes, cli.install_dir.as_deref())?,
    };

    step(&format!("installed herdr to {}", installed.display()));
    verify_installed(&installed)?;
    report_path(installed.parent().unwrap_or(&installed))?;
    Ok(())
}

/// Where the binary will end up, for `--dry-run`.
fn describe_destination(
    target: Target,
    install_dir: Option<&Path>,
    version: &str,
) -> Result<PathBuf> {
    Ok(match target.os {
        Os::Windows => {
            win_layout(install_dir, version, target)?
                .visible_bin
                .join(place::exe_name())
        }
        Os::MacOs | Os::Linux => {
            place::unix_install_dir(&home_dir()?, install_dir).join(place::exe_name())
        }
    })
}

fn install_unix(bytes: &[u8], install_dir: Option<&Path>) -> Result<PathBuf> {
    let dir = place::unix_install_dir(&home_dir()?, install_dir);
    let target = dir.join(place::exe_name());
    place::place_binary(bytes, &target)?;
    Ok(target)
}

fn win_layout(
    install_dir: Option<&Path>,
    version: &str,
    target: Target,
) -> Result<place::WindowsLayout> {
    let herdr_home = match std::env::var_os("HERDR_HOME").filter(|v| !v.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home_dir()?.join(".herdr"),
    };
    let local_app_data = std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    Ok(place::windows_layout(
        &herdr_home,
        local_app_data.as_deref(),
        install_dir,
        version,
        target.triple(),
    ))
}

/// Unpack into a versioned directory, then retarget the junctions at it.
fn install_windows(
    bytes: &[u8],
    format: &Format,
    install_dir: Option<&Path>,
    version: &str,
    target: Target,
    retain: usize,
) -> Result<PathBuf> {
    let layout = win_layout(install_dir, version, target)?;
    std::fs::create_dir_all(&layout.releases_dir)
        .with_context(|| format!("creating {}", layout.releases_dir.display()))?;

    let staging = layout.releases_dir.join(format!(
        ".staging.{}.{}",
        layout
            .release_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)?;

    match format {
        // The zip carries herdr.exe plus the app-local ConPTY runtime it needs.
        Format::Zip => place::extract_zip(bytes, &staging)?,
        Format::Binary => place::place_binary(bytes, &staging.join(place::exe_name()))?,
    }
    let staged_exe = staging.join(place::exe_name());
    if !staged_exe.is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        bail!(
            "the downloaded package does not contain {}; refusing to install",
            place::exe_name()
        );
    }

    place::promote_release(&staging, &layout.release_dir)?;
    step(&format!("unpacked to {}", layout.release_dir.display()));

    win::set_junction(&layout.current_dir, &layout.release_dir)?;
    win::set_junction(&layout.visible_bin, &layout.release_dir)?;
    place::prune_old_releases(&layout.releases_dir, &layout.release_dir, retain);

    Ok(layout.visible_bin.join(place::exe_name()))
}

/// Run the freshly installed binary. A release that cannot report its own version
/// is broken, and it is better to say so now than at first use.
fn verify_installed(path: &Path) -> Result<()> {
    match Command::new(path).arg("--version").output() {
        Ok(out) if out.status.success() => {
            let reported = String::from_utf8_lossy(&out.stdout).trim().to_string();
            step(&format!("verified: {reported}"));
            Ok(())
        }
        Ok(out) => bail!(
            "{} --version failed ({}): {}",
            path.display(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => Err(e).with_context(|| format!("running {} --version", path.display())),
    }
}

/// Make sure the install directory is reachable, and say what to do if not.
fn report_path(dir: &Path) -> Result<()> {
    if cfg!(windows) {
        match win::ensure_user_path(dir) {
            Ok(true) => {
                step("PATH updated for future terminals.");
                println!("\n  Open a new terminal, then run: herdr");
                return Ok(());
            }
            Ok(false) => {
                step(&format!("{} is already first on PATH.", dir.display()));
                println!("\n  Ready. Run: herdr");
                return Ok(());
            }
            Err(e) => {
                warn(&format!("could not update PATH automatically: {e:#}"));
                println!("\n  Add it yourself, in PowerShell:");
                println!("    $env:Path = \"{};$env:Path\"", dir.display());
                return Ok(());
            }
        }
    }

    if on_path(dir) {
        println!("\n  Ready. Run: herdr");
        return Ok(());
    }
    println!();
    warn(&format!("{} is not in your PATH", dir.display()));
    println!("  add it to your shell config:\n");
    println!("    export PATH=\"{}:$PATH\"", dir.display());
    println!();
    Ok(())
}

fn on_path(dir: &Path) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    let target = dir.to_string_lossy().trim_end_matches('/').to_string();
    std::env::split_paths(&paths)
        .any(|p| p.to_string_lossy().trim_end_matches('/') == target)
}

fn home_dir() -> Result<PathBuf> {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .with_context(|| format!("{key} is not set, so the install directory cannot be resolved"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn channel_can_be_overridden_on_the_command_line() {
        let cli = Cli::try_parse_from(["herdr-install", "--channel", "preview"]).unwrap();
        assert_eq!(cli.channel, Some(Channel::Preview));
        assert!(Cli::try_parse_from(["herdr-install", "--channel", "nightly"]).is_err());
    }

    #[test]
    fn defaults_are_sensible() {
        let cli = Cli::try_parse_from(["herdr-install"]).unwrap();
        assert_eq!(cli.channel, None, "channel is chosen per platform");
        assert_eq!(cli.retain, 2);
        assert!(!cli.dry_run);
    }
}
