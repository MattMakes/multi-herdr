# herdr-docs-sync lists pages from the sitemap and prunes only from a complete run

`crates/herdr-docs-sync` discovers the documentation pages from herdr.dev's
`sitemap.xml` (`crates/herdr-docs-sync/src/sitemap.rs`) instead of crawling
links. The sitemap is the authoritative page list. Pages appear and
disappear between releases, and a crawl would also have to decide what
counts as documentation. Because pages disappear, the sync prunes local
files that the site no longer lists, and pruning deletes local
documentation. So it acts only on a complete and trustworthy picture of the
site.

## Consequences

- `sitemap.xml` is a `<sitemapindex>` pointing at `<sitemap><loc>`
  children. Those are `<urlset>`s of `<url><loc>`.
- `sitemap::parse` counts only `<loc>` element text. Each entry also carries
  `xhtml:link href="..."` alternates for the translated sites. Scanning
  attributes pulls in every `/ja/` and `/zh-cn/` page plus malformed
  fragments such as `https://herdr.dev/docs/"/>`.
- `prune_stale` runs only when every page was fetched and converted in this
  run (`crates/herdr-docs-sync/src/main.rs`). If any page failed, the run
  does not know what the site really contains, and deleting on that basis
  could destroy good docs.
- A URL under `https://herdr.dev/docs/` that maps to no local file, such as
  a nested page `docs/guides/install/`, is printed as not mirrored and also
  blocks `prune_stale`, and so does a nested sitemap index. The mirror is
  flat, so a site that nests its docs lists fewer pages than it has, and
  pruning from that list would delete every page that moved.
- `SitemapScan::finish` rejects XML that ends with elements still open. A
  truncated download parses as valid-so-far XML and yields a short page
  list, which the prune step would act on by deleting good local files.
