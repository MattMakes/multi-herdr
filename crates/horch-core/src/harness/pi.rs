//! pi: `pi --model <pattern> --thinking <level> -- "<prompt>"`.
//!
//! pi's `--session-id` creates the session if it is missing, so horch mints
//! the id and nothing is discovered after launch.

use std::process::Command;

use anyhow::Result;

use super::{launch, CommandSpec, Harness, HarnessKind, LaunchEnv};

pub struct Pi;

impl Harness for Pi {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Pi
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        launch::pi_family_command(
            env.bins.pi.clone(),
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }
}
