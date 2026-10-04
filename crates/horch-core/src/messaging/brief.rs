//! The brief: everything a worker pane needs to know about itself, written by
//! `horch spawn` and read by `horch worker`.
//!
//! The bash implementation wrote a shell fragment of `export`s quoted with
//! `printf %q`; JSON avoids that quoting problem entirely and has the same
//! meaning on Windows.
//!
//! Schema 1 is the brief before A2: no `schema` key, and only the claude and
//! codex binary overrides. Schema 2 adds `schema`, `workdir` and every
//! `HORCH_*_BIN` override in `bin_overrides`. A schema 2 brief still writes
//! `claude_bin` and `codex_bin`, so an older worker binary can read it.

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::execution::{ReportTarget, SessionMode};
use crate::harness::HarnessKind;
use crate::roster::Teammate;
use crate::runtime::BinOverrides;

/// The schema this binary writes.
pub const SCHEMA: u32 = 2;

fn schema_v1() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    /// 1 for a brief without the key; this binary writes [`SCHEMA`].
    #[serde(default = "schema_v1")]
    pub schema: u32,
    pub role: String,
    /// The teammate's name, i.e. the `teammates/<name>.md` this worker is.
    pub teammate: String,
    pub agent: String,
    pub model: String,
    pub record_id: String,
    /// Fresh or resumed, and the session id when it is known at spawn. Claude
    /// session ids are minted at spawn; codex only reveals its id after launch.
    /// On disk this is still the `session_id` string (empty until known) and
    /// the `resume` flag.
    #[serde(flatten, with = "session_wire")]
    pub session: SessionMode,
    #[serde(default)]
    pub task: String,
    pub project_dir: String,
    /// Set only when the caller overrode the ledger location.
    #[serde(default)]
    pub state_dir: Option<String>,
    /// Schema 1 agent-CLI overrides. A pane is a fresh shell that does not
    /// inherit the spawning process's environment, so these have to travel in
    /// the brief to reach the worker at all. [`Brief::overrides`] folds them
    /// into [`Brief::bin_overrides`].
    #[serde(default)]
    pub claude_bin: Option<String>,
    #[serde(default)]
    pub codex_bin: Option<String>,
    /// The teammate resolved at spawn time, carried so the worker renders the
    /// briefing the spawner actually chose. A worker's cwd is the target
    /// project, not this repo, so re-reading the roster in the pane could
    /// resolve a different file - or none at all.
    #[serde(default)]
    pub resolved: Option<Teammate>,
    /// Where the roster was read from at spawn time. A pane does not inherit
    /// `$HORCH_TEAMMATES_DIR`, so the path travels here or the worker falls
    /// back to the compiled-in copies and silently ignores local edits.
    #[serde(default)]
    pub teammates_dir: Option<String>,
    /// Schema 2: the directory the agent works in, when it is not the project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workdir: Option<String>,
    /// Schema 2: every `HORCH_*_BIN` override the spawner ran with.
    #[serde(default, skip_serializing_if = "BinOverrides::is_empty")]
    pub bin_overrides: BinOverrides,
    /// Who `horch done` reports to. A brief without the key reports to the
    /// orchestrator, as every brief did before B3; a candidate reports to
    /// nobody.
    #[serde(default = "report_to_orchestrator")]
    pub report_to: ReportTarget,
}

fn report_to_orchestrator() -> ReportTarget {
    ReportTarget::Orchestrator
}

impl Brief {
    pub fn agent(&self) -> Result<HarnessKind> {
        self.agent.parse().map_err(anyhow::Error::msg)
    }

    /// Parse a brief of either schema, with the schema 1 overrides folded
    /// into [`Brief::bin_overrides`].
    pub(crate) fn from_json(raw: &str) -> Result<Brief> {
        let mut brief: Brief = serde_json::from_str(raw)?;
        brief.bin_overrides = brief.overrides();
        Ok(brief)
    }

    /// Every binary override: `bin_overrides`, with `claude_bin` and
    /// `codex_bin` filling the gaps.
    pub fn overrides(&self) -> BinOverrides {
        let mut out = BinOverrides {
            claude: self.claude_bin.as_ref().map(Into::into),
            codex: self.codex_bin.as_ref().map(Into::into),
            ..BinOverrides::default()
        };
        out.merge(&self.bin_overrides);
        out
    }

    /// Record `overrides` in both schemas: the full set in `bin_overrides`,
    /// and the claude and codex values in their schema 1 fields.
    pub(crate) fn set_overrides(&mut self, overrides: BinOverrides) {
        let text =
            |p: &Option<std::path::PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned());
        self.claude_bin = text(&overrides.claude);
        self.codex_bin = text(&overrides.codex);
        self.bin_overrides = overrides;
    }

    /// The directory the agent works in.
    pub(crate) fn workdir_or_project(&self) -> &str {
        self.workdir.as_deref().unwrap_or(&self.project_dir)
    }

    /// The environment the worker's agent receives, and with it every
    /// `horch` command the agent runs: who it is (`horch note` and
    /// `horch done` find their record by it), where the ledger and roster
    /// are, and every binary override. Applied to the child command only.
    pub(crate) fn transport_env(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = [
            ("HORCH_ROLE", self.role.as_str()),
            ("HORCH_TEAMMATE", self.teammate.as_str()),
            ("HORCH_AGENT", self.agent.as_str()),
            ("HORCH_MODEL", self.model.as_str()),
            ("HORCH_RECORD_ID", self.record_id.as_str()),
            (
                "HORCH_SESSION_ID",
                self.session.id().map(|id| id.as_str()).unwrap_or_default(),
            ),
            (
                "HORCH_RESUME",
                if self.session.is_resume() { "1" } else { "0" },
            ),
            ("HORCH_TASK", self.task.as_str()),
            ("HORCH_PROJECT_DIR", self.project_dir.as_str()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        if let Some(dir) = &self.state_dir {
            out.push(("HORCH_STATE_DIR".into(), dir.clone()));
        }
        // The agent runs `horch spawn` and `horch done` from inside this pane;
        // they must resolve the same roster this worker was briefed from.
        if let Some(dir) = &self.teammates_dir {
            out.push(("HORCH_TEAMMATES_DIR".into(), dir.clone()));
        }
        for (key, value) in self.overrides().env_pairs() {
            out.push((key.to_string(), value.to_string_lossy().into_owned()));
        }
        out
    }
}

/// The brief's on-disk session fields, which predate [`SessionMode`].
mod session_wire {
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use crate::execution::SessionMode;
    use crate::ids::SessionId;

    #[derive(Serialize, Deserialize)]
    struct Wire {
        #[serde(default)]
        session_id: String,
        #[serde(rename = "resume")]
        resuming: bool,
    }

    pub(crate) fn serialize<S: Serializer>(session: &SessionMode, s: S) -> Result<S::Ok, S::Error> {
        Wire {
            session_id: session.id().map(|id| id.to_string()).unwrap_or_default(),
            resuming: session.is_resume(),
        }
        .serialize(s)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SessionMode, D::Error> {
        let wire = Wire::deserialize(d)?;
        let id = match wire.session_id.as_str() {
            "" => None,
            _ => Some(SessionId::new(wire.session_id).map_err(D::Error::custom)?),
        };
        match (wire.resuming, id) {
            (true, Some(id)) => Ok(SessionMode::Resume(id)),
            (true, None) => Err(D::Error::custom("a resume brief needs a session_id")),
            (false, id) => Ok(SessionMode::Fresh(id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn brief() -> Brief {
        Brief {
            schema: SCHEMA,
            role: "sonnet-1".into(),
            teammate: "sonnet".into(),
            agent: "claude".into(),
            model: "sonnet".into(),
            record_id: "r1".into(),
            session: SessionMode::Fresh(Some("s1".parse().unwrap())),
            task: "do the thing".into(),
            project_dir: "/tmp/proj".into(),
            state_dir: Some("/state".into()),
            claude_bin: None,
            codex_bin: None,
            resolved: None,
            teammates_dir: Some("/roster".into()),
            workdir: None,
            bin_overrides: BinOverrides::default(),
            report_to: crate::execution::ReportTarget::Orchestrator,
        }
    }

    /// Today's brief file, byte for byte the shape a pre-A2 `horch spawn`
    /// wrote, still reads: as schema 1, with its two overrides folded in.
    #[test]
    fn arc_07_brief_v1_readable() {
        let v1 = r#"{
  "role": "codex-1",
  "teammate": "codex-sol",
  "agent": "codex",
  "model": "gpt-5",
  "record_id": "r9",
  "session_id": "",
  "resume": false,
  "task": "t",
  "project_dir": "/p",
  "state_dir": null,
  "claude_bin": "/bin/fake-claude",
  "codex_bin": "/bin/fake-codex",
  "resolved": null,
  "teammates_dir": null
}"#;
        let b = Brief::from_json(v1).unwrap();
        assert_eq!(b.schema, 1);
        assert_eq!(b.session, SessionMode::Fresh(None));
        assert_eq!(b.workdir, None);
        assert_eq!(b.workdir_or_project(), "/p");
        assert_eq!(
            b.bin_overrides,
            BinOverrides {
                claude: Some("/bin/fake-claude".into()),
                codex: Some("/bin/fake-codex".into()),
                ..BinOverrides::default()
            }
        );
        let env = b.transport_env();
        assert!(env.contains(&("HORCH_CLAUDE_BIN".into(), "/bin/fake-claude".into())));
        assert!(env.contains(&("HORCH_CODEX_BIN".into(), "/bin/fake-codex".into())));
        // The skill store travels in the pane command, not the brief.
        assert!(!env.iter().any(|(k, _)| k == "HORCH_DATA_DIR"));

        // The oldest briefs have neither override nor task.
        let older = r#"{"role":"old","teammate":"sonnet","agent":"claude","model":"sonnet",
            "record_id":"r1","session_id":"s1","resume":false,"project_dir":"/tmp/proj"}"#;
        let b = Brief::from_json(older).unwrap();
        assert_eq!(b.schema, 1);
        assert!(b.bin_overrides.is_empty());
    }

    /// A schema 2 brief keeps the schema 1 keys, so an older worker reads it.
    #[test]
    fn a_v2_brief_round_trips_and_keeps_the_v1_keys() {
        let mut b = brief();
        b.workdir = Some("/wt".into());
        b.set_overrides(BinOverrides {
            claude: Some("/c".into()),
            opencode: Some("/o".into()),
            ..BinOverrides::default()
        });
        let v = serde_json::to_value(&b).unwrap();
        assert_eq!(v["schema"], 2);
        assert_eq!(v["claude_bin"], "/c");
        assert_eq!(v["codex_bin"], serde_json::Value::Null);
        assert_eq!(v["bin_overrides"]["opencode"], "/o");
        assert_eq!(v["workdir"], "/wt");
        assert!(v.get("data_root").is_none(), "{v}");

        let back = Brief::from_json(&v.to_string()).unwrap();
        assert_eq!(back.schema, 2);
        assert_eq!(back.bin_overrides.opencode, Some(PathBuf::from("/o")));
        assert_eq!(back.workdir_or_project(), "/wt");
    }

    #[test]
    fn transport_env_carries_identity_paths_and_every_override() {
        let mut b = brief();
        let mut all = BinOverrides::default();
        for (i, key) in BinOverrides::VARS.iter().enumerate() {
            let mut one = BinOverrides::default();
            let env = crate::runtime::MapEnv::new("/").with(key, &format!("/bin/{i}"));
            one.merge(&BinOverrides::from_env(&env));
            all.merge(&one);
        }
        b.set_overrides(all);
        let env: std::collections::BTreeMap<String, String> =
            b.transport_env().into_iter().collect();
        for (key, want) in [
            ("HORCH_ROLE", "sonnet-1"),
            ("HORCH_TEAMMATE", "sonnet"),
            ("HORCH_AGENT", "claude"),
            ("HORCH_MODEL", "sonnet"),
            ("HORCH_RECORD_ID", "r1"),
            ("HORCH_SESSION_ID", "s1"),
            ("HORCH_RESUME", "0"),
            ("HORCH_TASK", "do the thing"),
            ("HORCH_PROJECT_DIR", "/tmp/proj"),
            ("HORCH_STATE_DIR", "/state"),
            ("HORCH_TEAMMATES_DIR", "/roster"),
        ] {
            assert_eq!(env.get(key).map(String::as_str), Some(want), "{key}");
        }
        for (i, key) in BinOverrides::VARS.iter().enumerate() {
            assert_eq!(env.get(*key), Some(&format!("/bin/{i}")), "{key}");
        }
        assert!(!env.contains_key("ANTHROPIC_API_KEY"));
        assert!(!env.contains_key("HORCH_DATA_DIR"));
    }
}
