use anyhow::{anyhow, bail, Result};
use quick_xml::events::Event;
use quick_xml::Reader;

pub const DOCS_PREFIX: &str = "https://herdr.dev/docs/";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sitemap {
    Index(Vec<String>),
    Urls(Vec<String>),
}

pub fn parse(xml: &str) -> Result<Sitemap> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut scan = SitemapScan::default();
    let mut buf = Vec::new();

    while scan.event(next_event(&mut reader, &mut buf)?)? {
        buf.clear();
    }

    scan.finish()
}

fn next_event<'b>(reader: &mut Reader<&[u8]>, buf: &'b mut Vec<u8>) -> Result<Event<'b>> {
    reader.read_event_into(buf).map_err(|e| {
        anyhow!(
            "malformed sitemap XML at byte {}: {e}",
            reader.buffer_position()
        )
    })
}

#[derive(Default)]
struct SitemapScan {
    is_index: bool,
    in_loc: bool,
    depth: i32,
    locations: Vec<String>,
}

impl SitemapScan {
    fn event(&mut self, event: Event<'_>) -> Result<bool> {
        match event {
            Event::Eof => return Ok(false),
            Event::Start(e) => self.start(e.name().as_ref()),
            Event::End(e) => self.end(e.name().as_ref()),
            Event::Text(text) if self.in_loc => self.text(text.unescape()?.into_owned()),
            _ => {}
        }
        Ok(true)
    }

    fn start(&mut self, name: &[u8]) {
        self.depth += 1;
        let name = local_name(name);
        self.is_index |= name == b"sitemapindex";
        self.in_loc |= name == b"loc";
    }

    fn end(&mut self, name: &[u8]) {
        self.depth -= 1;
        self.in_loc &= local_name(name) != b"loc";
    }

    fn text(&mut self, value: String) {
        let value = value.trim();
        if !value.is_empty() {
            self.locations.push(value.to_string());
        }
    }

    fn finish(self) -> Result<Sitemap> {
        if self.depth != 0 {
            bail!(
                "sitemap XML ended with {} element(s) still open; \
                 the download looks truncated",
                self.depth
            );
        }
        Ok(if self.is_index {
            Sitemap::Index(self.locations)
        } else {
            Sitemap::Urls(self.locations)
        })
    }
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|b| *b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DocPage {
    pub url: String,
    pub file_name: String,
}

pub fn doc_pages(urls: &[String]) -> Vec<DocPage> {
    let mut pages: Vec<DocPage> = urls
        .iter()
        .filter_map(|url| {
            Some(DocPage {
                url: url.clone(),
                file_name: file_name(docs_slug(url)?)?,
            })
        })
        .collect();
    pages.sort();
    pages.dedup_by(|a, b| a.file_name == b.file_name);
    pages
}

pub fn unmirrored(urls: &[String]) -> Vec<String> {
    let mut skipped: Vec<String> = urls
        .iter()
        .filter(|url| docs_slug(url).is_some_and(|slug| file_name(slug).is_none()))
        .cloned()
        .collect();
    skipped.sort();
    skipped.dedup();
    skipped
}

fn docs_slug(url: &str) -> Option<&str> {
    Some(url.strip_prefix(DOCS_PREFIX)?.trim_end_matches('/'))
}

fn file_name(slug: &str) -> Option<String> {
    if slug.is_empty() {
        Some("README.md".to_string())
    } else if is_plain_slug(slug) {
        Some(format!("{slug}.md"))
    } else {
        None
    }
}

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
    fn collects_only_loc_text_and_trims_it() {
        let xml = "<urlset><url><loc> https://herdr.dev/docs/a/ </loc>\
                   <lastmod>2026-01-01</lastmod></url></urlset>";
        assert_eq!(
            parse(xml).unwrap(),
            Sitemap::Urls(vec!["https://herdr.dev/docs/a/".to_string()])
        );
    }

    #[test]
    fn rejects_mismatched_tags_as_malformed() {
        let err = parse("<urlset><url></urlset>").unwrap_err().to_string();
        assert!(err.contains("malformed sitemap XML"), "{err}");
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
        assert_eq!(
            names,
            vec!["cli-reference.md", "config-reference.md", "install.md"]
        );
    }

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
        assert_eq!(unmirrored(&urls).len(), urls.len());
    }

    #[test]
    fn only_docs_urls_without_a_file_count_as_unmirrored() {
        let urls: Vec<String> = [
            "https://herdr.dev/",
            "https://herdr.dev/blog/a/b/",
            "https://herdr.dev/ja/docs/guides/install/",
            "https://herdr.dev/docs/",
            "https://herdr.dev/docs/install/",
            "https://herdr.dev/docs/guides/install/",
            "https://herdr.dev/docs/guides/install/",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            unmirrored(&urls),
            ["https://herdr.dev/docs/guides/install/"]
        );
        assert_eq!(doc_pages(&urls).len(), 2);
    }

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
