#![warn(unreachable_pub)]

pub mod bootstrap;
pub mod exit;
pub mod output;
pub mod project;

pub const HERDR_INSTALL_BIN: &str = "herdr-install";

pub const RELEASE_BINARIES: [&str; 2] = ["horch", HERDR_INSTALL_BIN];

pub const RETIRED_BINARIES: [&str; 1] = ["multi-herdr-dataset"];

pub fn remove_retired_binaries(dir: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    use anyhow::Context as _;
    let mut removed = Vec::new();
    for bin in RETIRED_BINARIES {
        let path = dir.join(format!("{bin}{}", std::env::consts::EXE_SUFFIX));
        if std::fs::symlink_metadata(&path).is_ok() {
            std::fs::remove_file(&path)
                .with_context(|| format!("removing the retired {}", path.display()))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::{remove_retired_binaries, RELEASE_BINARIES, RETIRED_BINARIES};

    #[test]
    fn trf_47_install_removes_old_dataset_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = |name: &str| {
            tmp.path()
                .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
        };
        std::fs::write(exe(RETIRED_BINARIES[0]), "old").unwrap();
        std::fs::write(exe("horch"), "horch").unwrap();

        let removed = remove_retired_binaries(tmp.path()).unwrap();

        assert_eq!(removed, vec![exe(RETIRED_BINARIES[0])]);
        assert!(!exe(RETIRED_BINARIES[0]).exists());
        assert_eq!(std::fs::read_to_string(exe("horch")).unwrap(), "horch");
        assert!(remove_retired_binaries(tmp.path()).unwrap().is_empty());
    }

    fn repo_file(rel: &str) -> String {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    fn line_with<'a>(text: &'a str, marker: &str) -> &'a str {
        let lines: Vec<&str> = text.lines().filter(|l| l.contains(marker)).collect();
        assert_eq!(lines.len(), 1, "{marker}: {lines:?}");
        lines[0]
    }

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

    #[test]
    fn trf_47_just_install_builds_only_horch_and_herdr_install() {
        let justfile = repo_file("justfile");
        let build = line_with(&justfile, "cargo build --release --bin");
        assert_eq!(
            build.trim(),
            "cargo build --release --bin horch --bin herdr-install"
        );
    }

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
