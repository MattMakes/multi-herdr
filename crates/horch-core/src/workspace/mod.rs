//! Workspace boundary: the herdr client and the pure tiler modules.

pub mod arrange;
pub mod balance;
pub mod client;
pub(crate) mod exec;
pub mod hc;
pub mod herdr;
pub mod host;
pub mod layout;
pub mod model;
pub mod paneshell;
pub mod place;
pub mod testing;
pub mod tile;

pub use hc::HcFailed;
pub use herdr::HerdrFailed;
pub use host::{Caps, PaneHost, Unsupported};

#[cfg(test)]
mod tests {
    const FORBIDDEN: [&str; 10] = [
        "std::process",
        "Command::new",
        "Herdr",
        "HcApp",
        "PaneHost",
        "WorkspaceClient",
        "crate::workspace::herdr",
        "crate::workspace::hc",
        "crate::workspace::host",
        "std::env",
    ];

    /// Lines of `source` that name a forbidden item. A comment line is not a
    /// dependency, so lines whose trimmed text starts with `//` are skipped.
    fn violations(source: &str) -> Vec<String> {
        source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .filter(|line| FORBIDDEN.iter().any(|f| line.contains(f)))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn arc_27_tile_balance_pure() {
        let sources = [
            ("layout.rs", include_str!("layout.rs")),
            ("tile.rs", include_str!("tile.rs")),
            ("balance.rs", include_str!("balance.rs")),
            ("place.rs", include_str!("place.rs")),
        ];
        for (name, source) in sources {
            assert_eq!(
                violations(source),
                Vec::<String>::new(),
                "{name} is not pure"
            );
        }
    }

    #[test]
    fn arc_27_scan_ignores_comment_lines_and_catches_code() {
        let comment = "    /// [`crate::workspace::herdr::Herdr::pane_focus_walk`] steers by.";
        assert!(violations(comment).is_empty());
        assert_eq!(violations("use crate::workspace::herdr::Herdr;").len(), 1);
        assert_eq!(violations("let c = Command::new(\"x\");").len(), 1);
    }
}
