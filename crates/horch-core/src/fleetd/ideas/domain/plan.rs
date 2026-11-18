//! plan.yaml: the schema of a task and the tickets in format C1 that it
//! names (FR-3, FR-4).
//!
//! A `Plan` holds what plan.yaml says, as written. The reader keeps each
//! value that the plan validator refuses (an unknown kind, an empty field),
//! so that the verdict names every problem and not only the first.

/// The kind of a task (FR-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Feature,
    Quality,
    Refactor,
    Security,
    Research,
    Design,
    Docs,
    Report,
    Creative,
    /// A judge task: it judges 1 task of a judgment kind and needs no judge.
    Judge,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Feature,
        Kind::Quality,
        Kind::Refactor,
        Kind::Security,
        Kind::Research,
        Kind::Design,
        Kind::Docs,
        Kind::Report,
        Kind::Creative,
        Kind::Judge,
    ];

    /// The word of plan.yaml.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Feature => "feature",
            Kind::Quality => "quality",
            Kind::Refactor => "refactor",
            Kind::Security => "security",
            Kind::Research => "research",
            Kind::Design => "design",
            Kind::Docs => "docs",
            Kind::Report => "report",
            Kind::Creative => "creative",
            Kind::Judge => "judge",
        }
    }

    /// The kind of a plan.yaml word.
    pub fn parse(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == word)
    }

    /// A code kind: its check runs a command.
    pub fn is_code(self) -> bool {
        matches!(
            self,
            Kind::Feature | Kind::Quality | Kind::Refactor | Kind::Security
        )
    }

    /// A judgment kind: a structural check plus a paired judge task.
    pub fn is_judgment(self) -> bool {
        matches!(
            self,
            Kind::Research | Kind::Design | Kind::Docs | Kind::Report | Kind::Creative
        )
    }
}

/// 1 item of the acceptance check of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acceptance {
    /// `run: <command>`: the command exits 0.
    Run(String),
    /// `sections: [<file>, <section>...]`: the file exists and has each
    /// section.
    Sections { file: String, sections: Vec<String> },
}

/// The ticket file that a task names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketFile {
    /// The path as plan.yaml has it, relative to the folder of plan.yaml.
    pub path: String,
    /// What the reader found wrong with the file: it is missing, outside
    /// the plan folder, or not in format C1 (each C1 problem with its line).
    pub problems: Vec<String>,
}

/// The budget of a task.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Budget {
    pub max_minutes: Option<u32>,
}

/// 1 task of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannedTask {
    pub key: String,
    pub title: String,
    pub ticket: Option<TicketFile>,
    /// `None` when plan.yaml has a word that is not a kind; `kind_word`
    /// keeps that word.
    pub kind: Option<Kind>,
    pub kind_word: String,
    pub profile: String,
    /// The requirement ids that the task covers.
    pub covers: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub acceptance: Vec<Acceptance>,
    /// The keys of the tasks that must be done and landed first.
    pub after: Vec<String>,
    /// For a judge task: the key of the task it judges. Not a dependency.
    pub judges: Option<String>,
    pub budget: Option<Budget>,
    /// Why the task needs a higher effort than the default (model guide).
    pub reason: Option<String>,
    /// The task makes no decision of its own (model guide, a Sonnet task).
    pub no_decisions: bool,
}

/// The plan.yaml of 1 idea.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub idea: String,
    pub repo: String,
    pub feature: String,
    /// The feature branch: each task of the plan lands on it.
    pub base_branch: String,
    /// `fable: approved`: the operator allows a Fable profile (model guide).
    pub fable_approved: bool,
    pub tasks: Vec<PlannedTask>,
}

impl Plan {
    /// The task of `key`, the first one when 2 tasks share it.
    pub fn task(&self, key: &str) -> Option<&PlannedTask> {
        self.tasks.iter().find(|t| t.key == key)
    }
}
