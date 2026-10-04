//! Harness trust in a repository root (PRE-14). Claude, Codex and
//! Antigravity show a trust dialog on their first launch in a repository;
//! a candidate pane stopped at it does no work. horch reads the stores and
//! never writes them: the operator accepts trust.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::HarnessKind;

/// What a harness has recorded about the repository root. A harness that
/// has not trusted it stops a new candidate pane at a trust dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    Trusted,
    Untrusted,
    /// horch cannot read the harness's decision.
    Unknown,
}

/// One harness's trust in the repository root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessTrust {
    /// [`HarnessKind::as_str`].
    pub harness: String,
    pub state: TrustState,
    /// Why, naming the store file. Never any content of the file: it can
    /// hold tokens.
    pub reason: String,
}

impl HarnessTrust {
    fn new(harness: HarnessKind, state: TrustState, reason: impl Into<String>) -> Self {
        HarnessTrust {
            harness: harness.as_str().to_string(),
            state,
            reason: reason.into(),
        }
    }
}

/// Whether `harness` shows a trust dialog on its first launch in a
/// repository. opencode, pi and prime have no trust step.
pub fn asks_for_trust(harness: HarnessKind) -> bool {
    matches!(
        harness,
        HarnessKind::Claude | HarnessKind::Codex | HarnessKind::Antigravity
    )
}

/// The store file names the reasons use. Claude keeps `.claude.json` in
/// `$CLAUDE_CONFIG_DIR` when it is set, else in the home directory (see
/// [`claude_config_file`]).
pub const CLAUDE_TRUST_FILE: &str = "$CLAUDE_CONFIG_DIR/.claude.json";
pub const CODEX_TRUST_FILE: &str = "$CODEX_HOME/config.toml";

/// The file Claude keeps its global config and trust decisions in:
/// `$CLAUDE_CONFIG_DIR/.claude.json` when the variable is set, else
/// `<home>/.claude.json`. Seen on claude 2.1.289: with the variable set, it
/// writes `.claude.json` in that directory and leaves the home one alone.
pub fn claude_config_file(home: &Path, claude_config_dir: Option<&Path>) -> PathBuf {
    claude_config_dir.unwrap_or(home).join(".claude.json")
}

/// Claude's trust in `roots` (the root and its canonical form): the key
/// `projects["<root>"].hasTrustDialogAccepted` is `true` in
/// [`claude_config_file`]. `file` is the file text, `None` when it does not exist.
/// A parent folder's entry does not count: Claude keys trust on the
/// repository root.
pub fn claude_trust(file: Option<&str>, roots: &[PathBuf]) -> HarnessTrust {
    let claude = HarnessKind::Claude;
    let Some(text) = file else {
        return HarnessTrust::new(
            claude,
            TrustState::Untrusted,
            format!("{CLAUDE_TRUST_FILE} does not exist"),
        );
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return HarnessTrust::new(
            claude,
            TrustState::Unknown,
            format!("{CLAUDE_TRUST_FILE} is not valid JSON"),
        );
    };
    let accepted = roots.iter().any(|root| {
        doc.get("projects")
            .and_then(|p| p.get(root.to_string_lossy().as_ref()))
            .and_then(|e| e.get("hasTrustDialogAccepted"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    });
    if accepted {
        HarnessTrust::new(
            claude,
            TrustState::Trusted,
            format!("{CLAUDE_TRUST_FILE} trusts the repository root"),
        )
    } else {
        HarnessTrust::new(
            claude,
            TrustState::Untrusted,
            format!("{CLAUDE_TRUST_FILE} has no accepted trust for the repository root"),
        )
    }
}

/// Codex's trust in `roots`: a table `[projects."<root>"]` with
/// `trust_level = "trusted"` in `$CODEX_HOME/config.toml`. `file` is the file
/// text, `None` when it does not exist.
pub fn codex_trust(file: Option<&str>, roots: &[PathBuf]) -> HarnessTrust {
    let codex = HarnessKind::Codex;
    let Some(text) = file else {
        return HarnessTrust::new(
            codex,
            TrustState::Untrusted,
            format!("{CODEX_TRUST_FILE} does not exist"),
        );
    };
    let roots: Vec<String> = roots
        .iter()
        .map(|r| r.to_string_lossy().into_owned())
        .collect();
    let mut in_root = false;
    let mut trusted = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_root = toml_project_header(line).is_some_and(|p| roots.contains(&p));
        } else if in_root {
            if let Some(level) = toml_string_value(line, "trust_level") {
                trusted = level == "trusted";
            }
        }
    }
    if trusted {
        HarnessTrust::new(
            codex,
            TrustState::Trusted,
            format!("{CODEX_TRUST_FILE} trusts the repository root"),
        )
    } else {
        HarnessTrust::new(
            codex,
            TrustState::Untrusted,
            format!("{CODEX_TRUST_FILE} has no trust_level = \"trusted\" for the repository root"),
        )
    }
}

/// The path of a `[projects."<path>"]` (or `[projects.'<path>']`) header.
fn toml_project_header(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.trim_start();
    let rest = inner
        .strip_prefix("projects")?
        .trim_start()
        .strip_prefix('.')?;
    let rest = rest.trim_start();
    let (key, after) = toml_quoted(rest)?;
    (after.trim() == "]").then_some(key)
}

/// `"<value>"` or `'<value>'` for `key = ...`, `None` for any other line.
fn toml_string_value(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?;
    toml_quoted(rest.trim_start()).map(|(value, _)| value)
}

/// A TOML basic (`"..."`, with `\\` and `\"` escapes) or literal (`'...'`)
/// string at the start of `text`, and the text after it.
fn toml_quoted(text: &str) -> Option<(String, &str)> {
    let mut chars = text.char_indices();
    let (_, quote) = chars.next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let mut out = String::new();
    let mut escaped = false;
    for (i, c) in chars {
        if quote == '"' && escaped {
            out.push(c);
            escaped = false;
        } else if quote == '"' && c == '\\' {
            escaped = true;
        } else if c == quote {
            return Some((out, &text[i + c.len_utf8()..]));
        } else {
            out.push(c);
        }
    }
    None
}

/// The one-time command that records `harness`'s trust in `root`. horch
/// never writes a trust store: the operator accepts trust.
pub fn trust_fix(harness: HarnessKind, root: &Path) -> String {
    let quoted = format!("'{}'", root.to_string_lossy().replace('\'', "'\\''"));
    match harness {
        HarnessKind::Claude => {
            format!("cd {quoted} && claude  (choose \"Yes, I trust this folder\", then exit)")
        }
        HarnessKind::Codex => {
            format!("cd {quoted} && codex  (choose \"Trust and continue\", then exit)")
        }
        HarnessKind::Antigravity => {
            format!("cd {quoted} && agy  (accept the trust question, then exit)")
        }
        other => format!(
            "cd {quoted} && {}  (accept the trust question, then exit)",
            other.as_str()
        ),
    }
}

/// What each harness in `harnesses` has recorded for `roots` (the
/// repository root and its canonical form). It reads the stores and keeps
/// only the verdict: the files can hold tokens, so nothing else of them
/// leaves this function. `home` is the home directory; the two directories
/// are `$CLAUDE_CONFIG_DIR` and `$CODEX_HOME` when they are set.
pub fn read_trust(
    harnesses: &[HarnessKind],
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
    roots: &[PathBuf],
) -> Vec<HarnessTrust> {
    harnesses
        .iter()
        .map(|&harness| match harness {
            HarnessKind::Claude => match read_store(&claude_config_file(home, claude_config_dir)) {
                Ok(text) => claude_trust(text.as_deref(), roots),
                Err(()) => unreadable(harness, CLAUDE_TRUST_FILE),
            },
            HarnessKind::Codex => {
                let file = super::codex::codex_home(home, codex_home_dir).join("config.toml");
                match read_store(&file) {
                    Ok(text) => codex_trust(text.as_deref(), roots),
                    Err(()) => unreadable(harness, CODEX_TRUST_FILE),
                }
            }
            other => HarnessTrust::new(
                other,
                TrustState::Unknown,
                format!("horch cannot read where {} keeps its trust", other.as_str()),
            ),
        })
        .collect()
}

/// The text of a trust store, `None` when it does not exist, `Err` when it
/// cannot be read.
fn read_store(path: &Path) -> Result<Option<String>, ()> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(()),
    }
}

fn unreadable(harness: HarnessKind, file: &str) -> HarnessTrust {
    HarnessTrust::new(
        harness,
        TrustState::Unknown,
        format!("{file} cannot be read"),
    )
}
