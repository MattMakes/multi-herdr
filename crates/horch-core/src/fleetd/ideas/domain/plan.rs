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

    pub fn parse(word: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == word)
    }

    pub fn is_code(self) -> bool {
        matches!(
            self,
            Kind::Feature | Kind::Quality | Kind::Refactor | Kind::Security
        )
    }

    pub fn is_judgment(self) -> bool {
        matches!(
            self,
            Kind::Research | Kind::Design | Kind::Docs | Kind::Report | Kind::Creative
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acceptance {
    Run(String),
    Sections { file: String, sections: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketFile {
    pub path: String,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Budget {
    pub max_minutes: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlannedTask {
    pub key: String,
    pub title: String,
    pub ticket: Option<TicketFile>,
    pub kind: Option<Kind>,
    pub kind_word: String,
    pub profile: String,
    pub covers: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub acceptance: Vec<Acceptance>,
    pub after: Vec<String>,
    pub judges: Option<String>,
    pub budget: Option<Budget>,
    pub reason: Option<String>,
    pub no_decisions: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub idea: String,
    pub repo: String,
    pub feature: String,
    pub base_branch: String,
    pub fable_approved: bool,
    pub tasks: Vec<PlannedTask>,
}

impl Plan {
    pub fn task(&self, key: &str) -> Option<&PlannedTask> {
        self.tasks.iter().find(|t| t.key == key)
    }
}
