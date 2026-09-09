//! Turning a docs page into markdown.
//!
//! Two steps: [`crate::clean`] reduces the Starlight page to plain content HTML,
//! then htmd converts it. The split keeps all the site-specific knowledge in one
//! place and leaves this module to formatting concerns.

use anyhow::{Context, Result};
use htmd::options::{BulletListMarker, Options};
use htmd::HtmlToMarkdown;

use crate::clean;

/// Convert a docs page's HTML into markdown.
pub fn to_markdown(html: &str) -> Result<String> {
    let page = clean::clean(html)?;
    let converter = HtmlToMarkdown::builder()
        .options(Options {
            // The existing mirror uses `-` bullets; keep diffs to real changes.
            bullet_list_marker: BulletListMarker::Dash,
            ..Options::default()
        })
        .build();

    let mut body = converter
        .convert(&page.content_html)
        .context("converting HTML to markdown")?;

    // Put the tables back where their placeholders sit. The converter has no table
    // support, so `clean` rendered them and left a marker behind.
    for (index, table) in page.tables.iter().enumerate() {
        body = body.replace(&clean::table_placeholder(index), table);
    }

    let mut markdown = String::new();
    if let Some(title) = &page.title {
        // Starlight renders the page title outside the content element, so it has
        // to be put back or every file would start mid-prose.
        markdown.push_str(&format!("# {title}\n\n"));
    }
    markdown.push_str(&body);
    Ok(normalize(&markdown))
}

/// Tidy the converter's output: one trailing newline, no trailing spaces, and at
/// most one blank line in a row.
///
/// Without this, incidental whitespace churn would show up as a diff on every sync
/// and hide the real content changes. Blank lines inside fenced code blocks are
/// left alone, because there they are content.
fn normalize(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len() + 1);
    let mut blank_run = 0;
    let mut in_fence = false;

    for line in markdown.lines() {
        let trimmed = line.trim_end();
        if trimmed.trim_start().starts_with("```") {
            in_fence = !in_fence;
            blank_run = 0;
            out.push_str(trimmed);
            out.push('\n');
            continue;
        }
        if in_fence {
            out.push_str(line.trim_end());
            out.push('\n');
            continue;
        }
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank_run = 0;
        }
        out.push_str(trimmed);
        out.push('\n');
    }

    while out.ends_with("\n\n") {
        out.pop();
    }
    if out.is_empty() {
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(body: &str) -> String {
        format!(
            r#"<!DOCTYPE html><html><head><title>Docs</title>
               <style>.x{{color:red}}</style></head><body>
               <header><nav>Nav link</nav></header>
               <main>
                 <div class="content-panel"><h1>Install Herdr</h1></div>
                 <div class="sl-markdown-content">{body}</div>
               </main>
               <footer>Footer text</footer></body></html>"#
        )
    }

    #[test]
    fn output_starts_with_the_page_title_as_an_h1() {
        let md = to_markdown(&page("<p>Some prose.</p>")).unwrap();
        assert!(md.starts_with("# Install Herdr\n\n"), "{md}");
        assert!(md.contains("Some prose."));
        assert!(!md.contains("Nav link"), "{md}");
        assert!(!md.contains("Footer text"), "{md}");
    }

    /// The whole point of the cleaning step: a real Starlight code block becomes
    /// one fenced block that names its language.
    #[test]
    fn code_blocks_keep_their_language_and_shape() {
        let md = to_markdown(&page(
            r#"<div class="expressive-code"><figure class="frame">
                 <figcaption><span class="sr-only">Terminal window</span></figcaption>
                 <pre data-language="bash"><code><div class="ec-line"><div class="code"><span>brew</span><span> install herdr</span></div></div></code></pre>
                 <div class="copy"><button data-code="brew install herdr">c</button></div>
               </figure></div>"#,
        ))
        .unwrap();

        assert!(md.contains("```bash"), "{md}");
        assert!(md.contains("brew install herdr"), "{md}");
        assert!(!md.contains("Terminal window"), "{md}");
        assert_eq!(md.matches("```").count(), 2, "exactly one fenced block:\n{md}");
    }

    #[test]
    fn heading_anchors_do_not_leak_into_the_text() {
        let md = to_markdown(&page(
            r##"<div class="heading-wrapper"><h2 id="install">Install</h2>
               <a class="sl-anchor-link" href="#install">
                 <span class="sr-only">Section titled &#8220;Install&#8221;</span></a></div>"##,
        ))
        .unwrap();
        assert!(md.contains("## Install"), "{md}");
        assert!(!md.contains("Section titled"), "{md}");
        assert!(!md.contains("(#install)"), "{md}");
    }

    #[test]
    fn converts_links_and_inline_code() {
        let md = to_markdown(&page(
            r#"<p>See <a href="/docs/quick-start/">Quick start</a> and
               <code>herdr pane split</code>.</p>"#,
        ))
        .unwrap();
        assert!(md.contains("[Quick start](/docs/quick-start/)"), "{md}");
        assert!(md.contains("`herdr pane split`"), "{md}");
    }

    /// The existing mirror uses `-` bullets, so keep that marker. The converter
    /// aligns list content to 4 columns, which is fine - only the marker matters.
    #[test]
    fn uses_dash_bullets() {
        let md = to_markdown(&page("<ul><li>one</li><li>two</li></ul>")).unwrap();
        let markers: Vec<char> = md
            .lines()
            .skip_while(|l| !l.trim_start().starts_with(['-', '*']))
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.trim_start().chars().next().unwrap())
            .collect();
        assert_eq!(markers, vec!['-', '-'], "{md}");
    }

    #[test]
    fn preserves_tables() {
        let md = to_markdown(&page(
            r#"<table><thead><tr><th>Flag</th><th>Meaning</th></tr></thead>
               <tbody><tr><td><code>--no-focus</code></td><td>Do not focus</td></tr></tbody></table>"#,
        ))
        .unwrap();
        assert!(md.contains("--no-focus"), "{md}");
        assert!(md.contains("Meaning"), "{md}");
    }

    #[test]
    fn output_ends_with_exactly_one_newline() {
        let md = to_markdown(&page("<p>a</p>")).unwrap();
        assert!(md.ends_with('\n'));
        assert!(!md.ends_with("\n\n"));
    }

    #[test]
    fn normalize_collapses_blank_runs_and_trailing_space() {
        assert_eq!(normalize("a   \n\n\n\nb\n\n\n"), "a\n\nb\n");
        assert_eq!(normalize("\n\n\na\n"), "a\n");
        assert_eq!(normalize(""), "\n");
        assert_eq!(normalize("   \n"), "\n");
    }

    /// A blank line inside a code block is content, so it must survive the
    /// blank-run collapsing that applies to prose.
    #[test]
    fn normalize_leaves_blank_lines_inside_code_fences() {
        let input = "text\n\n```toml\n[ui]\n\n\nkey = 1\n```\n\n\nafter\n";
        assert_eq!(
            normalize(input),
            "text\n\n```toml\n[ui]\n\n\nkey = 1\n```\n\nafter\n"
        );
    }
}
