use std::collections::BTreeMap;
use std::path::Path;

use horch_core::taskledger::finish::complexity::{
    gdscript_runner, in_changed_functions, no_gdscript_analyser, write_gdcomplexity, GdRoute,
    GDSCRIPT_INSTALL_HINT, NOT_APPLICABLE,
};

#[path = "../common/warm.rs"]
mod warm;

fn tangled(name: &str) -> String {
    let mut body = format!("func {name}(v):\n");
    for i in 0..10 {
        body.push_str(&format!("\tif v == {i}: pass\n"));
    }
    body
}

fn source() -> String {
    format!(
        "extends Node\n\n{}\n{}\nvar after = 1\n",
        tangled("old_tangled"),
        tangled("new_tangled")
    )
}

#[test]
fn grs_03_gdcomplexity_reports_exact_lines_and_metrics_for_changed_functions() {
    let top = tempfile::tempdir().unwrap();
    let dir = top.path().join("game");
    let evidence = top.path().join("evidence");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&evidence).unwrap();
    std::fs::write(dir.join("player.gd"), source()).unwrap();
    let cli = write_gdcomplexity(top.path()).unwrap();
    assert_eq!(cli, top.path().join("bin/gdcomplexity.py"));
    let runner = gdscript_runner(Path::new("uv"), Path::new("python3"))
        .unwrap_or_else(|| panic!("no gdscript analyser: {GDSCRIPT_INSTALL_HINT}"));
    let route = GdRoute {
        runner: &runner,
        cli: &cli,
        dir: &dir,
    };
    let reports = route
        .reports(&["player.gd".to_string()], &evidence, 7)
        .unwrap()
        .expect("gdcomplexity runs");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].0, evidence.join("7-gdcomplexity.json"));
    let json = &reports[0].1;
    let judged = |lines: (u64, u64)| -> Vec<(String, u64, String, f64, f64)> {
        let changed = BTreeMap::from([("player.gd".to_string(), vec![lines])]);
        in_changed_functions(json, &changed, &dir)
            .unwrap()
            .into_iter()
            .map(|v| (v.function_name, v.line, v.metric, v.value, v.threshold))
            .collect()
    };
    assert_eq!(
        judged((20, 20)),
        vec![(
            "new_tangled".to_string(),
            15,
            "cyclomaticComplexity".to_string(),
            11.0,
            10.0
        )],
        "{json}"
    );
    assert_eq!(judged((4, 4)).len(), 1, "{json}");
    assert_eq!(judged((4, 4))[0].1, 3, "{json}");
    assert_eq!(
        judged((27, 27)),
        Vec::new(),
        "a change after the last function is outside it: {json}"
    );
}

#[test]
fn grs_03_missing_gdscript_analyser_fails_with_install_hint() {
    let top = tempfile::tempdir().unwrap();
    let python = top.path().join("python3");
    let warmed = warm::write_warmed(&python, "#!/bin/sh\nexit 1\n", &["warm"]);
    assert_eq!(warmed.status.code(), Some(1));
    assert_eq!(
        gdscript_runner(&top.path().join("uv"), &python),
        None,
        "no uv and a python3 without gdtoolkit"
    );
    let failed = no_gdscript_analyser();
    assert_eq!(failed.verdict.as_deref(), Some("no gdscript analyser"));
    for hint in [
        "uv tool install gdtoolkit==4.5.0",
        "pipx install \"gdtoolkit==4.*\"",
    ] {
        assert!(failed.note.contains(hint), "{hint}: {}", failed.note);
    }
}

#[test]
fn grs_04_not_applicable_message_names_all_routed_languages_and_tools() {
    assert_eq!(
        NOT_APPLICABLE,
        "no changed file is in a language that the analyser (TypeScript, JavaScript, Python), clippy (Rust) or gdcomplexity (GDScript) reads"
    );
}

#[test]
fn grs_04_adr_0086_documents_rust_and_gdscript_routes() {
    let adr = Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../docs/adr/0086-complexity-sweep-sends-each-language-to-a-tool-that-parses-it.md",
    );
    let text = std::fs::read_to_string(&adr).unwrap();
    for needle in [
        "nearest `Cargo.toml`",
        "clippy::cognitive_complexity",
        "clippy::too_many_lines",
        "clippy::too_many_arguments",
        "scripts/godot/gdcomplexity.py",
        "gdtoolkit.parser",
        "HORCH_GDCOMPLEXITY_CLI",
        "no gdscript analyser",
    ] {
        assert!(text.contains(needle), "ADR 0086 lacks {needle}");
    }
}
