//! T-26: the skill-intake teammates (spec `ai_docs/specs/skill-intake/spec.md`
//! sections 6.4 and 9.2). Each role's trust wall is static frontmatter, so
//! this test pins it: tools, deny rules, and the sandbox read, write and deny
//! lists of each side root.

use std::path::Path;

use horch_core::execution::SessionMode;
use horch_core::harness::launch;
use horch_core::harness::{HarnessKind, LaunchEnv, Session};
use horch_core::prompts;
use horch_core::roster::{HorchAccess, Roster, Teammate};
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
        model: "opus",
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
fn assert_common(t: &Teammate, base: &str) {
    let who = &t.name;
    assert_eq!(t.agent, HarnessKind::Claude, "{who}");
    assert_eq!(t.base.as_deref(), Some(base), "{who}");
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
    // sonnet's pre-approvals without `horch` commands: report mode refuses
    // every `horch` command but progress, tell and done.
    let sonnet: Vec<String> = r
        .require("sonnet")
        .unwrap()
        .allowed_tools
        .iter()
        .filter(|p| !p.starts_with("Bash(horch "))
        .cloned()
        .collect();
    let sonnet = &sonnet;
    for role in &ROLES {
        let t = r.require(role.name).unwrap();
        let who = role.name;
        assert_common(t, "report-worker");
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
    assert_common(t, "fleet-worker");
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
    assert_eq!(strings(&fs["allowWrite"]), [B]);
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
            "HERDR_SOCKET_PATH",
            &home.join("herdr.sock").to_string_lossy(),
        )
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

/// The launched `--settings` of `t` in `env`.
fn launched_settings(env: &LaunchEnv, t: &Teammate) -> Value {
    let cmd =
        launch::command_with_skills_in(env, t, Session::Unmanaged, "PROMPT", None, None).unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let i = args.iter().position(|a| a == "--settings").unwrap();
    serde_json::from_str(&args[i + 1]).unwrap()
}

/// T-20: the 5 roles have `horch_access: report`. Their launched settings
/// hold no state root, ledger folder, relay socket or herdr socket: only
/// the pane's report socket. skill-importer keeps `full` and all 4 grants.
#[test]
fn t20_the_roles_launch_with_only_the_report_socket() {
    let home = tempfile::tempdir().unwrap();
    let mut env = launch_env(home.path());
    let state_root = env.state_root.clone().unwrap();
    let ledger_dir = env.ledger_dir.clone().unwrap();
    let herdr = env.herdr_socket.clone().unwrap();
    let socket = state_root.join("report").join("r1.sock");
    env.report_socket = Some(socket.clone());
    let r = Roster::builtin().unwrap();
    for name in ROLES.iter().map(|r| r.name) {
        let t = r.require(name).unwrap();
        assert_eq!(t.horch_access, HorchAccess::Report, "{name}");
        let s = &launched_settings(&env, t)["sandbox"];
        assert_eq!(
            s["network"]["allowUnixSockets"],
            json!([socket.to_string_lossy()]),
            "{name}"
        );
        for key in ["allowRead", "allowWrite"] {
            let launched = strings(&s["filesystem"][key]);
            for path in [&state_root, &ledger_dir] {
                let path = path.to_string_lossy().into_owned();
                assert!(!launched.contains(&path), "{name}: {key} {path}");
            }
        }
        let text = s.to_string();
        assert!(!text.contains(&*herdr.to_string_lossy()), "{name}: {text}");
    }
    let importer = r.require("skill-importer").unwrap();
    assert_eq!(importer.horch_access, HorchAccess::Full);
    let s = &launched_settings(&env, importer)["sandbox"];
    let sockets = strings(&s["network"]["allowUnixSockets"]);
    for path in [&state_root, &herdr] {
        assert!(sockets.contains(&path.to_string_lossy().into_owned()));
    }
    assert!(!sockets.contains(&socket.to_string_lossy().into_owned()));
}

/// The horch commands report mode refuses (T-20). A briefing that names one
/// tells the worker to run a command that fails in its pane.
const REFUSED: [&str; 21] = [
    "horch note",
    "horch skills",
    "horch ledger claim",
    "horch ledger worktree",
    "horch ledger status",
    "horch ledger show",
    "horch ledger steps",
    "horch ledger finish",
    "horch ledger done",
    "horch worktree",
    "horch spawn",
    "horch assign",
    "horch inbox",
    "horch compact",
    "horch context",
    "horch mode",
    "horch teammates",
    "horch route",
    "horch tell orchestrator",
    "horch done --unlanded",
    "horch fleet",
];

/// T-22: each role renders a briefing that names no command report mode
/// refuses, for a fresh task and an idle start.
#[test]
fn t22_the_role_briefings_name_no_refused_horch_command() {
    let r = Roster::builtin().unwrap();
    let sessions = [
        ("task", "Read the brief", SessionMode::Fresh(None)),
        ("idle", "", SessionMode::Fresh(None)),
    ];
    for name in ROLES.iter().map(|r| r.name) {
        let t = r.require(name).unwrap();
        for (label, task, session) in &sessions {
            let p = prompts::worker_prompt(&r, t, "w-1", task, session).unwrap();
            for cmd in REFUSED {
                assert!(!p.contains(cmd), "{name} {label}: names `{cmd}`");
            }
            assert!(
                p.contains("horch ledger progress \"<milestone>\""),
                "{name}"
            );
            assert!(p.contains("horch tell <controller>"), "{name}");
            assert!(p.contains("horch done \"<summary>\""), "{name}");
        }
    }
    // The check catches the old base: fleet-worker names refused commands.
    let mut t = r.require("intake-writer").unwrap().clone();
    t.base = Some("fleet-worker".into());
    let p =
        prompts::worker_prompt(&r, &t, "w-1", "Read the brief", &SessionMode::Fresh(None)).unwrap();
    assert!(REFUSED.iter().any(|cmd| p.contains(cmd)));
}

/// T-27: each intake teammate launches with its own skills bundle open to
/// read (a working directory for Read, `allowRead` for Bash) and closed to
/// write (deny rules for Edit and Write, `denyWrite` for Bash). No other
/// bundle path is opened, and a `report` role still gets no state root.
#[test]
fn t27_every_intake_teammate_reads_but_does_not_write_its_bundle() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let state_root = env.state_root.clone().unwrap();
    let r = Roster::builtin().unwrap();
    for name in ROLES.iter().map(|r| r.name).chain(["skill-importer"]) {
        let t = r.require(name).unwrap();
        let bundle = horch_core::skills::Bundle::install(&state_root, t)
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: no skills bundle"));
        assert!(!bundle.plan().activated.is_empty(), "{name}: no skills");
        let cmd = launch::command_with_skills_in(
            &env,
            t,
            Session::Unmanaged,
            "PROMPT",
            None,
            Some(&bundle),
        )
        .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let i = args.iter().position(|a| a == "--settings").unwrap();
        let settings: Value = serde_json::from_str(&args[i + 1]).unwrap();
        let dir = bundle.root().to_string_lossy().into_owned();
        let fs = &settings["sandbox"]["filesystem"];
        assert!(strings(&fs["allowRead"]).contains(&dir), "{name}");
        assert!(!strings(&fs["allowWrite"]).contains(&dir), "{name}");
        let denied = strings(&fs["denyWrite"]);
        assert_eq!(denied.last(), Some(&dir), "{name}");
        let perms = &settings["permissions"];
        assert_eq!(perms["blockReadsOutsideWorkingDirectories"], true, "{name}");
        // Before the bundle: the teammate's own sandbox paths (T-31).
        let dirs = strings(&perms["additionalDirectories"]);
        assert_eq!(dirs.last(), Some(&dir), "{name}");
        let deny = strings(&perms["deny"]);
        for tool in ["Edit", "Write"] {
            assert!(
                deny.contains(&format!("{tool}(/{dir}/**)")),
                "{name}: {deny:?}"
            );
        }
        let root = state_root.to_string_lossy().into_owned();
        let opened = strings(&fs["allowRead"]).contains(&root);
        assert_eq!(opened, t.horch_access == HorchAccess::Full, "{name}");
    }
}

/// T-28: each full sandboxed teammate (skill-importer, app-release-preparer)
/// launches with the state root's whole `skill-bundles` folder closed to
/// write for every tool (`denyWrite` for Bash, deny rules for Edit and
/// Write), with or without a bundle of its own. The state root stays open
/// to write, so its `horch` commands work. A `report` role gets no such
/// path: it has no state root.
#[test]
fn t28_a_full_sandboxed_teammate_writes_no_skills_bundle() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let state_root = env.state_root.clone().unwrap();
    let root = state_root.to_string_lossy().into_owned();
    let bundles = state_root
        .join("skill-bundles")
        .to_string_lossy()
        .into_owned();
    let r = Roster::builtin().unwrap();
    for name in ["skill-importer", "app-release-preparer"] {
        let t = r.require(name).unwrap();
        assert!(t.sandbox.is_some(), "{name}");
        assert_eq!(t.horch_access, HorchAccess::Full, "{name}");
        let bundle = horch_core::skills::Bundle::install(&state_root, t).unwrap();
        for own in [None, bundle.as_ref()] {
            let cmd =
                launch::command_with_skills_in(&env, t, Session::Unmanaged, "PROMPT", None, own)
                    .unwrap();
            let args: Vec<String> = cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let i = args.iter().position(|a| a == "--settings").unwrap();
            let settings: Value = serde_json::from_str(&args[i + 1]).unwrap();
            let fs = &settings["sandbox"]["filesystem"];
            assert!(strings(&fs["allowWrite"]).contains(&root), "{name}");
            assert!(strings(&fs["denyWrite"]).contains(&bundles), "{name}");
            let deny = strings(&settings["permissions"]["deny"]);
            for tool in ["Edit", "Write"] {
                assert!(
                    deny.contains(&format!("{tool}(/{bundles}/**)")),
                    "{name}: {deny:?}"
                );
            }
        }
    }
    for name in ROLES.iter().map(|r| r.name) {
        let text = launched_settings(&env, r.require(name).unwrap()).to_string();
        assert!(!text.contains(&bundles), "{name}: {text}");
    }
}

/// T-31: each intake teammate's own `allowRead` and `allowWrite` paths, with
/// `~` expanded, are working directories, so a Bash command that names one
/// does not stop at a permission prompt. No other path is one: the Read tool
/// reaches no more than Bash. An `allowRead`-only path gets an `Edit(...)`
/// deny rule. skill-importer keeps its quarantine Read deny.
#[test]
fn t31_an_intake_teammate_works_in_its_own_sandbox_paths() {
    let home = tempfile::tempdir().unwrap();
    let env = launch_env(home.path());
    let expand = |p: &str| match p.strip_prefix("~/") {
        Some(rest) => home.path().join(rest).to_string_lossy().into_owned(),
        None => p.to_string(),
    };
    let r = Roster::builtin().unwrap();
    for name in ROLES.iter().map(|r| r.name).chain(["skill-importer"]) {
        let t = r.require(name).unwrap();
        let own = sandbox(t);
        let reads: Vec<String> = strings(&own["filesystem"]["allowRead"])
            .iter()
            .map(|p| expand(p))
            .collect();
        let writes: Vec<String> = strings(&own["filesystem"]["allowWrite"])
            .iter()
            .map(|p| expand(p))
            .collect();
        let settings = launched_settings(&env, t);
        let perms = &settings["permissions"];
        let dirs = strings(&perms["additionalDirectories"]);
        for dir in reads.iter().chain(&writes) {
            assert!(dirs.contains(dir), "{name}: {dir} not in {dirs:?}");
        }
        for dir in &dirs {
            assert!(
                reads.contains(dir) || writes.contains(dir),
                "{name}: {dir} is not its own sandbox path"
            );
        }
        // No `deny` list when every path is also in `allowWrite`.
        let deny = perms.get("deny").map(strings).unwrap_or_default();
        for dir in reads.iter().filter(|d| !writes.contains(d)) {
            assert!(
                deny.contains(&format!("Edit(/{dir}/**)")),
                "{name}: {deny:?}"
            );
        }
    }
    let importer = r.require("skill-importer").unwrap();
    let workspace = expand(B);
    let dirs = strings(&launched_settings(&env, importer)["permissions"]["additionalDirectories"]);
    assert!(dirs.contains(&workspace), "{dirs:?}");
    let cmd =
        launch::command_with_skills_in(&env, importer, Session::Unmanaged, "PROMPT", None, None)
            .unwrap();
    let args: Vec<String> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let i = args
        .iter()
        .position(|a| a == "--disallowedTools")
        .expect("--disallowedTools");
    let quarantine = format!("Read({B}/contaminated/*/quarantine/**)");
    assert!(args[i + 1].contains(&quarantine), "{}", args[i + 1]);
}
