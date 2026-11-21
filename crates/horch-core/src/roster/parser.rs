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
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            if stem != "README" {
                out.push((stem, path));
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Split `---\n<frontmatter>\n---\n<body>`.
pub(crate) fn split_frontmatter(text: &str) -> Result<(&str, &str)> {
    let nl = if text.starts_with("---\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let rest = text
        .strip_prefix(&format!("---{nl}"))
        .context("file does not start with a '---' frontmatter fence")?;
    let fence = format!("{nl}---{nl}");
    let end = rest
        .find(&fence)
        .context("frontmatter is not closed by a '---' line")?;
    Ok((&rest[..end], &rest[end + fence.len()..]))
}

fn body_text(body: &str) -> String {
    body.replace("\r\n", "\n")
        .trim_end_matches('\n')
        .to_string()
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
    t.persona = body_text(body);
    Ok(t)
}

pub(crate) fn parse_base(stem: &str, text: &str) -> Result<Base> {
    let (front, body) = split_frontmatter(text)?;
    let mut b: Base = serde_yaml::from_str(front).context("parsing frontmatter")?;
    if b.name != stem {
        bail!("name is '{}' but the filename says '{stem}'", b.name);
    }
    b.body = body_text(body);
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, names: &[&str]) {
        for name in names {
            std::fs::write(dir.join(name), "x").unwrap();
        }
    }

    #[test]
    fn md_files_lists_stem_and_path_sorted_by_stem() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), &["b.md", "a.md", "c.d.md"]);
        assert_eq!(
            md_files(tmp.path()).unwrap(),
            vec![
                ("a".to_string(), tmp.path().join("a.md")),
                ("b".to_string(), tmp.path().join("b.md")),
                ("c.d".to_string(), tmp.path().join("c.d.md")),
            ]
        );
    }

    #[test]
    fn md_files_skips_readmes_and_files_that_are_not_markdown() {
        let tmp = tempfile::tempdir().unwrap();
        touch(
            tmp.path(),
            &[
                "README.md",
                "READMEs.md",
                "notes.txt",
                "plain",
                "upper.MD",
                ".md",
                "keep.md",
            ],
        );
        std::fs::create_dir(tmp.path().join("_base")).unwrap();
        let stems: Vec<String> = md_files(tmp.path())
            .unwrap()
            .into_iter()
            .map(|(stem, _)| stem)
            .collect();
        assert_eq!(stems, ["READMEs", "keep"]);
    }

    #[test]
    fn split_frontmatter_accepts_crlf_fences() {
        assert_eq!(
            split_frontmatter("---\r\nname: a\r\n---\r\nbody\r\n").unwrap(),
            ("name: a", "body\r\n")
        );
    }

    #[test]
    fn a_crlf_file_parses_to_the_same_teammate_and_base_as_its_lf_form() {
        let teammates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
        let lf = std::fs::read_to_string(teammates.join("opus.md")).unwrap();
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(
            parse_teammate("opus", &crlf).unwrap(),
            parse_teammate("opus", &lf).unwrap()
        );
        let lf = std::fs::read_to_string(teammates.join("_base/channel.md")).unwrap();
        let crlf = lf.replace('\n', "\r\n");
        assert_eq!(
            parse_base("channel", &crlf).unwrap(),
            parse_base("channel", &lf).unwrap()
        );
    }

    #[test]
    fn md_files_names_the_directory_it_cannot_read() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing");
        let message = format!("{:#}", md_files(&missing).unwrap_err());
        assert!(
            message.starts_with(&format!("reading {}: ", missing.display())),
            "{message}"
        );
    }
}
