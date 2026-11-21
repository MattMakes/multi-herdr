use serde_json::json;

use super::*;

fn spec_in(dir: &Path, name: &str, source: &str) -> Specification {
    let path = dir.join(name);
    std::fs::write(&path, source).unwrap();
    Specification::from_file(&path).unwrap()
}

fn load(source: &str) -> SpecRuntime {
    let dir = tempfile::tempdir().unwrap();
    let spec = spec_in(dir.path(), "spec.js", source);
    SpecRuntime::load(DriverKind::Terminal, Some(&spec), 7).unwrap()
}

#[test]
fn exports_are_classified_by_runtime_tag() {
    let runtime = load(
        "import { always, extract, constant, action } from \"ui-prop-tester\";\n\
         export const count = extract((s) => s.count);\n\
         export const stays = always(() => count.current >= 0);\n\
         export const poke = action({ kind: \"step\", name: \"poke\" });\n\
         export const helper = 5;\n\
         export function ignored() {}\n",
    );
    assert_eq!(runtime.properties, vec!["stays"]);
    assert_eq!(runtime.categories, vec!["poke"]);
    assert_eq!(runtime.extractors.len(), 1);
}

#[test]
fn local_extraction_fills_exported_cells() {
    let mut runtime = load(
        "import { extract } from \"ui-prop-tester\";\n\
         export const count = extract((s) => s.count * 2);\n\
         const hidden = extract((s) => s.count);\n\
         export const firstCell = extract((s) => s.cell(0, 0).text);\n",
    );
    runtime
        .extract_local(
            &json!({"count": 4, "cells": [{"row": 0, "column": 0, "text": "x", "bold": true}]}),
        )
        .unwrap();
    assert_eq!(
        runtime.exported_cells().unwrap(),
        json!({"count": 8, "firstCell": "x"})
    );
}

#[test]
fn an_extractor_exception_keeps_its_javascript_stack() {
    let mut runtime = load(
        "import { extract } from \"ui-prop-tester\";\n\
         function explode(state) {\n  throw new Error(\"kaboom \" + state.n);\n}\n\
         export const bad = extract((s) => explode(s));\n",
    );
    let error = runtime.extract_local(&json!({"n": 3})).unwrap_err();
    assert!(error.contains("the extractor bad"), "{error}");
    assert!(error.contains("kaboom 3"), "{error}");
    assert!(error.contains("explode"), "{error}");
    assert!(error.contains("spec.js:3"), "{error}");
}

#[test]
fn reading_a_cell_inside_an_extractor_is_rejected() {
    let mut runtime = load(
        "import { extract } from \"ui-prop-tester\";\n\
         export const first = extract((s) => s.n);\n\
         export const second = extract((s) => first.current + 1);\n",
    );
    let error = runtime.extract_local(&json!({"n": 1})).unwrap_err();
    assert!(
        error.contains("an extractor cannot read another extractor's cell"),
        "{error}"
    );
    assert!(error.contains("the extractor second"), "{error}");
}

#[test]
fn typescript_specifications_load_after_stripping() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("helpers.ts"),
        "export interface Limits { max: number }\nexport function under(value: number, limits: Limits): boolean { return value < limits.max; }\n",
    )
    .unwrap();
    let spec = spec_in(
        dir.path(),
        "spec.ts",
        "import { always, extract } from \"ui-prop-tester\";\n\
         import type { Limits } from \"./helpers.ts\";\n\
         import { under } from \"./helpers.ts\";\n\
         const limits: Limits = { max: 10 };\n\
         export const n = extract((s: { n: number }): number => s.n as number);\n\
         export const small = always((): boolean => under(n.current!, limits));\n",
    );
    let mut runtime = SpecRuntime::load(DriverKind::Terminal, Some(&spec), 1).unwrap();
    runtime.extract_local(&json!({"n": 3})).unwrap();
    assert!(runtime.step(0, 0).unwrap().is_empty());
    runtime.extract_local(&json!({"n": 30})).unwrap();
    let violations = runtime.step(1, 10).unwrap();
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].property, "small");
    assert_eq!(violations[0].cells, json!({"n": 30}));
}

#[test]
fn bare_imports_and_unknown_core_modules_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let spec = spec_in(dir.path(), "spec.js", "import \"left-pad\";\n");
    let error = SpecRuntime::load(DriverKind::Terminal, Some(&spec), 1)
        .err()
        .unwrap();
    assert!(error.contains("left-pad"), "{error}");
    let spec = spec_in(dir.path(), "other.js", "import \"ui-prop-tester/nope\";\n");
    let error = SpecRuntime::load(DriverKind::Terminal, Some(&spec), 1)
        .err()
        .unwrap();
    assert!(error.contains("ui-prop-tester/nope"), "{error}");
}

#[test]
fn custom_actions_are_collected_with_their_source() {
    let dir = tempfile::tempdir().unwrap();
    let spec = spec_in(
        dir.path(),
        "spec.js",
        "import { defineAction, custom } from \"ui-prop-tester/browser\";\n\
         defineAction(\"fill\", async ({ document, args }) => { document.title = args.title; });\n\
         export const filling = custom(\"fill\", { title: \"x\" });\n",
    );
    let runtime = SpecRuntime::load(DriverKind::Browser, Some(&spec), 1).unwrap();
    assert_eq!(runtime.custom_actions.len(), 1);
    assert_eq!(runtime.custom_actions[0].name, "fill");
    assert!(
        runtime.custom_actions[0]
            .source
            .contains("document.title = args.title"),
        "{}",
        runtime.custom_actions[0].source
    );
}

#[test]
fn every_driver_defaults_module_loads() {
    for kind in [DriverKind::Terminal, DriverKind::Browser] {
        let runtime = SpecRuntime::load(kind, None, 1).unwrap();
        assert!(!runtime.properties.is_empty(), "{kind:?}");
        assert!(!runtime.categories.is_empty(), "{kind:?}");
    }
}
