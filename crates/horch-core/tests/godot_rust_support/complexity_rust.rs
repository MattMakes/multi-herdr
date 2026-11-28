use std::path::Path;

use horch_core::taskledger::finish::complexity::clippy_violations;

fn diagnostic(code: &str, message: &str, line: u64, cols: (usize, usize), text: &[&str]) -> String {
    let text: Vec<serde_json::Value> = text
        .iter()
        .map(|t| serde_json::json!({ "text": t }))
        .collect();
    serde_json::json!({
        "reason": "compiler-message",
        "message": {
            "message": message,
            "code": { "code": code },
            "spans": [{
                "file_name": "src/lib.rs",
                "line_start": line,
                "is_primary": true,
                "column_start": cols.0,
                "column_end": cols.1,
                "text": text
            }]
        }
    })
    .to_string()
}

fn cognitive(line: u64) -> String {
    diagnostic(
        "clippy::cognitive_complexity",
        "the function has a cognitive complexity of (30/25)",
        line,
        (8, 15),
        &["pub fn tangled(v: &[u32]) -> u32 {"],
    )
}

fn too_long(line: u64) -> String {
    diagnostic(
        "clippy::too_many_lines",
        "this function has too many lines (120/100)",
        line,
        (1, 2),
        &["pub fn tangled(v: &[u32]) -> u32 {", "    0", "}"],
    )
}

#[test]
fn grs_02_clippy_parses_three_lints_and_extracts_fn_identifier() {
    let signature =
        "    pub(crate) fn many<T: Copy>(a: T, b: T, c: T, d: T, e: T, f: T, g: T, h: T) -> T {";
    let args = diagnostic(
        "clippy::too_many_arguments",
        "this function has too many arguments (8/7)",
        40,
        (5, signature.len()),
        &[signature],
    );
    let other = diagnostic(
        "clippy::needless_return",
        "unneeded return (1/0)",
        9,
        (1, 5),
        &["x"],
    );
    let json = [cognitive(5), too_long(20), args, other].join("\n");
    let v = clippy_violations(&json, Path::new("/w/rust"), Path::new("/w"));
    let got: Vec<(&str, &str, u64, &str, f64, f64)> = v
        .iter()
        .map(|v| {
            (
                v.file_path.as_str(),
                v.function_name.as_str(),
                v.line,
                v.metric.as_str(),
                v.value,
                v.threshold,
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (
                "rust/src/lib.rs",
                "tangled",
                5,
                "cognitiveComplexity",
                30.0,
                25.0
            ),
            (
                "rust/src/lib.rs",
                "tangled",
                20,
                "functionLength",
                120.0,
                100.0
            ),
            ("rust/src/lib.rs", "many", 40, "parameterCount", 8.0, 7.0),
        ]
    );
}

#[test]
fn grs_02_clippy_dedupes_by_file_line_and_metric() {
    let json = [cognitive(5), cognitive(5), too_long(5), too_long(5)].join("\n");
    let v = clippy_violations(&json, Path::new("/w/rust"), Path::new("/w"));
    let got: Vec<(u64, &str)> = v.iter().map(|v| (v.line, v.metric.as_str())).collect();
    assert_eq!(
        got,
        vec![(5, "cognitiveComplexity"), (5, "functionLength")],
        "1 function that breaks 2 lints gives 2 violations"
    );
}
