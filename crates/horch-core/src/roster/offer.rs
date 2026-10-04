//! Which teammates the orchestrator is offered in THIS project.
//!
//! A domain teammate (Unreal, Swift) is noise in every other project's
//! briefing. `offer_when` names the files that make it relevant; the roster
//! lists it only when the project has one. The filter is a pure function of the
//! teammate and [`ProjectFacts`]: the CLI reads the directory, this module
//! never touches the filesystem.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use super::Teammate;

/// A host tool a teammate cannot work without. `horch doctor` checks each
/// one when a teammate the project is offered names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Requirement {
    /// `xcodebuild` on PATH, with Xcode's first launch done.
    Xcode,
    /// `blender` on PATH, or `BLENDER_PATH`, and `--version` runs.
    Blender,
}

impl Requirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Requirement::Xcode => "xcode",
            Requirement::Blender => "blender",
        }
    }
}

/// The entry names (files and directories) at the top of the project and one
/// level down. `*.xcodeproj` is a directory, so directories count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectFacts {
    names: BTreeSet<String>,
}

impl ProjectFacts {
    pub fn from_names<I, S>(names: I) -> ProjectFacts
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        ProjectFacts {
            names: names.into_iter().map(Into::into).collect(),
        }
    }

    /// Whether any entry name matches `pattern`.
    pub fn has_match(&self, pattern: &str) -> bool {
        self.names.iter().any(|name| glob_match(pattern, name))
    }
}

/// Whether the orchestrator is offered `t`. A teammate without `offer_when` is
/// always offered; so is every teammate when no facts were gathered (`None`),
/// which keeps `horch teammates` and the compiled-in goldens project-free.
pub fn offered_in(t: &Teammate, facts: Option<&ProjectFacts>) -> bool {
    if t.hidden {
        return false;
    }
    match facts {
        Some(facts) if !t.offer_when.is_empty() => {
            t.offer_when.iter().any(|pattern| facts.has_match(pattern))
        }
        _ => true,
    }
}

/// What `check` says about one `offer_when` pattern, if anything. Patterns
/// match an entry NAME, so a path separator can never match.
pub(crate) fn pattern_problem(pattern: &str) -> Option<&'static str> {
    if pattern.trim().is_empty() {
        Some("is empty")
    } else if pattern.contains('/') || pattern.contains('\\') {
        Some("contains a path separator; patterns match a file or directory name")
    } else {
        None
    }
}

/// `*` matches any run of characters, `?` exactly one; everything else is
/// literal and case-sensitive.
pub(crate) fn glob_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    let (mut pi, mut ni) = (0, 0);
    // The last `*` seen, and the name position it is currently absorbing to.
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(patterns: &[&str]) -> Teammate {
        Teammate {
            name: "ue-dev".into(),
            brief_description: "Unreal Engine work".into(),
            offer_when: patterns.iter().map(|p| p.to_string()).collect(),
            ..Teammate::default()
        }
    }

    #[test]
    fn glob_matches_stars_and_question_marks() {
        for (pattern, name, want) in [
            ("*.uproject", "Game.uproject", true),
            ("*.uproject", "Game.uproject.bak", false),
            ("*.uproject", ".uproject", true),
            ("Package.swift", "Package.swift", true),
            ("Package.swift", "package.swift", false),
            ("*.xc*", "App.xcworkspace", true),
            ("?.txt", "a.txt", true),
            ("?.txt", "ab.txt", false),
            ("a*b*c", "aXbYbZc", true),
            ("a*b*c", "aXbYbZ", false),
            ("*", "", true),
            ("", "x", false),
        ] {
            assert_eq!(glob_match(pattern, name), want, "{pattern} vs {name}");
        }
    }

    #[test]
    fn offered_when_a_top_level_entry_matches() {
        let facts = ProjectFacts::from_names(["Game.uproject", "Source", "README.md"]);
        assert!(offered_in(&domain(&["*.uproject"]), Some(&facts)));
    }

    /// The CLI puts names from one level down into the same set, so a match
    /// there offers the teammate too.
    #[test]
    fn offered_when_an_entry_one_level_down_matches() {
        let facts = ProjectFacts::from_names(["ios", "Package.swift", "web"]);
        let swift = domain(&["*.xcodeproj", "*.xcworkspace", "Package.swift"]);
        assert!(offered_in(&swift, Some(&facts)));
    }

    #[test]
    fn hidden_when_nothing_matches() {
        let facts = ProjectFacts::from_names(["Cargo.toml", "src", "main.rs"]);
        assert!(!offered_in(&domain(&["*.uproject"]), Some(&facts)));
        assert!(!offered_in(
            &domain(&["*.uproject"]),
            Some(&ProjectFacts::default())
        ));
    }

    #[test]
    fn a_teammate_without_offer_when_is_always_offered() {
        let facts = ProjectFacts::from_names(["Cargo.toml"]);
        assert!(offered_in(&domain(&[]), Some(&facts)));
        assert!(offered_in(&domain(&[]), None));
    }

    #[test]
    fn no_facts_offers_everything_and_hidden_is_never_offered() {
        assert!(offered_in(&domain(&["*.uproject"]), None));
        let hidden = Teammate {
            hidden: true,
            ..domain(&[])
        };
        assert!(!offered_in(&hidden, None));
    }

    #[test]
    fn patterns_with_a_separator_or_nothing_in_them_are_problems() {
        assert!(pattern_problem("*.uproject").is_none());
        assert!(pattern_problem("ios/Package.swift").is_some());
        assert!(pattern_problem(" ").is_some());
    }
}
