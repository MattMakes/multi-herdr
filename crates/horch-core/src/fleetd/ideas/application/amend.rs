use anyhow::{anyhow, bail, Result};
use rusqlite::Connection;

use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::ideas::application::approval::{record_amendment, Amendment};
use crate::fleetd::ideas::application::intake::{add_dependent_idea, Added, Target};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::ideas::domain::approval::{AWAITING_APPROVAL, PAST_APPROVAL, REVISED_TO};
use crate::fleetd::ideas::domain::idea::TERMINAL;
use crate::fleetd::ideas::domain::idea_folder::FILE_LIMIT;
use crate::fleetd::integration::application::new_features::Project;
use crate::taskledger::model::Actor;

pub const AMENDED_CAUSE: &str = "you amended the idea";

pub const AMEND_SOURCE: &str = "horch idea amend";

#[derive(Debug)]
pub enum Amended {
    Amendment(Amendment),
    NewIdea(Added),
    Refused(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Amendment(Option<&'static str>),
    NewIdea,
    Refused,
}

fn step(state: &str) -> Step {
    match state {
        "captured" | "clarifying" => Step::Amendment(None),
        AWAITING_APPROVAL => Step::Amendment(Some(REVISED_TO)),
        s if PAST_APPROVAL.contains(&s) => Step::Amendment(Some(AWAITING_APPROVAL)),
        s if TERMINAL.contains(&s) => Step::NewIdea,
        _ => Step::Refused,
    }
}

pub fn amend_idea(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    folder: &IdeaFolder,
    idea: &str,
    text: &str,
    project: Project<'_>,
) -> Result<Amended> {
    if text.trim().is_empty() {
        bail!("the amendment is empty: give its text");
    }
    if text.len() as u64 > FILE_LIMIT {
        bail!(
            "the amendment is {} bytes: an amendment holds at most {FILE_LIMIT} bytes",
            text.len()
        );
    }
    let row =
        ideas_store::idea(tx, idea)?.ok_or_else(|| anyhow!("no idea {idea} in the ledger"))?;
    let cause = Cause {
        actor: Actor::Operator,
        text: AMENDED_CAUSE,
    };
    let to = match step(&row.state) {
        Step::Refused => {
            return Ok(Amended::Refused(format!(
                "idea {idea} is {}: amend it after it resumes",
                row.state
            )))
        }
        Step::NewIdea => {
            let depends_on = [idea.to_string()];
            let target = Target::New(project);
            let added =
                add_dependent_idea(tx, ctx, text, AMEND_SOURCE, target, &depends_on, cause)?;
            return Ok(Amended::NewIdea(added));
        }
        Step::Amendment(to) => to,
    };
    if let Some(to) = to {
        transition(tx, ctx.guard, Entity::Idea, idea, to, cause, &NeedsYouList)?;
    }
    Ok(Amended::Amendment(record_amendment(
        tx, ctx.guard, folder, idea, text, cause,
    )?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::ideas::domain::idea::STATES;

    #[test]
    fn each_state_has_its_step() {
        let steps: Vec<Step> = STATES.iter().map(|s| step(s)).collect();
        assert_eq!(
            steps,
            vec![
                Step::Amendment(None),
                Step::Amendment(None),
                Step::Amendment(Some("clarifying")),
                Step::Amendment(Some("awaiting_approval")),
                Step::Amendment(Some("awaiting_approval")),
                Step::Amendment(Some("awaiting_approval")),
                Step::Amendment(Some("awaiting_approval")),
                Step::NewIdea,
                Step::Refused,
                Step::NewIdea,
            ]
        );
    }
}
