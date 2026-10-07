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

// ─── u3 review findings (ai_docs/plans/wave2/w1/u3-review.md) ───────────────

fn shipped(rel: &str) -> String {
    std::fs::read_to_string(teammates_dir().join(rel)).unwrap()
}

/// The shipped `_base/context-windows.md` with `extra` lines under `windows:`.
fn windows_with(extra: &str) -> String {
    shipped("_base/context-windows.md").replace("windows:\n", &format!("windows:\n{extra}"))
}

/// The shipped `_base/context-messages.md` with message `key` set to `value`.
fn messages_with(key: &str, value: &str) -> String {
    let text = shipped("_base/context-messages.md");
    let old = text
        .lines()
        .find(|l| l.starts_with(&format!("  {key}: ")))
        .unwrap()
        .to_string();
    text.replace(&old, &format!("  {key}: {value}"))
}

fn all_vars() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        ("role", "sonnet-1"),
        ("tokens", "311225"),
        ("threshold", "300000"),
        ("handoff", "ai_docs/handoffs/sonnet-1-whats-next.md"),
        ("pre", "311225"),
        ("post", "20000"),
        ("step", "send"),
        ("reason", "pane closed"),
        ("log", "ai_docs/x.log"),
    ])
}

fn warnings(roster: &Roster) -> Vec<String> {
    horch_core::roster::validation::context_policy_warnings(roster)
}

/// Finding 1 (R5, R4): every `windows` key is held to the floor by itself,
/// also a `<fallback harness>/<teammate>` key and a model no teammate runs.
#[test]
fn ctx_08_check_holds_every_windows_key_to_the_floor() {
    let dir = roster_dir(&[(
        "_base/context-windows.md",
        windows_with("  claude/codex-sol: 50000\n  claude/haiku: 100000\n"),
    )]);
    let roster = load(dir.path());
    assert_eq!(
        problems_with(&roster, &["headroom"]),
        vec![
            "_base/context-windows.md: windows claude/codex-sol 50000 leaves headroom 3400 tokens, \
             below the 20000 floor (native trigger 17000, threshold 13600)"
                .to_string(),
            "_base/context-windows.md: windows claude/haiku 100000 leaves headroom 13400 tokens, \
             below the 20000 floor (native trigger 67000, threshold 53600)"
                .to_string(),
        ]
    );
}

/// Finding 2a (R1): a blank message fails the check.
#[test]
fn check_refuses_a_blank_context_message() {
    for (key, value) in [("failed", "\"\""), ("resume", "\"   \"")] {
        let dir = roster_dir(&[("_base/context-messages.md", messages_with(key, value))]);
        assert_eq!(
            problems_with(&load(dir.path()), &[]),
            vec![format!("_base/context-messages.md: message {key} is empty")],
            "{key}"
        );
    }
}

/// Finding 2b (R2): a copy of the messages base that differs from the
/// built-in one is a warning, not a problem. The shipped file is quiet.
#[test]
fn context_policy_warnings_name_a_copied_messages_base() {
    let text = messages_with(
        "failed",
        "\"Compaction of {role} failed at step {step}: {reason}. Log: {log}.\"",
    );
    let dir = roster_dir(&[("_base/context-messages.md", text)]);
    let roster = load(dir.path());
    assert!(
        problems_with(&roster, &[]).is_empty(),
        "{:?}",
        roster.check()
    );
    let path = dir.path().join("_base/context-messages.md");
    assert_eq!(
        warnings(&roster),
        vec![format!(
            "_base/context-messages.md: {} replaces the built-in protocol messages; the \
             briefings name their prefixes, so delete the copy",
            path.display()
        )]
    );

    assert!(warnings(&builtin()).is_empty());
    assert!(warnings(&load(&teammates_dir())).is_empty());
}

/// Finding 3 (R6): a windows key that is both a teammate (on another model)
/// and a model fails the check.
#[test]
fn check_refuses_a_windows_key_with_two_readings() {
    let opus = shipped("opus.md").replace("\nmodel: opus\n", "\nmodel: sonnet\n");
    assert!(opus.contains("\nmodel: sonnet\n"));
    let dir = roster_dir(&[("opus.md", opus)]);
    assert_eq!(
        problems_with(&load(dir.path()), &["windows key"]),
        vec![
            "_base/context-windows.md: windows key claude/opus is teammate opus (model sonnet) \
             and model opus; set compact_window in opus.md instead"
                .to_string()
        ]
    );
    assert!(problems_with(&builtin(), &["windows key"]).is_empty());
}

/// Finding 4 (R8): a key on a harness with no window setting, or with an
/// empty name, fails; a key that names no teammate and no model warns.
#[test]
fn check_refuses_dead_windows_keys() {
    let dir = roster_dir(&[(
        "_base/context-windows.md",
        windows_with(
            "  claude/sonet: 150000\n  opencode/opencode/big-pickle: 100000\n  antigravity/x: 1\n  claude/: 5\n",
        ),
    )]);
    let roster = load(dir.path());
    assert_eq!(
        problems_with(&roster, &["windows key"]),
        vec![
            "_base/context-windows.md: windows key antigravity/x does nothing on harness \
             antigravity: it has no window setting"
                .to_string(),
            "_base/context-windows.md: windows key claude/ has no teammate or model name"
                .to_string(),
            "_base/context-windows.md: windows key opencode/opencode/big-pickle does nothing on \
             harness opencode: it has no window setting"
                .to_string(),
        ]
    );
    assert!(problems_with(&roster, &["sonet"]).is_empty());
    assert_eq!(
        warnings(&roster),
        vec![
            "_base/context-windows.md: windows key claude/sonet names no teammate and no model \
             of a teammate on claude"
                .to_string()
        ]
    );
    assert!(warnings(&builtin()).is_empty());
}

/// Finding 5 (R7): `in_place` refuses a harness without a compact command,
/// and the accessor never routes it in place.
#[test]
fn check_refuses_in_place_without_a_compact_command() {
    let text = shipped("_base/context-windows.md").replace(
        "in_place: [claude, codex]",
        "in_place: [claude, codex, antigravity, none]",
    );
    let dir = roster_dir(&[("_base/context-windows.md", text)]);
    let roster = load(dir.path());
    assert_eq!(
        problems_with(&roster, &["in_place"]),
        vec![
            "_base/context-windows.md: in_place entry antigravity: horch has no compact command \
             for antigravity"
                .to_string(),
            "_base/context-windows.md: in_place entry none: horch has no compact command for none"
                .to_string(),
        ]
    );
    assert!(roster.compacts_in_place(HarnessKind::Claude));
    for kind in [HarnessKind::Antigravity, HarnessKind::None] {
        assert!(!roster.compacts_in_place(kind), "{kind:?}");
    }
}

/// Finding 6 (R9): a newline in a value never reaches the rendered line.
#[test]
fn context_message_renders_one_line_whatever_the_values() {
    let mut vars = all_vars();
    vars.insert("reason", "exit 1\r\nstderr: boom");
    assert_eq!(
        context_message(&builtin(), "failed", &vars).unwrap(),
        "[horch] BLOCKED: Compaction of sonnet-1 failed at step send: exit 1  stderr: boom. \
         Log: ai_docs/x.log."
    );
}

/// Finding 7 (R3): a windows base that leaves out a key fails the check; an
/// explicit empty value stays valid.
#[test]
fn check_refuses_a_windows_base_without_both_keys() {
    let dir = roster_dir(&[(
        "_base/context-windows.md",
        "---\nname: context-windows\nin_place: [claude]\n---\nbody\n".into(),
    )]);
    assert_eq!(
        problems_with(&load(dir.path()), &[]),
        vec![
            "_base/context-windows.md sets no windows; a copy replaces the whole file, so keep \
             both keys"
                .to_string()
        ]
    );
    let dir = roster_dir(&[(
        "_base/context-windows.md",
        "---\nname: context-windows\nwindows:\n  claude/opus: 200000\n---\nbody\n".into(),
    )]);
    assert_eq!(
        problems_with(&load(dir.path()), &[]),
        vec![
            "_base/context-windows.md sets no in_place; a copy replaces the whole file, so keep \
             both keys"
                .to_string()
        ]
    );
    let dir = roster_dir(&[(
        "_base/context-windows.md",
        "---\nname: context-windows\nwindows: {}\nin_place: []\n---\nbody\n".into(),
    )]);
    assert!(problems_with(&load(dir.path()), &[]).is_empty());
}

/// Finding 8 (R10, R15): rule 2 reads a settings file and every window key.
#[test]
fn check_refuses_every_window_key_in_env_and_a_settings_file() {
    let settings_dir = tempfile::tempdir().unwrap();
    let settings = settings_dir.path().join("win.json");
    std::fs::write(
        &settings,
        r#"{"statusLine": {"type": "command", "command": "true"}, "env": {"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "100000"}}"#,
    )
    .unwrap();
    let dir = roster_dir(&[
        (
            "pct.md",
            teammate_file(
                "pct",
                "claude",
                "env:\n  CLAUDE_AUTOCOMPACT_PCT_OVERRIDE: \"50\"\n",
            ),
        ),
        (
            "winfile.md",
            teammate_file(
                "winfile",
                "claude",
                &format!("settings: {}\n", settings.display()),
            ),
        ),
        (
            "inline.md",
            teammate_file(
                "inline",
                "claude",
                "settings: '{\"statusLine\": {\"type\": \"command\", \"command\": \"true\"}, \"autoCompactWindow\": 100000}'\n",
            ),
        ),
    ]);
    let roster = load(dir.path());
    assert_eq!(
        problems_with(&roster, &["set the compaction window"]),
        vec![
            "inline: set the compaction window with compact_window, not settings autoCompactWindow"
                .to_string(),
            "pct: set the compaction window with compact_window, not env \
             CLAUDE_AUTOCOMPACT_PCT_OVERRIDE"
                .to_string(),
            "winfile: set the compaction window with compact_window, not settings \
             env.CLAUDE_CODE_AUTO_COMPACT_WINDOW"
                .to_string(),
        ]
    );
}

/// Nit 9 (R14): after a substitution the detail names the fallback's file.
#[test]
fn fleet_window_names_the_fallback_after_a_substitution() {
    let opus =
        shipped("opus.md").replace("\nmodel: opus\n", "\nmodel: opus\ncompact_window: 180000\n");
    let dir = roster_dir(&[("opus.md", opus)]);
    let roster = load(dir.path());
    let original = roster.get("codex-sol").unwrap();
    assert_eq!(original.compact_window, None);
    let merged = horch_core::routing::decision::merge(original, roster.get("opus").unwrap());
    assert_eq!(
        roster.fleet_window(&merged, "opus"),
        Some((
            180_000,
            "teammate opus compact_window (fallback of codex-sol)".into()
        ))
    );
    assert_eq!(
        roster.fleet_window(roster.get("opus").unwrap(), "opus"),
        Some((180_000, "teammate opus compact_window".into()))
    );
}

/// Inline settings JSON (outside the review): a JSON object passes the
/// settings check, a broken one fails, a missing file still fails.
#[test]
fn check_accepts_inline_settings_json() {
    let dir = roster_dir(&[
        (
            "good.md",
            teammate_file(
                "good",
                "claude",
                "settings: '{\"statusLine\": {\"type\": \"command\", \"command\": \"true\"}}'\n",
            ),
        ),
        (
            "nostatus.md",
            teammate_file("nostatus", "claude", "settings: '{\"env\": {}}'\n"),
        ),
        (
            "broken.md",
            teammate_file("broken", "claude", "settings: '{\"env\": '\n"),
        ),
        (
            "nofile.md",
            teammate_file("nofile", "claude", "settings: /no/such/settings.json\n"),
        ),
    ]);
    let roster = load(dir.path());
    assert!(
        problems_with(&roster, &["good:"]).is_empty(),
        "{:?}",
        roster.check()
    );
    assert_eq!(
        problems_with(&roster, &["nostatus:"]),
        vec!["nostatus: inline settings define no statusLine, so this pane would be the only one in the fleet without one".to_string()]
    );
    assert_eq!(
        problems_with(&roster, &["broken:"]),
        vec!["broken: inline settings are not a JSON object".to_string()]
    );
    assert_eq!(
        problems_with(&roster, &["nofile:"]),
        vec![
            "nofile: settings file '/no/such/settings.json' is missing or not valid JSON"
                .to_string()
        ]
    );
}

/// Q1 (R13): `horch fleet sol` runs orchestrator-codex on gpt-5.6-sol, and
/// it takes that model's window; Codex has no orchestrator key.
#[test]
fn sol_orchestrator_takes_its_model_window() {
    let roster = builtin();
    let mut orchestrator = roster.get("orchestrator-codex").unwrap().clone();
    orchestrator.model = Some("gpt-5.6-sol".into());
    assert_eq!(
        roster.fleet_window(&orchestrator, "gpt-5.6-sol"),
        Some((200_000, "context-policy windows codex/gpt-5.6-sol".into()))
    );
}

/// CTX-21: `compact_at` is the teammate's watch base. The check refuses a
/// value outside 50,000 to 1,000,000 and names the range.
#[test]
fn ctx_21_compact_at_range_checked() {
    let dir = roster_dir(&[(
        "low.md",
        teammate_file("low", "claude", "compact_at: 40000\n"),
    )]);
    let errors = problems_with(&load(dir.path()), &["compact_at"]);
    assert_eq!(
        errors,
        vec!["low: compact_at 40000 is outside 50000 to 1000000".to_string()]
    );

    let dir = roster_dir(&[(
        "high.md",
        teammate_file("high", "claude", "compact_at: 1000001\n"),
    )]);
    let errors = problems_with(&load(dir.path()), &["compact_at"]);
    assert_eq!(
        errors,
        vec!["high: compact_at 1000001 is outside 50000 to 1000000".to_string()]
    );

    for ok in [50_000, 60_000, 1_000_000] {
        let dir = roster_dir(&[(
            "fine.md",
            teammate_file("fine", "claude", &format!("compact_at: {ok}\n")),
        )]);
        let roster = load(dir.path());
        assert_eq!(roster.get("fine").unwrap().compact_at, Some(ok));
        let errors = problems_with(&roster, &["compact_at"]);
        assert!(errors.is_empty(), "{ok}: {errors:?}");
    }
}
