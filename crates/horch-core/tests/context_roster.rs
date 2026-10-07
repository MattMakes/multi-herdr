//! Context-policy data in the roster: the 2 bases `_base/context-windows.md`
//! and `_base/context-messages.md`, the teammate `compact_window`, and the
//! roster check rules of design `ai_docs/plans/wave2/w1/design.md` section
//! 6.3 (CTX-08, CTX-17). No harness and no herdr: each test loads a roster.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use horch_core::harness::HarnessKind;
use horch_core::prompts::context_message;
use horch_core::roster::{Roster, Teammate};

const MESSAGE_KEYS: [&str; 8] = [
    "request",
    "warning",
    "warning-orchestrator",
    "instructions",
    "instructions-orchestrator",
    "resume",
    "reported",
    "failed",
];

fn teammates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates")
}

/// The shipped file `rel`, with `extra` frontmatter lines added after the
/// opening `---`.
fn shipped_with(rel: &str, extra: &str) -> String {
    let text = std::fs::read_to_string(teammates_dir().join(rel)).unwrap();
    let rest = text.strip_prefix("---\n").expect("frontmatter");
    format!("---\n{extra}{rest}")
}

/// A teammates dir holding `files` (relative paths, `_base/` included).
fn roster_dir(files: &[(&str, String)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("_base")).unwrap();
    for (rel, text) in files {
        std::fs::write(dir.path().join(rel), text).unwrap();
    }
    dir
}

fn load(dir: &Path) -> Roster {
    Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap()
}

fn builtin() -> Roster {
    Roster::builtin().unwrap()
}

fn teammate_file(name: &str, agent: &str, extra: &str) -> String {
    format!(
        "---\nname: {name}\nbrief_description: test teammate\nhidden: true\nbase: fleet-worker\nagent: {agent}\nmodel: m\n{extra}---\nbody\n"
    )
}

/// The problems of `roster` that contain every one of `parts`.
fn problems_with(roster: &Roster, parts: &[&str]) -> Vec<String> {
    roster
        .check()
        .into_iter()
        .filter(|p| parts.iter().all(|part| p.contains(part)))
        .collect()
}

/// A `context-messages` base with every key in `keys`, each a valid line,
/// plus `extra` frontmatter lines.
fn messages_base(keys: &[&str], extra: &str) -> String {
    let lines: String = keys
        .iter()
        .map(|k| format!("  {k}: \"NOTE: {{role}} {k}.\"\n"))
        .collect();
    format!("---\nname: context-messages\n{extra}messages:\n{lines}---\nbody\n")
}

#[test]
fn ctx_08_roster_check_fails_headroom_below_floor() {
    let dir = roster_dir(&[(
        "sonnet.md",
        shipped_with("sonnet.md", "compact_window: 100000\n"),
    )]);
    let roster = load(dir.path());
    let errors = problems_with(&roster, &["headroom"]);
    assert_eq!(errors.len(), 1, "{errors:?}");
    for part in ["sonnet", "13400", "20000"] {
        assert!(errors[0].contains(part), "{part} missing: {}", errors[0]);
    }
    assert_eq!(
        errors[0],
        "sonnet: compact window 100000 leaves headroom 13400 tokens, below the 20000 floor \
         (native trigger 67000, threshold 53600)"
    );

    let dir = roster_dir(&[(
        "sonnet.md",
        shipped_with("sonnet.md", "compact_window: 150000\n"),
    )]);
    let errors = problems_with(&load(dir.path()), &["headroom"]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn ctx_08_builtin_roster_passes_the_floor() {
    let errors = problems_with(&builtin(), &["headroom"]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn ctx_17_in_place_only_for_listed_harnesses() {
    let roster = builtin();
    for kind in [HarnessKind::Claude, HarnessKind::Codex] {
        assert!(roster.compacts_in_place(kind), "{kind:?}");
    }
    let fresh = [
        HarnessKind::Pi,
        HarnessKind::Prime,
        HarnessKind::OpenCode,
        HarnessKind::Antigravity,
        HarnessKind::None,
    ];
    for kind in fresh {
        assert!(!roster.compacts_in_place(kind), "{kind:?}");
    }

    // Finding 9: an overlay of the windows file freezes no message.
    let dir = roster_dir(&[(
        "_base/context-windows.md",
        "---\nname: context-windows\nwindows:\n  claude/opus: 200000\nin_place: []\n---\nbody\n"
            .into(),
    )]);
    let overlaid = load(dir.path());
    for kind in HarnessKind::ALL {
        assert!(!overlaid.compacts_in_place(*kind), "{kind:?}");
    }
    let vars = BTreeMap::from([
        ("role", "sonnet-1"),
        ("tokens", "311225"),
        ("threshold", "300000"),
        ("handoff", "ai_docs/handoffs/sonnet-1-whats-next.md"),
    ]);
    assert_eq!(
        context_message(&overlaid, "request", &vars).unwrap(),
        context_message(&roster, "request", &vars).unwrap()
    );
    assert!(context_message(&overlaid, "request", &vars)
        .unwrap()
        .starts_with("NOTE: Your context is 311225 tokens."));
}

#[test]
fn fleet_window_takes_teammate_then_model_key() {
    let roster = builtin();
    let window = |name: &str, model: &str| roster.fleet_window(roster.get(name).unwrap(), model);
    assert_eq!(
        window("orchestrator", "opus"),
        Some((300_000, "context-policy windows claude/orchestrator".into()))
    );
    assert_eq!(
        window("opus", "opus"),
        Some((200_000, "context-policy windows claude/opus".into()))
    );
    assert_eq!(
        window("sonnet-bugfix", "sonnet"),
        Some((150_000, "context-policy windows claude/sonnet".into()))
    );
    assert_eq!(
        window("codex-sol", "gpt-5.6-sol"),
        Some((200_000, "context-policy windows codex/gpt-5.6-sol".into()))
    );
    assert_eq!(window("codex-luna", "gpt-5.6-luna"), None);

    let own = Teammate {
        name: "wide".into(),
        compact_window: Some(180_000),
        ..Teammate::default()
    };
    assert_eq!(
        roster.fleet_window(&own, "sonnet"),
        Some((180_000, "teammate wide compact_window".into()))
    );
}

#[test]
fn compact_window_parses_and_defaults_to_none() {
    let dir = roster_dir(&[(
        "sonnet.md",
        shipped_with("sonnet.md", "compact_window: 150000\n"),
    )]);
    assert_eq!(
        load(dir.path()).get("sonnet").unwrap().compact_window,
        Some(150_000)
    );
    assert_eq!(builtin().get("sonnet").unwrap().compact_window, None);
}

#[test]
fn context_message_renders_the_warning_line() {
    let vars = BTreeMap::from([
        ("role", "sonnet-1"),
        ("tokens", "311225"),
        ("threshold", "300000"),
        ("handoff", "ai_docs/handoffs/sonnet-1-whats-next.md"),
    ]);
    assert_eq!(
        context_message(&builtin(), "warning", &vars).unwrap(),
        "NOTE: Context warning. Your context is 311225 tokens. Your threshold is 300000 tokens. \
         This horch note call is your stopping point. Do not start new work. Write \
         ai_docs/handoffs/sonnet-1-whats-next.md with the horch:handoff skill. Then run: horch \
         note \"handoff: ai_docs/handoffs/sonnet-1-whats-next.md\". Then send: horch tell \
         orchestrator \"[sonnet-1] NOTE: COMPACT-READY ai_docs/handoffs/sonnet-1-whats-next.md\". \
         Then end your turn and wait."
    );
}

#[test]
fn context_message_fails_on_an_unknown_key() {
    let err = context_message(&builtin(), "nope", &BTreeMap::new()).unwrap_err();
    assert!(err.to_string().contains("nope"), "{err:#}");
}

#[test]
fn builtin_messages_have_every_key_on_one_line() {
    let roster = builtin();
    let vars: BTreeMap<&str, &str> = [
        "role",
        "tokens",
        "threshold",
        "handoff",
        "pre",
        "post",
        "step",
        "reason",
        "log",
    ]
    .into_iter()
    .map(|k| (k, "x"))
    .collect();
    for key in MESSAGE_KEYS {
        let line = context_message(&roster, key, &vars).unwrap();
        assert!(!line.contains('\n'), "{key}: {line}");
    }
    let orchestrator = context_message(&roster, "instructions-orchestrator", &vars).unwrap();
    assert!(orchestrator.contains("every COMPACT-READY line you did not yet act on"));
    let errors = problems_with(&roster, &["context-"]);
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn check_refuses_a_window_set_outside_compact_window() {
    let dir = roster_dir(&[
        (
            "in-env.md",
            teammate_file(
                "in-env",
                "claude",
                "env:\n  CLAUDE_CODE_AUTO_COMPACT_WINDOW: \"200000\"\n",
            ),
        ),
        (
            "in-settings.md",
            teammate_file(
                "in-settings",
                "claude",
                "settings: '{\"env\": {\"CLAUDE_CODE_AUTO_COMPACT_WINDOW\": \"200000\"}}'\n",
            ),
        ),
        (
            "in-args.md",
            teammate_file(
                "in-args",
                "codex",
                "args: [\"-c\", \"model_auto_compact_token_limit=200000\"]\n",
            ),
        ),
    ]);
    let roster = load(dir.path());
    for name in ["in-env", "in-settings", "in-args"] {
        let errors = problems_with(
            &roster,
            &[
                &format!("{name}: "),
                "set the compaction window with compact_window, not",
            ],
        );
        assert_eq!(errors.len(), 1, "{name}: {errors:?}");
    }
}

#[test]
fn check_refuses_compact_window_on_a_harness_without_a_lever() {
    let dir = roster_dir(&[
        (
            "oc.md",
            teammate_file("oc", "opencode", "compact_window: 200000\n"),
        ),
        (
            "ag.md",
            teammate_file("ag", "antigravity", "compact_window: 200000\n"),
        ),
        (
            "cx.md",
            teammate_file("cx", "codex", "compact_window: 200000\n"),
        ),
    ]);
    let roster = load(dir.path());
    assert_eq!(
        problems_with(&roster, &["oc: compact_window"]).len(),
        1,
        "{:?}",
        roster.check()
    );
    assert_eq!(problems_with(&roster, &["ag: compact_window"]).len(), 1);
    assert!(problems_with(&roster, &["cx: compact_window"]).is_empty());
}

#[test]
fn check_holds_the_messages_base_to_its_keys_and_lines() {
    let valid = roster_dir(&[(
        "_base/context-messages.md",
        messages_base(&MESSAGE_KEYS, ""),
    )]);
    let errors = problems_with(&load(valid.path()), &["context-messages"]);
    assert!(errors.is_empty(), "{errors:?}");

    let newline = messages_base(&MESSAGE_KEYS, "").replace(
        "request: \"NOTE: {role} request.\"",
        "request: \"NOTE: {role}\\nrequest.\"",
    );
    let dir = roster_dir(&[("_base/context-messages.md", newline)]);
    assert_eq!(
        problems_with(
            &load(dir.path()),
            &["context-messages", "request", "newline"]
        )
        .len(),
        1
    );

    let unknown = messages_base(&MESSAGE_KEYS, "").replace(
        "resume: \"NOTE: {role} resume.\"",
        "resume: \"NOTE: {nope} resume.\"",
    );
    let dir = roster_dir(&[("_base/context-messages.md", unknown)]);
    assert_eq!(
        problems_with(&load(dir.path()), &["context-messages", "resume", "{nope}"]).len(),
        1
    );

    let dir = roster_dir(&[(
        "_base/context-messages.md",
        messages_base(&MESSAGE_KEYS[1..], ""),
    )]);
    assert_eq!(
        problems_with(
            &load(dir.path()),
            &["context-messages", "request", "missing"]
        )
        .len(),
        1
    );

    let dir = roster_dir(&[(
        "_base/context-messages.md",
        messages_base(&MESSAGE_KEYS, "in_place: [claude]\n"),
    )]);
    assert_eq!(
        problems_with(&load(dir.path()), &["context-messages", "in_place"]).len(),
        1
    );
}

#[test]
fn check_holds_the_windows_base_to_harness_names() {
    let dir = roster_dir(&[(
        "_base/context-windows.md",
        "---\nname: context-windows\nwindows:\n  foo/opus: 200000\n  claude/opus: 200000\nin_place: [claude, foo]\n---\nbody\n"
            .into(),
    )]);
    let roster = load(dir.path());
    assert_eq!(
        problems_with(&roster, &["context-windows", "foo/opus"]).len(),
        1,
        "{:?}",
        roster.check()
    );
    assert_eq!(
        problems_with(&roster, &["context-windows", "in_place", "foo"]).len(),
        1
    );

    let dir = roster_dir(&[(
        "_base/context-windows.md",
        "---\nname: context-windows\nin_place: [claude]\nmessages:\n  request: \"NOTE: x\"\n---\nbody\n"
            .into(),
    )]);
    assert_eq!(
        problems_with(&load(dir.path()), &["context-windows", "messages"]).len(),
        1
    );
}

#[test]
fn check_refuses_context_keys_in_another_base() {
    let dir = roster_dir(&[(
        "_base/harness-defaults.md",
        shipped_with("_base/harness-defaults.md", "in_place: [claude]\n"),
    )]);
    assert_eq!(
        problems_with(&load(dir.path()), &["harness-defaults", "in_place"]).len(),
        1
    );
}

#[test]
fn check_names_a_missing_context_base() {
    let errors = Roster::default().check();
    for base in ["context-windows", "context-messages"] {
        assert_eq!(
            errors
                .iter()
                .filter(|p| p.contains(base) && p.contains("missing"))
                .count(),
            1,
            "{base}: {errors:?}"
        );
    }
}
