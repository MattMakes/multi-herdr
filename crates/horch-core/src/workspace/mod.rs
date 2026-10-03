//! Workspace boundary: the herdr client and the pure tiler modules.

pub mod balance;
pub mod client;
pub mod herdr;
pub mod layout;
pub mod model;
pub mod paneshell;
pub mod testing;
pub mod tile;

#[cfg(test)]
mod tests {
    const FORBIDDEN: [&str; 7] = [
        "std::process",
        "Command::new",
        "Herdr",
        "WorkspaceClient",
        "crate::workspace::herdr",
        "crate::herdr",
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
        let comment = "    /// [`crate::herdr::Herdr::pane_focus_walk`] steers by.";
        assert!(violations(comment).is_empty());
        assert_eq!(violations("use crate::herdr::Herdr;").len(), 1);
        assert_eq!(violations("let c = Command::new(\"x\");").len(), 1);
    }
}
