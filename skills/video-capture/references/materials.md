# Brand and material capture

`<skill-dir>/scripts/materials.mjs` reads 1 product page in headless Chrome
and saves what the film needs to rebuild its interface: colours, fonts,
logo files, screenshots and wording. Rebuilt interfaces use these captures
at the same size. Do not invent a colour, a font, a logo or a line of copy
that the capture does not hold.

## Command

Run it from the video project folder:

```sh
node "<skill-dir>/scripts/materials.mjs" . <url>
node "<skill-dir>/scripts/materials.mjs" . <url> --allow-network
node "<skill-dir>/scripts/materials.mjs" --check .
```

- 1 run reads 1 page and replaces `materials/` completely. To keep 2 pages,
  run the second capture in another project folder.
- The viewport is `meta.width` x `meta.height` of `timeline.json`, or
  1920x1080 when there is no timeline. The scale is 1, so 1 image pixel is
  1 CSS pixel. Measure sizes straight from the screenshots.
- The run prints 1 JSON line: `files`, `blocked`, `not_saved` and `fails`.
  It exits 0 when `--check` finds no fail, 1 when it does, and 2 on a usage
  error (no URL, an external URL without the flag, no Chrome).

## Network rule

- A local URL needs no flag: `file:`, `data:`, `localhost`, a
  `*.localhost` host, `127.0.0.1` and `[::1]`.
- Any other URL needs `--allow-network`. Pass it only when the operator
  says in the task that you may read that site. Without the flag, the
  script refuses before Chrome starts.
- Without the flag, every request to another host fails in the page and
  goes into the "Blocked requests" list of `materials.md`. A local page
  that loads a CDN font or image therefore shows what it would need.
- With the flag, the page loads normally. Nothing is sent anywhere except
  the requests the page makes itself.

## Output: `materials/`

| File | Holds |
|---|---|
| `first-screen.png` | The first screen at the viewport size. |
| `full-page.png` | The whole page at the viewport width, cut at 16384 px. |
| `colors.json` | Up to 24 colours as `#rrggbb`, sorted by painted area (`weight`, CSS px squared), each with its `roles` (`background`, `text`, `border`). Also `css_variables`: the custom properties set on `:root` or `html`. |
| `fonts.json` | `families`: each computed `font-family` with its weights, sizes and character count, most used first. `faces`: the loaded `FontFace` entries. `files`: the URLs of font files the page fetched. |
| `wording.md` | The title, the meta description, `og:title`, then the visible text of headings, paragraphs, list items, links, buttons, labels and table cells in page order, each line once. |
| `logo-<n>.<ext>` | Images and inline SVG inside an element whose class, id, alt, aria-label or src holds "logo" or "brand", and the page icons. |
| `materials.md` | The page URL, the viewport and the network mode, then 1 table row per file: file, source URL, what it is. Then the blocked requests and the logos it could not save. |

The source URL of a screenshot, `colors.json`, `fonts.json`, `wording.md`
and an inline SVG or `data:` logo is the page URL. The source URL of a
logo image or icon file is the URL of that file.

Font files are not saved. The fleet uses local fonts only. Use
`fonts.json` to pick the same family from the fonts on the machine, or
tell the orchestrator which font file the operator must supply.

## Check

`--check` passes when `materials/materials.md` exists, every other file in
`materials/` has a row, every row names a file that exists, and every row
has an `http`, `https` or `file` source URL. It prints
`{"fails": [...]}`. The capture runs the same check at its end.

## Gotchas

- Lazy images that load on scroll can be empty in `full-page.png`. Capture
  the section you need with a take instead.
- Cookie banners and sign-in walls are part of the page. Capture a local
  build or a page without them.
- A logo drawn as CSS background or text is not found. Crop it from
  `first-screen.png` and list the crop in `materials.md` with the page URL.
- Self-test (about 4 s, needs Chrome, no network):
  `node "<skill-dir>/scripts/materials.mjs" --self-test`.
