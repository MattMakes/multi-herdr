//! The gates of a project (ticket 04).
//!
//! A project declares its fast gate and its full gate in the file
//! `fleet.toml` at its root, in the table `[gates]`, with the optional
//! top-level keys `pr` and `remote` (`docs/specs/fleetd/design.md`, "Between
//! features"). This file is not the `fleet.toml` of the fleet root. fleetd
//! reads it at the tip of the feature branch through [`ProjectFileReader`].
//!
//! This module does no I/O and parses no text: `adapters/project_file.rs`
//! parses the text into a [`ProjectSettings`], then calls
//! [`ProjectSettings::validate`].

/// The name of the project file at the root of the project.
pub const FILE_NAME: &str = "fleet.toml";

/// The 2 declared gates of a project, each a shell command. `None` when the
/// project does not declare the gate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gates {
    /// The fast gate: the unit tests of the project.
    pub fast: Option<String>,
    /// The full gate: the fast gate plus the end-to-end tests.
    pub full: Option<String>,
}

/// Who opens the pull request of a ready feature. Default [`Pr::Gh`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Pr {
    /// fleetd opens the pull request with `gh`.
    #[default]
    Gh,
    /// fleetd pushes the feature branch and shows the compare URL.
    Manual,
}

impl Pr {
    /// The word of the file for each value.
    pub fn parse(word: &str) -> Option<Pr> {
        match word {
            "gh" => Some(Pr::Gh),
            "manual" => Some(Pr::Manual),
            _ => None,
        }
    }
}

/// The remote of a project. Default the remote `origin`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remote {
    /// The name of the remote that fleetd fetches and pushes.
    Named(String),
    /// `remote = none`: fleetd does not fetch, push or open a pull request.
    None,
}

impl Default for Remote {
    fn default() -> Self {
        Remote::Named(DEFAULT_REMOTE.to_string())
    }
}

/// The remote of a project that does not set `remote`.
pub const DEFAULT_REMOTE: &str = "origin";

impl Remote {
    /// The value of the key `remote`: `none`, or the name of the remote.
    pub fn from_word(word: &str) -> Remote {
        if word == "none" {
            Remote::None
        } else {
            Remote::Named(word.to_string())
        }
    }
}

/// The settings of 1 project from its file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectSettings {
    pub gates: Gates,
    pub pr: Pr,
    pub remote: Remote,
}

/// A value of the project file that is not valid, with the name of its key.
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
    /// The fast gate, or `None` when the project did not declare it.
    pub fn fast_gate(&self) -> Option<&str> {
        self.gates.fast.as_deref()
    }

    /// The full gate, or `None` when the project did not declare it.
    pub fn full_gate(&self) -> Option<&str> {
        self.gates.full.as_deref()
    }

    /// Refuse a gate with an empty command and a remote with no name.
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

/// The reader port of the project file: the text of `fleet.toml` at the
/// commit `tip` of the project, or `None` when that commit has no such
/// file. The git gateway of ticket 23 reads `git show <tip>:fleet.toml`.
pub trait ProjectFileReader {
    fn read(&self, tip: &str) -> std::io::Result<Option<String>>;
}

/// The needs-you kind of a project with no fast gate.
pub const MISSING_FAST_GATE_KIND: &str = "missing_fast_gate";

/// The needs-you key of the project `project_id` with no fast gate.
pub fn missing_fast_gate_key(project_id: &str) -> String {
    format!("{MISSING_FAST_GATE_KIND}:{project_id}")
}

/// The cause of the hold of the project `project` with no fast gate.
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
