//! The parts of the `horch` binary that every entry point shares: reading the
//! process environment once ([`bootstrap`]), writing to stdout ([`output`]),
//! and the exit codes ([`exit`]).

#![warn(unreachable_pub)]

pub mod bootstrap;
pub mod exit;
pub mod output;
pub mod project;

/// The binary that `horch install` puts next to `horch`.
pub const HERDR_INSTALL_BIN: &str = "herdr-install";

/// The binaries a release archive holds, `horch install` puts on PATH and
/// `horch self-update` replaces, in that order.
pub const RELEASE_BINARIES: [&str; 2] = ["horch", HERDR_INSTALL_BIN];

#[cfg(test)]
mod tests {
    use super::RELEASE_BINARIES;

    fn repo_file(rel: &str) -> String {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    /// The line of `text` that contains `marker`; exactly 1 must.
    fn line_with<'a>(text: &'a str, marker: &str) -> &'a str {
        let lines: Vec<&str> = text.lines().filter(|l| l.contains(marker)).collect();
        assert_eq!(lines.len(), 1, "{marker}: {lines:?}");
        lines[0]
    }

    /// TRF-47: the release build and both archives name only horch and
    /// herdr-install.
    #[test]
    fn trf_47_release_assets_hold_only_horch_and_herdr_install() {
        let release = repo_file(".github/workflows/release.yml");
        let build = line_with(&release, "cargo build --release --locked");
        assert!(
            build.ends_with("--bin horch --bin herdr-install"),
            "{build}"
        );
        let tar = line_with(&release, "tar -czf");
        assert!(tar.ends_with("/release\" horch herdr-install"), "{tar}");
        let zip = line_with(&release, "Compress-Archive");
        assert!(
            zip.contains("-Path \"$dir/horch.exe\", \"$dir/herdr-install.exe\" -DestinationPath"),
            "{zip}"
        );
        assert_eq!(RELEASE_BINARIES, ["horch", "herdr-install"]);
    }

    /// TRF-47: `just install` builds only the binaries `horch install` takes.
    #[test]
    fn trf_47_just_install_builds_only_horch_and_herdr_install() {
        let justfile = repo_file("justfile");
        let build = line_with(&justfile, "cargo build --release --bin");
        assert_eq!(
            build.trim(),
            "cargo build --release --bin horch --bin herdr-install"
        );
    }

    /// TRF-47: no file the build ships names the removed binary.
    #[test]
    fn trf_47_no_shipped_file_names_multi_herdr_dataset() {
        for rel in [
            ".github/workflows/release.yml",
            "justfile",
            "crates/horch/Cargo.toml",
            "crates/horch/src/cmd/install.rs",
            "crates/horch/src/cmd/selfupdate.rs",
        ] {
            let text = repo_file(rel);
            let hits: Vec<&str> = text
                .lines()
                .filter(|l| l.contains("multi-herdr-dataset"))
                .collect();
            assert!(hits.is_empty(), "{rel}: {hits:?}");
        }
    }
}
