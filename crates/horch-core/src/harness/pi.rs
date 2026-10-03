//! pi: `pi --model <pattern> --thinking <level> -- "<prompt>"`.
//!
//! pi's `--session-id` creates the session if it is missing, so horch mints
//! the id and nothing is discovered after launch.

use std::path::Path;
use std::process::Command;

use anyhow::Result;

use super::launch::model_for;
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, Session};
use crate::skills::Bundle;
use crate::teammates::Teammate;

pub struct Pi;

impl Harness for Pi {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    fn expose_skills(
        &self,
        teammate: &Teammate,
        skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        Ok(with_skill_flag(teammate, skills))
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        pi_family_command(
            env.bins.pi.clone(),
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }
}

/// pi and Prime Agent read skills from `--skill <dir>`. One flag names the
/// bundle's `skills/` directory, which holds exactly the activated skills; it
/// goes into `args`, before the `--` that fences off the prompt.
pub(super) fn with_skill_flag(teammate: &Teammate, skills: &Bundle) -> Teammate {
    let mut adjusted = teammate.clone();
    adjusted.args.extend([
        "--skill".to_owned(),
        skills.skills_dir().to_string_lossy().into_owned(),
    ]);
    adjusted
}

/// pi and Prime Agent: `<bin> --model <pattern> --thinking <level> -- "<prompt>"`.
///
/// Neither has an approval gate to bypass: their tools run, which is what makes
/// them usable in a pane nobody is watching. `permission_mode` therefore has
/// nothing to map onto, and the roster check rejects it rather than pretending.
pub(super) fn pi_family_command(
    bin: std::path::PathBuf,
    teammate: &Teammate,
    session: Session<'_>,
    prompt: &str,
    model_override: Option<&str>,
) -> Result<Command> {
    let mut cmd = Command::new(bin);
    cmd.arg("--model").arg(model_for(teammate, model_override)?);

    if let Some(effort) = &teammate.effort {
        cmd.arg("--thinking").arg(effort);
    }
    if let Some(tools) = &teammate.tools {
        // `Some([])` means "no tools at all", which is its own flag here rather
        // than an empty list.
        if tools.is_empty() {
            cmd.arg("--no-tools");
        } else {
            cmd.arg("--tools").arg(tools.join(","));
        }
    }
    // `--exclude-tools` is pi's; Prime 0.9.4 has no denylist, and the roster
    // check rejects `disallowed_tools` on a Prime teammate rather than dropping
    // it here, so this only ever fires for pi.
    if !teammate.disallowed_tools.is_empty() && teammate.agent.capabilities().tool_denylist {
        cmd.arg("--exclude-tools")
            .arg(teammate.disallowed_tools.join(","));
    }
    // Discovery of everything the repo or the operator might have lying around.
    // Nothing here is on by default in a fleet: a worker with one narrow job
    // should not inherit a project's extensions or the operator's skills.
    if !teammate.inherit_plugins {
        cmd.arg("--no-extensions")
            .arg("--no-skills")
            .arg("--no-prompt-templates")
            .arg("--no-themes");
    }

    match session {
        // pi's `--session-id` creates the session if it does not exist, so a
        // fresh launch and a resume are the same flag with a different id.
        // Prime has no such flag: `horch worker` gives it a `--session-dir` it
        // owns instead, and reads back whatever session appears there.
        Session::Fresh(id) if teammate.agent.capabilities().caller_minted_session => {
            cmd.arg("--session-id").arg(id);
        }
        Session::Resume(id) => {
            if teammate.agent.capabilities().caller_minted_session {
                cmd.arg("--session-id").arg(id);
            } else {
                cmd.arg("--resume").arg(id);
            }
        }
        Session::Fresh(_) | Session::Unmanaged => {}
    }
    cmd.args(&teammate.args);
    // `--` ends option parsing: without it a prompt beginning with a dash would
    // be read as a flag.
    cmd.arg("--").arg(prompt);
    Ok(cmd)
}
