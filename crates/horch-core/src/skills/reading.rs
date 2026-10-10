//! Reading a catalog skill's files outside a bundle (`horch skills read`),
//! and finding the catalog skills a bundle's texts name but do not hold.
//!
//! A worker's bundle holds only its teammate's skills, but the skill texts
//! name many others. The briefing says how many, and `horch skills read`
//! prints one on demand, so no listed skill costs context it does not use.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::catalog::{CatalogEntry, CatalogSource, SkillCatalog};

/// The files of skill `id`, as paths relative to its directory, sorted.
/// A bundled skill lists its compiled-in files; any other skill lists the
/// regular files of its directory on disk.
pub fn files(catalog: &SkillCatalog, id: &str) -> Result<Vec<String>> {
    let entry = lookup(catalog, id)?;
    let mut out: Vec<String> = match disk_dir(catalog, entry)? {
        None => entry.files.iter().map(|(p, _)| (*p).to_owned()).collect(),
        Some(dir) => {
            let mut found = Vec::new();
            walk(&dir, &dir, &mut found)?;
            found
        }
    };
    out.sort();
    Ok(out)
}

/// The bytes of `file` (relative to the skill directory) of skill `id`.
/// Refuses an empty, absolute or `..` path, and a file the skill does not
/// have. Reads only; writes nothing.
pub fn read(catalog: &SkillCatalog, id: &str, file: &str) -> Result<Vec<u8>> {
    let rel = Path::new(file);
    if file.is_empty()
        || rel
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        bail!(
            "'{file}' is not a path inside the skill directory: give a relative path without '..'"
        );
    }
    let wanted = rel
        .components()
        .filter(|c| *c != Component::CurDir)
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    let entry = lookup(catalog, id)?;
    match disk_dir(catalog, entry)? {
        None => entry
            .files
            .iter()
            .find(|(p, _)| *p == wanted)
            .map(|(_, b)| b.to_vec())
            .with_context(|| no_file(id, file)),
        Some(dir) => {
            if !files(catalog, id)?.contains(&wanted) {
                bail!(no_file(id, file));
            }
            std::fs::read(dir.join(&wanted))
                .with_context(|| format!("skill '{id}': cannot read '{file}'"))
        }
    }
}

fn no_file(id: &str, file: &str) -> String {
    format!("skill '{id}' has no file '{file}'; list its files with horch skills read {id} --files")
}

fn lookup<'a>(catalog: &'a SkillCatalog, id: &str) -> Result<&'a CatalogEntry> {
    catalog
        .lookup(id)
        .with_context(|| format!("no skill '{id}' in the catalog; list skills with horch skills"))
}

/// The directory on disk of a skill whose files are not compiled in.
fn disk_dir(catalog: &SkillCatalog, entry: &CatalogEntry) -> Result<Option<PathBuf>> {
    Ok(match &entry.source {
        CatalogSource::Bundled => None,
        CatalogSource::Marketplace { .. } => Some(
            catalog
                .store_dir(entry)?
                .with_context(|| format!("skill '{}': no marketplace store", entry.id))?,
        ),
        CatalogSource::Operator { .. } => entry.operator_dir(),
        CatalogSource::Plugin { dir, .. } => Some(dir.clone()),
    })
}

/// Every regular file under `dir`, relative to `root`, with `/`
/// separators. Symlinks are skipped: a skill tree never holds one.
fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    for item in std::fs::read_dir(dir).with_context(|| format!("{}", dir.display()))? {
        let item = item?;
        let kind = item.file_type()?;
        let path = item.path();
        if kind.is_dir() {
            walk(root, &path, out)?;
        } else if kind.is_file() {
            let rel = path.strip_prefix(root).expect("walk stays under root");
            out.push(
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join("/"),
            );
        }
    }
    Ok(())
}

/// How often each of `ids` is named in `texts`.
///
/// A name is a maximal run of `[A-Za-z0-9_-]`, so an id never matches
/// inside a longer word or id: `code-review` does not match in
/// `godot-code-review`. An id with a hyphen counts on every such match. An
/// id without one (`tdd`, `check`) is often an English word, so it counts
/// only in 3 forms: in backticks (`` `tdd` ``), in a link path
/// (`../tdd/SKILL.md`), or qualified by a namespace (`horch:tdd`). Plain
/// prose such as "check the build" does not count.
pub fn mentions<'a>(
    texts: impl IntoIterator<Item = &'a [u8]>,
    ids: &BTreeSet<&str>,
) -> BTreeMap<String, usize> {
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    let mut out = BTreeMap::new();
    for text in texts {
        let mut i = 0;
        while i < text.len() {
            if !word(text[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < text.len() && word(text[i]) {
                i += 1;
            }
            let Ok(name) = std::str::from_utf8(&text[start..i]) else {
                continue;
            };
            if !ids.contains(name) {
                continue;
            }
            let before = start.checked_sub(1).map(|j| text[j]);
            let after = text.get(i).copied();
            let counts = name.contains('-')
                || (before == Some(b'`') && after == Some(b'`'))
                || (before == Some(b'/') && after == Some(b'/'))
                || (before == Some(b':') && start >= 2 && word(text[start - 2]));
            if counts {
                *out.entry(name.to_owned()).or_insert(0) += 1;
            }
        }
    }
    out
}

/// The catalog skills that the `.md` files of the `activated` skills name
/// but that are not activated, most-named first, ties in id order.
/// Candidates are the bundled and marketplace skills, which
/// `horch skills read` can print. Only compiled-in text is scanned.
pub fn outside_bundle(catalog: &SkillCatalog, activated: &[&str]) -> Vec<(String, usize)> {
    let ids: BTreeSet<&str> = catalog
        .entries()
        .filter(|e| {
            matches!(
                e.source,
                CatalogSource::Bundled | CatalogSource::Marketplace { .. }
            )
        })
        .map(|e| e.id.as_str())
        .filter(|id| !activated.contains(id))
        .collect();
    let texts = activated
        .iter()
        .filter_map(|id| catalog.lookup(id))
        .flat_map(|e| e.files.iter())
        .filter(|(p, _)| p.ends_with(".md"))
        .map(|(_, b)| *b);
    let mut found: Vec<(String, usize)> = mentions(texts, &ids).into_iter().collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    found
}
