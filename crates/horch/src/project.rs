//! What the CLI reads from the project directory for the roster.

use std::path::Path;

use horch_core::roster::ProjectFacts;

/// The entry names at the top of `dir` and one level down, for `offer_when`.
///
/// One level covers the usual layouts (`ios/App.xcodeproj`,
/// `game/Game.uproject`) at the cost of one `read_dir` per subdirectory.
/// Dot-directories (`.git`, `.worktrees`) are not entered: what they hold is
/// never the project's own layout. An unreadable directory adds nothing.
pub fn project_facts(dir: &Path) -> ProjectFacts {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let descend = !name.starts_with('.') && entry.file_type().is_ok_and(|t| t.is_dir());
        if descend {
            for inner in std::fs::read_dir(entry.path())
                .into_iter()
                .flatten()
                .flatten()
            {
                names.push(inner.file_name().to_string_lossy().into_owned());
            }
        }
        names.push(name);
    }
    ProjectFacts::from_names(names)
}
