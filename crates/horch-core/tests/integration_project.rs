use horch_core::fleetd::integration::adapters::project_file::{self, FolderReader};
use horch_core::fleetd::integration::domain::project::{Pr, ProjectFileReader, Remote};

fn project(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("fleet.toml"), text).unwrap();
    dir
}

#[test]
fn gates_parse_fast_and_full() {
    let dir = project("[gates]\nfast = \"cargo test --lib\"\nfull = \"cargo test\"\n");
    let s = project_file::load(&FolderReader::new(dir.path()), "HEAD").unwrap();
    assert_eq!(s.fast_gate(), Some("cargo test --lib"));
    assert_eq!(s.full_gate(), Some("cargo test"));
}

#[test]
fn missing_fast_gate_is_none() {
    let s = project_file::parse("[gates]\nfull = \"cargo test\"\n").unwrap();
    assert_eq!(s.fast_gate(), None);
    assert_eq!(s.full_gate(), Some("cargo test"));
    let dir = tempfile::tempdir().unwrap();
    let s = project_file::load(&FolderReader::new(dir.path()), "HEAD").unwrap();
    assert_eq!(s.fast_gate(), None);
}

#[test]
fn unknown_gate_key_is_refused() {
    let e = project_file::parse("[gates]\nfast = \"a\"\nslow = \"b\"\n").unwrap_err();
    assert!(format!("{e:#}").contains("slow"), "{e:#}");
    let e = project_file::parse("[gates]\nfast = \"\"\n").unwrap_err();
    assert!(format!("{e:#}").contains("gates.fast"), "{e:#}");
}

#[test]
fn pr_and_remote_defaults() {
    let s = project_file::parse("[gates]\nfast = \"a\"\nfull = \"b\"\n").unwrap();
    assert_eq!(s.pr, Pr::Gh);
    assert_eq!(s.remote, Remote::Named("origin".into()));
    let s = project_file::parse("pr = \"manual\"\nremote = \"up\"\n").unwrap();
    assert_eq!(s.pr, Pr::Manual);
    assert_eq!(s.remote, Remote::Named("up".into()));
    let e = project_file::parse("pr = \"gitlab\"\n").unwrap_err();
    assert!(format!("{e:#}").contains("pr"), "{e:#}");
}

#[test]
fn remote_none_is_read() {
    let s = project_file::parse("remote = \"none\"\n[gates]\nfast = \"a\"\n").unwrap();
    assert_eq!(s.remote, Remote::None);
}

#[test]
fn the_folder_reader_gives_none_with_no_file() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(FolderReader::new(dir.path()).read("HEAD").unwrap(), None);
}
