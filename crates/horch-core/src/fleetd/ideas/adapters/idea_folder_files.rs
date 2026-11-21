use std::fmt;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use horch_platform::channel::User;
use serde::{Deserialize, Serialize};

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::adapters::protected_files::{read_protected, Rule};
use crate::fleetd::ideas::domain::approval::APPROVAL_JSON;
use crate::fleetd::ideas::domain::idea_folder::{
    check_idea_id, check_requirements, check_spec, interview_answers, interview_entry,
    interview_heading, is_idea_id, join_front_matter, parse_assumptions, split_front_matter,
    Assumptions, IdeaFile, Invalid, AMENDMENTS, ASSUMPTIONS_MD, FILE_LIMIT, IDEA_MD, INTERVIEW_MD,
    REQUIREMENTS_YAML, SPEC_MD,
};
use crate::fleetd::ideas::domain::plan_validator::Requirement;
use crate::fsx::{self, Created, FsxError};

const FILE_MODE: u32 = 0o600;

#[derive(Debug)]
pub enum FolderError {
    Missing { file: String },
    Rejected { file: String, reason: String },
    Invalid { file: String, reason: String },
    Write(FsxError),
}

impl fmt::Display for FolderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FolderError::Missing { file } => write!(f, "{file} does not exist"),
            FolderError::Rejected { file, reason } => write!(f, "{file} is rejected: {reason}"),
            FolderError::Invalid { file, reason } => write!(f, "{file} is not valid: {reason}"),
            FolderError::Write(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FolderError {}

impl From<FsxError> for FolderError {
    fn from(e: FsxError) -> FolderError {
        FolderError::Write(e)
    }
}

fn invalid_in(file: &str) -> impl Fn(Invalid) -> FolderError + '_ {
    move |e| FolderError::Invalid {
        file: file.to_string(),
        reason: e.reason,
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Front {
    id: String,
    created: String,
    source: String,
    priority: i64,
    depends_on: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequirementsFile {
    idea: String,
    requirements: Vec<RequirementEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequirementEntry {
    id: String,
    text: String,
    #[serde(default)]
    cites: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IdeaFolder {
    dir: PathBuf,
    idea: String,
    owner: User,
}

impl IdeaFolder {
    pub fn new(ideas: &Path, idea: &str, owner: User) -> Result<IdeaFolder, FolderError> {
        if !is_idea_id(idea) {
            return Err(FolderError::Invalid {
                file: idea.to_string(),
                reason: format!("`{idea}` is not an idea id I-NNNN"),
            });
        }
        Ok(IdeaFolder {
            dir: ideas.join(idea),
            idea: idea.to_string(),
            owner,
        })
    }

    pub fn create(ideas: &Path, idea: &str, owner: User) -> Result<IdeaFolder, FolderError> {
        let folder = IdeaFolder::new(ideas, idea, owner)?;
        fsx::ensure_private_dir(&folder.dir)?;
        fsx::ensure_private_dir(&folder.dir.join(AMENDMENTS))?;
        Ok(folder)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn write_idea(&self, idea: &IdeaFile) -> Result<Created, FolderError> {
        let front = Front {
            id: idea.id.clone(),
            created: idea.created.clone(),
            source: idea.source.clone(),
            priority: idea.priority,
            depends_on: idea.depends_on.clone(),
        };
        let front = serde_yaml::to_string(&front).map_err(|e| FolderError::Invalid {
            file: IDEA_MD.to_string(),
            reason: e.to_string(),
        })?;
        let file = join_front_matter(&front, &idea.text);
        Ok(fsx::create_immutable(
            &self.dir.join(IDEA_MD),
            file.as_bytes(),
            FILE_MODE,
        )?)
    }

    pub fn read_idea(&self) -> Result<IdeaFile, FolderError> {
        let file = self.read_text(IDEA_MD)?;
        let (front, text) = split_front_matter(&file).map_err(invalid_in(IDEA_MD))?;
        let front: Front = serde_yaml::from_str(front).map_err(|e| FolderError::Invalid {
            file: IDEA_MD.to_string(),
            reason: e.to_string(),
        })?;
        check_idea_id(IDEA_MD, &front.id, &self.idea).map_err(invalid_in(IDEA_MD))?;
        Ok(IdeaFile {
            id: front.id,
            created: front.created,
            source: front.source,
            priority: front.priority,
            depends_on: front.depends_on,
            text: text.to_string(),
        })
    }

    pub fn write_amendment(&self, n: u32, text: &str) -> Result<Created, FolderError> {
        Ok(fsx::create_immutable(
            &self.dir.join(amendment_file(n)),
            text.as_bytes(),
            FILE_MODE,
        )?)
    }

    pub fn read_amendments(&self) -> Result<Vec<String>, FolderError> {
        let entries = match std::fs::read_dir(self.dir.join(AMENDMENTS)) {
            Ok(entries) => entries,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => {
                return Err(FsxError::Io {
                    path: self.dir.join(AMENDMENTS),
                    source,
                }
                .into())
            }
        };
        let mut numbers: Vec<u32> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| amendment_number(&e.file_name().to_string_lossy()))
            .collect();
        numbers.sort_unstable();
        numbers
            .into_iter()
            .map(|n| self.read_text(&amendment_file(n)))
            .collect()
    }

    pub fn append_interview(&self, question: &str, answer: &str) -> Result<String, FolderError> {
        let mut file = match self.read_text(INTERVIEW_MD) {
            Ok(file) => file,
            Err(FolderError::Missing { .. }) => interview_heading(&self.idea),
            Err(e) => return Err(e),
        };
        let n = interview_answers(&file).len() + 1;
        file.push_str(&interview_entry(n, question, answer));
        self.replace(INTERVIEW_MD, &file)?;
        Ok(format!("Q{n}"))
    }

    pub fn read_answers(&self) -> Result<Vec<String>, FolderError> {
        match self.read_text(INTERVIEW_MD) {
            Ok(file) => Ok(interview_answers(&file)),
            Err(FolderError::Missing { .. }) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    pub fn write_assumptions(&self, text: &str) -> Result<(), FolderError> {
        self.replace(ASSUMPTIONS_MD, text)
    }

    pub fn read_assumptions(&self) -> Result<Assumptions, FolderError> {
        parse_assumptions(&self.read_text(ASSUMPTIONS_MD)?).map_err(invalid_in(ASSUMPTIONS_MD))
    }

    pub fn write_spec(&self, text: &str) -> Result<(), FolderError> {
        self.replace(SPEC_MD, text)
    }

    pub fn read_spec(&self) -> Result<String, FolderError> {
        let file = self.read_text(SPEC_MD)?;
        check_spec(&file).map_err(invalid_in(SPEC_MD))?;
        Ok(file)
    }

    pub fn write_requirements(&self, text: &str) -> Result<(), FolderError> {
        self.replace(REQUIREMENTS_YAML, text)
    }

    pub fn read_requirements(&self) -> Result<Vec<Requirement>, FolderError> {
        let file = self.read_text(REQUIREMENTS_YAML)?;
        let parsed: RequirementsFile =
            serde_yaml::from_str(&file).map_err(|e| FolderError::Invalid {
                file: REQUIREMENTS_YAML.to_string(),
                reason: e.to_string(),
            })?;
        check_idea_id(REQUIREMENTS_YAML, &parsed.idea, &self.idea)
            .map_err(invalid_in(REQUIREMENTS_YAML))?;
        let requirements: Vec<Requirement> = parsed
            .requirements
            .into_iter()
            .map(|r| Requirement {
                id: r.id,
                text: r.text,
                cites: r.cites,
            })
            .collect();
        let assumptions = match self.read_assumptions() {
            Ok(a) => a.ids(),
            Err(FolderError::Missing { .. }) => Vec::new(),
            Err(e) => return Err(e),
        };
        check_requirements(&requirements, &assumptions, &self.read_answers()?)
            .map_err(invalid_in(REQUIREMENTS_YAML))?;
        Ok(requirements)
    }

    pub fn sha256_of(&self, name: &str) -> Result<String, FolderError> {
        Ok(sha256_bytes(self.read_text(name)?.as_bytes()).hex())
    }

    pub fn write_approval(&self, text: &str) -> Result<(), FolderError> {
        self.replace(APPROVAL_JSON, text)
    }

    pub fn read_approval(&self) -> Result<String, FolderError> {
        self.read_text(APPROVAL_JSON)
    }

    fn replace(&self, name: &str, text: &str) -> Result<(), FolderError> {
        Ok(fsx::write_atomic(
            &self.dir.join(name),
            text.as_bytes(),
            FILE_MODE,
        )?)
    }

    pub fn read_text(&self, name: &str) -> Result<String, FolderError> {
        let path = self.dir.join(name);
        if let Err(e) = std::fs::symlink_metadata(&path) {
            if e.kind() == ErrorKind::NotFound {
                return Err(FolderError::Missing {
                    file: name.to_string(),
                });
            }
        }
        let rule = Rule {
            root: &self.dir,
            owner: &self.owner,
            limit: FILE_LIMIT,
        };
        let bytes = read_protected(&path, &rule).map_err(|r| FolderError::Rejected {
            file: name.to_string(),
            reason: r.reason,
        })?;
        String::from_utf8(bytes).map_err(|_| FolderError::Invalid {
            file: name.to_string(),
            reason: "the file is not UTF-8 text".to_string(),
        })
    }
}

fn amendment_file(n: u32) -> String {
    format!("{AMENDMENTS}/{n:04}.md")
}

fn amendment_number(name: &str) -> Option<u32> {
    let n = name.strip_suffix(".md")?.parse().ok()?;
    (amendment_file(n) == format!("{AMENDMENTS}/{name}")).then_some(n)
}
