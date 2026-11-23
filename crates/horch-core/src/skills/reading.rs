use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::catalog::{CatalogEntry, CatalogSource, SkillCatalog};

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

pub fn mentions<'a>(
    texts: impl IntoIterator<Item = &'a [u8]>,
    ids: &BTreeSet<&str>,
) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for text in texts {
        let named = words(text)
            .filter_map(|(s, e)| Some((s, e, std::str::from_utf8(&text[s..e]).ok()?)))
            .filter(|(_, _, name)| ids.contains(name))
            .filter(|(s, e, name)| is_mention(text, *s, *e, name));
        for (_, _, name) in named {
            *out.entry(name.to_owned()).or_insert(0) += 1;
        }
    }
    out
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')
}

fn words(text: &[u8]) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut at = 0;
    text.split(|&b| !is_word(b)).filter_map(move |run| {
        let start = at;
        at += run.len() + 1;
        (!run.is_empty()).then_some((start, start + run.len()))
    })
}

fn is_mention(text: &[u8], start: usize, end: usize, name: &str) -> bool {
    let before = start.checked_sub(1).map(|j| text[j]);
    let after = text.get(end).copied();
    name.contains('-')
        || match (before, after) {
            (Some(b'`'), Some(b'`')) | (Some(b'/'), Some(b'/')) => true,
            (Some(b':'), _) => start >= 2 && is_word(text[start - 2]),
            _ => false,
        }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn count(text: &str, id: &str) -> usize {
        let ids = BTreeSet::from([id]);
        mentions([text.as_bytes()], &ids)
            .get(id)
            .copied()
            .unwrap_or(0)
    }

    #[test]
    fn a_hyphenated_id_counts_in_plain_prose() {
        assert_eq!(
            count("run code-review now, then code-review again", "code-review"),
            2
        );
    }

    #[test]
    fn a_hyphenated_id_does_not_match_inside_a_longer_id() {
        assert_eq!(
            count("godot-code-review and code-review-all", "code-review"),
            0
        );
    }

    #[test]
    fn a_bare_id_counts_in_backticks() {
        assert_eq!(count("use `tdd` here", "tdd"), 1);
    }

    #[test]
    fn a_bare_id_counts_between_slashes() {
        assert_eq!(count("see ../tdd/SKILL.md", "tdd"), 1);
    }

    #[test]
    fn a_bare_id_counts_after_a_namespace() {
        assert_eq!(count("load horch:tdd first", "tdd"), 1);
    }

    #[test]
    fn a_bare_id_in_prose_does_not_count() {
        assert_eq!(count("check the build, tdd is fine", "tdd"), 0);
        assert_eq!(count("`tdd and tdd/ and /tdd and :tdd", "tdd"), 0);
    }

    #[test]
    fn a_colon_counts_only_after_a_word_byte() {
        assert_eq!(count(" :tdd", "tdd"), 0);
        assert_eq!(count(":tdd", "tdd"), 0);
        assert_eq!(count("a:tdd", "tdd"), 1);
    }

    #[test]
    fn a_hyphenated_id_counts_after_a_lone_colon() {
        assert_eq!(count(" :code-review", "code-review"), 1);
    }

    #[test]
    fn id_at_the_text_edges_is_judged() {
        assert_eq!(count("code-review", "code-review"), 1);
        assert_eq!(count("tdd", "tdd"), 0);
    }

    #[test]
    fn counts_add_up_across_texts_and_ids() {
        let ids = BTreeSet::from(["tdd", "check-it"]);
        let texts = [&b"`tdd` check-it"[..], &b"check-it `tdd` `tdd`"[..]];
        let got = mentions(texts, &ids);
        assert_eq!(got.get("tdd"), Some(&3));
        assert_eq!(got.get("check-it"), Some(&2));
    }
}
