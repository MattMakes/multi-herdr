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
    /// `GODOT_PATH`, `godot` on PATH, or the macOS app bundle, and
    /// `--version` reports 4.3 or later. In a C# project (`"C#"` in
    /// `config/features`, or a `*.csproj`), also the Godot .NET engine
    /// (`GODOT_MONO_PATH`, or the macOS `Godot_mono.app`) and `dotnet` on PATH.
    Godot,
    /// `git lfs version` runs with the fleet's git (`HORCH_GIT_BIN`, else
    /// `git` on PATH). In an Unreal project, `.gitattributes` also needs an
    /// LFS rule for `*.uasset`.
    #[serde(rename = "git-lfs")]
    GitLfs,
}

impl Requirement {
    pub fn as_str(self) -> &'static str {
        match self {
            Requirement::Xcode => "xcode",
            Requirement::Blender => "blender",
            Requirement::Godot => "godot",
            Requirement::GitLfs => "git-lfs",
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

/// The skills `t.skills_when` adds in a project with `facts`: every skill of
/// every matching pattern, sorted, without the ones `skills:` already names.
pub fn project_skills(t: &Teammate, facts: &ProjectFacts) -> Vec<String> {
    let added: BTreeSet<&String> = t
        .skills_when
        .iter()
        .filter(|(pattern, _)| facts.has_match(pattern))
        .flat_map(|(_, skills)| skills)
        .filter(|s| !t.skills.contains(s))
        .collect();
    added.into_iter().cloned().collect()
}

/// `t` as it launches in a project with `facts`: each [`project_skills`]
/// entry joins `skills:`, so the activation plan, the briefing and the
/// ledger record treat it as an expected skill, and it leaves
/// `available_skills:`.
pub fn with_project_skills(mut t: Teammate, facts: &ProjectFacts) -> Teammate {
    let added = project_skills(&t, facts);
    t.available_skills.retain(|s| !added.contains(s));
    t.skills.extend(added);
    t
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

    fn csharp_builder() -> Teammate {
        Teammate {
            name: "godot-gameplay-dev".into(),
            skills: vec!["tdd".into()],
            available_skills: vec!["godot-csharp-signals".into()],
            skills_when: [
                (
                    "*.csproj".to_string(),
                    vec!["godot-csharp-godot".into(), "godot-csharp-signals".into()],
                ),
                ("*.sln".to_string(), vec!["godot-csharp-godot".into()]),
                ("limboai".to_string(), vec!["tdd".into()]),
            ]
            .into(),
            ..Teammate::default()
        }
    }

    #[test]
    fn skills_when_adds_the_skills_of_every_matching_pattern() {
        let t = csharp_builder();
        let facts = ProjectFacts::from_names(["project.godot", "Game.csproj", "Game.sln"]);
        assert_eq!(
            project_skills(&t, &facts),
            ["godot-csharp-godot", "godot-csharp-signals"]
        );
        let launched = with_project_skills(t, &facts);
        assert_eq!(
            launched.skills,
            ["tdd", "godot-csharp-godot", "godot-csharp-signals"]
        );
        // A skill made expected is no longer only offered.
        assert!(launched.available_skills.is_empty());
    }

    /// No match adds nothing; a skill `skills:` already names is not added
    /// twice.
    #[test]
    fn skills_when_without_a_match_or_with_a_named_skill_adds_nothing() {
        let t = csharp_builder();
        let gdscript = ProjectFacts::from_names(["project.godot", "player.gd"]);
        assert!(project_skills(&t, &gdscript).is_empty());
        assert_eq!(with_project_skills(t.clone(), &gdscript), t);

        let addon = ProjectFacts::from_names(["addons", "limboai"]);
        assert!(project_skills(&t, &addon).is_empty());
    }

    #[test]
    fn skills_when_parses_from_frontmatter() {
        let text = "---\nname: x\nbrief_description: X\nagent: claude\nmodel: opus\n\
                    skills_when: {\"*.csproj\": [godot-csharp-godot, godot-csharp-signals]}\n\
                    ---\nbody";
        let t = super::super::parser::parse_teammate("x", text).unwrap();
        assert_eq!(
            t.skills_when["*.csproj"],
            ["godot-csharp-godot", "godot-csharp-signals"]
        );
    }

    #[test]
    fn requires_parses_every_host_tool() {
        let text = "---\nname: x\nbrief_description: X\nagent: claude\nmodel: opus\n\
                    requires: [xcode, blender, godot, git-lfs]\n---\nbody";
        let t = super::super::parser::parse_teammate("x", text).unwrap();
        assert_eq!(
            t.requires,
            [
                Requirement::Xcode,
                Requirement::Blender,
                Requirement::Godot,
                Requirement::GitLfs
            ]
        );
        let names: Vec<_> = t.requires.iter().map(|r| r.as_str()).collect();
        assert_eq!(names, ["xcode", "blender", "godot", "git-lfs"]);
    }

    /// U-17: the value is `git-lfs`, as `as_str` prints it. The spellings
    /// serde would derive (`gitlfs`) or a writer might guess are rejected.
    #[test]
    fn requires_git_lfs_has_one_spelling() {
        for wrong in ["gitlfs", "git_lfs", "lfs"] {
            let text = format!(
                "---\nname: x\nbrief_description: X\nagent: claude\nmodel: opus\n\
                 requires: [{wrong}]\n---\nbody"
            );
            assert!(
                super::super::parser::parse_teammate("x", &text).is_err(),
                "{wrong} parsed"
            );
        }
    }

    #[test]
    fn patterns_with_a_separator_or_nothing_in_them_are_problems() {
        assert!(pattern_problem("*.uproject").is_none());
        assert!(pattern_problem("ios/Package.swift").is_some());
        assert!(pattern_problem(" ").is_some());
    }
}
