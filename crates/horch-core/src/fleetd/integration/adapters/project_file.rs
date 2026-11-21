use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::fleetd::integration::domain::project::{
    Gates, Pr, ProjectFileReader, ProjectSettings, Remote, FILE_NAME,
};

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

pub fn parse(text: &str) -> Result<ProjectSettings> {
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

pub fn load(reader: &dyn ProjectFileReader, tip: &str) -> Result<ProjectSettings> {
    let text = reader
        .read(tip)
        .with_context(|| format!("reading the project {FILE_NAME} at {tip}"))?;
    match text {
        None => Ok(ProjectSettings::default()),
        Some(text) => parse(&text).with_context(|| format!("at {tip}")),
    }
}

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
