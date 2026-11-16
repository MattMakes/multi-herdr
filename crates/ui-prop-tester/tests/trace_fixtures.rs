#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use ui_prop_tester::trace::{
    write_trace_file, SpecificationRecord, Trace, TraceHeader, TraceState, TraceSummary, Violation,
};
use ui_prop_tester::{DriverKind, Io, RealDrivers};

pub struct Outcome {
    pub code: i32,
    pub out: String,
    pub err: String,
}

pub fn run_cli(args: &[&str]) -> Outcome {
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let mut full: Vec<OsString> = vec!["ui-prop-tester".into()];
    full.extend(args.iter().map(OsString::from));
    let code = {
        let mut io = Io {
            out: &mut out,
            err: &mut err,
        };
        ui_prop_tester::run(full, &mut io, &RealDrivers)
    };
    Outcome {
        code,
        out: String::from_utf8(out).expect("stdout is UTF-8"),
        err: String::from_utf8(err).expect("stderr is UTF-8"),
    }
}

pub fn path_text(path: &Path) -> &str {
    path.to_str().expect("temporary paths are UTF-8")
}

pub fn png(width: u32, height: u32, paint: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut raw: Vec<u8> = Vec::with_capacity(((width * 3 + 1) * height) as usize);
    for y in 0..height {
        raw.push(0);
        for x in 0..width {
            raw.extend_from_slice(&paint(x, y));
        }
    }
    let mut zlib: Vec<u8> = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65_535).collect();
    for (position, block) in blocks.iter().enumerate() {
        let last = u8::from(position + 1 == blocks.len());
        let length = block.len() as u16;
        zlib.push(last);
        zlib.extend_from_slice(&length.to_le_bytes());
        zlib.extend_from_slice(&(!length).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 2, 0, 0, 0]);
    let mut file = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    chunk(&mut file, b"IHDR", &header);
    chunk(&mut file, b"IDAT", &zlib);
    chunk(&mut file, b"IEND", &[]);
    file
}

fn chunk(file: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    file.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = file.len();
    file.extend_from_slice(kind);
    file.extend_from_slice(data);
    let crc = crc32(&file[start..]);
    file.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                (crc >> 1) ^ 0xedb8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for byte in bytes {
        a = (a + u32::from(*byte)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

pub fn page_png(width: u32, height: u32, accent: [u8; 3], button_lit: bool) -> Vec<u8> {
    png(width, height, |x, y| {
        let bar = height / 8;
        if y < bar {
            return accent;
        }
        let inside_button =
            x > width / 3 && x < width * 2 / 3 && y > height / 2 && y < height / 2 + bar;
        if inside_button {
            return if button_lit {
                [40, 170, 90]
            } else {
                [70, 110, 200]
            };
        }
        let line =
            (y - bar) % 24 < 6 && x > width / 10 && x < width * 7 / 10 && y < height / 2 - 10;
        if line {
            [200, 204, 210]
        } else {
            [250, 250, 252]
        }
    })
}

pub fn browser_page(url: &str, title: &str, heap: u64, nodes: u64, console: Value) -> Value {
    json!({
        "url": url,
        "title": title,
        "mime_type": "text/html",
        "viewport": {"width": 320, "height": 240, "device_scale_factor": 1},
        "navigation": {"status": 200, "kind": "load"},
        "history": {"index": 0, "entries": [{"url": url, "title": title}]},
        "console": console,
        "exceptions": [],
        "resources": {
            "js_heap_used": heap,
            "js_heap_total": heap * 2,
            "dom_nodes": nodes,
            "event_listeners": nodes / 4,
            "layout_objects": nodes - 3
        },
        "coverage": null,
        "scroll": {"x": 0, "y": 0, "max_x": 0, "max_y": 0},
        "content": {"is_html": true, "has_body": true},
        "focused": null,
        "clickables": [],
        "downloads": [],
        "last_action": null
    })
}

pub fn save_violation(state_index: u64) -> Violation {
    Violation {
        property: "savedMessageAppears".into(),
        state_index,
        path: "always > implies > eventually".into(),
        message: Some("the saved banner never appeared".into()),
        cells: json!({"banner": null, "saving": true}),
    }
}

pub fn browser_trace() -> Trace {
    let shots = [
        page_png(320, 240, [43, 103, 216], false),
        page_png(320, 240, [43, 103, 216], true),
        page_png(320, 240, [196, 43, 54], true),
    ];
    let violation = save_violation(3);
    Trace {
        header: TraceHeader {
            tool_version: "0.4.10".into(),
            driver: DriverKind::Browser,
            seed: 42,
            target: json!({"origin": "http://localhost:8080/", "width": 320, "height": 240}),
            specification: Some(SpecificationRecord {
                path: "specs/app.js".into(),
                source: "export * from \"ui-prop-tester/browser/defaults\";".into(),
            }),
            started_unix_ms: 1_763_300_000_000,
            coverage_symbols: None,
        },
        states: vec![
            TraceState {
                index: 0,
                timestamp_ms: 0,
                action: None,
                state: browser_page("http://localhost:8080/", "Notes", 1_200_000, 120, json!([])),
                cells: json!({"banner": null, "saving": false}),
                violations: vec![],
                screenshot: Some(shots[0].clone()),
            },
            TraceState {
                index: 1,
                timestamp_ms: 140,
                action: Some(json!({
                    "kind": "click",
                    "category": "clicks",
                    "target": {"test_id": "save", "role": "button", "name": "Save note", "path": "body > main > button"},
                    "point": {"x": 160, "y": 135}
                })),
                state: browser_page("http://localhost:8080/", "Notes", 1_350_000, 131, json!([])),
                cells: json!({"banner": null, "saving": true}),
                violations: vec![],
                screenshot: Some(shots[1].clone()),
            },
            TraceState {
                index: 2,
                timestamp_ms: 420,
                action: Some(json!({"kind": "type", "text": "hello", "category": "inputs"})),
                state: browser_page(
                    "http://localhost:8080/",
                    "Notes",
                    1_500_000,
                    140,
                    json!([{"level": "warning", "text": "slow save", "url": "http://localhost:8080/app.js", "line": 12}]),
                ),
                cells: json!({"banner": null, "saving": true}),
                violations: vec![],
                screenshot: None,
            },
            TraceState {
                index: 3,
                timestamp_ms: 1_900,
                action: Some(json!({"kind": "key", "key": "Enter", "category": "inputs"})),
                state: browser_page(
                    "http://localhost:8080/",
                    "Notes",
                    1_800_000,
                    152,
                    json!([{"level": "error", "text": "save failed: 500", "url": "http://localhost:8080/app.js", "line": 40}]),
                ),
                cells: json!({"banner": null, "saving": true}),
                violations: vec![violation.clone()],
                screenshot: Some(shots[2].clone()),
            },
        ],
        summary: Some(TraceSummary {
            outcome: "violation".into(),
            states: 4,
            violations: vec![violation],
            message: None,
        }),
    }
}

pub fn terminal_state(lines: &[&str], cells: Value) -> Value {
    json!({
        "columns": 12,
        "rows": 3,
        "lines": lines,
        "cells": cells,
        "cursor": {"row": 1, "column": 4, "visible": true, "blink": true, "shape": "bar", "color": null},
        "scroll_offset": 0,
        "scrollback": {"lines": [], "cells": []},
        "exit": null,
        "last_action": null
    })
}

pub fn terminal_trace() -> Trace {
    let first = terminal_state(
        &["$ ok", "\u{4f60}\u{597d} wide"],
        json!([
            {"row": 0, "column": 0, "text": "$", "bold": true, "fg": {"indexed": 2}},
            {"row": 0, "column": 2, "text": "o", "italic": true, "underline": "curly", "underline_color": {"rgb": [255, 0, 0]}},
            {"row": 0, "column": 3, "text": "k", "bg": {"rgb": [10, 20, 30]}, "inverse": true, "strikethrough": true},
            {"row": 1, "column": 0, "text": "\u{4f60}", "fg": {"indexed": 200}, "dim": true},
            {"row": 1, "column": 1, "text": "", "wide_continuation": true},
            {"row": 1, "column": 2, "text": "\u{597d}", "blink": true, "overline": true, "underline": "double"},
            {"row": 1, "column": 3, "text": "", "wide_continuation": true},
            {"row": 1, "column": 5, "text": "w", "invisible": true, "fg": {"indexed": 9}, "bg": {"indexed": 4}},
            {"row": 1, "column": 6, "text": "i", "underline": "single", "underline_color": {"indexed": 33}},
            {"row": 1, "column": 7, "text": "d", "underline": "dotted"},
            {"row": 1, "column": 8, "text": "e", "underline": "dashed", "fg": {"rgb": [1, 2, 3]}}
        ]),
    );
    let mut second = terminal_state(&["$ ok", "bye"], json!([]));
    second["exit"] = json!({"code": 3, "signal": null});
    Trace {
        header: TraceHeader {
            tool_version: "0.4.10".into(),
            driver: DriverKind::Terminal,
            seed: 7,
            target: json!({"program": "sh", "args": ["-c", "echo ok"], "columns": 12, "rows": 3, "scrollback_lines_max": 1000, "quiescence_timeout_ms": 100}),
            specification: None,
            started_unix_ms: 1,
            coverage_symbols: None,
        },
        states: vec![
            TraceState {
                index: 0,
                timestamp_ms: 0,
                action: None,
                state: first,
                cells: json!({}),
                violations: vec![],
                screenshot: None,
            },
            TraceState {
                index: 1,
                timestamp_ms: 55,
                action: Some(json!({"kind": "write", "text": "exit 3\r", "category": "typing"})),
                state: second,
                cells: json!({"exitCode": 3}),
                violations: vec![Violation {
                    property: "exitsCleanly".into(),
                    state_index: 1,
                    path: "always > now".into(),
                    message: Some("the program exited with code 3".into()),
                    cells: json!({"exitCode": 3}),
                }],
                screenshot: None,
            },
        ],
        summary: Some(TraceSummary {
            outcome: "violation".into(),
            states: 2,
            violations: vec![],
            message: None,
        }),
    }
}

pub fn write_trace(directory: &Path, name: &str, trace: &Trace) -> PathBuf {
    let path = directory.join(name);
    write_trace_file(&path, trace).expect("trace is written");
    path
}
