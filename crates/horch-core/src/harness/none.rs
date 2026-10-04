//! `agent: none`: the smoke teammate, which exercises the machinery and
//! launches no CLI.

use std::path::Path;
use std::process::Command;

use anyhow::{bail, Result};

use super::{CommandSpec, Harness, HarnessKind, LaunchEnv};
use crate::roster::Teammate;
use crate::skills::Bundle;

pub(crate) struct NoAgent;

impl Harness for NoAgent {
    fn kind(&self) -> HarnessKind {
        HarnessKind::None
    }

    /// Nothing is launched, so no field can fail to take effect.
    fn validate(&self, _t: &Teammate) -> Vec<&'static str> {
        Vec::new()
    }

    fn expose_skills(
        &self,
        _teammate: &Teammate,
        _skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        bail!("skills need an agent harness")
    }

    fn command(&self, _env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        bail!(
            "teammate '{}' has agent: none and cannot be launched",
            spec.teammate.name
        )
    }
}
