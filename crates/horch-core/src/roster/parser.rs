//! Reading one `---` frontmatter file into a [`Teammate`] or a [`Base`].

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use super::{Base, Teammate};

/// `*.md` in one directory as `(stem, path)`, sorted, skipping READMEs.
pub(crate) fn md_files(dir: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        if stem == "README" {
            continue;
        }
        out.push((stem, path));
    }
    out.sort();
    Ok(out)
}

/// Split `---\n<frontmatter>\n---\n<body>`.
pub(crate) fn split_frontmatter(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .context("file does not start with a '---' frontmatter fence")?;
    let end = rest
        .find("\n---\n")
        .context("frontmatter is not closed by a '---' line")?;
    Ok((&rest[..end], &rest[end + 5..]))
}

pub(crate) fn parse_teammate(stem: &str, text: &str) -> Result<Teammate> {
    let (front, body) = split_frontmatter(text)?;
    let mut t: Teammate = serde_yaml::from_str(front).context("parsing frontmatter")?;
    if t.name != stem {
        bail!(
            "name is '{}' but the filename says '{stem}'; the filename is the id",
            t.name
        );
    }
    t.persona = body.trim_end_matches('\n').to_string();
    Ok(t)
}

pub(crate) fn parse_base(stem: &str, text: &str) -> Result<Base> {
    let (front, body) = split_frontmatter(text)?;
    let mut b: Base = serde_yaml::from_str(front).context("parsing frontmatter")?;
    if b.name != stem {
        bail!("name is '{}' but the filename says '{stem}'", b.name);
    }
    b.body = body.trim_end_matches('\n').to_string();
    Ok(b)
}
