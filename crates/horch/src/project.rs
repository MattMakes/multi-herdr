use std::path::Path;

use horch_core::roster::ProjectFacts;

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
