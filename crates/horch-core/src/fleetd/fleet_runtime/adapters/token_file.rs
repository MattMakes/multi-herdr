use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use horch_platform::channel::User;
use horch_platform::fs as platform;
use horch_platform::NotSupported;

use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};

const TOKEN_BYTES: usize = 32;

const FILE_LIMIT: u64 = 128;

#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn matches(&self, text: &str) -> bool {
        let token = self.0.as_bytes();
        let given = text.as_bytes();
        let mut diff = u8::from(token.len() != given.len());
        for (i, byte) in token.iter().enumerate() {
            diff |= byte ^ given.get(i).copied().unwrap_or(0);
        }
        std::hint::black_box(diff) == 0
    }

    pub fn text(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

pub fn path(fleet_root: &Path) -> PathBuf {
    run_folder(fleet_root).join("operator.token")
}

fn run_folder(fleet_root: &Path) -> PathBuf {
    fleet_root.join("run")
}

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

fn make_run_folder(run: &Path) -> Result<()> {
    platform::create_dir_owner_only(run).with_context(|| format!("making {}", run.display()))?;
    let meta =
        std::fs::symlink_metadata(run).with_context(|| format!("reading {}", run.display()))?;
    if !meta.is_dir() {
        bail!("{} is not a folder", run.display());
    }
    match platform::set_mode(run, 0o700) {
        Err(e) if is_not_supported(&e) => Ok(()),
        set => set.with_context(|| format!("setting mode 0700 on {}", run.display())),
    }
}

fn is_not_supported(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<NotSupported>())
}

fn create(path: &Path) -> io::Result<()> {
    let mut file = platform::create_new_owner_only(path)?;
    let written = file
        .write_all(new_token_text().as_bytes())
        .and_then(|()| file.sync_all());
    if let Err(e) = written {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(e);
    }
    if let Some(folder) = path.parent() {
        match platform::sync_dir(folder) {
            Err(e) if is_not_supported(&e) => {}
            synced => synced?,
        }
    }
    Ok(())
}

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
