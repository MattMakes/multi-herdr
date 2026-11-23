use horch_marketplace::git::GitRunner;
use horch_platform::start::FORBIDDEN_ENV;

const CONTROL: &str = "HORCH_P7_GIT_START_CONTROL";

#[test]
fn a_git_child_never_has_a_forbidden_name() {
    let mut runner = GitRunner::new("git").with_env(CONTROL, "kept");
    for key in FORBIDDEN_ENV {
        std::env::set_var(key, "forbidden-in-test");
        runner = runner.with_env(key, "forbidden-in-test");
    }
    let dir = tempfile::tempdir().unwrap();
    runner.run(dir.path(), &["init", "-q"]).unwrap();
    let out = runner
        .run(dir.path(), &["-c", "alias.vars=!env", "vars"])
        .unwrap();
    let vars = String::from_utf8_lossy(&out.stdout);
    assert!(vars.contains(&format!("{CONTROL}=kept")), "{vars}");
    for key in FORBIDDEN_ENV {
        assert!(
            !vars.lines().any(|l| l.starts_with(&format!("{key}="))),
            "{key} reached the git child"
        );
    }
}
