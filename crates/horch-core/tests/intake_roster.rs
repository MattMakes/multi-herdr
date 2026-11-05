//! T-26: the skill-intake teammates (spec `ai_docs/specs/skill-intake/spec.md`
//! sections 6.4 and 9.2). Each role's trust wall is static frontmatter, so
//! this test pins it: tools, deny rules, and the sandbox read, write and deny
//! lists of each side root.

use std::path::Path;

use horch_core::harness::launch;
use horch_core::harness::{HarnessKind, LaunchEnv, Session};
use horch_core::roster::{Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::{json, Value};

const B: &str = "~/.local/state/skill-intake";
const CREDENTIALS: [&str; 4] = ["~/.ssh", "~/.aws", "~/.config", "~/Library/Keychains"];
const COMMON_DENIES: [&str; 4] = ["Agent", "WebFetch", "WebSearch", "NotebookEdit"];
const INTERPRETERS: [&str; 10] = [
    "Bash(python*)",
    "Bash(node*)",
    "Bash(sh *)",
    "Bash(bash *)",
    "Bash(zsh *)",
    "Bash(perl *)",
    "Bash(ruby *)",
    "Bash(./*)",
    "Bash(source *)",
    "Bash(chmod *)",
];

/// One row of the spec 6.4 table.
struct Role {
    name: &'static str,
    model: &'static str,
    effort: &'static str,
    tools: &'static [&'static str],
    allow_read: &'static [&'static str],
    allow_write: &'static [&'static str],
    deny_read: &'static [&'static str],
    /// Path rules as (tool, side root).
    path_rules: &'static [(&'static str, &'static str)],
    interpreters: bool,
    git: bool,
}

const ALL_TOOLS: &[&str] = &["Read", "Grep", "Glob", "Write", "Edit", "Bash"];
const CLEAN_RULES: &[(&str, &str)] = &[
    ("Read", "contaminated"),
    ("Read", "drafts"),
    ("Edit", "contaminated"),
    ("Edit", "drafts"),
];

const ROLES: [Role; 5] = [
    Role {
        name: "intake-auditor",
        model: "opus",
        effort: "high",
        tools: &["Read", "Grep", "Glob", "Write", "Bash"],
        allow_read: &["contaminated"],
        allow_write: &["contaminated"],
        deny_read: &["drafts", "clean"],
        path_rules: &[("Edit", "drafts"), ("Edit", "clean")],
        interpreters: true,
        git: true,
    },
    Role {
        name: "intake-analyst",
        model: "opus",
        effort: "medium",
        tools: ALL_TOOLS,
        allow_read: &["contaminated", "drafts"],
        allow_write: &["drafts"],
        deny_read: &["clean"],
        path_rules: &[("Edit", "contaminated"), ("Edit", "clean")],
        interpreters: true,
        git: false,
    },
    Role {
        name: "intake-sanitizer",
        model: "sonnet",
        effort: "medium",
        tools: ALL_TOOLS,
        allow_read: &["drafts"],
        allow_write: &["drafts"],
        deny_read: &["contaminated", "clean"],
        path_rules: &[
            ("Read", "contaminated"),
            ("Edit", "contaminated"),
            ("Edit", "clean"),
        ],
        interpreters: true,
        git: false,
    },
    Role {
        name: "intake-writer",
        model: "opus",
        effort: "medium",
        tools: ALL_TOOLS,
        allow_read: &["clean"],
        allow_write: &["clean"],
        deny_read: &["contaminated", "drafts"],
        path_rules: CLEAN_RULES,
        interpreters: false,
        git: false,
    },
    Role {
        name: "intake-reviewer",
        model: "opus",
        effort: "medium",
        tools: ALL_TOOLS,
        allow_read: &["clean"],
        allow_write: &["clean"],
        deny_read: &["contaminated", "drafts"],
        path_rules: CLEAN_RULES,
        interpreters: false,
        git: false,
    },
];

fn sides(roots: &[&str]) -> Vec<String> {
    roots.iter().map(|r| format!("{B}/{r}")).collect()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap_or_else(|| panic!("not a list: {v}"))
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

fn sandbox(t: &Teammate) -> Value {
    Value::Object(
        t.sandbox
            .clone()
            .unwrap_or_else(|| panic!("{}: no sandbox", t.name)),
    )
}

/// The common frontmatter of spec 6.4 and 9.2.
fn assert_common(t: &Teammate) {
    let who = &t.name;
    assert_eq!(t.agent, HarnessKind::Claude, "{who}");
    assert_eq!(t.base.as_deref(), Some("fleet-worker"), "{who}");
    assert_eq!(t.skills, ["skill-intake"], "{who}");
    assert!(!t.allow_subagents, "{who}");
    for tool in COMMON_DENIES {
        assert!(
            t.disallowed_tools.iter().any(|d| d == tool),
            "{who}: {tool}"
        );
    }
}

#[test]
fn t26_the_role_teammates_match_the_spec_table() {
    let r = Roster::builtin().unwrap();
    let sonnet = &r.require("sonnet").unwrap().allowed_tools;
    for role in &ROLES {
        let t = r.require(role.name).unwrap();
        let who = role.name;
        assert_common(t);
        assert!(t.hidden, "{who}");
        assert!(t.fallbacks.is_empty(), "{who}");
        assert_eq!(&t.allowed_tools, sonnet, "{who}");
        assert_eq!(t.model.as_deref(), Some(role.model), "{who}");
        assert_eq!(t.effort.as_deref(), Some(role.effort), "{who}");
        assert_eq!(t.tools.as_deref().unwrap(), role.tools, "{who}");

        let mut denies: Vec<String> = COMMON_DENIES.iter().map(|s| s.to_string()).collect();
        denies.extend(
            role.path_rules
                .iter()
                .map(|(tool, side)| format!("{tool}({B}/{side}/**)")),
        );
        if role.interpreters {
            denies.extend(INTERPRETERS.iter().map(|s| s.to_string()));
        }
        if role.git {
            denies.push("Bash(git *)".into());
        }
        assert_eq!(t.disallowed_tools, denies, "{who}");

        let block = sandbox(t);
        assert!(
            block.pointer("/network/allowedDomains").is_none(),
            "{who}: a role has no network"
        );
        let fs = &block["filesystem"];
        assert_eq!(strings(&fs["allowRead"]), sides(role.allow_read), "{who}");
        assert_eq!(strings(&fs["allowWrite"]), sides(role.allow_write), "{who}");
        let mut deny_read = sides(role.deny_read);
        deny_read.extend(CREDENTIALS.iter().map(|s| s.to_string()));
        assert_eq!(strings(&fs["denyRead"]), deny_read, "{who}");
    }
}

#[test]
fn t26_the_skill_importer_is_the_offered_controller() {
    let r = Roster::builtin().unwrap();
    let t = r.require("skill-importer").unwrap();
    assert_common(t);
    assert!(!t.hidden);
    assert!(t.offer_when.is_empty(), "offered everywhere");
    assert_eq!(t.model.as_deref(), Some("opus"));
    assert_eq!(t.effort.as_deref(), Some("high"));
    assert_eq!(
        &t.allowed_tools,
        &r.require("sonnet").unwrap().allowed_tools
    );
    assert!(t
        .disallowed_tools
        .iter()
        .any(|d| d == &format!("Read({B}/contaminated/*/quarantine/**)")));
    let block = sandbox(t);
    assert_eq!(
        block["network"]["allowedDomains"],
        json!([
            "github.com",
            "codeload.github.com",
            "objects.githubusercontent.com"
        ])
    );
    let fs = &block["filesystem"];
    assert_eq!(strings(&fs["allowRead"]), [B]);
    assert_eq!(strings(&fs["allowWrite"]), [B, "~/.claude/skills"]);
    for spawn_target in ROLES.iter().map(|r| r.name) {
        assert!(
            t.persona.contains(spawn_target),
            "spawn grant names {spawn_target}"
        );
    }
    assert!(t.persona.contains("--exact"));
}

#[test]
fn t26_the_roster_check_passes_for_the_intake_teammates() {
    let r = Roster::builtin().unwrap();
    let names: Vec<&str> = ROLES
        .iter()
        .map(|r| r.name)
        .chain(["skill-importer"])
        .collect();
    let problems: Vec<String> = r
        .check()
        .into_iter()
        .filter(|p| names.iter().any(|n| p.contains(n)))
        .collect();
    assert!(problems.is_empty(), "{problems:?}");
}

/// A launch environment with a temp home and stub Linux sandbox tools, so
/// the builder runs on every host.
fn launch_env(home: &Path) -> LaunchEnv {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["bwrap", "socat"] {
        let path = bin.join(tool);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }
    let env = MapEnv::new(home)
        .with("HOME", &home.to_string_lossy())
        .with("PATH", &bin.to_string_lossy())
        .with(
            "HORCH_CLAUDE_MANAGED_SETTINGS",
            &home.join("no-managed-settings.json").to_string_lossy(),
        );
    LaunchEnv::from_context(&RuntimeContext::from_env(&env).unwrap())
}

/// Each of the 6 teammates launches with its sandbox block in `--settings`,
/// also on the launch path without exposed skills (ticket 18).
#[test]
fn t26_every_intake_teammate_launches_sandboxed() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let r = Roster::builtin().unwrap();
    for name in ROLES.iter().map(|r| r.name).chain(["skill-importer"]) {
        let t = r.require(name).unwrap();
        let cmd = launch::command_with_skills_in(&env, t, Session::Unmanaged, "PROMPT", None, None)
            .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let i = args
            .iter()
            .position(|a| a == "--settings")
            .unwrap_or_else(|| panic!("{name}: no --settings"));
        let settings: Value = serde_json::from_str(&args[i + 1]).unwrap();
        let s = &settings["sandbox"];
        assert_eq!(s["enabled"], true, "{name}: no sandbox block at launch");
        assert_eq!(s["network"]["strictAllowlist"], true, "{name}");
        let own = sandbox(t);
        for key in ["allowRead", "allowWrite"] {
            let launched = strings(&s["filesystem"][key]);
            for path in strings(&own["filesystem"][key]) {
                assert!(launched.contains(&path), "{name}: {key} {path}");
            }
        }
    }
}
