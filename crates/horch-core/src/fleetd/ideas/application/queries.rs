//! horch ideas and horch idea show: the board of each idea, what it waits
//! on, and the open needs-you items (read model `IdeaBoard`); 1 idea with
//! what you approve (`ApprovedAssumptionsAndSpec`); and the diffs of the
//! features in flight, to build on them at plan time (`InFlightDiffs`;
//! docs/specs/fleetd/design.md, "Ideas", "Between features"; MAP.md of
//! fleetd-p6, Q13).
//!
//! Each query only reads: the ledger, opened read-only by the caller, the
//! idea folder and git.

use std::io::Read;
use std::path::Path;

use anyhow::{bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

use crate::digest::sha256_bytes;
use crate::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use crate::fleetd::ideas::adapters::sqlite as needs_you_rows;
use crate::fleetd::ideas::domain::idea::TERMINAL;
use crate::fleetd::ideas::domain::needs_you::Item;
use crate::fleetd::integration::adapters::sqlite as feature_rows;
use crate::fleetd::integration::adapters::trial_merges_store::in_flight_features;

/// The size limit of 1 file of an idea folder (SR-6).
const FILE_LIMIT: u64 = 64 * 1024;

/// 1 line of the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardLine {
    pub id: String,
    /// The feature name, or its id when it has no name.
    pub feature: String,
    pub state: String,
    /// `you`, `question <id>`, `task`, `gate` or `idea-lead`.
    pub waits_on: String,
    /// A ledger stamp.
    pub since: String,
}

/// 1 open needs-you item, as the board shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeedsYouLine {
    pub key: String,
    pub kind: String,
    pub text: String,
    pub since: String,
}

/// The read model `IdeaBoard`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaBoard {
    pub ideas: Vec<BoardLine>,
    pub needs_you: Vec<NeedsYouLine>,
}

/// Each idea that is not terminal, by id, and each open needs-you item,
/// oldest first.
pub fn idea_board(conn: &Connection) -> Result<IdeaBoard> {
    let sql = format!(
        "SELECT id FROM ideas WHERE state NOT IN ('{}', '{}') ORDER BY id",
        TERMINAL[0], TERMINAL[1]
    );
    let ids: Vec<String> = conn
        .prepare(&sql)?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut ideas = Vec::with_capacity(ids.len());
    for id in ids {
        let row = ideas_store::idea(conn, &id)?.context("an idea row went away")?;
        ideas.push(board_line(conn, row)?);
    }
    let needs_you = needs_you_rows::open(conn)?
        .into_iter()
        .map(|i: Item| NeedsYouLine {
            key: i.key,
            kind: i.kind,
            text: i.text,
            since: i.created_at,
        })
        .collect();
    Ok(IdeaBoard { ideas, needs_you })
}

/// What the idea `row` waits on, and since when: you first, then an open
/// question of its feature, then its state.
fn board_line(conn: &Connection, row: IdeaRow) -> Result<BoardLine> {
    let question = open_question(conn, &row.feature_id)?;
    let (waits_on, since) = if row.waiting_on_you_since.is_some()
        || matches!(row.state.as_str(), "awaiting_approval" | "parked")
    {
        let since = row.waiting_on_you_since.clone();
        (
            "you".to_string(),
            since.unwrap_or_else(|| row.updated_at.clone()),
        )
    } else if let Some((id, asked_at)) = question {
        (format!("question {id}"), asked_at)
    } else {
        let on = match row.state.as_str() {
            "in_progress" => "task",
            "integrating" => "gate",
            _ => "idea-lead",
        };
        (on.to_string(), row.updated_at.clone())
    };
    Ok(BoardLine {
        feature: feature_name(conn, &row.feature_id)?,
        id: row.id,
        state: row.state,
        waits_on,
        since,
    })
}

/// The oldest question of `feature` with no answer: its id and when it was
/// asked.
fn open_question(conn: &Connection, feature: &str) -> Result<Option<(String, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, asked_at FROM questions
             WHERE feature_id = ?1 AND state IN ('open', 'escalated')
             ORDER BY asked_at, id LIMIT 1",
            [feature],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

fn feature_name(conn: &Connection, id: &str) -> Result<String> {
    let name = feature_rows::feature(conn, id)?.and_then(|f| f.name);
    Ok(name.unwrap_or_else(|| id.to_string()))
}

/// `assumptions.md` and the sha256 of its bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assumptions {
    pub text: String,
    pub sha256: String,
}

/// 1 file of `amendments/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Amendment {
    pub name: String,
    pub text: String,
}

/// The read model `ApprovedAssumptionsAndSpec`: what you approve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IdeaView {
    pub id: String,
    /// The feature name, or its id when it has no name.
    pub feature: String,
    /// The feature id (`F-NNNN`), as plan.yaml names it.
    pub feature_id: String,
    pub state: String,
    /// `idea.md`, or `None` when the folder has none.
    pub idea: Option<String>,
    pub amendments: Vec<Amendment>,
    pub assumptions: Option<Assumptions>,
    /// `plan.verdict.json`, as fleetd wrote it.
    pub plan_verdict: Option<String>,
}

/// The idea `id` with the files of its folder `<fleet_root>/ideas/<id>/`,
/// or `None` when the ledger has no such idea.
pub fn idea_view(conn: &Connection, fleet_root: &Path, id: &str) -> Result<Option<IdeaView>> {
    let Some(row) = ideas_store::idea(conn, id)? else {
        return Ok(None);
    };
    if id.contains(['/', '\\']) || id.starts_with('.') {
        bail!("the idea id {id:?} is not a folder name");
    }
    let dir = fleet_root.join("ideas").join(id);
    let assumptions = read_checked(&dir.join("assumptions.md"))?.map(|bytes| Assumptions {
        sha256: sha256_bytes(&bytes).hex(),
        text: String::from_utf8_lossy(&bytes).into_owned(),
    });
    Ok(Some(IdeaView {
        feature: feature_name(conn, &row.feature_id)?,
        feature_id: row.feature_id,
        id: row.id,
        state: row.state,
        idea: read_text(&dir.join("idea.md"))?,
        amendments: amendments(&dir.join("amendments"))?,
        assumptions,
        plan_verdict: read_text(&dir.join("plan.verdict.json"))?,
    }))
}

/// Each regular file of `dir`, by name. No folder gives no amendment.
fn amendments(dir: &Path) -> Result<Vec<Amendment>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("reading {}", dir.display())),
    };
    let mut names: Vec<String> = entries
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()?;
    names.sort();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        if let Some(text) = read_text(&dir.join(&name))? {
            out.push(Amendment { name, text });
        }
    }
    Ok(out)
}

fn read_text(path: &Path) -> Result<Option<String>> {
    Ok(read_checked(path)?.map(|b| String::from_utf8_lossy(&b).into_owned()))
}

/// The bytes of `path`, or `None` when it does not exist. A sub-orchestrator
/// writes the folder, so a link, a file that is not regular and a file over
/// [`FILE_LIMIT`] are refused (SR-6).
fn read_checked(path: &Path) -> Result<Option<Vec<u8>>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    if !meta.file_type().is_file() {
        bail!("{} is not a regular file", path.display());
    }
    if meta.len() > FILE_LIMIT {
        bail!("{} is over {FILE_LIMIT} bytes", path.display());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(FILE_LIMIT + 1).read_to_end(&mut bytes))
        .with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() as u64 > FILE_LIMIT {
        bail!("{} is over {FILE_LIMIT} bytes", path.display());
    }
    Ok(Some(bytes))
}

/// The git reads of [`in_flight_diffs`]. Each compares `branch` with the
/// merge base of `primary` and `branch`.
pub trait BranchDiffs {
    /// The files that `branch` changed, by path.
    fn changed_files(&self, primary: &str, branch: &str) -> Result<Vec<String>>;
    /// The diff of `branch`.
    fn diff(&self, primary: &str, branch: &str) -> Result<String>;
}

/// 1 other feature in flight and what it changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InFlightFeature {
    pub feature: String,
    pub branch: String,
    pub files: Vec<String>,
    pub diff: String,
}

/// The read model `InFlightDiffs`: each feature in flight (`open` or
/// `ready`) of the primary branch of the feature of the idea `id`, but that
/// feature, oldest first. `None` when the ledger has no such idea.
pub fn in_flight_diffs(
    conn: &Connection,
    git: &dyn BranchDiffs,
    id: &str,
) -> Result<Option<Vec<InFlightFeature>>> {
    let Some(row) = ideas_store::idea(conn, id)? else {
        return Ok(None);
    };
    let own = feature_rows::feature(conn, &row.feature_id)?
        .with_context(|| format!("the idea {id} names no feature row {}", row.feature_id))?;
    let mut out = Vec::new();
    for (feature, branch) in in_flight_features(conn, &own.primary_branch)? {
        if feature == own.id {
            continue;
        }
        out.push(InFlightFeature {
            files: git.changed_files(&own.primary_branch, &branch)?,
            diff: git.diff(&own.primary_branch, &branch)?,
            feature,
            branch,
        });
    }
    Ok(Some(out))
}
