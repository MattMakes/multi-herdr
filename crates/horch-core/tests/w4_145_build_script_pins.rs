#[allow(dead_code)]
#[path = "../build.rs"]
mod build_script;

use std::path::Path;

use build_script::skill_digests;

fn hex(digest: &[u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn tree(root: &Path, files: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (relative, body) in files {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        out.push((relative.to_string(), path.to_string_lossy().into_owned()));
    }
    out.sort();
    out
}

const SKILLS: &[(&str, &str)] = &[
    ("alpha/SKILL.md", "alpha skill\n"),
    ("alpha/nested/SKILL.md", "not a skill of its own\n"),
    ("alpha/references/a.md", "a reference\n"),
    ("alpha/scripts/run.sh", "#!/bin/sh\necho run\n"),
    ("alpha-two/SKILL.md", "another skill\n"),
    ("beta/notes.md", "a folder with no skill file\n"),
    ("loose.md", "a file outside every skill\n"),
];

#[test]
fn each_top_level_skill_has_the_marketplace_digest_of_its_own_files() {
    let tmp = tempfile::tempdir().unwrap();
    let files = tree(tmp.path(), SKILLS);
    let digests = skill_digests(&files, &[]);

    let names: Vec<&str> = digests.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["alpha-two", "alpha"]);
    for (name, digest) in &digests {
        let marketplace =
            horch_marketplace::integrity::tree_digest(&tmp.path().join(name)).unwrap();
        assert_eq!(format!("sha256:{}", hex(digest)), marketplace, "{name}");
    }
}

#[test]
fn a_tree_without_a_top_level_skill_file_has_no_digest() {
    let tmp = tempfile::tempdir().unwrap();
    let files = tree(
        tmp.path(),
        &[
            ("loose.md", "x\n"),
            ("beta/notes.md", "y\n"),
            ("gamma/deep/SKILL.md", "z\n"),
            ("gamma/SKILL.md.bak", "w\n"),
        ],
    );
    assert!(skill_digests(&files, &[]).is_empty());
    assert!(skill_digests(&[], &[]).is_empty());
}

#[test]
fn the_execute_bit_is_part_of_the_digest_of_its_own_skill() {
    let tmp = tempfile::tempdir().unwrap();
    let files = tree(tmp.path(), SKILLS);
    let plain = skill_digests(&files, &[]);
    let exec = skill_digests(&files, &["alpha/scripts/run.sh"]);

    let digest_of = |all: &[(String, [u8; 32])], name: &str| {
        all.iter()
            .find(|(n, _)| n == name)
            .map(|(_, d)| *d)
            .unwrap()
    };
    assert_ne!(digest_of(&plain, "alpha"), digest_of(&exec, "alpha"));
    assert_eq!(
        digest_of(&plain, "alpha-two"),
        digest_of(&exec, "alpha-two")
    );
    assert_eq!(skill_digests(&files, &["scripts/run.sh"]), plain);
    assert_eq!(skill_digests(&files, &["beta/run.sh"]), plain);
    assert_eq!(
        hex(&digest_of(&exec, "alpha")),
        "1a8080dfc7398aef8ad56e7190258c498fe1ae2b2cebca3b341d912be0e8fc5b",
        "the digest of `alpha` with its script executable"
    );
}
