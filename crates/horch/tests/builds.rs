//! Having any integration test makes `cargo test` build the `horch` binary
//! itself, which the end-to-end tests in `crates/horch-e2e` run.

#[test]
fn the_binary_is_built_and_answers() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_horch"))
        .arg("--version")
        .output()
        .expect("running horch");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("horch "));
}
