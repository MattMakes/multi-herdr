//! Reducing a Starlight docs page to plain content HTML.
//!
//! The docs site is Astro Starlight with Expressive Code, which wraps content in
//! presentation markup that a generic HTML-to-markdown pass turns into noise:
//!
//! * The page `<h1>` sits in a sibling panel, *outside* `.sl-markdown-content`,
//!   so a naive extraction loses every page title.
//! * Each heading carries an `<a class="sl-anchor-link">` holding an icon and a
//!   `<span class="sr-only">Section titled "..."</span>`, which would otherwise
//!   appear as stray link text after every heading.
//! * Code blocks are not `<pre><code>` with a language class. They are
//!   `div.expressive-code > figure.frame > pre[data-language] > code`, where the
//!   code is split into one `div.ec-line` per line and every token is a `<span>`
//!   with inline colours. Converting that directly loses the language, gains a
//!   blank line per token, and picks up the "Terminal window" caption and the
//!   copy-to-clipboard button.
//!
//! So the DOM is walked once and re-serialised into simple HTML that htmd can
//! convert faithfully.

use std::fmt::Write as _;

use anyhow::{bail, Result};
use ego_tree::NodeRef;
use scraper::{Html, Node, Selector};

/// The Starlight wrapper around page content.
const CONTENT_SELECTOR: &str = ".sl-markdown-content";

/// Presentation-only elements: no content of ours lives inside them.
const DROPPED_TAGS: &[&str] = &[
    "script", "style", "link", "noscript", "button", "svg", "template", "figcaption",
];

/// Classes marking screen-reader and navigation affordances rather than content.
const DROPPED_CLASSES: &[&str] = &["sr-only", "sl-anchor-link", "copy"];

/// Placeholder text swapped in for a table, and back out after conversion.
///
/// The converter has no table support and would flatten a table into a run of
/// loose cell text, so tables are rendered to markdown here and stitched back in
/// afterwards. The token is deliberately plain alphanumeric so no markdown
/// escaping can touch it.
const TABLE_TOKEN: &str = "HERDRDOCSYNCTABLE";

/// The placeholder standing in for table `index`.
pub fn table_placeholder(index: usize) -> String {
    format!("{TABLE_TOKEN}{index}X")
}

/// A page reduced to a title, content HTML, and its extracted tables.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanPage {
    pub title: Option<String>,
    pub content_html: String,
    /// Markdown for each table, indexed by the placeholder in `content_html`.
    pub tables: Vec<String>,
}

/// Extract the page title and simplified content HTML.
pub fn clean(html: &str) -> Result<CleanPage> {
    let document = Html::parse_document(html);

    let content_selector = Selector::parse(CONTENT_SELECTOR)
        .map_err(|e| anyhow::anyhow!("invalid selector {CONTENT_SELECTOR}: {e}"))?;
    let Some(content) = document.select(&content_selector).next() else {
        bail!(
            "no {CONTENT_SELECTOR} element in the page. The docs site layout may have \
             changed; the extraction selectors in clean.rs need updating."
        );
    };

    let mut content_html = String::new();
    let mut tables = Vec::new();
    for child in content.children() {
        write_node(child, &mut content_html, &mut tables);
    }

    Ok(CleanPage {
        title: page_title(&document),
        content_html,
        tables,
    })
}

/// The page `<h1>`, which Starlight renders outside the content element.
fn page_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("h1").ok()?;
    let heading = document.select(&selector).next()?;
    let text: String = heading.text().collect::<String>().trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// Should this element be dropped wholesale?
fn is_dropped(element: &scraper::node::Element) -> bool {
    if DROPPED_TAGS.contains(&element.name()) {
        return true;
    }
    element
        .attr("class")
        .is_some_and(|classes| classes.split_whitespace().any(|c| DROPPED_CLASSES.contains(&c)))
}

fn has_class(element: &scraper::node::Element, class: &str) -> bool {
    element
        .attr("class")
        .is_some_and(|classes| classes.split_whitespace().any(|c| c == class))
}

/// Serialise one node into simplified HTML, collecting tables as markdown.
fn write_node(node: NodeRef<'_, Node>, out: &mut String, tables: &mut Vec<String>) {
    match node.value() {
        Node::Text(text) => escape_into(text, out),
        Node::Element(element) => {
            if is_dropped(element) {
                return;
            }
            if has_class(element, "expressive-code") {
                write_code_block(node, out);
                return;
            }
            if element.name() == "table" {
                let _ = write!(out, "<p>{}</p>", table_placeholder(tables.len()));
                tables.push(render_table(node));
                return;
            }
            write_element(node, element, out, tables);
        }
        // Comments, doctype and processing instructions carry nothing.
        _ => {}
    }
}

/// Render a `<table>` as a GitHub-flavoured pipe table.
///
/// The first row is the header, whether it uses `th` or `td`. The docs use no
/// `rowspan` or `colspan`, so a plain grid is enough.
fn render_table(node: NodeRef<'_, Node>) -> String {
    let mut rows: Vec<Vec<String>> = Vec::new();
    collect_rows(node, &mut rows);
    rows.retain(|row| !row.is_empty());
    if rows.is_empty() {
        return String::new();
    }

    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let render_row = |cells: &Vec<String>| {
        let mut padded: Vec<&str> = cells.iter().map(String::as_str).collect();
        padded.resize(columns, "");
        format!("| {} |", padded.join(" | "))
    };

    let mut out = String::new();
    out.push_str(&render_row(&rows[0]));
    out.push('\n');
    let _ = write!(out, "| {} |", vec!["---"; columns].join(" | "));
    for row in &rows[1..] {
        out.push('\n');
        out.push_str(&render_row(row));
    }
    out
}

/// Gather one string per cell, one vector per `<tr>`.
fn collect_rows(node: NodeRef<'_, Node>, rows: &mut Vec<Vec<String>>) {
    for child in node.children() {
        let Some(element) = child.value().as_element() else {
            continue;
        };
        match element.name() {
            "tr" => {
                let cells: Vec<String> = child
                    .children()
                    .filter(|c| {
                        c.value()
                            .as_element()
                            .is_some_and(|e| matches!(e.name(), "td" | "th"))
                    })
                    .map(|c| escape_cell(&inline_markdown(c)))
                    .collect();
                rows.push(cells);
            }
            // thead, tbody, tfoot
            _ => collect_rows(child, rows),
        }
    }
}

/// A cell must stay on one line and must not introduce column breaks.
fn escape_cell(text: &str) -> String {
    text.replace('|', r"\|")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert a cell's inline content to markdown.
///
/// Only the inline elements the docs actually use in tables are handled; anything
/// else contributes its text. Surveyed across every docs page, tables use `code`
/// and nothing more, so the rest is cheap insurance.
fn inline_markdown(node: NodeRef<'_, Node>) -> String {
    let mut out = String::new();
    for child in node.children() {
        match child.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(element) => match element.name() {
                "code" | "kbd" | "samp" => {
                    let _ = write!(out, "`{}`", text_of(child));
                }
                "strong" | "b" => {
                    let _ = write!(out, "**{}**", inline_markdown(child));
                }
                "em" | "i" => {
                    let _ = write!(out, "*{}*", inline_markdown(child));
                }
                "a" => {
                    let label = inline_markdown(child);
                    match element.attr("href") {
                        Some(href) => {
                            let _ = write!(out, "[{label}]({href})");
                        }
                        None => out.push_str(&label),
                    }
                }
                "br" => out.push(' '),
                _ if is_dropped(element) => {}
                _ => out.push_str(&inline_markdown(child)),
            },
            _ => {}
        }
    }
    out
}

/// Attributes worth keeping. Everything else is styling or framework bookkeeping
/// that the markdown conversion ignores anyway.
const KEPT_ATTRS: &[&str] = &[
    "href", "src", "alt", "title", "colspan", "rowspan", "start", "align", "id",
];

fn write_element(
    node: NodeRef<'_, Node>,
    element: &scraper::node::Element,
    out: &mut String,
    tables: &mut Vec<String>,
) {
    let tag = element.name();
    let _ = write!(out, "<{tag}");
    for (name, value) in element.attrs() {
        if KEPT_ATTRS.contains(&name) {
            let mut escaped = String::new();
            escape_into(value, &mut escaped);
            let _ = write!(out, " {name}=\"{escaped}\"");
        }
    }
    out.push('>');
    for child in node.children() {
        write_node(child, out, tables);
    }
    let _ = write!(out, "</{tag}>");
}

/// Rebuild an Expressive Code block as a plain fenced code block.
///
/// The language comes from `pre[data-language]`, and the source from one
/// `div.ec-line` per line - which is why the token `<span>`s never reach the
/// converter.
fn write_code_block(node: NodeRef<'_, Node>, out: &mut String) {
    let Some(pre) = find_descendant(node, |e| e.name() == "pre") else {
        return;
    };
    let language = pre
        .value()
        .as_element()
        .and_then(|e| e.attr("data-language"))
        .filter(|l| !l.is_empty() && *l != "plaintext");

    let mut lines: Vec<String> = Vec::new();
    collect_code_lines(pre, &mut lines);
    // A block with no `.ec-line` markers (plain `<pre>`) still has its text.
    let code = if lines.is_empty() {
        text_of(pre)
    } else {
        lines.join("\n")
    };
    let code = code.trim_end_matches('\n');

    out.push_str("<pre><code");
    if let Some(language) = language {
        let mut escaped = String::new();
        escape_into(language, &mut escaped);
        let _ = write!(out, " class=\"language-{escaped}\"");
    }
    out.push('>');
    escape_into(code, out);
    out.push_str("</code></pre>");
}

/// Gather one string per `div.ec-line`, in document order.
fn collect_code_lines(node: NodeRef<'_, Node>, lines: &mut Vec<String>) {
    for child in node.children() {
        if let Some(element) = child.value().as_element() {
            if has_class(element, "ec-line") {
                lines.push(text_of(child));
                continue;
            }
        }
        collect_code_lines(child, lines);
    }
}

fn find_descendant<'a>(
    node: NodeRef<'a, Node>,
    predicate: impl Fn(&scraper::node::Element) -> bool + Copy,
) -> Option<NodeRef<'a, Node>> {
    for child in node.children() {
        if child.value().as_element().is_some_and(predicate) {
            return Some(child);
        }
        if let Some(found) = find_descendant(child, predicate) {
            return Some(found);
        }
    }
    None
}

/// All text under a node, concatenated.
fn text_of(node: NodeRef<'_, Node>) -> String {
    let mut out = String::new();
    fn walk(node: NodeRef<'_, Node>, out: &mut String) {
        match node.value() {
            Node::Text(text) => out.push_str(text),
            Node::Element(_) => {
                for child in node.children() {
                    walk(child, out);
                }
            }
            _ => {}
        }
    }
    walk(node, &mut out);
    out
}

fn escape_into(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real Starlight page skeleton: h1 in its own panel, content in another.
    fn starlight_page(body: &str) -> String {
        format!(
            r#"<!DOCTYPE html><html><head><title>t</title>
               <style>.x{{color:red}}</style></head><body>
               <header><nav><a href="/docs/">Docs nav</a></nav></header>
               <div class="main-pane"><main>
                 <div class="content-panel"><div class="sl-container">
                   <h1 id="_top" class="astro-j6tvhyss">Install Herdr</h1>
                 </div></div>
                 <div class="content-panel"><div class="sl-container">
                   <div class="sl-markdown-content">{body}</div>
                 </div></div>
               </main></div>
               <footer>Footer</footer>
               <script>console.log(1)</script></body></html>"#
        )
    }

    #[test]
    fn recovers_the_page_title_from_outside_the_content_element() {
        let page = clean(&starlight_page("<p>prose</p>")).unwrap();
        assert_eq!(page.title.as_deref(), Some("Install Herdr"));
        assert!(page.content_html.contains("prose"));
    }

    /// The heading anchor is an icon plus a screen-reader label; neither is content.
    #[test]
    fn strips_heading_anchor_links() {
        let page = clean(&starlight_page(
            r##"<div class="heading-wrapper"><h2 id="install">Install</h2>
               <a class="sl-anchor-link" href="#install">
                 <span class="sl-anchor-icon"><svg><path d="M0 0"></path></svg></span>
                 <span class="sr-only">Section titled &#8220;Install&#8221;</span>
               </a></div>"##,
        ))
        .unwrap();
        assert!(page.content_html.contains("Install"));
        assert!(!page.content_html.contains("Section titled"), "{}", page.content_html);
        assert!(!page.content_html.contains("svg"), "{}", page.content_html);
        assert!(!page.content_html.contains("sl-anchor"), "{}", page.content_html);
    }

    /// An Expressive Code block must come out as one fenced block with its
    /// language, no per-token spans, no caption and no copy button.
    #[test]
    fn rebuilds_expressive_code_blocks() {
        let page = clean(&starlight_page(
            r#"<div class="expressive-code">
                 <link rel="stylesheet" href="/_astro/ec.css">
                 <script type="module" src="/_astro/ec.js"></script>
                 <figure class="frame is-terminal not-content">
                   <figcaption class="header"><span class="title"></span>
                     <span class="sr-only">Terminal window</span></figcaption>
                   <pre data-language="bash"><code><div class="ec-line"><div class="code"><span style="--0:#82AAFF">curl</span><span style="--0:#D6DEEB"> </span><span style="--0:#82AAFF">-fsSL</span><span> https://herdr.dev/install.sh</span></div></div><div class="ec-line"><div class="code"><span>echo done</span></div></div></code></pre>
                   <div class="copy"><button data-code="curl">x</button></div>
                 </figure>
               </div>"#,
        ))
        .unwrap();

        let html = &page.content_html;
        assert!(html.contains(r#"<pre><code class="language-bash">"#), "{html}");
        assert!(html.contains("curl -fsSL https://herdr.dev/install.sh"), "{html}");
        assert!(html.contains("echo done"), "{html}");
        assert!(!html.contains("Terminal window"), "{html}");
        assert!(!html.contains("button"), "{html}");
        assert!(!html.contains("stylesheet"), "{html}");
        assert!(!html.contains("span"), "{html}");
    }

    /// Two lines must stay two lines, not collapse or gain blanks per token.
    #[test]
    fn code_lines_are_preserved_exactly() {
        let page = clean(&starlight_page(
            r#"<div class="expressive-code"><figure><pre data-language="toml"><code><div class="ec-line"><div class="code"><span>  [ui]</span></div></div><div class="ec-line"><div class="code"></div></div><div class="ec-line"><div class="code"><span>tab_bar_position = </span><span>"bottom"</span></div></div></code></pre></figure></div>"#,
        ))
        .unwrap();
        let code = &page.content_html;
        // Leading indentation is code, so it must survive verbatim; the empty
        // `.ec-line` must survive as a blank line.
        assert!(
            code.contains("  [ui]\n\ntab_bar_position = &quot;bottom&quot;"),
            "{code}"
        );
    }

    /// A plain `<pre>` with no Expressive Code wrapper still works.
    #[test]
    fn plain_pre_blocks_pass_through() {
        let page = clean(&starlight_page("<pre><code>plain text\nsecond</code></pre>")).unwrap();
        assert!(page.content_html.contains("plain text\nsecond"), "{}", page.content_html);
    }

    #[test]
    fn drops_page_chrome_outside_the_content_element() {
        let page = clean(&starlight_page("<p>only this</p>")).unwrap();
        for noise in ["Docs nav", "Footer", "console.log", "color:red"] {
            assert!(!page.content_html.contains(noise), "{noise} in {}", page.content_html);
        }
    }

    #[test]
    fn keeps_link_and_image_targets() {
        let page = clean(&starlight_page(
            r#"<p><a href="/docs/quick-start/">Quick start</a>
               <img src="/_astro/logo.svg" alt="Herdr"></p>"#,
        ))
        .unwrap();
        assert!(page.content_html.contains(r#"href="/docs/quick-start/""#));
        assert!(page.content_html.contains(r#"src="/_astro/logo.svg""#));
        assert!(page.content_html.contains(r#"alt="Herdr""#));
    }

    #[test]
    fn escapes_text_that_would_otherwise_be_markup() {
        let page = clean(&starlight_page("<p>use &lt;pane-id&gt; &amp; go</p>")).unwrap();
        assert!(page.content_html.contains("&lt;pane-id&gt; &amp; go"), "{}", page.content_html);
    }

    #[test]
    fn a_missing_content_element_is_an_error_that_says_why() {
        let err = clean("<html><body><main><p>redesigned</p></main></body></html>")
            .unwrap_err()
            .to_string();
        assert!(err.contains("sl-markdown-content"), "{err}");
        assert!(err.contains("clean.rs"), "{err}");
    }

    #[test]
    fn a_page_without_an_h1_still_converts() {
        let html = r#"<html><body><div class="sl-markdown-content"><p>x</p></div></body></html>"#;
        let page = clean(html).unwrap();
        assert_eq!(page.title, None);
        assert!(page.content_html.contains('x'));
    }
}

#[cfg(test)]
mod table_tests {
    use super::*;

    fn table_page(table: &str) -> String {
        format!(
            r#"<html><body><h1>T</h1>
               <div class="sl-markdown-content">{table}</div></body></html>"#
        )
    }

    /// The shape every docs table has: a thead, a tbody, and `<code>` in cells.
    #[test]
    fn renders_a_pipe_table_with_inline_code() {
        let page = clean(&table_page(
            r#"<table><thead><tr><th>Source</th><th>Meaning</th></tr></thead>
               <tbody>
                 <tr><td><code dir="auto">visible</code></td>
                     <td>Current rendered screen.</td></tr>
                 <tr><td><code dir="auto">recent-unwrapped</code></td>
                     <td>Recent scrollback. Best for logs.</td></tr>
               </tbody></table>"#,
        ))
        .unwrap();

        assert_eq!(page.tables.len(), 1);
        assert_eq!(
            page.tables[0],
            "| Source | Meaning |\n\
             | --- | --- |\n\
             | `visible` | Current rendered screen. |\n\
             | `recent-unwrapped` | Recent scrollback. Best for logs. |"
        );
        // The content HTML carries only the placeholder.
        assert!(page.content_html.contains(&table_placeholder(0)));
        assert!(!page.content_html.contains("<table"));
    }

    #[test]
    fn several_tables_get_distinct_placeholders() {
        let page = clean(&table_page(
            "<table><tr><td>a</td></tr></table><p>between</p><table><tr><td>b</td></tr></table>",
        ))
        .unwrap();
        assert_eq!(page.tables.len(), 2);
        assert!(page.tables[0].contains('a'));
        assert!(page.tables[1].contains('b'));
        assert!(page.content_html.contains(&table_placeholder(0)));
        assert!(page.content_html.contains(&table_placeholder(1)));
        assert_ne!(table_placeholder(0), table_placeholder(1));
    }

    /// A table with no thead still needs a header row for valid markdown.
    #[test]
    fn a_table_without_a_head_uses_its_first_row() {
        let page = clean(&table_page(
            "<table><tr><td>Key</td><td>Value</td></tr><tr><td>a</td><td>1</td></tr></table>",
        ))
        .unwrap();
        assert_eq!(
            page.tables[0],
            "| Key | Value |\n| --- | --- |\n| a | 1 |"
        );
    }

    /// A literal pipe in a cell would otherwise create a phantom column.
    #[test]
    fn pipes_in_cells_are_escaped() {
        let page = clean(&table_page(
            "<table><tr><th>Cmd</th></tr><tr><td><code>a | b</code></td></tr></table>",
        ))
        .unwrap();
        assert!(page.tables[0].contains(r"`a \| b`"), "{}", page.tables[0]);
    }

    /// Cells must stay on one line, or the table breaks apart.
    #[test]
    fn multiline_cell_content_is_flattened() {
        let page = clean(&table_page(
            "<table><tr><th>H</th></tr><tr><td>first\n   second\n\nthird</td></tr></table>",
        ))
        .unwrap();
        assert!(page.tables[0].contains("| first second third |"), "{}", page.tables[0]);
    }

    #[test]
    fn ragged_rows_are_padded_to_the_widest() {
        let page = clean(&table_page(
            "<table><tr><th>a</th><th>b</th><th>c</th></tr><tr><td>1</td></tr></table>",
        ))
        .unwrap();
        assert_eq!(
            page.tables[0],
            "| a | b | c |\n| --- | --- | --- |\n| 1 |  |  |"
        );
    }

    #[test]
    fn cell_links_and_emphasis_convert() {
        let page = clean(&table_page(
            r#"<table><tr><th>H</th></tr>
               <tr><td><a href="/docs/x/">See x</a> and <strong>bold</strong> and <em>it</em></td></tr>
               </table>"#,
        ))
        .unwrap();
        assert!(page.tables[0].contains("[See x](/docs/x/)"), "{}", page.tables[0]);
        assert!(page.tables[0].contains("**bold**"), "{}", page.tables[0]);
        assert!(page.tables[0].contains("*it*"), "{}", page.tables[0]);
    }
}
