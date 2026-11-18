//! The operator token `<fleet root>/run/operator.token`: fleetd makes it 1
//! time with mode 0600, a `horch` command reads it, and fleetd compares it
//! in constant time.
//!
//! Requirements: SR-8. The token is 32 random bytes as 64 lowercase hex
//! digits, with no newline. A token file that exists is never replaced: a
//! file that fails a check is an error with its reason.

use std::fmt;
use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::channel::User;
use horch_platform::fs as platform;

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};

/// The count of random bytes in a token.
const TOKEN_BYTES: usize = 32;

/// The size limit of the token file, in bytes.
const FILE_LIMIT: u64 = 128;

/// The operator token. Its text never shows in a log: `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    /// True when `text` is this token. The time of the compare does not
    /// depend on where `text` differs from the token.
    pub fn matches(&self, text: &str) -> bool {
        let token = self.0.as_bytes();
        let given = text.as_bytes();
        let mut diff = u8::from(token.len() != given.len());
        for (i, byte) in token.iter().enumerate() {
            diff |= byte ^ given.get(i).copied().unwrap_or(0);
        }
        std::hint::black_box(diff) == 0
    }

    /// The text of the token, which a `horch` command sends with an
    /// `operator` request.
    pub fn text(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

/// The token file of the fleet at `fleet_root`.
pub fn path(fleet_root: &Path) -> PathBuf {
    run_folder(fleet_root).join("operator.token")
}

fn run_folder(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run")
}

/// Make the token file when it does not exist, then read it. fleetd calls
/// it at each start. A token file that exists is read and never replaced.
pub fn ensure(fleet_root: &Path) -> Result<Token> {
    make_run_folder(&run_folder(fleet_root))?;
    let path = path(fleet_root);
    match create(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(anyhow!(e).context(format!("making {}", path.display()))),
    }
    read(fleet_root)
}

/// Read the token file through the protected file reader: a regular file,
/// not a link, of this user, mode 0600 or narrower, in the folder `run/` of
/// mode 0700 or narrower, at most 128 bytes, and 64 lowercase hex digits.
pub fn read(fleet_root: &Path) -> Result<Token> {
    let path = path(fleet_root);
    let refused = |reason: String| anyhow!("token file {} is refused: {reason}", path.display());
    let owner = User::current().context("reading the user of this process")?;
    let rule = Rule {
        root: &run_folder(fleet_root),
        owner: &owner,
        limit: FILE_LIMIT,
    };
    let bytes = read_protected(&path, &rule).map_err(|r| refused(r.reason))?;
    let valid = bytes.len() == TOKEN_BYTES * 2
        && bytes.iter().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !valid {
        return Err(refused(format!(
            "the text is not {} lowercase hex digits",
            TOKEN_BYTES * 2
        )));
    }
    let text = String::from_utf8(bytes).map_err(|_| refused("the text is not UTF-8".into()))?;
    Ok(Token(text))
}

/// Make the folder `run/` with mode 0700 when it does not exist, and narrow
/// the mode of a folder `run/` that exists to 0700. A link or a file at
/// that path is an error.
fn make_run_folder(run: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    platform::create_dir_mode(&mut builder, 0o700)
        .map_err(|e| anyhow!("making {} with mode 0700: {e}", run.display()))?;
    builder
        .create(run)
        .with_context(|| format!("making {}", run.display()))?;
    let meta =
        std::fs::symlink_metadata(run).with_context(|| format!("reading {}", run.display()))?;
    if !meta.is_dir() {
        bail!("{} is not a folder", run.display());
    }
    platform::set_mode(run, 0o700)
        .with_context(|| format!("setting mode 0700 on {}", run.display()))
}

/// Create the token file at `path` only when no file is there (`O_EXCL`),
/// with mode 0600 at the create, and write a new token to it. A failed
/// write removes the file, so no part of a token stays.
fn create(path: &Path) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    platform::create_mode(&mut options, 0o600)
        .map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e.to_string()))?;
    let mut file = options.open(path)?;
    let written = file
        .write_all(new_token_text().as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    if let Some(folder) = path.parent() {
        platform::sync_dir(folder)?;
    }
    Ok(())
}

/// 32 random bytes from the random source of the OS, as 64 lowercase hex
/// digits. The source is the `uuid` v4 generator; each UUID gives the 14
/// bytes that hold no version or variant bits.
fn new_token_text() -> String {
    let mut bytes = Vec::with_capacity(TOKEN_BYTES + 14);
    while bytes.len() < TOKEN_BYTES {
        let uuid = uuid::Uuid::new_v4();
        let random = uuid
            .as_bytes()
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 6 && *i != 8)
            .map(|(_, b)| *b);
        bytes.extend(random);
    }
    bytes.truncate(TOKEN_BYTES);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
