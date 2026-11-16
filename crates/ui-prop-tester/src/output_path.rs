use std::path::Path;

pub fn check_output_path(path: &Path, overwrite: bool) -> Result<(), String> {
    let parent = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    if !parent.is_dir() {
        return Err(format!(
            "the folder {} for --output-path does not exist",
            parent.display()
        ));
    }
    if path.symlink_metadata().is_ok() && !overwrite {
        return Err(format!(
            "{} already exists; pass --output-path-overwrite to replace it",
            path.display()
        ));
    }
    Ok(())
}

pub fn check_distinct_from_input(output: &Path, input: &Path) -> Result<(), String> {
    if same_file(output, input) {
        return Err(format!(
            "--output-path {} is the same file as the input trace {}",
            output.display(),
            input.display()
        ));
    }
    Ok(())
}

fn same_file(left: &Path, right: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (left.canonicalize(), right.canonicalize()) {
        if a == b {
            return true;
        }
    }
    same_inode(left, right)
}

#[cfg(unix)]
fn same_inode(left: &Path, right: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(left), std::fs::metadata(right)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_inode(_left: &Path, _right: &Path) -> bool {
    false
}

pub fn numbered_sibling(path: &Path, number: u64) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match path.extension() {
        Some(extension) => format!("{stem}-{number}.{}", extension.to_string_lossy()),
        None => format!("{stem}-{number}"),
    };
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_parent_folder_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let error = check_output_path(&dir.path().join("absent/out.trace"), false).unwrap_err();
        assert!(error.contains("does not exist"), "{error}");
    }

    #[test]
    fn an_existing_file_needs_the_overwrite_flag() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.trace");
        assert!(check_output_path(&path, false).is_ok());
        std::fs::write(&path, b"x").unwrap();
        assert!(check_output_path(&path, false).is_err());
        assert!(check_output_path(&path, true).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn the_input_is_recognised_directly_by_symlink_and_by_hard_link() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in.trace");
        std::fs::write(&input, b"x").unwrap();
        let symlink = dir.path().join("sym.trace");
        std::os::unix::fs::symlink(&input, &symlink).unwrap();
        let hard = dir.path().join("hard.trace");
        std::fs::hard_link(&input, &hard).unwrap();
        let other = dir.path().join("other.trace");
        assert!(check_distinct_from_input(&input, &input).is_err());
        assert!(check_distinct_from_input(&symlink, &input).is_err());
        assert!(check_distinct_from_input(&hard, &input).is_err());
        assert!(check_distinct_from_input(&other, &input).is_ok());
    }

    #[test]
    fn numbered_siblings_keep_the_extension() {
        assert_eq!(
            numbered_sibling(Path::new("/tmp/run.trace"), 3),
            Path::new("/tmp/run-3.trace")
        );
        assert_eq!(numbered_sibling(Path::new("run"), 1), Path::new("run-1"));
    }
}
