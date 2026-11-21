use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::digest::sha256_bytes;
use crate::fleetd::fleet_runtime::application::handlers_run::INPUT_MAX_BYTES;
use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::Cause;
use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::fleet_runtime::domain::requests::{AddTarget, IdeaAdd, IdeaInput, Kind};
use crate::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use crate::fleetd::ideas::adapters::ideas_store;
use crate::fleetd::ideas::application::amend::{amend_idea, Amended};
use crate::fleetd::ideas::application::approval::{
    self, Amendment, ApprovalFile, Approved, Revised,
};
use crate::fleetd::ideas::application::direct::{self, add_direct_idea, Direct, DirectAdded};
use crate::fleetd::ideas::application::intake::{add_idea, Added, Target};
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::ideas::application::park_cancel;
use crate::fleetd::ideas::domain::approval::{APPROVED_TO, REVISED_TO};
use crate::fleetd::ideas::domain::plan_validator::RosterProfile;
use crate::fleetd::integration::adapters::sqlite as feature_rows;
use crate::fleetd::integration::application::new_features::{BranchRef, Project};
use crate::fleetd::integration::application::ports::IntegrationGitGateway;
use crate::fleetd::worktree_custody::application::create::fleet_clock;
use crate::taskledger::model::Actor;
use crate::taskledger::runs;

pub const ADD_SOURCE: &str = "horch idea add";

pub struct IdeaHandlers {
    pub guard: Generation,
    pub config: Arc<FleetConfig>,
    pub ideas: PathBuf,
    pub owner: User,
    pub roster: Arc<dyn Fn() -> Vec<RosterProfile> + Send + Sync>,
    pub git: Arc<dyn IntegrationGitGateway + Send + Sync>,
}

impl IdeaHandlers {
    fn jobs(&self) -> JobCtx<'_> {
        JobCtx {
            guard: Some(self.guard),
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    fn folder(&self, idea: &str) -> Result<IdeaFolder> {
        Ok(IdeaFolder::new(&self.ideas, idea, self.owner.clone())?)
    }
}

type IdeaHandler = fn(&IdeaHandlers, &Connection, &Value) -> Result<Value>;

pub fn register(handlers: &mut Handlers, ctx: IdeaHandlers) {
    let ctx = Arc::new(ctx);
    let entries: [(Kind, IdeaHandler); 6] = [
        (Kind::IdeaAdd, add),
        (Kind::IdeaAmend, amend),
        (Kind::IdeaApprove, approve),
        (Kind::IdeaRevise, revise),
        (Kind::IdeaPark, park),
        (Kind::IdeaCancel, cancel),
    ];
    for (kind, handler) in entries {
        let ctx = Arc::clone(&ctx);
        handlers.insert(kind, move |tx, payload| handler(&ctx, tx, payload));
    }
}

fn parse<T: DeserializeOwned>(kind: &str, payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).with_context(|| format!("a bad {kind} payload"))
}

fn operator(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Operator,
        text,
    }
}

fn result(idea: &str, mut values: Map<String, Value>, mut effects: Map<String, Value>) -> Value {
    effects.insert("idea".into(), json!(idea));
    values.insert("idea".into(), json!(idea));
    values.insert("effects".into(), Value::Object(effects));
    Value::Object(values)
}

fn approval_effect(file: &ApprovalFile) -> Value {
    json!({ "text": file.text, "sha256": file.sha256 })
}

fn amendment_effect(amendment: &Amendment) -> Value {
    json!({ "n": amendment.n, "text": amendment.text })
}

fn branch_effect(branch: &BranchRef) -> Value {
    json!({ "repo": branch.repo, "name": branch.name, "tip": branch.tip })
}

fn add(h: &IdeaHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let add = idea_add(payload)?;
    let target = match &add.target {
        AddTarget::New {
            project,
            primary_branch,
        } => Target::New(Project {
            root: Path::new(project),
            primary_branch,
        }),
        AddTarget::Join(feature) => Target::Join(feature),
    };
    match &add.idea {
        IdeaInput::Text(text) => {
            let added = add_idea(
                tx,
                &h.jobs(),
                text,
                ADD_SOURCE,
                target,
                operator(ADD_SOURCE),
            )?;
            Ok(added_result(&added))
        }
        IdeaInput::Direct(ticket) => {
            let text = read_ticket(Path::new(ticket.path()), ticket.sha256())?;
            let added = add_direct(h, tx, &text, target)?;
            Ok(direct_result(&added))
        }
    }
}

fn idea_add(payload: &Value) -> Result<IdeaAdd> {
    let object = payload.as_object().context("a bad idea_add payload")?;
    let mut fields = Vec::with_capacity(object.len());
    for (key, value) in object {
        let Some(text) = value.as_str() else {
            bail!("a bad idea_add payload: the field {key} is not a text");
        };
        fields.push((key.as_str(), text));
    }
    Ok(IdeaAdd::from_fields(&fields)?)
}

fn read_ticket(path: &Path, sha256: &str) -> Result<String> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("reading the ticket file {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(INPUT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading the ticket file {}", path.display()))?;
    if bytes.len() as u64 > INPUT_MAX_BYTES {
        bail!(
            "the ticket file {} is over {INPUT_MAX_BYTES} bytes",
            path.display()
        );
    }
    let now = sha256_bytes(&bytes).hex();
    if now != sha256 {
        bail!(
            "the ticket file {} has the sha256 {now}, not {sha256} of the request: it changed after the request was sent",
            path.display()
        );
    }
    String::from_utf8(bytes)
        .with_context(|| format!("the ticket file {} is not UTF-8", path.display()))
}

fn add_direct(
    h: &IdeaHandlers,
    tx: &Connection,
    text: &str,
    target: Target<'_>,
) -> Result<DirectAdded> {
    let tip = match target {
        Target::New(project) => primary_tip(h, tx, &project)?,
        Target::Join(_) => String::new(),
    };
    let roster = (h.roster)();
    let direct = Direct {
        roster: &roster,
        git: h.git.as_ref(),
        primary_tip: &tip,
        clock: fleet_clock(tx)?,
    };
    let cause = operator(direct::SOURCE);
    add_direct_idea(tx, &h.jobs(), &direct, text, target, cause)
}

fn primary_tip(h: &IdeaHandlers, tx: &Connection, project: &Project<'_>) -> Result<String> {
    let repo = project.root.to_string_lossy();
    if let Some(row) = feature_rows::remote_tip(tx, &repo, project.primary_branch)? {
        if row.stale_since.is_none() {
            return Ok(row.tip);
        }
    }
    let local = format!("refs/heads/{}", project.primary_branch);
    h.git.tip(project.root, &local)?.with_context(|| {
        format!(
            "the project {repo} has no branch {}: a new feature starts there",
            project.primary_branch
        )
    })
}

fn added_result(added: &Added) -> Value {
    let mut values = Map::new();
    values.insert("feature".into(), json!(added.feature));
    let mut effects = Map::new();
    effects.insert("idea_md".into(), json!(added.file.id));
    result(&added.file.id, values, effects)
}

fn direct_result(added: &DirectAdded) -> Value {
    let id = &added.idea.file.id;
    let mut values = Map::new();
    values.insert("feature".into(), json!(added.idea.feature));
    let mut effects = Map::new();
    effects.insert("idea_md".into(), json!(id));
    effects.insert(
        "ticket".into(),
        json!({ "path": added.ticket, "text": added.ticket_text }),
    );
    effects.insert("plan_yaml".into(), json!(added.plan));
    effects.insert("approval_json".into(), approval_effect(&added.approval));
    if let Some(branch) = &added.branch {
        effects.insert("branch".into(), branch_effect(branch));
    }
    result(id, values, effects)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdeaText {
    idea: String,
    text: String,
}

fn amend(h: &IdeaHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: IdeaText = parse("idea_amend", payload)?;
    let row = ideas_store::idea(tx, &a.idea)?
        .with_context(|| format!("no idea {} in the ledger", a.idea))?;
    let run = runs::feature_run(tx, &row.feature_id)?
        .with_context(|| format!("the feature {} has no internal run", row.feature_id))?;
    let feature = feature_rows::feature(tx, &row.feature_id)?
        .with_context(|| format!("the ledger has no feature {}", row.feature_id))?;
    let project = Project {
        root: Path::new(&run.project_root),
        primary_branch: &feature.primary_branch,
    };
    let folder = h.folder(&a.idea)?;
    match amend_idea(tx, &h.jobs(), &folder, &a.idea, &a.text, project)? {
        Amended::Amendment(amendment) => {
            let row = ideas_store::idea(tx, &a.idea)?.context("the idea is gone")?;
            let mut values = Map::new();
            values.insert("state".into(), json!(row.state));
            let mut effects = Map::new();
            effects.insert("amendment".into(), amendment_effect(&amendment));
            Ok(result(&a.idea, values, effects))
        }
        Amended::NewIdea(added) => {
            let mut out = added_result(&added);
            out["amends"] = json!(a.idea);
            Ok(out)
        }
        Amended::Refused(reason) => bail!(reason),
    }
}

fn revise(h: &IdeaHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: IdeaText = parse("idea_revise", payload)?;
    let folder = h.folder(&a.idea)?;
    match approval::revise(tx, Some(h.guard), &folder, &a.idea, &a.text)? {
        Revised::Revised(amendment) => {
            let mut values = Map::new();
            values.insert("state".into(), json!(REVISED_TO));
            let mut effects = Map::new();
            effects.insert("amendment".into(), amendment_effect(&amendment));
            Ok(result(&a.idea, values, effects))
        }
        Revised::Refused(refusal) => bail!("{refusal}"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdeaApprove {
    idea: String,
    sha256: String,
}

fn approve(h: &IdeaHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: IdeaApprove = parse("idea_approve", payload)?;
    let folder = h.folder(&a.idea)?;
    match approval::approve(tx, Some(h.guard), &folder, &a.idea, &a.sha256)? {
        Approved::Approved(file) => {
            let mut values = Map::new();
            values.insert("state".into(), json!(APPROVED_TO));
            let mut effects = Map::new();
            effects.insert("approval_json".into(), approval_effect(&file));
            Ok(result(&a.idea, values, effects))
        }
        Approved::Refused(refusal) => bail!("{refusal}"),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IdeaReason {
    idea: String,
    reason: String,
}

fn park(h: &IdeaHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: IdeaReason = parse("idea_park", payload)?;
    let rescues = park_cancel::park(&h.jobs(), tx, &a.idea, &a.reason, fleet_clock(tx)?)?;
    let mut values = Map::new();
    values.insert("state".into(), json!(park_cancel::PARKED));
    values.insert("rescues".into(), json!(rescues));
    Ok(result(&a.idea, values, Map::new()))
}

fn cancel(h: &IdeaHandlers, tx: &Connection, payload: &Value) -> Result<Value> {
    let a: IdeaReason = parse("idea_cancel", payload)?;
    if a.reason.trim().is_empty() {
        bail!("give a nonempty reason for the cancel of {}", a.idea);
    }
    let done = park_cancel::cancel(&h.jobs(), tx, &a.idea, &a.reason)?;
    let mut values = Map::new();
    values.insert("cancelled".into(), json!(done.cancelled));
    values.insert("stop".into(), json!(done.stop));
    Ok(result(&a.idea, values, Map::new()))
}
