use std::io::Read;
use std::path::Path;

use anyhow::{bail, Context, Result};
use horch_platform::channel::User;
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::fact_data;
use crate::fleetd::fleet_runtime::application::jobs::JobCtx;
use crate::fleetd::fleet_runtime::application::ledger::{record, Cause};
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use crate::fleetd::ideas::adapters::ideas_store::{insert_idea, NewIdea};
use crate::fleetd::ideas::domain::idea::FIRST;
use crate::fleetd::ideas::domain::idea_folder::{IdeaFile, FILE_LIMIT, IDEA_MD};
use crate::fleetd::integration::application::new_features::{join_feature, open_feature, Project};
use crate::fsx::Created;
use crate::taskledger::model::EventKind;
use crate::taskledger::util;

pub const ADDED_CAUSE: &str = "idea added";

const FRONT_ROOM: u64 = 1024;

pub const MAX_TEXT: u64 = FILE_LIMIT - FRONT_ROOM;

const PRIORITY: i64 = 0;

#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    New(Project<'a>),
    Join(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub feature: String,
    pub file: IdeaFile,
}

pub fn add_idea(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    text: &str,
    source: &str,
    target: Target<'_>,
    cause: Cause<'_>,
) -> Result<Added> {
    add_dependent_idea(tx, ctx, text, source, target, &[], cause)
}

pub fn add_dependent_idea(
    tx: &Connection,
    ctx: &JobCtx<'_>,
    text: &str,
    source: &str,
    target: Target<'_>,
    depends_on: &[String],
    cause: Cause<'_>,
) -> Result<Added> {
    if text.trim().is_empty() {
        bail!("the idea is empty: give its text, or a file with -f");
    }
    if text.len() as u64 > MAX_TEXT {
        bail!(
            "the idea is {} bytes: an idea holds at most {MAX_TEXT} bytes",
            text.len()
        );
    }
    let feature = match target {
        Target::New(project) => open_feature(tx, ctx, &project, cause)?,
        Target::Join(feature) => join_feature(tx, ctx, feature, cause)?,
    };
    let file = IdeaFile {
        id: next_id(tx)?,
        created: util::now(),
        source: source.to_string(),
        priority: PRIORITY,
        depends_on: depends_on.to_vec(),
        text: text.to_string(),
    };
    let new = NewIdea {
        id: &file.id,
        feature_id: &feature,
        priority: file.priority,
    };
    insert_idea(tx, ctx.guard, &new, cause)?;
    record(
        tx,
        ctx.guard,
        EventKind::Added,
        Entity::Idea,
        &file.id,
        Cause {
            actor: cause.actor,
            text: ADDED_CAUSE,
        },
        &json!({
            "created": file.created,
            "source": file.source,
            "priority": file.priority,
            "depends_on": file.depends_on,
            "text": file.text,
        }),
    )?;
    Ok(Added { feature, file })
}

fn next_id(tx: &Connection) -> Result<String> {
    let last: i64 = tx.query_row(
        "SELECT COALESCE(MAX(CAST(substr(id, 3) AS INTEGER)), 0) FROM ideas",
        [],
        |r| r.get(0),
    )?;
    Ok(format!("I-{:04}", last + 1))
}

pub fn idea_text_from_file(path: &Path) -> Result<String> {
    let file =
        std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read {}", path.display()))?;
    if bytes.len() as u64 > FILE_LIMIT {
        bail!(
            "{} is over 64 KiB: an idea file holds at most {FILE_LIMIT} bytes",
            path.display()
        );
    }
    String::from_utf8(bytes).with_context(|| format!("{} is not UTF-8 text", path.display()))
}

pub fn write_idea_file(ideas: &Path, owner: User, file: &IdeaFile) -> Result<Created> {
    let folder = IdeaFolder::create(ideas, &file.id, owner)?;
    Ok(folder.write_idea(file)?)
}

pub fn pending_idea_files(conn: &Connection, ideas: &Path) -> Result<Vec<IdeaFile>> {
    let mut stmt = conn.prepare("SELECT id FROM ideas WHERE state = ?1 ORDER BY id")?;
    let ids = stmt
        .query_map([FIRST], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    let mut pending = Vec::new();
    for id in ids {
        if std::fs::symlink_metadata(ideas.join(&id).join(IDEA_MD)).is_ok() {
            continue;
        }
        let Some(data) = fact_data(conn, Entity::Idea, &id, EventKind::Added, ADDED_CAUSE)? else {
            continue;
        };
        pending.push(idea_file(id, &data)?);
    }
    Ok(pending)
}

fn idea_file(id: String, data: &str) -> Result<IdeaFile> {
    let v: serde_json::Value = serde_json::from_str(data)
        .with_context(|| format!("the added event of {id} is not JSON"))?;
    let field = |name: &str| -> Result<String> {
        v[name]
            .as_str()
            .map(str::to_string)
            .with_context(|| format!("the added event of {id} has no {name}"))
    };
    let priority = v["priority"]
        .as_i64()
        .with_context(|| format!("the added event of {id} has no priority"))?;
    let depends_on = match &v["depends_on"] {
        serde_json::Value::Null => Vec::new(),
        d => serde_json::from_value(d.clone())
            .with_context(|| format!("the added event of {id} has a bad depends_on"))?,
    };
    Ok(IdeaFile {
        created: field("created")?,
        source: field("source")?,
        priority,
        depends_on,
        text: field("text")?,
        id,
    })
}
