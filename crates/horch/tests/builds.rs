//! Having any integration test makes `cargo test` build the `horch` binary
//! itself, which the end-to-end tests in `crates/horch-e2e` run.

#[test]
fn the_binary_is_built_and_answers() {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_horch"));
    for key in [
        "HERDR_PANE_ID",
        "HC_TERMINAL_ID",
        "HC_SCOPE",
        "HC_TERMINAL_SOCKET",
        "HORCH_BACKEND",
    ] {
        cmd.env_remove(key);
    }
    let out = cmd.arg("--version").output().expect("running horch");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("horch "));
}
