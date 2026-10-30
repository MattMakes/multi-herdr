//! `arrange::run` and `arrange::workspace_of` against a scripted `herdr`:
//! the plan report, the refusals, the fast path and the move path. The script
//! answers each herdr call with canned JSON and moves nothing.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::workspace::arrange;
use horch_core::workspace::herdr::Herdr;

/// One tab's layout JSON: `(pane, x, y, width, height)` in a 120x40 area.
fn layout(tab: &str, zoomed: bool, panes: &[(&str, i64, i64, i64, i64)]) -> String {
    let panes: Vec<String> = panes
        .iter()
        .map(|(id, x, y, w, h)| {
            format!(r#"{{"pane_id":"{id}","rect":{{"x":{x},"y":{y},"width":{w},"height":{h}}}}}"#)
        })
        .collect();
    format!(
        r#"{{"workspace_id":"w1","tab_id":"{tab}","area":{{"x":0,"y":0,"width":120,"height":40}},"panes":[{}],"zoomed":{zoomed}}}"#,
        panes.join(",")
    )
}

struct World {
    _dir: tempfile::TempDir,
    ctx: RuntimeContext,
    herdr: Herdr,
}

/// A script that answers `workspace list`, `tab list`, `pane list`, `pane
/// layout --pane <p>` and the move and focus calls.
fn world(label: &str, tabs: &[(&str, String, &[&str])]) -> World {
    let dir = tempfile::tempdir().unwrap();
    let tab_list: Vec<String> = tabs
        .iter()
        .enumerate()
        .map(|(i, (t, _, _))| {
            format!(
                r#"{{"tab_id":"{t}","workspace_id":"w1","number":{}}}"#,
                i + 1
            )
        })
        .collect();
    let pane_list: Vec<String> = tabs
        .iter()
        .flat_map(|(t, _, panes)| {
            panes
                .iter()
                .map(move |p| format!(r#"{{"pane_id":"{p}","workspace_id":"w1","tab_id":"{t}"}}"#))
        })
        .collect();
    let mut body = format!(
        "case \"$1 $2\" in\n\
         \"workspace list\") echo '{{\"result\":{{\"workspaces\":[{{\"workspace_id\":\"w1\",\"label\":\"{label}\",\"active_tab_id\":\"t1\"}}]}}}}';;\n\
         \"tab list\") echo '{{\"result\":{{\"tabs\":[{}]}}}}';;\n\
         \"pane list\") echo '{{\"result\":{{\"panes\":[{}]}}}}';;\n\
         \"tab focus\") echo '{{\"result\":{{}}}}';;\n",
        tab_list.join(","),
        pane_list.join(",")
    );
    body.push_str("\"pane layout\")\ncase \"$4\" in\n");
    for (_, json, panes) in tabs {
        for p in *panes {
            body.push_str(&format!(
                "{p}) echo '{{\"result\":{{\"layout\":{json}}}}}';;\n"
            ));
        }
    }
    body.push_str("esac;;\n");
    // A declined resize ends any balance loop.
    if let Some((_, json, _)) = tabs.first() {
        body.push_str(&format!(
            "\"pane resize\") echo '{{\"result\":{{\"resize\":{{\"changed\":false,\"layout\":{json}}}}}}}';;\n"
        ));
    }
    body.push_str(
        "\"pane move\")\n\
         case \"$*\" in\n\
         *--new-tab*) echo '{\"result\":{\"move_result\":{\"changed\":true,\"pane\":{\"pane_id\":\"x\"},\"created_tab\":{\"tab_id\":\"tnew\",\"workspace_id\":\"w1\",\"number\":9}}}}';;\n\
         *) echo '{\"result\":{\"move_result\":{\"changed\":true,\"pane\":{\"pane_id\":\"x\"}}}}';;\n\
         esac;;\n\
         \"pane get\") echo '{\"result\":{\"pane\":{\"pane_id\":\"p1\",\"workspace_id\":\"wget\"}}}';;\n\
         *) echo \"unscripted: $*\" >&2; exit 1;;\nesac\n",
    );
    let script = dir.path().join("herdr");
    std::fs::write(&script, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let state = dir.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let env = MapEnv::new(dir.path())
        .with("HORCH_STATE_DIR", state.to_str().unwrap())
        .with("TMPDIR", dir.path().to_str().unwrap());
    let ctx = RuntimeContext::from_env(&env).unwrap();
    let herdr = Herdr::with_bin(&script);
    warm(&script);
    World {
        _dir: dir,
        ctx,
        herdr,
    }
}

/// macOS scans a new executable at its first exec; do it here, not in the
/// call under test.
fn warm(script: &Path) {
    std::process::Command::new(script)
        .arg("warm")
        .output()
        .unwrap();
}

/// Orchestrator on the left, 2 workers stacked on the right.
fn stacked(tab: &str) -> String {
    layout(
        tab,
        false,
        &[
            ("p1", 0, 0, 60, 40),
            ("p2", 60, 0, 60, 20),
            ("p3", 60, 20, 60, 20),
        ],
    )
}

/// Orchestrator on the left, 2 workers side by side: not the canonical grid.
fn scrambled(tab: &str, zoomed: bool) -> String {
    layout(
        tab,
        zoomed,
        &[
            ("p1", 0, 0, 60, 40),
            ("p2", 60, 0, 30, 40),
            ("p3", 90, 0, 30, 40),
        ],
    )
}

fn run(w: &World, plan_only: bool) -> anyhow::Result<String> {
    arrange::run(&w.ctx, &w.herdr, "w1", None, plan_only)
}

fn fleet(tab_layout: String) -> World {
    world("fleet", &[("t1", tab_layout, &["p1", "p2", "p3"])])
}

#[test]
fn the_plan_of_a_canonical_grid_moves_nothing() {
    let out = run(&fleet(stacked("t1")), true).unwrap();
    assert!(
        out.starts_with(
            "2 worker(s) over 1 tab(s); 4 move(s) to get there\n\n\
             the grid is already laid out; `horch tile` would only even out the \
             columns and change nothing else\n\n"
        ),
        "{out}"
    );
    assert!(!out.contains("park every worker"), "{out}");
    assert!(
        out.contains("after: tab 1  orchestrator p1 (1/2 of the tab)"),
        "{out}"
    );
}

#[test]
fn the_plan_of_another_shape_lists_every_move() {
    let out = run(&fleet(scrambled("t1", false)), true).unwrap();
    assert!(
        out.starts_with("2 worker(s) over 1 tab(s); 4 move(s) to get there\n\npark every worker"),
        "{out}"
    );
    for line in [
        "  herdr pane move p2 --new-tab --label \"horch-tile-scratch\" --no-focus\n",
        "  herdr pane move p3 --tab <scratch tab> --split down --target-pane p2 --ratio 0.5000 --no-focus\n",
        "\nthen place them, slot by slot:\n",
        "  herdr pane move p2 --tab t1 --split right --target-pane p1 --ratio 0.5000 --no-focus\n",
        "emptied tabs close themselves, so no tab is ever closed by hand.\n",
        "then: even out every column, and put focus back where it was.\n\n",
        "after: tab 1  orchestrator p1 (1/2 of the tab)  2 worker(s), 1 column(s)\n",
    ] {
        assert!(out.contains(line), "missing {line:?} in\n{out}");
    }
}

#[test]
fn a_canonical_grid_is_not_rebuilt() {
    let out = run(&fleet(stacked("t1")), false).unwrap();
    assert!(
        out.starts_with(
            "2 worker(s) over 1 tab(s): 0 pane move(s), 0 resize(s)\n\
             the grid was already laid out; only the columns needed evening\n\n=== tab 1 of 1"
        ),
        "{out}"
    );
}

#[test]
fn another_shape_is_moved_then_reported() {
    let out = run(&fleet(scrambled("t1", false)), false).unwrap();
    assert!(
        out.starts_with("2 worker(s) over 1 tab(s): 4 pane move(s), 1 resize(s)\n\n=== tab 1 of 1"),
        "{out}"
    );
    assert!(!out.contains("already laid out"), "{out}");
}

#[test]
fn a_zoomed_tab_refuses_the_live_run_but_not_the_plan() {
    let w = fleet(scrambled("t1", true));
    assert_eq!(
        format!("{:#}", run(&w, false).unwrap_err()),
        "tab t1 is zoomed, and herdr refuses to move panes into or out of a zoomed tab. \
         Unzoom it, then tile again"
    );
    assert!(run(&w, true).is_ok());
}

#[test]
fn the_telemetry_space_is_left_alone() {
    let w = world(
        "horch telemetry",
        &[("t1", stacked("t1"), &["p1", "p2", "p3"])],
    );
    assert_eq!(
        format!("{:#}", run(&w, false).unwrap_err()),
        "workspace w1 is the telemetry space; horch tile leaves it alone"
    );
}

#[test]
fn a_workspace_without_panes_is_refused() {
    let w = world("fleet", &[]);
    assert_eq!(
        format!("{:#}", run(&w, false).unwrap_err()),
        "workspace w1 has no panes"
    );
}

#[test]
fn workspace_of_prefers_the_named_workspace_then_the_pane() {
    let w = fleet(stacked("t1"));
    let of = |pane, ws| arrange::workspace_of(&w.ctx, &w.herdr, pane, ws);
    assert_eq!(of(Some("p1"), Some("wx")).unwrap(), "wx");
    // An empty name counts as none.
    assert_eq!(of(Some("p1"), Some("")).unwrap(), "wget");
    assert_eq!(of(Some("p1"), None).unwrap(), "wget");
    let err = format!("{:#}", of(None, None).unwrap_err());
    assert!(
        err.starts_with("horch needs --workspace, or to run inside a herdr pane"),
        "{err}"
    );
    assert!(err.contains("HORCH_WORKSPACE_ID is not set"), "{err}");
}
