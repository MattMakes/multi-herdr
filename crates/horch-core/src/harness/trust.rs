//! Harness trust in a folder (PRE-14, ARC-29). Claude, Codex and
//! Antigravity show a trust dialog on their first launch in a folder; a
//! pane stopped at it does no work. horch reads the stores, and before an
//! agent starts it writes the one missing entry for the agent's workdir
//! ([`ensure_trusted`]): atomic, every other key kept, never `/`, the home
//! directory or an ancestor of it, and never over an operator's explicit
//! Codex `trust_level = "untrusted"`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context as _;

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
pub const ANTIGRAVITY_TRUST_FILE: &str = "~/.gemini/antigravity-cli/settings.json";

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
    parse_store(claude, CLAUDE_TRUST_FILE, file).map_or_else(std::convert::identity, |doc| {
        let accepted = roots.iter().any(|root| {
            doc.get("projects")
                .zip(root.to_str())
                .and_then(|(p, root)| p.get(root))
                .and_then(|e| e.get("hasTrustDialogAccepted"))
                .and_then(serde_json::Value::as_bool)
                == Some(true)
        });
        trust_result(
            claude,
            CLAUDE_TRUST_FILE,
            accepted,
            "has no accepted trust for the repository root",
        )
    })
}

/// A JSON store's document, or the verdict for a store that does not exist
/// or is not JSON.
fn parse_store(
    harness: HarnessKind,
    label: &str,
    file: Option<&str>,
) -> Result<serde_json::Value, HarnessTrust> {
    let Some(text) = file else {
        return Err(HarnessTrust::new(
            harness,
            TrustState::Untrusted,
            format!("{label} does not exist"),
        ));
    };
    serde_json::from_str(text).map_err(|_| {
        HarnessTrust::new(
            harness,
            TrustState::Unknown,
            format!("{label} is not valid JSON"),
        )
    })
}

/// The verdict of a store that parsed: `trusts the repository root`, else
/// the `missing` text after the store's name.
fn trust_result(harness: HarnessKind, label: &str, trusted: bool, missing: &str) -> HarnessTrust {
    if trusted {
        HarnessTrust::new(
            harness,
            TrustState::Trusted,
            format!("{label} trusts the repository root"),
        )
    } else {
        HarnessTrust::new(harness, TrustState::Untrusted, format!("{label} {missing}"))
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
    let roots: Vec<&str> = roots.iter().filter_map(|r| r.to_str()).collect();
    let mut in_root = false;
    let mut trusted = false;
    for line in toml_lines(text) {
        if line.starts_with('[') {
            in_root = toml_project_header(line).is_some_and(|p| roots.contains(&p.as_str()));
        } else if in_root {
            if let Some(level) = toml_string_value(line, "trust_level") {
                trusted = level == "trusted";
            }
        }
    }
    trust_result(
        codex,
        CODEX_TRUST_FILE,
        trusted,
        "has no trust_level = \"trusted\" for the repository root",
    )
}

/// Antigravity's trust in `roots`: the list `trustedWorkspaces` in
/// `~/.gemini/antigravity-cli/settings.json` holds the root. `file` is the
/// file text, `None` when it does not exist. Trust is exact-path: a parent
/// folder or a subdirectory does not count. Seen on agy 1.2.17
/// (`docs/live-checks/harnesses.md`): `{"trustedWorkspaces": ["<dir>", ...]}`.
pub fn antigravity_trust(file: Option<&str>, roots: &[PathBuf]) -> HarnessTrust {
    let agy = HarnessKind::Antigravity;
    parse_store(agy, ANTIGRAVITY_TRUST_FILE, file).map_or_else(std::convert::identity, |doc| {
        let trusted = doc
            .get("trustedWorkspaces")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|dirs| {
                dirs.iter()
                    .filter_map(serde_json::Value::as_str)
                    .any(|dir| roots.iter().any(|root| Path::new(dir) == root))
            });
        trust_result(
            agy,
            ANTIGRAVITY_TRUST_FILE,
            trusted,
            "has no trustedWorkspaces entry for the repository root",
        )
    })
}

/// The path of a strict `[projects."<path>"]` or `[projects.'<path>']`
/// header (a trimmed line): spaces or tabs around the parts, then
/// optionally a `#` comment. `None` for any other line.
fn toml_project_header(line: &str) -> Option<String> {
    fn ws(s: &str) -> &str {
        s.trim_start_matches([' ', '\t'])
    }
    let rest = ws(line.strip_prefix('[')?);
    let rest = ws(ws(rest.strip_prefix("projects")?).strip_prefix('.')?);
    let (key, after) = toml_quoted(rest)?;
    let after = ws(ws(after).strip_prefix(']')?);
    (after.is_empty() || after.starts_with('#')).then_some(key)
}

/// The trimmed lines of a TOML text that start outside a multi-line
/// string. A line inside `"""` or `'''` is string content, not a header or a key.
fn toml_lines(text: &str) -> Vec<&str> {
    let mut open: Option<u8> = None; // the quote of the open multi-line string
    let mut out = Vec::new();
    for line in text.lines() {
        if open.is_none() {
            out.push(line.trim());
        }
        open = toml_line_end_state(line.as_bytes(), open);
    }
    out
}

/// The quote of the multi-line string that is open at the end of line `b`,
/// given the one open at its start.
fn toml_line_end_state(b: &[u8], mut open: Option<u8>) -> Option<u8> {
    let mut i = 0;
    while i < b.len() {
        let step = match open {
            Some(q) => Some(step_in_multiline(b, i, q)),
            None => step_outside(b, i),
        };
        let Some((next, state)) = step else {
            break;
        };
        (i, open) = (next, state);
    }
    open
}

fn triple(b: &[u8], q: u8, i: usize) -> bool {
    b[i..].starts_with(&[q, q, q])
}

/// One step inside a multi-line string with quote `q`: the next offset and
/// the state after it.
fn step_in_multiline(b: &[u8], i: usize, q: u8) -> (usize, Option<u8>) {
    if q == b'"' && b[i] == b'\\' {
        (i + 2, Some(q))
    } else if triple(b, q, i) {
        // Up to 2 more quotes are content before the delimiter.
        let extra = b[i + 3..].iter().take(2).take_while(|c| **c == q).count();
        (i + 3 + extra, None)
    } else {
        (i + 1, Some(q))
    }
}

/// One step outside any string: the next offset and the state after it.
/// `None` at a comment, which ends the line.
fn step_outside(b: &[u8], i: usize) -> Option<(usize, Option<u8>)> {
    match b[i] {
        b'#' => None,
        q @ (b'"' | b'\'') => Some(if triple(b, q, i) {
            (i + 3, Some(q))
        } else {
            (skip_quoted(b, i, q), None)
        }),
        _ => Some((i + 1, None)),
    }
}

/// The offset past the single-line string that opens at `i` with quote `q`.
fn skip_quoted(b: &[u8], i: usize, q: u8) -> usize {
    let mut i = i + 1;
    while i < b.len() && b[i] != q {
        i += if q == b'"' && b[i] == b'\\' { 2 } else { 1 };
    }
    i + 1
}

/// `"<value>"` or `'<value>'` for `key = ...`, `None` for any other line.
fn toml_string_value(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start().strip_prefix('=')?;
    toml_quoted(rest.trim_start()).map(|(value, _)| value)
}

/// A TOML basic (`"..."`) or literal (`'...'`) string at the start of
/// `text`, and the text after it. A basic string may use only the escapes
/// `\\` and `\"`; any other escape gives `None`, so horch never takes a
/// spelling it does not decode for the path it names.
fn toml_quoted(text: &str) -> Option<(String, &str)> {
    let quote = text.chars().next()?;
    let body = &text[quote.len_utf8()..];
    match quote {
        '\'' => toml_literal(body),
        '"' => toml_basic(body),
        _ => None,
    }
}

/// The body of a literal string (after its opening quote) and the rest.
fn toml_literal(body: &str) -> Option<(String, &str)> {
    let end = body.find('\'')?;
    Some((body[..end].to_string(), &body[end + 1..]))
}

/// The body of a basic string (after its opening quote) and the rest.
fn toml_basic(body: &str) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = body.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &body[i + 1..])),
            '\\' => {
                let (_, escaped) = chars.next()?;
                if escaped != '\\' && escaped != '"' {
                    return None;
                }
                out.push(escaped);
            }
            c => out.push(c),
        }
    }
    None
}

/// The one-time command that records `harness`'s trust in `root`, for when
/// [`ensure_trusted`] skipped or failed.
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
/// leaves this function. `home` is the home directory (agy keeps its store
/// under it); the two directories are `$CLAUDE_CONFIG_DIR` and
/// `$CODEX_HOME` when they are set.
pub fn read_trust(
    harnesses: &[HarnessKind],
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
    roots: &[PathBuf],
) -> Vec<HarnessTrust> {
    harnesses
        .iter()
        .map(|&harness| {
            if !asks_for_trust(harness) {
                return HarnessTrust::new(
                    harness,
                    TrustState::Unknown,
                    format!(
                        "horch cannot read where {} keeps its trust",
                        harness.as_str()
                    ),
                );
            }
            let (file, label) = trust_store_of(harness, home, claude_config_dir, codex_home_dir);
            read_store(&file).map_or_else(
                |()| unreadable(harness, label),
                |text| verdict_of(harness, text.as_deref(), roots),
            )
        })
        .collect()
}

/// The store file of `harness` and the name the reasons use for it.
fn trust_store_of(
    harness: HarnessKind,
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
) -> (PathBuf, &'static str) {
    match harness {
        HarnessKind::Claude => (
            claude_config_file(home, claude_config_dir),
            CLAUDE_TRUST_FILE,
        ),
        HarnessKind::Codex => (
            super::codex::codex_home(home, codex_home_dir).join("config.toml"),
            CODEX_TRUST_FILE,
        ),
        _ => (
            home.join(".gemini/antigravity-cli/settings.json"),
            ANTIGRAVITY_TRUST_FILE,
        ),
    }
}

/// What the store text says about `roots`, for a harness that asks.
fn verdict_of(harness: HarnessKind, text: Option<&str>, roots: &[PathBuf]) -> HarnessTrust {
    match harness {
        HarnessKind::Claude => claude_trust(text, roots),
        HarnessKind::Codex => codex_trust(text, roots),
        _ => antigravity_trust(text, roots),
    }
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

/// What [`ensure_trusted`] did for one harness and workdir.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustWrite {
    /// The store already trusted the workdir. horch wrote nothing.
    AlreadyTrusted,
    /// horch wrote the trust entry to this store file.
    Written(PathBuf),
    /// horch did not write, for this reason. It names the store file or the
    /// path, never any content of the file.
    Skipped(String),
}

/// Make `harness` trust `workdir` before an agent starts in it (ARC-29).
///
/// It reads the store first and writes only when the store does not trust
/// the workdir or its canonical form, so most launches write nothing. The
/// write keeps every other key and value and is atomic: a temporary file in
/// the same directory, with the store's mode, renamed over the store. horch
/// reads the store again just before the rename and starts again, at most 3
/// times, when another process changed it. For every harness it holds the
/// lock directory `<store>.lock` across the read and the rename: for Claude
/// that is Claude's own lock, and for Codex and Antigravity it serializes
/// horch with horch. Codex and Antigravity write their stores without a
/// lock, so a native write that lands between horch's last read and its
/// rename is lost (2 Codex sessions have the same exposure to each other).
/// After the write it reads the store again and fails when it does not
/// trust the workdir.
///
/// The temporary file is `<store>.horch-tmp.<pid>.<12 hex>`. A process
/// that dies before the rename leaves it, so while it holds the lock horch
/// removes such a file of a dead pid that is older than 60 s.
///
/// It never trusts `/`, the home directory or an ancestor of it, a relative
/// path, a path that is not valid UTF-8 (the store would name a different
/// folder), or a path that is not an existing directory. It never writes
/// through a symlinked store, never changes an explicit Codex
/// `trust_level = "untrusted"`, and never creates a Claude or Antigravity
/// store: those return [`TrustWrite::Skipped`]. `home`,
/// `claude_config_dir` and `codex_home_dir` are as in [`read_trust`].
pub fn ensure_trusted(
    harness: HarnessKind,
    workdir: &Path,
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
) -> anyhow::Result<TrustWrite> {
    ensure_trusted_with(
        harness,
        workdir,
        home,
        claude_config_dir,
        codex_home_dir,
        &mut || {},
    )
}

/// The stderr line for what [`ensure_trusted`] did, `None` when the store
/// already trusted `workdir`. It names the store file, never its content.
pub fn trust_note(
    harness: HarnessKind,
    workdir: &Path,
    outcome: &anyhow::Result<TrustWrite>,
) -> Option<String> {
    let name = harness.as_str();
    let dir = workdir.display();
    match outcome {
        Ok(TrustWrite::AlreadyTrusted) => None,
        Ok(TrustWrite::Written(file)) => Some(format!(
            "horch: trusted {dir} for {name} ({})",
            file.display()
        )),
        Ok(TrustWrite::Skipped(why)) => Some(format!(
            "horch: NOTE: horch did not trust {dir} for {name}: {why}"
        )),
        Err(e) => Some(format!(
            "horch: NOTE: horch could not trust {dir} for {name}: {e:#}"
        )),
    }
}

/// How many times a write starts again when the store changes under it.
const WRITE_TRIES: usize = 3;
/// A store lock directory older than this is stale (proper-lockfile's
/// default, which Claude uses).
const LOCK_STALE: Duration = Duration::from_secs(10);
/// How long horch waits for a live store lock before it skips.
const LOCK_WAIT: Duration = Duration::from_secs(2);
/// The tag in horch's temporary store name, `<store>.horch-tmp.<pid>.<12 hex>`.
/// It differs from Claude's `.tmp.`, so horch never removes Claude's files.
const TEMP_TAG: &str = "horch-tmp";
/// A temporary store file of a dead pid older than this is left over.
const TEMP_STALE: Duration = Duration::from_secs(60);

/// [`ensure_trusted`], calling `before_rename` after the new store is in its
/// temporary file and before the store is read again. A test changes the
/// store there.
fn ensure_trusted_with(
    harness: HarnessKind,
    workdir: &Path,
    home: &Path,
    claude_config_dir: Option<&Path>,
    codex_home_dir: Option<&Path>,
    before_rename: &mut dyn FnMut(),
) -> anyhow::Result<TrustWrite> {
    let (roots, keys) = match trust_plan(harness, workdir, home) {
        Ok(plan) => plan,
        Err(why) => return Ok(TrustWrite::Skipped(why)),
    };
    let (file, label) = trust_store_of(harness, home, claude_config_dir, codex_home_dir);
    let store = TrustStore {
        harness,
        file,
        label,
        roots,
        keys,
    };
    (0..WRITE_TRIES)
        .find_map(|_| store.attempt(workdir, &mut *before_rename).transpose())
        .unwrap_or_else(|| {
            anyhow::bail!(
                "{} ({}) changed during each of {WRITE_TRIES} tries; horch wrote nothing",
                store.label,
                store.file.display()
            )
        })
}

/// The roots and store keys of a trust write, or why horch skips it: the
/// harness has no trust step, or `workdir` may not be trusted.
fn trust_plan(
    harness: HarnessKind,
    workdir: &Path,
    home: &Path,
) -> Result<(Vec<PathBuf>, Vec<String>), String> {
    if !asks_for_trust(harness) {
        return Err(format!("{} has no trust step", harness.as_str()));
    }
    let roots = trust_roots(workdir, home)?;
    let keys = trust_keys(&roots)?;
    Ok((roots, keys))
}

/// One harness's store, and the roots and keys horch makes it trust.
struct TrustStore {
    harness: HarnessKind,
    file: PathBuf,
    label: &'static str,
    roots: Vec<PathBuf>,
    keys: Vec<String>,
}

/// What a store's text asks of horch.
enum Decision {
    /// Nothing to write: the outcome is final.
    Done(TrustWrite),
    /// Write this new store text.
    Write(String),
}

impl TrustStore {
    fn unreadable(&self) -> anyhow::Error {
        anyhow::anyhow!("{} ({}) cannot be read", self.label, self.file.display())
    }

    fn verdict(&self, text: Option<&str>) -> HarnessTrust {
        verdict_of(self.harness, text, &self.roots)
    }

    /// The store text with the trust entries added, or why horch does not
    /// edit it.
    fn edit(&self, text: Option<&str>) -> Result<String, String> {
        match (self.harness, text) {
            (HarnessKind::Codex, text) => {
                codex_add_trust(text.unwrap_or(""), &self.keys, self.label)
            }
            (_, None) => Err(format!(
                "{} does not exist: {} has never run with this store",
                self.label,
                self.harness.as_str()
            )),
            (HarnessKind::Claude, Some(text)) => claude_add_trust(text, &self.keys, self.label),
            (_, Some(text)) => antigravity_add_trust(text, &self.keys, self.label),
        }
    }

    /// Why horch does not touch the store path: a symlink, or no directory.
    fn path_problem(&self) -> Option<String> {
        let linked =
            std::fs::symlink_metadata(&self.file).is_ok_and(|m| m.file_type().is_symlink());
        if linked {
            Some(format!(
                "{} ({}) is a symlink; horch does not write through it",
                self.label,
                self.file.display()
            ))
        } else if !self.file.parent().is_some_and(Path::is_dir) {
            Some(format!(
                "the directory of {} ({}) does not exist",
                self.label,
                self.file.display()
            ))
        } else {
            None
        }
    }

    /// One try: the outcome, or `None` when the store changed under the
    /// write and the caller starts again.
    fn attempt(
        &self,
        workdir: &Path,
        before_rename: &mut dyn FnMut(),
    ) -> anyhow::Result<Option<TrustWrite>> {
        if let Some(why) = self.path_problem() {
            return Ok(Some(TrustWrite::Skipped(why)));
        }
        let Some(lock) = StoreLock::take(&self.file)? else {
            return Ok(Some(TrustWrite::Skipped(format!(
                "{}.lock stays held by another process",
                self.file.display()
            ))));
        };
        remove_dead_temps(&self.file);
        let before = read_store(&self.file).map_err(|()| self.unreadable())?;
        match self.decide(before.as_deref()) {
            Decision::Done(done) => Ok(Some(done)),
            Decision::Write(after) => self.commit(lock, &before, &after, workdir, before_rename),
        }
    }

    fn decide(&self, before: Option<&str>) -> Decision {
        let found = self.verdict(before);
        match found.state {
            TrustState::Trusted => Decision::Done(TrustWrite::AlreadyTrusted),
            TrustState::Unknown => Decision::Done(TrustWrite::Skipped(found.reason)),
            TrustState::Untrusted => match self.edit(before) {
                Ok(after) => Decision::Write(after),
                Err(why) => Decision::Done(TrustWrite::Skipped(why)),
            },
        }
    }

    /// Write `after` over the store when it is still `before`; `None` when
    /// it changed. The lock is held until the rename.
    fn commit(
        &self,
        lock: StoreLock,
        before: &Option<String>,
        after: &str,
        workdir: &Path,
        before_rename: &mut dyn FnMut(),
    ) -> anyhow::Result<Option<TrustWrite>> {
        let temp = TempStore::write(&self.file, after)?;
        before_rename();
        match read_store(&self.file) {
            Err(()) => Err(temp.discard_with(self.unreadable())),
            Ok(now) if now != *before => temp.discard().map(|()| None),
            Ok(_) => {
                temp.rename_over(&self.file)?;
                drop(lock);
                self.verify(workdir)?;
                Ok(Some(TrustWrite::Written(self.file.clone())))
            }
        }
    }

    /// The store must trust `workdir` after the write.
    fn verify(&self, workdir: &Path) -> anyhow::Result<()> {
        let written = read_store(&self.file).map_err(|()| self.unreadable())?;
        anyhow::ensure!(
            self.verdict(written.as_deref()).state == TrustState::Trusted,
            "{} ({}) does not trust {} after horch wrote it",
            self.label,
            self.file.display(),
            workdir.display()
        );
        Ok(())
    }
}

/// `workdir` and its canonical form, when horch may trust them: an
/// absolute path of an existing directory that is not `/`, the home
/// directory or an ancestor of it. Else why not.
fn trust_roots(workdir: &Path, home: &Path) -> Result<Vec<PathBuf>, String> {
    check_workdir(workdir, home)?;
    let roots = with_canonical(workdir);
    let homes = with_canonical(home);
    let refused = roots
        .iter()
        .any(|r| r.parent().is_none() || homes.iter().any(|h| h.starts_with(r)));
    if refused {
        Err(format!(
            "{} is / or the home directory or an ancestor of it; horch never trusts those",
            workdir.display()
        ))
    } else {
        Ok(roots)
    }
}

/// The first reason `workdir` cannot be a trust root before any path
/// comparison: not absolute, not a directory, or no known home.
fn check_workdir(workdir: &Path, home: &Path) -> Result<(), String> {
    let shown = workdir.display();
    if !workdir.is_absolute() {
        Err(format!("{shown} is not an absolute path"))
    } else if !workdir.is_dir() {
        Err(format!("{shown} is not an existing directory"))
    } else if !home.is_absolute() {
        Err(format!(
            "the home directory is not known, so horch cannot protect it; {shown} is not trusted"
        ))
    } else {
        Ok(())
    }
}

/// `path`, then its canonical form when that differs.
fn with_canonical(path: &Path) -> Vec<PathBuf> {
    let mut all = vec![path.to_path_buf()];
    all.extend(path.canonicalize().ok().filter(|c| c != path));
    all
}

/// The store keys for `roots`. Else why not: a path that is not valid UTF-8
/// has no exact key, and a lossy one names a different folder.
fn trust_keys(roots: &[PathBuf]) -> Result<Vec<String>, String> {
    roots
        .iter()
        .map(|root| {
            root.to_str().map(str::to_owned).ok_or_else(|| {
                format!(
                    "{} is not valid UTF-8, so horch cannot name it in a trust store",
                    root.display()
                )
            })
        })
        .collect()
}

/// A lock on a store: the directory `<store>.lock`, as proper-lockfile
/// makes it (Claude's own lock on `.claude.json`). Dropping the guard
/// removes the directory.
struct StoreLock(PathBuf);

impl StoreLock {
    /// Take the lock. A lock directory older than [`LOCK_STALE`] is stale and
    /// is taken over. `None` when a live lock stays held for [`LOCK_WAIT`].
    fn take(store: &Path) -> anyhow::Result<Option<StoreLock>> {
        let mut dir = store.as_os_str().to_os_string();
        dir.push(".lock");
        let dir = PathBuf::from(dir);
        let deadline = Instant::now() + LOCK_WAIT;
        loop {
            match std::fs::create_dir(&dir) {
                Ok(()) => break Ok(Some(StoreLock(dir))),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let reclaimed = lock_is_stale(&dir) && std::fs::remove_dir(&dir).is_ok();
                    if !reclaimed {
                        if Instant::now() >= deadline {
                            break Ok(None);
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
                Err(e) => {
                    break Err(anyhow::Error::new(e)
                        .context(format!("creating the lock {}", dir.display())))
                }
            }
        }
    }
}

/// Whether the lock directory is older than [`LOCK_STALE`].
fn lock_is_stale(dir: &Path) -> bool {
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age > LOCK_STALE)
}

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// The new store text in a temporary file next to the store,
/// `<store>.horch-tmp.<pid>.<12 hex>`, with the store's mode (`0600` for a
/// new store). On an error path [`TempStore::discard`] removes it and
/// reports a failure; `Drop` is the last resort and cannot report.
struct TempStore(Option<PathBuf>);

impl TempStore {
    fn write(store: &Path, text: &str) -> anyhow::Result<TempStore> {
        use std::io::Write as _;
        let mut name = store.as_os_str().to_os_string();
        let random = uuid::Uuid::new_v4().simple().to_string();
        name.push(format!(
            ".{TEMP_TAG}.{}.{}",
            std::process::id(),
            &random[..12]
        ));
        let path = PathBuf::from(name);
        let mut open = std::fs::OpenOptions::new();
        open.write(true).create_new(true);
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
            let mode = std::fs::metadata(store)
                .map(|m| m.permissions().mode() & 0o7777)
                .unwrap_or(0o600);
            open.mode(mode);
            mode
        };
        let mut f = open
            .open(&path)
            .with_context(|| format!("creating {}", path.display()))?;
        let temp = TempStore(Some(path.clone()));
        let mut fill = || -> anyhow::Result<()> {
            // The umask can narrow the mode `open` asked for.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                    .with_context(|| format!("setting the mode of {}", path.display()))?;
            }
            f.write_all(text.as_bytes())
                .and_then(|()| f.sync_all())
                .with_context(|| format!("writing {}", path.display()))
        };
        let filled = fill();
        drop(f);
        match filled {
            Ok(()) => Ok(temp),
            Err(e) => Err(temp.discard_with(e)),
        }
    }

    fn rename_over(mut self, store: &Path) -> anyhow::Result<()> {
        let Some(path) = self.0.clone() else {
            return Ok(());
        };
        match std::fs::rename(&path, store) {
            Ok(()) => {
                self.0 = None;
                Ok(())
            }
            Err(e) => {
                let e = anyhow::Error::new(e).context(format!(
                    "renaming {} over {}",
                    path.display(),
                    store.display()
                ));
                Err(self.discard_with(e))
            }
        }
    }

    /// Remove the file. Err names it when it stays.
    fn discard(mut self) -> anyhow::Result<()> {
        let Some(path) = self.0.take() else {
            return Ok(());
        };
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(anyhow::Error::new(e).context(format!(
                "removing horch's temporary file {}, a copy of the store",
                path.display()
            ))),
        }
    }

    /// `err`, with the failure to remove the file added when it stays.
    fn discard_with(self, err: anyhow::Error) -> anyhow::Error {
        match self.discard() {
            Ok(()) => err,
            Err(left) => anyhow::anyhow!("{err:#}; also {left:#}"),
        }
    }
}

/// Remove the temporary files that a dead horch writer left next to
/// `store`: only names that are exactly `<store file name>.horch-tmp.<pid>.<12 hex>`,
/// where the pid is not alive and the file is older than [`TEMP_STALE`].
/// No other file is touched. The caller holds the store lock. A file that
/// cannot be removed stays for a later write.
fn remove_dead_temps(store: &Path) {
    let (Some(dir), Some(name)) = (store.parent(), store.file_name().and_then(|n| n.to_str()))
    else {
        return;
    };
    let prefix = format!("{name}.{TEMP_TAG}.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let dead = dead_temp_pid(&entry.file_name(), &prefix)
            .is_some_and(|pid| !crate::fsx::pid_alive(pid));
        if dead && is_stale_file(&entry.path()) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// The pid in a temporary store name, `<prefix><pid>.<12 hex>`.
fn dead_temp_pid(file_name: &std::ffi::OsStr, prefix: &str) -> Option<u32> {
    file_name
        .to_str()
        .and_then(|n| n.strip_prefix(prefix))
        .and_then(temp_pid)
}

/// A regular file older than [`TEMP_STALE`].
fn is_stale_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.file_type().is_file())
        .and_then(|m| m.modified().ok())
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age > TEMP_STALE)
}

/// The pid of a temporary name's tail, `<pid>.<12 lowercase hex>`; `None`
/// for any other text.
fn temp_pid(tail: &str) -> Option<u32> {
    let (pid, hex) = tail.split_once('.')?;
    let hex_ok = hex.len() == 12 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let pid_ok = !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit());
    if hex_ok && pid_ok {
        pid.parse().ok()
    } else {
        None
    }
}

impl Drop for TempStore {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// `text` with `projects["<key>"].hasTrustDialogAccepted` set to `true`
/// for each key. Only those bytes change: horch inserts members or replaces
/// one value, and never re-serializes the file, so no other value can change
/// (a re-parsed float could). Err: why horch cannot edit it safely.
fn claude_add_trust(text: &str, keys: &[String], label: &str) -> Result<String, String> {
    let unsafe_edit = || format!("{label} has a shape horch does not edit");
    let doc: serde_json::Value =
        serde_json::from_str(text).map_err(|_| format!("{label} is not valid JSON"))?;
    let out = keys
        .iter()
        .try_fold(text.to_string(), |out, key| add_claude_key(&out, key));
    let expected = trusted_projects(doc, keys);
    out.zip(expected)
        .and_then(|(out, expected)| json_check(&out, &expected))
        .ok_or_else(unsafe_edit)
}

/// `out` with `projects["<key>"].hasTrustDialogAccepted` set to `true`.
/// None: the file has a shape horch does not edit.
fn add_claude_key(out: &str, key: &str) -> Option<String> {
    let top = json_object_at(out, 0)?;
    let accepted = r#""hasTrustDialogAccepted":true"#;
    let entry = format!("{}:{{{accepted}}}", json_quote(key));
    let Some(projects) = json_member(&top, "projects") else {
        return Some(json_insert(out, &top, &format!("\"projects\":{{{entry}}}")));
    };
    let projects = json_object_at(out, projects.0)?;
    let Some(project) = json_member(&projects, key) else {
        return Some(json_insert(out, &projects, &entry));
    };
    let project = json_object_at(out, project.0)?;
    Some(match json_member(&project, "hasTrustDialogAccepted") {
        None => json_insert(out, &project, accepted),
        Some((start, end)) => format!("{}true{}", &out[..start], &out[end..]),
    })
}

/// What the edited file must parse to: `doc` with each key trusted.
fn trusted_projects(mut doc: serde_json::Value, keys: &[String]) -> Option<serde_json::Value> {
    for key in keys {
        let projects = doc
            .as_object_mut()?
            .entry("projects")
            .or_insert_with(|| serde_json::json!({}));
        let project = projects
            .as_object_mut()?
            .entry(key.clone())
            .or_insert_with(|| serde_json::json!({}));
        project
            .as_object_mut()?
            .insert("hasTrustDialogAccepted".into(), true.into());
    }
    Some(doc)
}

/// `text` with each key that is not in `trustedWorkspaces` added to the
/// front of that list. Only those bytes change, as in [`claude_add_trust`].
fn antigravity_add_trust(text: &str, keys: &[String], label: &str) -> Result<String, String> {
    let unsafe_edit = || format!("{label} has a shape horch does not edit");
    let doc: serde_json::Value =
        serde_json::from_str(text).map_err(|_| format!("{label} is not valid JSON"))?;
    let (expected, missing) = trusted_workspaces(doc, keys).ok_or_else(unsafe_edit)?;
    let items = missing
        .iter()
        .map(String::as_str)
        .map(json_quote)
        .collect::<Vec<_>>()
        .join(",");
    insert_workspaces(text, &items)
        .and_then(|out| json_check(&out, &expected))
        .ok_or_else(unsafe_edit)
}

/// `text` with `items` (JSON strings, comma-separated) put in front of its
/// `trustedWorkspaces` list. None: the file has a shape horch does not edit.
fn insert_workspaces(text: &str, items: &str) -> Option<String> {
    let top = json_object_at(text, 0)?;
    match json_member(&top, "trustedWorkspaces") {
        None => Some(json_insert(
            text,
            &top,
            &format!("\"trustedWorkspaces\":[{items}]"),
        )),
        Some((start, _)) => append_to_array(text, start, items),
    }
}

/// What the edited file must parse to: `doc` with each key that is not in
/// `trustedWorkspaces` put in front of that list, and those keys.
fn trusted_workspaces(
    mut doc: serde_json::Value,
    keys: &[String],
) -> Option<(serde_json::Value, Vec<String>)> {
    let list = doc
        .as_object_mut()?
        .entry("trustedWorkspaces")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()?;
    let missing: Vec<String> = keys
        .iter()
        .filter(|k| !list.iter().any(|d| d.as_str() == Some(k.as_str())))
        .cloned()
        .collect();
    for (i, key) in missing.iter().enumerate() {
        list.insert(i, serde_json::Value::String(key.clone()));
    }
    Some((doc, missing))
}

/// `text` with `items` first in the array that starts at `start`. None when
/// no array starts there.
fn append_to_array(text: &str, start: usize, items: &str) -> Option<String> {
    let t = text.as_bytes();
    (t.get(start) == Some(&b'[')).then_some(())?;
    let empty = t.get(json_ws(t, start + 1)) == Some(&b']');
    let sep = if empty { "" } else { "," };
    Some(format!(
        "{}{items}{sep}{}",
        &text[..=start],
        &text[start + 1..]
    ))
}

/// `out` when it parses to `expected`; a guard on the byte edits.
fn json_check(out: &str, expected: &serde_json::Value) -> Option<String> {
    let got: serde_json::Value = serde_json::from_str(out).ok()?;
    (&got == expected).then(|| out.to_string())
}

/// `text` as a JSON string.
fn json_quote(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

/// The span of the object whose `{` is the first non-space byte at or
/// after `at`: the offset of `{` and the members, as (key, value start,
/// value end). `None` when it is not an object.
fn json_object_at(text: &str, at: usize) -> Option<JsonObject> {
    let t = text.as_bytes();
    let open = json_ws(t, at);
    (t.get(open) == Some(&b'{')).then_some(())?;
    let members = json_members(text, json_ws(t, open + 1))?;
    Some(JsonObject { open, members })
}

/// The members of an object, from the first byte after its `{`.
fn json_members(text: &str, mut i: usize) -> Option<Vec<(String, usize, usize)>> {
    let t = text.as_bytes();
    let mut members = Vec::new();
    if t.get(i) == Some(&b'}') {
        return Some(members);
    }
    loop {
        let member = json_member_at(text, i)?;
        i = json_ws(t, member.2);
        members.push(member);
        match t.get(i) {
            Some(b',') => i = json_ws(t, i + 1),
            Some(b'}') => break Some(members),
            _ => break None,
        }
    }
}

/// One `"key": value` member at `i`: the key and the value span.
fn json_member_at(text: &str, i: usize) -> Option<(String, usize, usize)> {
    let t = text.as_bytes();
    (t.get(i) == Some(&b'"')).then_some(())?;
    let key_end = json_string_end(t, i)?;
    let key: String = serde_json::from_str(&text[i..key_end]).ok()?;
    let colon = json_ws(t, key_end);
    (t.get(colon) == Some(&b':')).then_some(())?;
    let start = json_ws(t, colon + 1);
    let end = json_value_end(t, start)?;
    Some((key, start, end))
}

/// An object in a JSON text: the offset of its `{` and its members.
struct JsonObject {
    open: usize,
    members: Vec<(String, usize, usize)>,
}

/// The value span of `key` in `object`. The last one wins, as when the
/// file is parsed.
fn json_member(object: &JsonObject, key: &str) -> Option<(usize, usize)> {
    object
        .members
        .iter()
        .rev()
        .find(|(k, _, _)| k == key)
        .map(|&(_, start, end)| (start, end))
}

/// `text` with `member` (`"key":value`) first in `object`.
fn json_insert(text: &str, object: &JsonObject, member: &str) -> String {
    let sep = if object.members.is_empty() { "" } else { "," };
    let at = object.open + 1;
    format!("{}{member}{sep}{}", &text[..at], &text[at..])
}

fn json_ws(t: &[u8], mut i: usize) -> usize {
    while matches!(t.get(i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        i += 1;
    }
    i
}

/// The end (past the closing quote) of the string that starts at `i`.
fn json_string_end(t: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    loop {
        match t.get(j)? {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
}

/// The end of the value that starts at `i`, in text that is valid JSON.
fn json_value_end(t: &[u8], i: usize) -> Option<usize> {
    match t.get(i)? {
        b'"' => json_string_end(t, i),
        b'{' | b'[' => json_container_end(t, i),
        _ => json_scalar_end(t, i),
    }
}

/// The end of the object or array that starts at `i`.
fn json_container_end(t: &[u8], i: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut j = i;
    loop {
        match t.get(j)? {
            b'"' => j = json_string_end(t, j)?,
            b'{' | b'[' => {
                depth += 1;
                j += 1;
            }
            b'}' | b']' => {
                depth -= 1;
                j += 1;
                if depth == 0 {
                    break Some(j);
                }
            }
            _ => j += 1,
        }
    }
}

/// The end of the number or word that starts at `i`.
fn json_scalar_end(t: &[u8], i: usize) -> Option<usize> {
    let len = t[i..]
        .iter()
        .take_while(|b| !matches!(b, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'))
        .count();
    (len > 0).then_some(i + len)
}

/// `text` with a `[projects."<key>"]` table, `trust_level = "trusted"`, for
/// each key appended. The text before it stays byte for byte. Err when a
/// table for a key exists already: an explicit `trust_level = "untrusted"`
/// is the operator's choice, and any other table horch does not edit. Err
/// too when horch does not fully understand the file, as an append could
/// then make it invalid (a duplicate table): it has a multi-line string, or
/// a line that can define `projects` other than a strict
/// `[projects."<path>"]` header (see [`toml_projects_line`]). The result
/// has exactly 1 strict header for each key, or it is Err.
fn codex_add_trust(text: &str, keys: &[String], label: &str) -> Result<String, String> {
    codex_editable(text, label)?;
    refuse_existing_levels(&codex_trust_levels(text, keys), label)?;
    let out = codex_trust_table(text, keys, label)?;
    check_single_tables(&out, keys, label)?;
    Ok(out)
}

/// Err when horch does not fully understand `text`.
fn codex_editable(text: &str, label: &str) -> Result<(), String> {
    let multi_line = text.contains("\"\"\"") || text.contains("'''");
    let odd_projects = text
        .lines()
        .map(str::trim)
        .any(|line| toml_projects_line(line) && toml_project_header(line).is_none());
    if multi_line {
        Err(format!(
            "{label} has a multi-line string, which horch does not edit; trust the path in codex"
        ))
    } else if odd_projects {
        Err(format!(
            "{label} keeps projects in a form horch does not edit; trust the path in codex"
        ))
    } else {
        Ok(())
    }
}

/// The `trust_level` of each `[projects."<key>"]` table that `text` has
/// for a key; `None` for a table without one.
fn codex_trust_levels(text: &str, keys: &[String]) -> BTreeMap<String, Option<String>> {
    let mut table: Option<String> = None;
    let mut levels: BTreeMap<String, Option<String>> = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            table = toml_project_header(line).filter(|p| keys.contains(p));
            if let Some(path) = &table {
                levels.entry(path.clone()).or_insert(None);
            }
        } else if let Some(path) = &table {
            if let Some(level) = toml_string_value(line, "trust_level") {
                levels.insert(path.clone(), Some(level));
            }
        }
    }
    levels
}

/// Err when a table for a key exists already: an explicit
/// `trust_level = "untrusted"` is the operator's choice, and any other table
/// horch does not edit.
fn refuse_existing_levels(
    levels: &BTreeMap<String, Option<String>>,
    label: &str,
) -> Result<(), String> {
    let untrusted = levels
        .iter()
        .find(|(_, level)| level.as_deref() == Some("untrusted"));
    if let Some((path, _)) = untrusted {
        Err(format!(
            "{label} sets trust_level = \"untrusted\" for {path}; horch keeps the operator's choice"
        ))
    } else if let Some(path) = levels.keys().next() {
        Err(format!(
            "{label} has a [projects.\"{path}\"] table without trust_level = \"trusted\"; horch does not edit it"
        ))
    } else {
        Ok(())
    }
}

/// `text` with a trusted table appended for each key. The text before it
/// stays byte for byte.
fn codex_trust_table(text: &str, keys: &[String], label: &str) -> Result<String, String> {
    let mut out = text.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    for key in keys {
        out.push_str(&codex_trust_header(key, label)?);
        out.push_str("\ntrust_level = \"trusted\"\n");
    }
    Ok(out)
}

/// The strict `[projects."<key>"]` header for `key`.
fn codex_trust_header(key: &str, label: &str) -> Result<String, String> {
    if key.chars().any(char::is_control) {
        return Err(format!("{label}: the path {key:?} has a control character"));
    }
    let quoted = key.replace('\\', "\\\\").replace('"', "\\\"");
    let header = format!("[projects.\"{quoted}\"]");
    if toml_project_header(&header).as_deref() == Some(key) {
        Ok(header)
    } else {
        Err(format!("{label}: horch cannot quote the path {key:?}"))
    }
}

/// Err unless `out` has exactly 1 strict header for each key.
fn check_single_tables(out: &str, keys: &[String], label: &str) -> Result<(), String> {
    let wrong = keys.iter().find_map(|key| {
        let headers = out
            .lines()
            .map(str::trim)
            .filter(|line| toml_project_header(line).as_deref() == Some(key.as_str()))
            .count();
        (headers != 1).then(|| {
            format!("{label} would have {headers} tables for {key:?}; horch wrote nothing")
        })
    });
    wrong.map_or(Ok(()), Err)
}

/// Whether a trimmed TOML line can define the `projects` table or a key in
/// it: a header that names `projects` or has an escape (an escape can spell
/// `projects`), a key `projects` in any quoting, or a quoted key with an
/// escape. It can say yes to a line that does not; horch then skips.
fn toml_projects_line(line: &str) -> bool {
    if line.starts_with('[') {
        return line.contains("projects") || line.contains('\\');
    }
    let bare_key = ["projects", "\"projects\"", "'projects'"]
        .iter()
        .any(|key| {
            line.strip_prefix(key)
                .is_some_and(|rest| rest.trim_start_matches([' ', '\t']).starts_with(['=', '.']))
        });
    let escaped_key =
        line.starts_with('"') && line.split('=').next().is_some_and(|key| key.contains('\\'));
    bare_key || escaped_key
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp home with a project directory in it. Nothing here touches the
    /// real home.
    struct Fake {
        _tmp: tempfile::TempDir,
        home: PathBuf,
        project: PathBuf,
    }

    impl Fake {
        fn new() -> Fake {
            let tmp = tempfile::tempdir().unwrap();
            // Canonical, so the workdir has one form and one key.
            let root = tmp.path().canonicalize().unwrap();
            let home = root.join("home");
            let project = home.join("projects/alpha");
            std::fs::create_dir_all(&project).unwrap();
            Fake {
                _tmp: tmp,
                home,
                project,
            }
        }

        fn claude_file(&self) -> PathBuf {
            self.home.join(".claude.json")
        }

        fn codex_file(&self) -> PathBuf {
            self.home.join(".codex/config.toml")
        }

        fn ensure(&self, harness: HarnessKind, workdir: &Path) -> TrustWrite {
            ensure_trusted(harness, workdir, &self.home, None, None).unwrap()
        }

        fn state(&self, harness: HarnessKind) -> TrustState {
            read_trust(
                &[harness],
                &self.home,
                None,
                None,
                std::slice::from_ref(&self.project),
            )[0]
            .state
        }
    }

    fn json(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn arc_29_claude_entry_written_when_missing_and_other_keys_kept() {
        let f = Fake::new();
        let other = f.home.join("projects/beta");
        let before = serde_json::json!({
            "numStartups": 42,
            "oauthAccount": {"emailAddress": "a@b.invalid", "token": "SECRET"},
            "cachedRatio": 0.25,
            "big": 123456789012345678901234567890u128.to_string(),
            "projects": {
                other.to_string_lossy(): {"hasTrustDialogAccepted": true, "allowedTools": ["Bash"]}
            }
        });
        let text = serde_json::to_string_pretty(&before).unwrap();
        std::fs::write(f.claude_file(), &text).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(f.claude_file(), std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        assert_eq!(f.state(HarnessKind::Claude), TrustState::Untrusted);

        let wrote = f.ensure(HarnessKind::Claude, &f.project);
        assert_eq!(wrote, TrustWrite::Written(f.claude_file()));
        assert_eq!(f.state(HarnessKind::Claude), TrustState::Trusted);
        let mut expected = before.clone();
        expected["projects"][f.project.to_string_lossy().as_ref()] =
            serde_json::json!({"hasTrustDialogAccepted": true});
        assert_eq!(json(&f.claude_file()), expected);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(f.claude_file())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        // No temp file and no lock directory stays behind.
        let mut left: Vec<_> = std::fs::read_dir(&f.home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [".claude.json", "projects"]);

        // An entry for the project without the key gets the key, and keeps
        // its other keys.
        let mut partial = before.clone();
        partial["projects"][f.project.to_string_lossy().as_ref()] =
            serde_json::json!({"allowedTools": ["Read"], "hasTrustDialogAccepted": false});
        std::fs::write(f.claude_file(), partial.to_string()).unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Written(_)
        ));
        partial["projects"][f.project.to_string_lossy().as_ref()]["hasTrustDialogAccepted"] =
            serde_json::Value::Bool(true);
        assert_eq!(json(&f.claude_file()), partial);

        // A number serde_json cannot round-trip keeps its bytes: horch edits
        // the text, it does not re-serialize it.
        let long = r#"{"ratio": 0.12345678901234567890123, "big": 123456789012345678901234567890}"#;
        std::fs::write(f.claude_file(), long).unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Written(_)
        ));
        let after = std::fs::read_to_string(f.claude_file()).unwrap();
        assert!(after.contains("0.12345678901234567890123"), "{after}");
        assert!(after.contains("123456789012345678901234567890"), "{after}");
    }

    #[test]
    fn arc_29_claude_already_trusted_writes_nothing() {
        let f = Fake::new();
        let text = serde_json::json!({"projects": {
            f.project.to_string_lossy(): {"hasTrustDialogAccepted": true}
        }})
        .to_string();
        std::fs::write(f.claude_file(), &text).unwrap();
        let before = std::fs::metadata(f.claude_file())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::AlreadyTrusted
        );
        assert_eq!(std::fs::read_to_string(f.claude_file()).unwrap(), text);
        let after = std::fs::metadata(f.claude_file())
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);

        // No file: Claude never ran with this config dir. Skip, write nothing.
        std::fs::remove_file(f.claude_file()).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Claude, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains("does not exist"), "{why}");
        assert!(!f.claude_file().exists());
    }

    #[cfg(unix)]
    #[test]
    fn arc_29_claude_symlinked_store_is_skipped() {
        let f = Fake::new();
        let real = f.home.join("dotfiles-claude.json");
        std::fs::write(&real, "{}").unwrap();
        std::os::unix::fs::symlink(&real, f.claude_file()).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Claude, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains("symlink"), "{why}");
        assert_eq!(std::fs::read_to_string(&real).unwrap(), "{}");
    }

    /// opencode, pi, prime and the smoke harness show no trust dialog: the
    /// skip names the harness, and no store is created.
    #[test]
    fn arc_29_a_harness_without_a_trust_step_is_skipped_by_name() {
        let f = Fake::new();
        for (harness, name) in [
            (HarnessKind::OpenCode, "opencode"),
            (HarnessKind::Pi, "pi"),
            (HarnessKind::Prime, "prime"),
            (HarnessKind::None, "none"),
        ] {
            assert_eq!(
                f.ensure(harness, &f.project),
                TrustWrite::Skipped(format!("{name} has no trust step")),
                "{name}"
            );
        }
        assert!(!f.claude_file().exists());
        assert!(!f.codex_file().exists());
    }

    #[test]
    fn arc_29_codex_table_appended_and_rest_of_file_byte_identical() {
        let f = Fake::new();
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        // No final newline, a comment, odd spacing: all kept byte for byte.
        let text = "model = \"gpt-5\"  # mine\n\n[projects.\"/elsewhere\"]\ntrust_level = \"trusted\"\n[tui]\nx=1";
        std::fs::write(f.codex_file(), text).unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::Written(f.codex_file())
        );
        let after = std::fs::read_to_string(f.codex_file()).unwrap();
        assert!(after.starts_with(text), "{after}");
        assert_eq!(
            &after[text.len()..],
            format!(
                "\n\n[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
                f.project.display()
            )
        );
        assert_eq!(f.state(HarnessKind::Codex), TrustState::Trusted);
        // A second launch writes nothing.
        assert_eq!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::AlreadyTrusted
        );
        assert_eq!(std::fs::read_to_string(f.codex_file()).unwrap(), after);

        // No file: horch creates it with only the table.
        std::fs::remove_file(f.codex_file()).unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::Written(_)
        ));
        assert_eq!(
            std::fs::read_to_string(f.codex_file()).unwrap(),
            format!(
                "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
                f.project.display()
            )
        );
    }

    #[test]
    fn arc_29_codex_explicit_untrusted_is_kept() {
        let f = Fake::new();
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        let text = format!(
            "[projects.\"{}\"]\ntrust_level = \"untrusted\"\n",
            f.project.display()
        );
        std::fs::write(f.codex_file(), &text).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Codex, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains("untrusted"), "{why}");
        assert!(
            why.contains(&f.project.to_string_lossy().into_owned()),
            "{why}"
        );
        assert!(why.contains("config.toml"), "{why}");
        assert_eq!(std::fs::read_to_string(f.codex_file()).unwrap(), text);
    }

    #[test]
    fn arc_29_antigravity_workspace_added_and_other_keys_kept() {
        let f = Fake::new();
        let file = f.home.join(".gemini/antigravity-cli/settings.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            &file,
            r#"{"theme": "dark", "trustedWorkspaces": ["/other"]}"#,
        )
        .unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Antigravity, &f.project),
            TrustWrite::Written(file.clone())
        );
        assert_eq!(f.state(HarnessKind::Antigravity), TrustState::Trusted);
        assert_eq!(
            json(&file),
            serde_json::json!({
                "theme": "dark",
                "trustedWorkspaces": [f.project.to_string_lossy(), "/other"]
            })
        );
    }

    #[test]
    fn arc_29_home_and_its_ancestors_are_never_trusted() {
        let f = Fake::new();
        std::fs::write(f.claude_file(), "{}").unwrap();
        let mut refused = vec![f.home.clone(), PathBuf::from("/")];
        refused.extend(f.home.ancestors().skip(1).map(Path::to_path_buf));
        for dir in &refused {
            for harness in [
                HarnessKind::Claude,
                HarnessKind::Codex,
                HarnessKind::Antigravity,
            ] {
                let got = f.ensure(harness, dir);
                assert!(
                    matches!(got, TrustWrite::Skipped(_)),
                    "{dir:?} {harness:?}: {got:?}"
                );
            }
        }
        // A path that is not an existing directory, or is relative.
        for dir in [
            f.home.join("missing"),
            f.claude_file(),
            PathBuf::from("rel"),
        ] {
            let got = f.ensure(HarnessKind::Claude, &dir);
            assert!(matches!(got, TrustWrite::Skipped(_)), "{dir:?}: {got:?}");
        }
        // The home through a symlink is still the home.
        #[cfg(unix)]
        {
            let link = f.project.join("home-link");
            std::os::unix::fs::symlink(&f.home, &link).unwrap();
            assert!(matches!(
                f.ensure(HarnessKind::Claude, &link),
                TrustWrite::Skipped(_)
            ));
        }
        assert_eq!(std::fs::read_to_string(f.claude_file()).unwrap(), "{}");
        assert!(!f.codex_file().exists());
        assert!(!f.home.join(".gemini").exists());
    }

    #[test]
    fn arc_29_a_concurrent_write_is_not_lost() {
        let f = Fake::new();
        std::fs::write(f.claude_file(), r#"{"numStartups": 1}"#).unwrap();
        let file = f.claude_file();
        let mut calls = 0;
        let wrote = ensure_trusted_with(
            HarnessKind::Claude,
            &f.project,
            &f.home,
            None,
            None,
            &mut || {
                calls += 1;
                if calls == 1 {
                    // Another Claude session saves its config between horch's
                    // read and its rename.
                    std::fs::write(&file, r#"{"numStartups": 2, "theirs": true}"#).unwrap();
                }
            },
        )
        .unwrap();
        assert_eq!(wrote, TrustWrite::Written(f.claude_file()));
        assert_eq!(calls, 2, "horch reads again after the other write");
        assert_eq!(
            json(&f.claude_file()),
            serde_json::json!({
                "numStartups": 2,
                "theirs": true,
                "projects": {f.project.to_string_lossy(): {"hasTrustDialogAccepted": true}}
            })
        );

        // A store that changes on every try fails after 3 tries, and leaves
        // the other writer's content.
        std::fs::write(f.claude_file(), "{}").unwrap();
        let mut n = 0;
        let err = ensure_trusted_with(
            HarnessKind::Claude,
            &f.project,
            &f.home,
            None,
            None,
            &mut || {
                n += 1;
                std::fs::write(&file, format!("{{\"n\": {n}}}")).unwrap();
            },
        )
        .unwrap_err();
        assert_eq!(n, 3);
        assert!(format!("{err:#}").contains("changed"), "{err:#}");
        assert_eq!(json(&f.claude_file()), serde_json::json!({"n": 3}));
    }

    #[test]
    fn arc_29_a_held_claude_lock_is_waited_for_and_a_stale_one_is_taken() {
        let f = Fake::new();
        std::fs::write(f.claude_file(), "{}").unwrap();
        let lock = f.home.join(".claude.json.lock");
        // A stale lock (older than 10 s): horch takes it and removes it after.
        std::fs::create_dir(&lock).unwrap();
        let old = SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(matches!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Written(_)
        ));
        assert!(!lock.exists());
        // A fresh lock that stays held: horch skips, writes nothing.
        std::fs::write(f.claude_file(), "{}").unwrap();
        std::fs::create_dir(&lock).unwrap();
        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Claude, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains(".claude.json.lock"), "{why}");
        assert_eq!(std::fs::read_to_string(f.claude_file()).unwrap(), "{}");
        assert!(lock.exists(), "horch never removes a live lock");
    }

    #[test]
    fn arc_29_codex_header_comment_cannot_create_a_duplicate_table() {
        let f = Fake::new();
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        let before = format!(
            "[projects.\"{}\"] # operator note\ntrust_level = \"untrusted\"\n",
            f.project.display()
        );
        std::fs::write(f.codex_file(), &before).unwrap();

        let TrustWrite::Skipped(why) = f.ensure(HarnessKind::Codex, &f.project) else {
            panic!("expected a skip");
        };
        assert!(why.contains("untrusted"), "{why}");
        assert_eq!(std::fs::read_to_string(f.codex_file()).unwrap(), before);
    }

    #[test]
    fn arc_29_non_utf8_workdir_does_not_trust_its_lossy_sibling() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        #[cfg(unix)]
        let raw = {
            use std::os::unix::ffi::OsStringExt as _;
            root.join(std::ffi::OsString::from_vec(b"project-\xff".to_vec()))
        };
        #[cfg(windows)]
        let raw = {
            use std::os::windows::ffi::OsStringExt as _;
            let mut name: Vec<u16> = "project-".encode_utf16().collect();
            name.push(0xD800); // an unpaired surrogate
            root.join(std::ffi::OsString::from_wide(&name))
        };
        // The key builder refuses the path on every system.
        assert!(trust_keys(std::slice::from_ref(&raw)).is_err());

        // Where the file system takes the name (Linux; APFS refuses it), the
        // whole write skips and the store is not touched.
        let home = root.join("home");
        let lossy_sibling = root.join("project-\u{fffd}");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&lossy_sibling).unwrap();
        std::fs::write(home.join(".claude.json"), "{}").unwrap();
        if std::fs::create_dir(&raw).is_ok() {
            assert!(matches!(
                ensure_trusted(HarnessKind::Claude, &raw, &home, None, None).unwrap(),
                TrustWrite::Skipped(_)
            ));
            assert_eq!(
                std::fs::read_to_string(home.join(".claude.json")).unwrap(),
                "{}"
            );
        }
    }

    #[test]
    fn arc_29_codex_unrecognised_project_headers_are_skipped() {
        let f = Fake::new();
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        let p = f.project.display().to_string();
        let escaped: String = p
            .chars()
            .map(|c| {
                if c == '/' {
                    "\\u002F".to_string()
                } else {
                    c.to_string()
                }
            })
            .collect();
        let files = [
            format!("[\"projects\".\"{p}\"]\ntrust_level = \"untrusted\"\n"),
            format!("[projects.\"{escaped}\"]\ntrust_level = \"untrusted\"\n"),
            format!("[projects.a.\"{p}\"]\nx = 1\n"),
            format!("note = \"\"\"\n[projects.\"{p}\"]\ntrust_level = \"trusted\"\n\"\"\"\n"),
            "note = '''\nhi\n'''\n".to_string(),
        ];
        for before in &files {
            std::fs::write(f.codex_file(), before).unwrap();
            let got = f.ensure(HarnessKind::Codex, &f.project);
            assert!(matches!(got, TrustWrite::Skipped(_)), "{before}: {got:?}");
            assert_eq!(&std::fs::read_to_string(f.codex_file()).unwrap(), before);
        }
        // A string that only looks like a header is not trust.
        assert_eq!(f.state(HarnessKind::Codex), TrustState::Untrusted);
    }

    #[test]
    fn arc_29_codex_and_antigravity_writes_take_the_lock() {
        let f = Fake::new();
        let agy = f.home.join(".gemini/antigravity-cli/settings.json");
        std::fs::create_dir_all(agy.parent().unwrap()).unwrap();
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        std::fs::write(&agy, "{}").unwrap();
        std::fs::write(f.codex_file(), "x = 1\n").unwrap();
        for (harness, store, text) in [
            (HarnessKind::Codex, f.codex_file(), "x = 1\n"),
            (HarnessKind::Antigravity, agy.clone(), "{}"),
        ] {
            let mut lock = store.as_os_str().to_os_string();
            lock.push(".lock");
            let lock = PathBuf::from(lock);
            std::fs::create_dir(&lock).unwrap();
            let started = Instant::now();
            let TrustWrite::Skipped(why) = f.ensure(harness, &f.project) else {
                panic!("expected a skip for {harness:?}");
            };
            assert!(started.elapsed() >= LOCK_WAIT, "{harness:?} waited");
            assert!(why.contains(".lock"), "{why}");
            assert_eq!(std::fs::read_to_string(&store).unwrap(), text);
            assert!(lock.exists(), "horch never removes a live lock");
            std::fs::remove_dir(&lock).unwrap();
            assert!(matches!(
                f.ensure(harness, &f.project),
                TrustWrite::Written(_)
            ));
            assert!(!lock.exists());
        }
    }

    #[test]
    fn arc_29_a_dead_writers_temp_file_is_removed() {
        let f = Fake::new();
        std::fs::write(f.claude_file(), "{}").unwrap();
        let dead = (90_000..99_999)
            .rev()
            .find(|&pid| !crate::fsx::pid_alive(pid))
            .expect("a pid that is not alive");
        let live = std::process::id();
        let old = SystemTime::now() - Duration::from_secs(120);
        let temp = |tag: &str, pid: u32| {
            f.home
                .join(format!(".claude.json.{tag}.{pid}.0123456789ab"))
        };
        let dead_old = temp("horch-tmp", dead);
        let dead_young = f
            .home
            .join(format!(".claude.json.horch-tmp.{dead}.ba9876543210"));
        let live_old = temp("horch-tmp", live);
        let claude_style = temp("tmp", dead);
        let other_name = f
            .home
            .join(format!(".claude.json.horch-tmp.{dead}.0123456789abc"));
        for (path, aged) in [
            (&dead_old, true),
            (&dead_young, false),
            (&live_old, true),
            (&claude_style, true),
            (&other_name, true),
        ] {
            std::fs::write(path, "SECRET").unwrap();
            if aged {
                std::fs::File::options()
                    .write(true)
                    .open(path)
                    .unwrap()
                    .set_modified(old)
                    .unwrap();
            }
        }
        assert!(matches!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Written(_)
        ));
        assert!(
            !dead_old.exists(),
            "a dead writer's old temp file is removed"
        );
        for kept in [&dead_young, &live_old, &claude_style, &other_name] {
            assert!(kept.exists(), "{} is kept", kept.display());
        }
    }

    #[test]
    fn arc_29_harnesses_without_trust_are_skipped() {
        let f = Fake::new();
        for harness in [HarnessKind::OpenCode, HarnessKind::Pi, HarnessKind::Prime] {
            assert!(matches!(
                f.ensure(harness, &f.project),
                TrustWrite::Skipped(_)
            ));
        }
    }
    // ---- Pins for the trust writer and scanner split (#198) ----

    fn keys(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    fn roots(paths: &[&str]) -> Vec<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn toml_lines_skips_the_inside_of_multi_line_strings() {
        let text = "a = 1\nb = \"\"\"\n[projects.\"/in\"]\n\"\"\"\n[projects.\"/out\"]\n\
                    c = 'x#y'  # \"\"\" is a comment\nd = \"q\\\"\"\n\
                    l = '''\\'''\n[t]\ne = '''\n[hidden]\n'''\n[last]";
        assert_eq!(
            toml_lines(text),
            [
                "a = 1",
                "b = \"\"\"",
                "[projects.\"/out\"]",
                "c = 'x#y'  # \"\"\" is a comment",
                "d = \"q\\\"\"",
                "l = '''\\'''",
                "[t]",
                "e = '''",
                "[last]"
            ]
        );
        // Up to 2 quotes before the closing delimiter are content.
        assert_eq!(
            toml_lines("v = \"\"\"a\"\"\"\"\n[h]"),
            ["v = \"\"\"a\"\"\"\"", "[h]"]
        );
        assert!(toml_lines("").is_empty());
    }

    #[test]
    fn toml_quoted_decodes_only_the_escapes_it_knows() {
        assert_eq!(
            toml_quoted(r#""a\\b\"c" rest"#),
            Some((r#"a\b"c"#.to_string(), " rest"))
        );
        assert_eq!(toml_quoted(r"'a\b' x"), Some((r"a\b".to_string(), " x")));
        assert_eq!(toml_quoted(r#""" tail"#), Some((String::new(), " tail")));
        assert_eq!(toml_quoted(r#""a\nb""#), None, "other escapes");
        assert_eq!(toml_quoted(r#""open"#), None, "unterminated");
        assert_eq!(toml_quoted("'open"), None);
        assert_eq!(toml_quoted("bare"), None);
        assert_eq!(toml_quoted(""), None);
        assert_eq!(toml_quoted(r#""é€" ok"#), Some(("é€".to_string(), " ok")));
    }

    #[test]
    fn toml_headers_and_values_are_read_strictly() {
        let header = |l: &str| toml_project_header(l);
        assert_eq!(header(r#"[projects."/a b"]"#).as_deref(), Some("/a b"));
        assert_eq!(header(r"[ projects . '/x' ]  # c").as_deref(), Some("/x"));
        assert_eq!(header("[\tprojects\t.\t\"/t\"\t]").as_deref(), Some("/t"));
        for no in [
            r#"[projects."/a"] x"#,
            r#"[projects./a]"#,
            r#"[projects."\u0041"]"#,
            r#"[other."/a"]"#,
            r#"projects."/a""#,
            r#"[projects."/a""#,
        ] {
            assert_eq!(header(no), None, "{no}");
        }
        assert_eq!(
            toml_string_value(r#"trust_level = "trusted""#, "trust_level").as_deref(),
            Some("trusted")
        );
        assert_eq!(
            toml_string_value("trust_level='x'", "trust_level").as_deref(),
            Some("x")
        );
        assert_eq!(toml_string_value("trust_level = 3", "trust_level"), None);
        assert_eq!(toml_string_value("other = 'x'", "trust_level"), None);
        assert_eq!(toml_string_value("trust_level 'x'", "trust_level"), None);
    }

    #[test]
    fn codex_trust_reads_the_last_level_of_a_root_table() {
        let r = roots(&["/p", "/real/p"]);
        let state = |text: &str| codex_trust(Some(text), &r).state;
        assert_eq!(
            state("[projects.\"/p\"]\ntrust_level = \"trusted\"\n"),
            TrustState::Trusted
        );
        assert_eq!(
            state("[projects.\"/real/p\"]\ntrust_level = 'trusted'\n"),
            TrustState::Trusted
        );
        assert_eq!(
            state("[projects.\"/p\"]\ntrust_level = \"trusted\"\ntrust_level = \"untrusted\"\n"),
            TrustState::Untrusted
        );
        assert_eq!(
            state("[projects.\"/other\"]\ntrust_level = \"trusted\"\n"),
            TrustState::Untrusted
        );
        assert_eq!(
            state("[projects.\"/p\"]\n[other]\ntrust_level = \"trusted\"\n"),
            TrustState::Untrusted,
            "a later table ends the root's table"
        );
        assert_eq!(
            state("note = \"\"\"\n[projects.\"/p\"]\ntrust_level = \"trusted\"\n\"\"\"\n"),
            TrustState::Untrusted,
            "inside a string"
        );
        let none = codex_trust(None, &r);
        assert_eq!(
            (none.state, none.reason.as_str()),
            (
                TrustState::Untrusted,
                "$CODEX_HOME/config.toml does not exist"
            )
        );
    }

    #[test]
    fn claude_and_agy_trust_name_the_store_in_their_reasons() {
        let r = roots(&["/p", "/real/p"]);
        let c = |t: Option<&str>| {
            let v = claude_trust(t, &r);
            (v.state, v.reason)
        };
        assert_eq!(
            c(None),
            (
                TrustState::Untrusted,
                "$CLAUDE_CONFIG_DIR/.claude.json does not exist".into()
            )
        );
        assert_eq!(
            c(Some("{oops")),
            (
                TrustState::Unknown,
                "$CLAUDE_CONFIG_DIR/.claude.json is not valid JSON".into()
            )
        );
        assert_eq!(
            c(Some(
                r#"{"projects":{"/real/p":{"hasTrustDialogAccepted":true}}}"#
            ))
            .0,
            TrustState::Trusted
        );
        assert_eq!(
            c(Some(
                r#"{"projects":{"/p":{"hasTrustDialogAccepted":false}}}"#
            ))
            .0,
            TrustState::Untrusted
        );
        assert_eq!(
            c(Some(
                r#"{"projects":{"/":{"hasTrustDialogAccepted":true}}}"#
            ))
            .0,
            TrustState::Untrusted,
            "a parent does not count"
        );
        let a = |t: Option<&str>| {
            let v = antigravity_trust(t, &r);
            (v.state, v.reason)
        };
        assert_eq!(
            a(None),
            (
                TrustState::Untrusted,
                "~/.gemini/antigravity-cli/settings.json does not exist".into()
            )
        );
        assert_eq!(a(Some("[")).0, TrustState::Unknown);
        assert_eq!(
            a(Some(r#"{"trustedWorkspaces":["/x","/p"]}"#)).0,
            TrustState::Trusted
        );
        assert_eq!(
            a(Some(r#"{"trustedWorkspaces":["/p/sub"]}"#)).0,
            TrustState::Untrusted
        );
        assert_eq!(
            a(Some(r#"{"trustedWorkspaces":"/p"}"#)).0,
            TrustState::Untrusted
        );
    }

    #[test]
    fn read_trust_covers_each_harness_and_an_unreadable_store() {
        let f = Fake::new();
        let r = std::slice::from_ref(&f.project);
        let all = [
            HarnessKind::Claude,
            HarnessKind::Codex,
            HarnessKind::Antigravity,
            HarnessKind::Pi,
        ];
        let got = read_trust(&all, &f.home, None, None, r);
        let states: Vec<(&str, TrustState)> =
            got.iter().map(|t| (t.harness.as_str(), t.state)).collect();
        assert_eq!(
            states,
            [
                ("claude", TrustState::Untrusted),
                ("codex", TrustState::Untrusted),
                ("antigravity", TrustState::Untrusted),
                ("pi", TrustState::Unknown)
            ]
        );
        assert_eq!(got[3].reason, "horch cannot read where pi keeps its trust");

        // A directory where the store should be cannot be read.
        std::fs::create_dir_all(f.claude_file()).unwrap();
        std::fs::create_dir_all(f.codex_file()).unwrap();
        std::fs::create_dir_all(f.home.join(".gemini/antigravity-cli/settings.json")).unwrap();
        let got = read_trust(&all[..3], &f.home, None, None, r);
        let reasons: Vec<&str> = got.iter().map(|t| t.reason.as_str()).collect();
        assert!(
            got.iter().all(|t| t.state == TrustState::Unknown),
            "{got:?}"
        );
        assert_eq!(
            reasons,
            [
                "$CLAUDE_CONFIG_DIR/.claude.json cannot be read",
                "$CODEX_HOME/config.toml cannot be read",
                "~/.gemini/antigravity-cli/settings.json cannot be read"
            ]
        );

        // CLAUDE_CONFIG_DIR moves the Claude store.
        let cfg = f.home.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join(".claude.json"),
            serde_json::json!({"projects": {f.project.to_string_lossy(): {"hasTrustDialogAccepted": true}}})
                .to_string(),
        )
        .unwrap();
        let got = read_trust(&[HarnessKind::Claude], &f.home, Some(&cfg), None, r);
        assert_eq!(got[0].state, TrustState::Trusted);
    }

    #[test]
    fn trust_roots_names_the_first_refusal() {
        let f = Fake::new();
        let why = |w: &Path, h: &Path| trust_roots(w, h).unwrap_err();
        assert_eq!(
            why(Path::new("rel/dir"), &f.home),
            "rel/dir is not an absolute path"
        );
        let missing = f.home.join("nope");
        assert_eq!(
            why(&missing, &f.home),
            format!("{} is not an existing directory", missing.display())
        );
        assert!(
            why(&f.project, Path::new("relhome")).starts_with("the home directory is not known")
        );
        assert!(why(&f.home, &f.home).ends_with("horch never trusts those"));
        assert!(why(Path::new("/"), &f.home).ends_with("horch never trusts those"));
        assert_eq!(
            trust_roots(&f.project, &f.home).unwrap(),
            vec![f.project.clone()]
        );
    }

    #[test]
    fn claude_edits_change_only_the_bytes_they_add() {
        let k = keys(&["/p"]);
        let edit = |t: &str| claude_add_trust(t, &k, "L");
        let accepted = r#""hasTrustDialogAccepted":true"#;
        assert_eq!(
            edit("{}").unwrap(),
            format!(r#"{{"projects":{{"/p":{{{accepted}}}}}}}"#)
        );
        assert_eq!(
            edit(r#"{"a":1}"#).unwrap(),
            format!(r#"{{"projects":{{"/p":{{{accepted}}}}},"a":1}}"#)
        );
        assert_eq!(
            edit(r#"{"projects":{"/q":{}}}"#).unwrap(),
            format!(r#"{{"projects":{{"/p":{{{accepted}}},"/q":{{}}}}}}"#)
        );
        assert_eq!(
            edit(r#"{"projects":{"/p":{"x":1}}}"#).unwrap(),
            format!(r#"{{"projects":{{"/p":{{{accepted},"x":1}}}}}}"#)
        );
        assert_eq!(
            edit(r#"{"projects": {"/p": {"hasTrustDialogAccepted" : false, "x": 1.50}}}"#).unwrap(),
            r#"{"projects": {"/p": {"hasTrustDialogAccepted" : true, "x": 1.50}}}"#
        );
        let two = keys(&["/p", "/real/p"]);
        let both = claude_add_trust("{}", &two, "L").unwrap();
        let v: serde_json::Value = serde_json::from_str(&both).unwrap();
        assert_eq!(v["projects"]["/p"]["hasTrustDialogAccepted"], true);
        assert_eq!(v["projects"]["/real/p"]["hasTrustDialogAccepted"], true);

        assert_eq!(edit("{oops").unwrap_err(), "L is not valid JSON");
        for odd in [
            "[]",
            r#"{"projects":3}"#,
            r#"{"projects":{"/p":3}}"#,
            r#"{"projects":[]}"#,
        ] {
            assert_eq!(
                edit(odd).unwrap_err(),
                "L has a shape horch does not edit",
                "{odd}"
            );
        }
    }

    #[test]
    fn agy_edits_put_the_new_workspaces_first() {
        let k = keys(&["/p", "/real/p"]);
        let edit = |t: &str| antigravity_add_trust(t, &k, "L");
        let list = |t: &str| -> serde_json::Value {
            serde_json::from_str::<serde_json::Value>(&edit(t).unwrap()).unwrap()
                ["trustedWorkspaces"]
                .clone()
        };
        assert_eq!(list("{}"), serde_json::json!(["/p", "/real/p"]));
        assert_eq!(
            edit("{}").unwrap(),
            r#"{"trustedWorkspaces":["/p","/real/p"]}"#
        );
        assert_eq!(
            list(r#"{"trustedWorkspaces":[]}"#),
            serde_json::json!(["/p", "/real/p"])
        );
        assert_eq!(
            list(r#"{"trustedWorkspaces": [ ]}"#),
            serde_json::json!(["/p", "/real/p"])
        );
        assert_eq!(
            edit(r#"{"keep":1,"trustedWorkspaces":["/q"]}"#).unwrap(),
            r#"{"keep":1,"trustedWorkspaces":["/p","/real/p","/q"]}"#
        );
        assert_eq!(edit("{oops").unwrap_err(), "L is not valid JSON");
        for odd in [
            "[]",
            r#"{"trustedWorkspaces":3}"#,
            r#"{"trustedWorkspaces":{}}"#,
        ] {
            assert_eq!(
                edit(odd).unwrap_err(),
                "L has a shape horch does not edit",
                "{odd}"
            );
        }
    }

    #[test]
    fn json_object_spans_cover_every_member_value() {
        let text = r#"  {"a" : 1, "b":[1,{"c":"}"}] ,"d":"x\"y","e":true, "n":null,"f":-2.5e3}"#;
        let obj = json_object_at(text, 0).unwrap();
        assert_eq!(obj.open, 2);
        let spans: Vec<(&str, &str)> = obj
            .members
            .iter()
            .map(|(k, s, e)| (k.as_str(), &text[*s..*e]))
            .collect();
        assert_eq!(
            spans,
            [
                ("a", "1"),
                ("b", r#"[1,{"c":"}"}]"#),
                ("d", r#""x\"y""#),
                ("e", "true"),
                ("n", "null"),
                ("f", "-2.5e3")
            ]
        );
        let at = text.find("true").unwrap();
        assert_eq!(json_member(&obj, "e"), Some((at, at + 4)));
        assert_eq!(json_member(&obj, "zz"), None);
        let dup_text = r#"{"k":1,"k":22}"#;
        let dup = json_object_at(dup_text, 0).unwrap();
        let (s, e) = json_member(&dup, "k").unwrap();
        assert_eq!(&dup_text[s..e], "22", "the last one wins");
        assert!(json_object_at("{}", 0).unwrap().members.is_empty());
        assert!(json_object_at(" { } ", 0).unwrap().members.is_empty());
        // From an offset.
        let nested = r#"{"o":{"i":1}}"#;
        let inner_at = nested.find("{\"i").unwrap();
        assert_eq!(json_object_at(nested, inner_at).unwrap().members.len(), 1);
        for bad in [
            "[]",
            "",
            "{",
            r#"{"a""#,
            r#"{"a" 1}"#,
            r#"{"a":}"#,
            r#"{1:2}"#,
            r#"{"a":1 "b":2}"#,
            r#"{"a":1,}"#,
            r#"{"a":1"#,
            r#"{"a":"open}"#,
            r#"{"a":[1,2}"#,
        ] {
            assert!(json_object_at(bad, 0).is_none(), "{bad}");
        }
    }

    #[test]
    fn json_value_ends_stop_at_the_value() {
        let end = |t: &str| json_value_end(t.as_bytes(), 0);
        assert_eq!(end(r#""a\"b" ,"#), Some(6));
        assert_eq!(end("[1,[2,\"]\"]] x"), Some(11));
        assert_eq!(end(r#"{"a":{"b":[]}},"#), Some(14));
        assert_eq!(end("123,"), Some(3));
        assert_eq!(end("true}"), Some(4));
        assert_eq!(end("null"), Some(4));
        assert_eq!(end(""), None);
        assert_eq!(end(","), None, "no value");
        assert_eq!(end("[1,2"), None);
        assert_eq!(end("\"open"), None);
        assert_eq!(end("{\"a\":\"x"), None);
    }

    #[test]
    fn codex_edits_append_tables_or_refuse() {
        let k = keys(&["/p", "/real/p"]);
        let edit = |t: &str| codex_add_trust(t, &k, "L");
        let tables = "[projects.\"/p\"]\ntrust_level = \"trusted\"\n[projects.\"/real/p\"]\ntrust_level = \"trusted\"\n";
        assert_eq!(edit("").unwrap(), tables);
        assert_eq!(edit("a = 1").unwrap(), format!("a = 1\n\n{tables}"));
        assert_eq!(edit("a = 1\n").unwrap(), format!("a = 1\n\n{tables}"));
        assert_eq!(
            edit("[projects.\"/other\"]\ntrust_level = \"trusted\"\n").unwrap(),
            format!("[projects.\"/other\"]\ntrust_level = \"trusted\"\n\n{tables}")
        );
        assert!(edit("s = \"\"\"x\"\"\"")
            .unwrap_err()
            .starts_with("L has a multi-line string"));
        assert!(edit("s = '''x'''")
            .unwrap_err()
            .starts_with("L has a multi-line string"));
        for odd in [
            "projects.x = 1",
            "[projects]",
            "[\"projects\".\"/p\"]",
            "[pro\\u006aects.\"/q\"]",
            "projects = {}",
        ] {
            assert!(
                edit(odd)
                    .unwrap_err()
                    .starts_with("L keeps projects in a form horch does not edit"),
                "{odd}"
            );
        }
        assert_eq!(
            edit("[projects.\"/p\"]\ntrust_level = \"untrusted\"\n").unwrap_err(),
            "L sets trust_level = \"untrusted\" for /p; horch keeps the operator's choice"
        );
        assert_eq!(
            edit("[projects.\"/real/p\"]\nother = 1\n").unwrap_err(),
            "L has a [projects.\"/real/p\"] table without trust_level = \"trusted\"; horch does not edit it"
        );
        assert_eq!(
            codex_add_trust("", &keys(&["/p\n"]), "L").unwrap_err(),
            "L: the path \"/p\\n\" has a control character"
        );
        let quoted = codex_add_trust("", &keys(&["/a\"b\\c"]), "L").unwrap();
        assert_eq!(
            quoted,
            "[projects.\"/a\\\"b\\\\c\"]\ntrust_level = \"trusted\"\n"
        );
        assert_eq!(
            codex_trust(Some(&quoted), &roots(&["/a\"b\\c"])).state,
            TrustState::Trusted
        );
        // Two copies of a key would make 2 tables.
        let dup = codex_add_trust("", &keys(&["/p", "/p"]), "L").unwrap_err();
        assert_eq!(dup, "L would have 2 tables for \"/p\"; horch wrote nothing");
    }

    #[test]
    fn projects_lines_are_detected_in_every_spelling() {
        for yes in [
            "[projects]",
            "[projects.\"/a\"]",
            "[a.\\u0070]",
            "projects = 1",
            "projects.x = 1",
            "\"projects\" = 1",
            "'projects' . x = 1",
            "\"pro\\u006aects\" = 1",
            "projects\t= 2",
        ] {
            assert!(toml_projects_line(yes), "{yes}");
        }
        for no in [
            "[other]",
            "a = 1",
            "projectsx = 1",
            "\"other\" = 1",
            "# projects = 1",
            "name = \"projects\"",
        ] {
            assert!(!toml_projects_line(no), "{no}");
        }
    }

    #[test]
    fn ensure_skips_for_a_missing_directory_or_store() {
        let f = Fake::new();
        // No directory for the store.
        let cfg = f.home.join("no-cfg");
        let got =
            ensure_trusted(HarnessKind::Claude, &f.project, &f.home, Some(&cfg), None).unwrap();
        assert_eq!(
            got,
            TrustWrite::Skipped(format!(
                "the directory of $CLAUDE_CONFIG_DIR/.claude.json ({}) does not exist",
                cfg.join(".claude.json").display()
            ))
        );
        // Claude and Antigravity never create a store.
        assert_eq!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Skipped(
                "$CLAUDE_CONFIG_DIR/.claude.json does not exist: claude has never run with this store"
                    .into()
            )
        );
        std::fs::create_dir_all(f.home.join(".gemini/antigravity-cli")).unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Antigravity, &f.project),
            TrustWrite::Skipped(
                "~/.gemini/antigravity-cli/settings.json does not exist: antigravity has never run with this store"
                    .into()
            )
        );
        // A store that is not JSON is the store's problem.
        std::fs::write(f.claude_file(), "{oops").unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Claude, &f.project),
            TrustWrite::Skipped("$CLAUDE_CONFIG_DIR/.claude.json is not valid JSON".into())
        );
        // Codex creates its config.toml when its directory exists.
        std::fs::create_dir_all(f.codex_file().parent().unwrap()).unwrap();
        assert_eq!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::Written(f.codex_file())
        );
        assert_eq!(
            std::fs::read_to_string(f.codex_file()).unwrap(),
            format!(
                "[projects.\"{}\"]\ntrust_level = \"trusted\"\n",
                f.project.display()
            )
        );
        assert_eq!(
            f.ensure(HarnessKind::Codex, &f.project),
            TrustWrite::AlreadyTrusted
        );
    }

    #[test]
    fn ensure_fails_loudly_on_an_unreadable_store_or_a_store_that_keeps_changing() {
        let f = Fake::new();
        std::fs::create_dir_all(f.claude_file()).unwrap();
        let err = ensure_trusted(HarnessKind::Claude, &f.project, &f.home, None, None)
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            format!(
                "$CLAUDE_CONFIG_DIR/.claude.json ({}) cannot be read",
                f.claude_file().display()
            )
        );
        std::fs::remove_dir(f.claude_file()).unwrap();

        std::fs::write(f.claude_file(), "{}").unwrap();
        let mut calls = 0;
        let err = ensure_trusted_with(
            HarnessKind::Claude,
            &f.project,
            &f.home,
            None,
            None,
            &mut || {
                calls += 1;
                std::fs::write(f.claude_file(), format!("{{\"n\":{calls}}}")).unwrap();
            },
        )
        .unwrap_err()
        .to_string();
        assert_eq!(calls, WRITE_TRIES);
        assert_eq!(
            err,
            format!(
                "$CLAUDE_CONFIG_DIR/.claude.json ({}) changed during each of 3 tries; horch wrote nothing",
                f.claude_file().display()
            )
        );
        let mut left: Vec<String> = std::fs::read_dir(&f.home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [".claude.json", "projects"],
            "no temp file or lock stays"
        );
    }

    #[test]
    fn the_store_lock_and_temp_file_report_their_failures() {
        let tmp = tempfile::tempdir().unwrap();
        let err = StoreLock::take(&tmp.path().join("no-dir/store"))
            .err()
            .expect("a missing directory is an error")
            .to_string();
        assert!(err.starts_with("creating the lock "), "{err}");

        let store = tmp.path().join("store.json");
        std::fs::write(&store, "{}").unwrap();
        let temp = TempStore::write(&store, "new").unwrap();
        let path = temp.0.clone().unwrap();
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("store.json.horch-tmp."));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        let gone = tmp.path().join("no-dir/store.json");
        let err = temp.rename_over(&gone).unwrap_err().to_string();
        assert!(err.starts_with("renaming "), "{err}");
        assert!(!path.exists(), "the failed rename removes its temp file");

        let temp = TempStore::write(&store, "again").unwrap();
        let path = temp.0.clone().unwrap();
        temp.rename_over(&store).unwrap();
        assert_eq!(std::fs::read_to_string(&store).unwrap(), "again");
        assert!(!path.exists());

        // Discarding a file that is already gone is not an error.
        let temp = TempStore::write(&store, "x").unwrap();
        std::fs::remove_file(temp.0.as_ref().unwrap()).unwrap();
        temp.discard().unwrap();
        // A dropped temp store removes its file.
        let temp = TempStore::write(&store, "y").unwrap();
        let path = temp.0.clone().unwrap();
        drop(temp);
        assert!(!path.exists());
    }

    #[test]
    fn temp_names_carry_a_pid_and_12_lowercase_hex() {
        assert_eq!(temp_pid("123.0123456789ab"), Some(123));
        for bad in [
            "",
            "123",
            "x.0123456789ab",
            "123.0123456789AB",
            "123.0123456789a",
            "123.0123456789abc",
            ".0123456789ab",
            "-1.0123456789ab",
        ] {
            assert_eq!(temp_pid(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_trust_note_and_fix_name_the_store_never_its_content() {
        let w = Path::new("/w");
        assert_eq!(
            trust_note(HarnessKind::Claude, w, &Ok(TrustWrite::AlreadyTrusted)),
            None
        );
        assert_eq!(
            trust_note(
                HarnessKind::Claude,
                w,
                &Ok(TrustWrite::Written("/s".into()))
            )
            .unwrap(),
            "horch: trusted /w for claude (/s)"
        );
        assert_eq!(
            trust_note(
                HarnessKind::Codex,
                w,
                &Ok(TrustWrite::Skipped("why".into()))
            )
            .unwrap(),
            "horch: NOTE: horch did not trust /w for codex: why"
        );
        assert_eq!(
            trust_note(HarnessKind::Codex, w, &Err(anyhow::anyhow!("boom"))).unwrap(),
            "horch: NOTE: horch could not trust /w for codex: boom"
        );
        assert_eq!(
            trust_fix(HarnessKind::Codex, Path::new("/a b/it's")),
            "cd '/a b/it'\\''s' && codex  (choose \"Trust and continue\", then exit)"
        );
        assert!(trust_fix(HarnessKind::Pi, w).contains("&& pi  (accept"));
    }
}
