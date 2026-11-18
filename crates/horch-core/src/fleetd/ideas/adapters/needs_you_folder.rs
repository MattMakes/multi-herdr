//! The needs-you folder `/fleet/needs-you/`: 1 file for each open item,
//! removed when the item ends.
//!
//! The folder mirrors the open items of the `needs_you` table (OR-3). Each
//! open item has the file `<kind>-<row>.md`, written atomically. A `.md`
//! file whose item is not open is removed. A pass mirrors at most
//! `pass_budget` files, in the turn order of its [`Cursor`].

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use anyhow::{Context, Result};

use crate::fleetd::fleet_runtime::domain::budget::{Cursor, PassBudget};
use crate::fleetd::ideas::domain::needs_you::Item;
use crate::fsx;

/// What 1 pass of the mirror did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mirrored {
    /// The files written for an item that had no file.
    pub new: usize,
    /// The files written again because their item changed.
    pub updated: usize,
    /// The files removed because their item is not open.
    pub removed: usize,
    /// The count of open items.
    pub open: usize,
}

/// The file name of `item`: `<kind>-<row>.md`. The row is the key after
/// `<kind>:`. Each character that is not ASCII alphanumeric, `-` or `_`
/// becomes `-`.
pub fn file_name(item: &Item) -> String {
    let prefix = format!("{}:", item.kind);
    let row = item.key.strip_prefix(&prefix).unwrap_or(&item.entity_id);
    let safe = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect()
    };
    format!("{}-{}.md", safe(&item.kind), safe(row))
}

/// The text of the file of `item`.
fn body(item: &Item) -> String {
    format!(
        "# {kind}: {entity} {id}\n\n{text}\n\nkey: {key}\nopen since: {since}\n",
        kind = item.kind,
        entity = item.entity,
        id = item.entity_id,
        text = item.text.trim_end(),
        key = item.key,
        since = item.created_at,
    )
}

/// Mirror `open` into `dir` for 1 pass. The pass takes at most `budget`
/// file names, in the turn order of `cursor`, from the open items and the
/// `.md` files in `dir`. It writes the file of each open item whose text
/// differs, and removes each file that no open item has. The caller saves
/// `cursor` in the transaction of its pass.
pub fn mirror(
    dir: &Path,
    open: &[Item],
    cursor: &mut Cursor,
    budget: PassBudget,
    now: &str,
) -> Result<Mirrored> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let wanted: BTreeMap<String, &Item> = open.iter().map(|i| (file_name(i), i)).collect();
    let mut names: Vec<String> = wanted.keys().cloned().collect();
    for name in md_files(dir)? {
        if !wanted.contains_key(&name) {
            names.push(name);
        }
    }
    let mut done = Mirrored {
        open: open.len(),
        ..Mirrored::default()
    };
    for name in cursor.take(&names, budget, now) {
        let path = dir.join(name);
        match wanted.get(name) {
            Some(item) => {
                let text = body(item);
                match std::fs::read_to_string(&path) {
                    Ok(old) if old == text => continue,
                    Ok(_) => done.updated += 1,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => done.new += 1,
                    Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
                }
                fsx::write_atomic(&path, text.as_bytes(), 0o600)?;
            }
            None => match std::fs::remove_file(&path) {
                Ok(()) => done.removed += 1,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("removing {}", path.display())),
            },
        }
    }
    Ok(done)
}

/// The names of the `.md` files in `dir`. A hidden name (a temp file of
/// [`fsx::write_atomic`]) is not one.
fn md_files(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.ends_with(".md") && !name.starts_with('.') && entry.file_type()?.is_file() {
            names.push(name);
        }
    }
    Ok(names)
}
