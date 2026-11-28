use std::path::Path;

use horch_core::harness::{HarnessKind, PrepareRequest};
use horch_core::roster::{Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};

fn cargo_home_values(
    ctx: &RuntimeContext,
    teammate: &Teammate,
    workdir: &Path,
    shell_env: &[(String, String)],
) -> Vec<String> {
    let prepared = HarnessKind::Codex
        .adapter()
        .prepare(
            ctx,
            &PrepareRequest {
                role: "codex-1",
                exec_rules: &[],
                skills: None,
                compact_window: None,
                teammate,
                model: "gpt-6.2-sol",
                workdir,
                shell_env,
            },
        )
        .unwrap();
    let values = prepared
        .extra_args
        .iter()
        .filter_map(|a| a.strip_prefix("shell_environment_policy.set.CARGO_HOME="))
        .map(str::to_string)
        .collect();
    prepared.finish();
    values
}

#[cfg(not(windows))]
#[test]
fn grs_17_codex_launch_provides_writable_cargo_home_or_rule() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let state = tmp.path().join("state");
    let work = tmp.path().join("work");
    for dir in [home.join(".codex"), home.join(".cargo"), work.clone()] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let ctx = RuntimeContext::from_env(
        &MapEnv::new("/")
            .with("HOME", &home.to_string_lossy())
            .with("HORCH_STATE_DIR", &state.to_string_lossy())
            .with("PATH", "/nonexistent"),
    )
    .unwrap();
    let roster = Roster::builtin().unwrap();
    let sol = roster.require("codex-sol").unwrap().clone();

    let fallback = ctx.paths.state_root.join("cargo");
    let want = serde_json::to_string(&fallback.to_string_lossy()).unwrap();
    assert_eq!(cargo_home_values(&ctx, &sol, &work, &[]), vec![want]);
    assert!(
        fallback.is_dir(),
        "the fallback CARGO_HOME exists before launch"
    );

    let own = vec![("CARGO_HOME".to_string(), "/opt/cargo".to_string())];
    assert_eq!(
        cargo_home_values(&ctx, &sol, &work, &own),
        vec!["\"/opt/cargo\"".to_string()],
        "an explicit CARGO_HOME wins"
    );

    let mut offline = sol.clone();
    offline.permission_mode = None;
    offline.args.extend(
        [
            "-s",
            "workspace-write",
            "-c",
            "sandbox_workspace_write.network_access=false",
        ]
        .map(String::from),
    );
    assert!(
        cargo_home_values(&ctx, &offline, &work, &[]).is_empty(),
        "without network an empty CARGO_HOME cannot fetch, so ~/.cargo stays"
    );

    let mut full = sol.clone();
    full.permission_mode = None;
    full.args.push("--yolo".into());
    assert!(
        cargo_home_values(&ctx, &full, &work, &[]).is_empty(),
        "no sandbox, so ~/.cargo is writable"
    );
}
