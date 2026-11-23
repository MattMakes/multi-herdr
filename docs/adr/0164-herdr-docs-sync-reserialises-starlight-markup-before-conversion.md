# herdr-docs-sync re-serialises Starlight markup before converting it

The herdr.dev docs site is Astro Starlight with Expressive Code, which wraps
content in presentation markup that a generic HTML-to-markdown pass turns
into noise. `crates/herdr-docs-sync/src/clean.rs` walks the DOM once and
re-serialises it into simple HTML that htmd can convert faithfully.

## Consequences

- The page `<h1>` sits in a sibling panel, outside `.sl-markdown-content`.
  A naive extraction loses every page title.
- Each heading carries an `<a class="sl-anchor-link">` holding an icon and a
  `<span class="sr-only">Section titled "..."</span>`, which would
  otherwise appear as stray link text after every heading.
- Code blocks are not `<pre><code>` with a language class. They are
  `div.expressive-code > figure.frame > pre[data-language] > code`, with
  the code split into 1 `div.ec-line` per line and every token a `<span>`
  with inline colours. Converting that directly loses the language, gains a
  blank line per token, and picks up the "Terminal window" caption and the
  copy-to-clipboard button.
- htmd has no table support and flattens a table into a run of loose cell
  text. `clean.rs` renders tables to markdown itself and swaps in a
  placeholder, `TABLE_TOKEN` (`HERDRDOCSYNCTABLE`) plus the table index plus
  `X`, which is stitched back after conversion. The token is plain
  alphanumeric so no markdown escaping can alter it.
