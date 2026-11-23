use std::path::{Path, PathBuf};

use horch_core::fleetd::worker_profiles::domain::operator_boundary::{
    Act, NotSupported, OperatorPaths,
};
use horch_core::harness::{antigravity, none, opencode, pi, prime};
use horch_core::harness::{CommandSpec, HarnessKind, LaunchEnv, Session};
use horch_core::roster::{Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};

struct Fleet {
    root: tempfile::TempDir,
    fleet_root: PathBuf,
    paths: OperatorPaths,
}

fn fleet() -> Fleet {
    let root = tempfile::tempdir().unwrap();
    let fleet_root = root.path().join("fleet");
    let paths = OperatorPaths::of(&fleet_root);
    for dir in [
        &paths.requests,
        &fleet_root.join("tasks/T-1/attempts/A1"),
        &fleet_root.join("work/T-1"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(&paths.token, "secret\n").unwrap();
    Fleet {
        root,
        fleet_root,
        paths,
    }
}

impl Fleet {
    fn env(&self) -> LaunchEnv {
        let home = self.root.path().join("home");
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let env = MapEnv::new(&home)
            .with("HOME", &s(&home))
            .with("HORCH_STATE_DIR", &s(&self.root.path().join("state")))
            .with(
                "HORCH_LEDGER_DB",
                &s(&self.root.path().join("ledger.sqlite3")),
            )
            .with("FLEET_ROOT", &s(&self.fleet_root));
        let mut env = LaunchEnv::from_context(&RuntimeContext::from_env(&env).unwrap());
        env.task_dir = Some(self.fleet_root.join("tasks/T-1/attempts/A1"));
        env.worktree = Some(self.fleet_root.join("work/T-1"));
        env
    }

    fn rendered(&self, teammate: &Teammate) -> String {
        let cmd = teammate
            .agent
            .adapter()
            .build_command(
                &self.env(),
                &CommandSpec {
                    teammate,
                    session: Session::Unmanaged,
                    prompt: "work",
                    model_override: None,
                },
            )
            .unwrap();
        let mut text: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        for (key, value) in cmd.get_envs() {
            let value = value.map(|v| v.to_string_lossy().into_owned());
            text.push(format!(
                "{}={}",
                key.to_string_lossy(),
                value.unwrap_or_default()
            ));
        }
        text.join("\n")
    }

    fn assert_no_rule_names_an_operator_path(&self, kind: HarnessKind) -> Vec<Teammate> {
        let personas = personas(kind);
        assert!(!personas.is_empty(), "no shipped persona runs on {kind:?}");
        for teammate in &personas {
            let text = self.rendered(teammate);
            for path in [self.fleet_root.as_path(), self.paths.run.as_path()]
                .into_iter()
                .chain(Act::ALL.iter().map(|a| self.paths.of_act(*a)))
            {
                assert!(
                    !text.contains(&*path.to_string_lossy()),
                    "{}: the launch names {}:\n{text}",
                    teammate.name,
                    path.display()
                );
            }
        }
        personas
    }
}

fn personas(kind: HarnessKind) -> Vec<Teammate> {
    let roster = Roster::builtin().unwrap();
    roster
        .names()
        .into_iter()
        .map(|name| roster.for_launch(roster.require(name).unwrap().clone()))
        .filter(|t| t.agent == kind)
        .collect()
}

fn assert_not_supported(gap: NotSupported, adapter: &str) {
    assert_eq!(gap.adapter, adapter);
    assert_eq!(gap.os, std::env::consts::OS);
    assert!(!gap.reason.is_empty());
}

#[test]
fn antigravity_boundary_denies_the_3_acts_or_is_not_supported() {
    let f = fleet();
    for t in f.assert_no_rule_names_an_operator_path(HarnessKind::Antigravity) {
        let gap = antigravity::operator_boundary(&t, &f.fleet_root).unwrap_err();
        assert_not_supported(gap, "antigravity");
    }
}

#[test]
fn opencode_boundary_denies_the_3_acts_or_is_not_supported() {
    let f = fleet();
    for t in f.assert_no_rule_names_an_operator_path(HarnessKind::OpenCode) {
        let gap = opencode::operator_boundary(&t, &f.fleet_root).unwrap_err();
        assert_not_supported(gap, "opencode");
    }
}

#[test]
fn pi_boundary_denies_the_3_acts_or_is_not_supported() {
    let f = fleet();
    for t in f.assert_no_rule_names_an_operator_path(HarnessKind::Pi) {
        let gap = pi::operator_boundary(&t, &f.fleet_root).unwrap_err();
        assert_not_supported(gap, "pi");
    }
}

#[test]
fn prime_boundary_denies_the_3_acts_or_is_not_supported() {
    let f = fleet();
    for t in f.assert_no_rule_names_an_operator_path(HarnessKind::Prime) {
        let gap = prime::operator_boundary(&t, &f.fleet_root).unwrap_err();
        assert_not_supported(gap, "prime");
    }
}

#[test]
fn none_boundary_denies_the_3_acts_or_is_not_supported() {
    let f = fleet();
    let smokes = personas(HarnessKind::None);
    assert!(!smokes.is_empty());
    for t in smokes {
        let launch = HarnessKind::None.adapter().build_command(
            &f.env(),
            &CommandSpec {
                teammate: &t,
                session: Session::Unmanaged,
                prompt: "work",
                model_override: None,
            },
        );
        assert!(launch.is_err(), "{} launched a process", t.name);
        let boundary = none::operator_boundary(&t, &f.fleet_root).unwrap();
        assert!(boundary.holds());
        for act in Act::ALL {
            assert!(boundary.denies(act), "{act}");
        }
    }
}
