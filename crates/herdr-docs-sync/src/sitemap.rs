//! Discovering the documentation pages from herdr.dev's sitemap.
//!
//! The sitemap is the authoritative page list, which is why it is used instead of
//! crawling links: pages appear and disappear between releases, and a crawl would
//! also have to decide what counts as documentation.
//!
//! `sitemap.xml` is a `<sitemapindex>` pointing at `<sitemap><loc>` children; those
//! are `<urlset>`s of `<url><loc>`. Only `<loc>` *element text* counts - the
//! entries also carry `xhtml:link href="..."` alternates for the translated sites,
//! and scanning attributes would pull in every `/ja/` and `/zh-cn/` page plus
//! malformed fragments.

use anyhow::{bail, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

/// The site whose docs this mirrors.
pub const DOCS_PREFIX: &str = "https://herdr.dev/docs/";

/// What a sitemap document turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sitemap {
    /// A `<sitemapindex>`: these are more sitemaps to fetch.
    Index(Vec<String>),
    /// A `<urlset>`: these are pages.
    Urls(Vec<String>),
}

/// Parse a sitemap, reading only `<loc>` element text.
pub fn parse(xml: &str) -> Result<Sitemap> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut is_index = false;
    let mut in_loc = false;
    let mut depth = 0i32;
    let mut locations = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => {
                // A truncated download parses as valid-so-far XML and would yield a
                // short page list, which the prune step would then act on by
                // deleting good local files. Refuse it instead.
                if depth != 0 {
                    bail!(
                        "sitemap XML ended with {depth} element(s) still open; \
                         the download looks truncated"
                    );
                }
                break;
            }
            Ok(Event::Start(e)) => {
                depth += 1;
                match local_name(e.name().as_ref()) {
                    b"sitemapindex" => is_index = true,
                    b"loc" => in_loc = true,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                depth -= 1;
                if local_name(e.name().as_ref()) == b"loc" {
                    in_loc = false;
                }
            }
            Ok(Event::Text(text)) if in_loc => {
                let value = text.unescape()?.trim().to_string();
                if !value.is_empty() {
                    locations.push(value);
                }
            }
            Ok(_) => {}
            Err(e) => bail!("malformed sitemap XML at byte {}: {e}", reader.buffer_position()),
        }
        buf.clear();
    }

    Ok(if is_index {
        Sitemap::Index(locations)
    } else {
        Sitemap::Urls(locations)
    })
}

/// Strip an XML namespace prefix, so `xhtml:link` compares as `link`.
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// One documentation page to mirror.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocPage {
    pub url: String,
    /// The file this page is written to, e.g. `install.md`.
    pub file_name: String,
}

/// Select the English documentation pages and name their files.
///
/// `/docs/` becomes `README.md`; `/docs/<slug>/` becomes `<slug>.md`. The prefix
/// match excludes the `/ja/` and `/zh-cn/` translations, and anything nested more
/// than one level deep is skipped rather than flattened into a colliding name.
pub fn doc_pages(urls: &[String]) -> Vec<DocPage> {
    let mut pages: Vec<DocPage> = urls
        .iter()
        .filter_map(|url| {
            let rest = url.strip_prefix(DOCS_PREFIX)?;
            let slug = rest.trim_end_matches('/');
            // A sitemap can list the same page with and without a trailing slash.
            let file_name = if slug.is_empty() {
                "README.md".to_string()
            } else if slug.contains('/') || !is_plain_slug(slug) {
                return None;
            } else {
                format!("{slug}.md")
            };
            Some(DocPage {
                url: url.clone(),
                file_name,
            })
        })
        .collect();
    pages.sort();
    pages.dedup_by(|a, b| a.file_name == b.file_name);
    pages
}

/// A slug is a filename component, so keep it to characters that are safe in one.
fn is_plain_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !slug.starts_with('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_sitemap_index() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
            <sitemapindex xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
              <sitemap><loc>https://herdr.dev/sitemap-0.xml</loc></sitemap>
            </sitemapindex>"#;
        assert_eq!(
            parse(xml).unwrap(),
            Sitemap::Index(vec!["https://herdr.dev/sitemap-0.xml".to_string()])
        );
    }

    /// The real sitemap carries `xhtml:link href` alternates inside each `<url>`.
    /// Those must not be mistaken for pages - that is what produced the
    /// `https://herdr.dev/docs/"/>` garbage a naive text scan finds.
    #[test]
    fn ignores_xhtml_link_alternates() {
        let xml = r#"<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"
                             xmlns:xhtml="http://www.w3.org/1999/xhtml">
              <url>
                <loc>https://herdr.dev/docs/install/</loc>
                <xhtml:link rel="alternate" hreflang="ja" href="https://herdr.dev/ja/docs/install/"/>
                <xhtml:link rel="alternate" hreflang="zh-CN" href="https://herdr.dev/zh-cn/docs/install/"/>
              </url>
            </urlset>"#;
        assert_eq!(
            parse(xml).unwrap(),
            Sitemap::Urls(vec!["https://herdr.dev/docs/install/".to_string()])
        );
    }

    /// A truncated sitemap must be rejected rather than parsed into a short page
    /// list, because a short list would make the prune step delete good files.
    #[test]
    fn rejects_truncated_xml() {
        let err = parse("<urlset><url><loc>https://herdr.dev/docs/install/</loc>")
            .unwrap_err()
            .to_string();
        assert!(err.contains("looks truncated"), "{err}");

        assert!(parse("<urlset><loc>unclosed").is_err());
        assert!(parse("<urlset></noturlset>").is_err(), "mismatched end tag");
    }

    #[test]
    fn accepts_a_well_formed_but_empty_urlset() {
        assert_eq!(parse("<urlset></urlset>").unwrap(), Sitemap::Urls(vec![]));
    }

    #[test]
    fn maps_the_docs_index_to_readme() {
        let pages = doc_pages(&["https://herdr.dev/docs/".to_string()]);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].file_name, "README.md");
    }

    #[test]
    fn maps_slugs_to_markdown_files() {
        let urls: Vec<String> = [
            "https://herdr.dev/docs/install/",
            "https://herdr.dev/docs/cli-reference/",
            "https://herdr.dev/docs/config-reference/",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pages = doc_pages(&urls);
        let names: Vec<&str> = pages.iter().map(|p| p.file_name.as_str()).collect();
        assert_eq!(names, vec!["cli-reference.md", "config-reference.md", "install.md"]);
    }

    /// Only the English docs are mirrored; blog, marketing and translated pages
    /// are not documentation.
    #[test]
    fn excludes_translations_and_non_docs_pages() {
        let urls: Vec<String> = [
            "https://herdr.dev/",
            "https://herdr.dev/blog/",
            "https://herdr.dev/blog/herdr-0-6-3/",
            "https://herdr.dev/compare/",
            "https://herdr.dev/ja/docs/install/",
            "https://herdr.dev/zh-cn/docs/install/",
            "https://herdr.dev/docs/install/",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pages = doc_pages(&urls);
        assert_eq!(pages.len(), 1, "{pages:#?}");
        assert_eq!(pages[0].url, "https://herdr.dev/docs/install/");
    }

    /// The malformed entries a sitemap can contain must not become filenames.
    #[test]
    fn skips_entries_that_are_not_clean_slugs() {
        let urls: Vec<String> = [
            "https://herdr.dev/docs/\"/>",
            "https://herdr.dev/docs/a/b/",
            "https://herdr.dev/docs/../etc/",
            "https://herdr.dev/docs/.hidden/",
            "https://herdr.dev/docs/with space/",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert!(doc_pages(&urls).is_empty(), "{:#?}", doc_pages(&urls));
    }

    /// A page listed with and without a trailing slash is still one file.
    #[test]
    fn deduplicates_pages_that_map_to_the_same_file() {
        let urls: Vec<String> = [
            "https://herdr.dev/docs/install/",
            "https://herdr.dev/docs/install",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pages = doc_pages(&urls);
        assert_eq!(pages.len(), 1, "{pages:#?}");
    }

    /// Guards the whole pipeline against the live sitemap's real shape.
    #[test]
    fn handles_the_live_sitemap_shape_end_to_end() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
          <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"
                  xmlns:xhtml="http://www.w3.org/1999/xhtml">
            <url><loc>https://herdr.dev/</loc></url>
            <url><loc>https://herdr.dev/docs/</loc>
              <xhtml:link rel="alternate" hreflang="ja" href="https://herdr.dev/ja/docs/"/></url>
            <url><loc>https://herdr.dev/docs/agent-automation/</loc></url>
            <url><loc>https://herdr.dev/docs/troubleshooting/</loc></url>
            <url><loc>https://herdr.dev/ja/docs/install/</loc></url>
          </urlset>"#;
        let Sitemap::Urls(urls) = parse(xml).unwrap() else {
            panic!("expected a urlset");
        };
        let pages = doc_pages(&urls);
        let names: Vec<&str> = pages.iter().map(|p| p.file_name.as_str()).collect();
        assert_eq!(
            names,
            vec!["README.md", "agent-automation.md", "troubleshooting.md"]
        );
    }
}
