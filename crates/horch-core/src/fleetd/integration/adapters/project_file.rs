//! Reads the gates of a project from its file (ticket 04).
//!
//! The project file is `fleet.toml` at the root of the project, committed in
//! the repository. It is not the `fleet.toml` of the fleet root. A missing
//! file or a missing key gives the default. An unknown key, a value of the
//! wrong type and an empty command are refused, with the name of the key.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::fleetd::integration::domain::project::{
    Gates, Pr, ProjectFileReader, ProjectSettings, Remote, FILE_NAME,
};

/// The keys as the file has them: each is optional.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileKeys {
    gates: Option<GateKeys>,
    pr: Option<String>,
    remote: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GateKeys {
    fast: Option<String>,
    full: Option<String>,
}

/// Parse the text of a project `fleet.toml`, then validate it.
pub fn parse(text: &str) -> Result<ProjectSettings> {
    // serde names an unknown key and a value of the wrong type.
    let file: FileKeys =
        toml::from_str(text).map_err(|e| anyhow::anyhow!("project {FILE_NAME}: {e}"))?;
    let pr = match file.pr.as_deref() {
        None => Pr::default(),
        Some(word) => Pr::parse(word).ok_or_else(|| {
            anyhow::anyhow!("project {FILE_NAME}: pr: `{word}` is not gh or manual")
        })?,
    };
    let gates = file.gates.map_or_else(Gates::default, |g| Gates {
        fast: g.fast,
        full: g.full,
    });
    let settings = ProjectSettings {
        gates,
        pr,
        remote: file
            .remote
            .as_deref()
            .map_or_else(Remote::default, Remote::from_word),
    };
    settings
        .validate()
        .map_err(|e| anyhow::anyhow!("project {FILE_NAME}: {e}"))?;
    Ok(settings)
}

/// The settings of the project at the commit `tip`, through `reader`. No
/// file at the tip gives the defaults: no gate.
pub fn load(reader: &dyn ProjectFileReader, tip: &str) -> Result<ProjectSettings> {
    let text = reader
        .read(tip)
        .with_context(|| format!("reading the project {FILE_NAME} at {tip}"))?;
    match text {
        None => Ok(ProjectSettings::default()),
        Some(text) => parse(&text).with_context(|| format!("at {tip}")),
    }
}

/// The test adapter of [`ProjectFileReader`]: it reads `<root>/fleet.toml`
/// of a folder, for each tip.
pub struct FolderReader {
    root: PathBuf,
}

impl FolderReader {
    pub fn new(root: &Path) -> Self {
        FolderReader {
            root: root.to_path_buf(),
        }
    }
}

impl ProjectFileReader for FolderReader {
    fn read(&self, _tip: &str) -> std::io::Result<Option<String>> {
        match std::fs::read_to_string(self.root.join(FILE_NAME)) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }
}
