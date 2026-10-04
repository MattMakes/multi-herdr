//! ARC-08: the roster split keeps strict serde, the overlay order and every
//! frontmatter shape the fleet has shipped.

use std::path::{Path, PathBuf};

use super::parser::{md_files, parse_base, parse_teammate};
use super::Roster;

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn repo_teammates() -> PathBuf {
    manifest_dir().join("../../teammates")
}

/// The built-in `opus.md`, with its `brief_description` replaced.
fn opus_saying(brief: &str) -> String {
    let text = std::fs::read_to_string(repo_teammates().join("opus.md")).unwrap();
    text.lines()
        .map(|line| {
            if line.starts_with("brief_description:") {
                format!("brief_description: {brief}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn write_opus(dir: &Path, brief: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("opus.md"), opus_saying(brief)).unwrap();
}

#[test]
fn arc_08_unknown_field_rejected() {
    let text = "---\nname: x\nbrief_description: X\nagent: claude\nmodel: opus\n\
                permision_mode: auto\n---\nbody";
    let err = parse_teammate("x", text).unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("permision_mode"), "{message}");
    assert!(message.contains("unknown field"), "{message}");
}

#[test]
fn arc_08_overlay_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let roster_override = tmp.path().join("override");
    let explicit = tmp.path().join("explicit");
    write_opus(
        &home.join(".config/horch/teammates"),
        "from the user overlay",
    );
    write_opus(&roster_override, "from the roster override");
    write_opus(&explicit, "from the explicit dir");
    let explicit = explicit.to_str().unwrap();

    let brief = |r: Roster| r.require("opus").unwrap().brief_description.clone();
    let builtin = brief(Roster::builtin().unwrap());
    assert!(builtin.starts_with("Generic worker"), "{builtin}");

    let all = Roster::load_layered(Some(&home), Some(&roster_override), Some(explicit)).unwrap();
    assert_eq!(brief(all), "from the explicit dir");
    let no_explicit = Roster::load_layered(Some(&home), Some(&roster_override), None).unwrap();
    assert_eq!(brief(no_explicit), "from the roster override");
    let user = Roster::load_layered(Some(&home), None, None).unwrap();
    assert_eq!(user.sources, vec![home.join(".config/horch/teammates")]);
    assert_eq!(brief(user), "from the user overlay");
    let none = Roster::load_layered(Some(&tmp.path().join("empty-home")), None, None).unwrap();
    assert!(none.sources.is_empty());
    assert_eq!(brief(none), builtin);
}

#[test]
fn arc_08_legacy_frontmatter_corpus_parses() {
    let dir = repo_teammates();
    let mut parsed = 0;
    for (stem, path) in md_files(&dir).unwrap() {
        if stem.starts_with('_') {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        parse_teammate(&stem, &text).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        parsed += 1;
    }
    assert!(parsed >= 33, "only {parsed} teammates parsed");
    for (stem, path) in md_files(&dir.join("_base")).unwrap() {
        let text = std::fs::read_to_string(&path).unwrap();
        parse_base(&stem, &text).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
    }

    // Before the usage-pool fallbacks (c3e673d~1).
    let fixtures = manifest_dir().join("tests/fixtures/roster");
    let text = std::fs::read_to_string(fixtures.join("pre-fallbacks/opus.md")).unwrap();
    let opus = parse_teammate("opus", &text).unwrap();
    assert!(opus.fallbacks.is_empty());
    assert_eq!(opus.effort.as_deref(), Some("medium"));

    // Before codex teammates stated an effort (a1e6798).
    let text = std::fs::read_to_string(fixtures.join("pre-effort/codex-sol.md")).unwrap();
    let sol = parse_teammate("codex-sol", &text).unwrap();
    assert_eq!(sol.effort, None);
    assert_eq!(sol.phase, None);
    assert!(sol.fallbacks.is_empty());
}

/// The roster reads files and nothing else: it never talks to the terminal
/// server, starts a process or reads the environment. This test file is
/// skipped: it is test code, and the test's own name holds a needle.
#[test]
fn arc_08_roster_never_calls_herdr() {
    let needles = ["herdr", "Herdr", "std::process", "std::env::var"];
    let dir = manifest_dir().join("src/roster");
    let mut scanned = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs")
            || path.file_name() == Some("tests.rs".as_ref())
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for needle in &needles {
            assert!(
                !text.contains(needle),
                "{} contains '{needle}'",
                path.display()
            );
        }
        scanned += 1;
    }
    assert!(scanned >= 9, "only {scanned} files scanned");
}

/// F7: a teammate file whose `requires` names a value this binary does not
/// know, as when the live roster is ahead of the installed `horch`.
fn write_ahead(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let text = opus_saying("ahead of the binary")
        .replacen("\n---\n", "\nrequires: [no-such-tool]\n---\n", 1)
        .replace("name: opus", &format!("name: {name}"));
    let path = dir.join(format!("{name}.md"));
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn f7_builtin_roster_always_parses() {
    let r = Roster::builtin().expect("every compiled-in teammate and base parses");
    assert!(r.load_warnings().is_empty());
    assert!(r.names().len() >= 33, "{:?}", r.names());
}

#[test]
fn f7_bad_overlay_file_skips_only_that_teammate() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("teammates");
    let path = write_ahead(&dir, "newcomer");
    write_opus(&dir, "overlaid opus");

    let r = Roster::load_layered(None, Some(&dir), None).unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        "overlaid opus"
    );
    assert_eq!(r.sources, vec![dir.clone()]);
    assert!(!r.names().contains(&"newcomer"));

    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let w = &warnings[0];
    assert!(w.contains(&path.display().to_string()), "{w}");
    assert!(w.contains("unknown variant `no-such-tool`"), "{w}");
    assert!(w.contains("teammate 'newcomer' is not available"), "{w}");

    // Spawn by name: the parse error, not "unknown teammate".
    let err = format!("{:#}", r.require("newcomer").unwrap_err());
    assert!(!err.contains("unknown teammate"), "{err}");
    assert!(err.contains("unknown variant `no-such-tool`"), "{err}");
    assert!(err.contains(&path.display().to_string()), "{err}");

    // The gate stays strict.
    assert!(r.check().contains(w), "{:?}", r.check());
}

#[test]
fn f7_bad_override_of_builtin_falls_back_to_builtin() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("teammates");
    write_ahead(&dir, "opus");

    let r = Roster::load_layered(None, Some(&dir), None).unwrap();
    let builtin = Roster::builtin().unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        builtin.require("opus").unwrap().brief_description
    );
    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("using the built-in 'opus' instead"),
        "{warnings:?}"
    );
    assert!(r.check().contains(&warnings[0]), "{:?}", r.check());
}

#[test]
fn f7_bad_layer_keeps_the_earlier_layer_and_a_later_layer_heals() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let user = home.join(".config/horch/teammates");
    let roster_override = tmp.path().join("override");
    let explicit = tmp.path().join("explicit");
    write_opus(&user, "from the user overlay");
    write_ahead(&roster_override, "opus");

    let r = Roster::load_layered(Some(&home), Some(&roster_override), None).unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        "from the user overlay"
    );
    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let earlier = format!(
        "using the earlier {} instead",
        user.join("opus.md").display()
    );
    assert!(warnings[0].contains(&earlier), "{warnings:?}");

    write_opus(&explicit, "from the explicit dir");
    let r = Roster::load_layered(
        Some(&home),
        Some(&roster_override),
        Some(explicit.to_str().unwrap()),
    )
    .unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        "from the explicit dir"
    );
    assert!(r.load_warnings().is_empty(), "{:?}", r.load_warnings());
}

#[test]
fn f7_bad_base_file_keeps_the_builtin_base() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("teammates");
    std::fs::create_dir_all(dir.join("_base")).unwrap();
    let path = dir.join("_base/fleet-worker.md");
    std::fs::write(&path, "no frontmatter here\n").unwrap();

    let r = Roster::load_layered(None, Some(&dir), None).unwrap();
    let builtin = Roster::builtin().unwrap();
    assert_eq!(
        r.base("fleet-worker").unwrap().body,
        builtin.base("fleet-worker").unwrap().body
    );
    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("base 'fleet-worker'"), "{warnings:?}");
    assert!(
        warnings[0].contains(&path.display().to_string()),
        "{warnings:?}"
    );
    assert!(r.check().contains(&warnings[0]), "{:?}", r.check());
}
