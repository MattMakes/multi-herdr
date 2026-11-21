pub const FILE_NAME: &str = "fleet.toml";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gates {
    pub fast: Option<String>,
    pub full: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pr {
    #[default]
    Gh,
    Manual,
}

impl Pr {
    pub fn parse(word: &str) -> Option<Pr> {
        match word {
            "gh" => Some(Pr::Gh),
            "manual" => Some(Pr::Manual),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    Named(String),
    None,
}

impl Default for Remote {
    fn default() -> Self {
        Remote::Named(DEFAULT_REMOTE.to_string())
    }
}

pub const DEFAULT_REMOTE: &str = "origin";

impl Remote {
    pub fn from_word(word: &str) -> Remote {
        if word == "none" {
            Remote::None
        } else {
            Remote::Named(word.to_string())
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSettings {
    pub gates: Gates,
    pub pr: Pr,
    pub remote: Remote,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidKey {
    pub key: &'static str,
    pub why: &'static str,
}

impl std::fmt::Display for InvalidKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.key, self.why)
    }
}

impl std::error::Error for InvalidKey {}

impl ProjectSettings {
    pub fn fast_gate(&self) -> Option<&str> {
        self.gates.fast.as_deref()
    }

    pub fn full_gate(&self) -> Option<&str> {
        self.gates.full.as_deref()
    }

    pub fn validate(&self) -> Result<(), InvalidKey> {
        let empty = |c: &Option<String>| c.as_deref().is_some_and(|c| c.trim().is_empty());
        if empty(&self.gates.fast) {
            return Err(InvalidKey {
                key: "gates.fast",
                why: "the command is empty",
            });
        }
        if empty(&self.gates.full) {
            return Err(InvalidKey {
                key: "gates.full",
                why: "the command is empty",
            });
        }
        if matches!(&self.remote, Remote::Named(n) if n.trim().is_empty()) {
            return Err(InvalidKey {
                key: "remote",
                why: "the name is empty; write a remote name or none",
            });
        }
        Ok(())
    }
}

pub trait ProjectFileReader {
    fn read(&self, tip: &str) -> std::io::Result<Option<String>>;
}

pub const MISSING_FAST_GATE_KIND: &str = "missing_fast_gate";

pub fn missing_fast_gate_key(project_id: &str) -> String {
    format!("{MISSING_FAST_GATE_KIND}:{project_id}")
}

pub fn missing_fast_gate_cause(project: &str) -> String {
    format!("missing fast gate: add [gates] fast to {FILE_NAME} in {project}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_gates(fast: Option<&str>, full: Option<&str>) -> ProjectSettings {
        ProjectSettings {
            gates: Gates {
                fast: fast.map(str::to_string),
                full: full.map(str::to_string),
            },
            ..ProjectSettings::default()
        }
    }

    #[test]
    fn defaults_are_gh_and_origin_with_no_gate() {
        let s = ProjectSettings::default();
        assert_eq!(s.pr, Pr::Gh);
        assert_eq!(s.remote, Remote::Named("origin".into()));
        assert_eq!(s.fast_gate(), None);
        assert_eq!(s.full_gate(), None);
    }

    #[test]
    fn an_empty_command_is_refused_with_its_key() {
        let fast = with_gates(Some("  "), Some("cargo test")).validate();
        assert_eq!(fast.unwrap_err().key, "gates.fast");
        let full = with_gates(Some("cargo test"), Some("")).validate();
        assert_eq!(full.unwrap_err().key, "gates.full");
        assert!(with_gates(Some("a"), Some("b")).validate().is_ok());
    }

    #[test]
    fn remote_none_is_not_a_name() {
        assert_eq!(Remote::from_word("none"), Remote::None);
        assert_eq!(Remote::from_word("up"), Remote::Named("up".into()));
        assert_eq!(Pr::parse("manual"), Some(Pr::Manual));
        assert_eq!(Pr::parse("gitlab"), None);
    }

    #[test]
    fn the_key_and_the_cause_name_the_project() {
        assert_eq!(missing_fast_gate_key("P1"), "missing_fast_gate:P1");
        assert_eq!(
            missing_fast_gate_cause("/src/app"),
            "missing fast gate: add [gates] fast to fleet.toml in /src/app"
        );
    }
}
