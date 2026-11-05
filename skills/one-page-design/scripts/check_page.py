#!/usr/bin/env python3
"""Static checks for a one-page design HTML file.

Reads the page as text and never renders it, so it cannot see clipping or
overlap: look at a preview image for those. Standard library only.

As a module:  findings, size, callouts = check_page(html_text)
As a command: python3 check_page.py page.html [--min-pt 6] [--max-in 44]
              [--preview out.png]
Exit status: 0 no findings, 1 at least one finding, 2 unreadable file or
bad arguments.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
from html.parser import HTMLParser
from pathlib import Path

# Markup names shared with assets/starter.html and references/drawing-parts.md.
SHEET_CLASS = "sheet"
CALLOUT_CLASS = "callout"
WIDTH_PROP = "--sheet-w"
HEIGHT_PROP = "--sheet-h"

MAX_CALLOUTS = 8
UNITS_PER_INCH = 100.0
PREVIEW_DPI = 96
DEFAULT_SIZE = (17.0, 11.0)
TO_INCHES = {"in": 1.0, "cm": 1 / 2.54, "mm": 1 / 25.4}
VOID_TAGS = {
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link",
    "meta", "param", "source", "track", "wbr",
}
EXTERNAL_TAGS = {"link", "script", "img", "iframe"}
CHROME_NAMES = (
    "google-chrome", "google-chrome-stable", "chromium", "chromium-browser",
    "chrome",
)
MAC_CHROME = (
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
)

NUM = r"(\d*\.?\d+)"
SIZE_VALUE = re.compile(r"^\s*" + NUM + r"\s*(in|cm|mm)\s*$", re.I)
FONT_SIZE_DECL = re.compile(r"(?<![\w-])font-size\s*:\s*([^;}\"]+)", re.I)
FONT_SHORTHAND_DECL = re.compile(r"(?<![\w-])font\s*:\s*([^;}\"]+)", re.I)
UNIT_SIZE = re.compile(r"(?<![\w.])" + NUM + r"(pt|px|em|rem|%)(?![\w])", re.I)
UNITLESS = re.compile(r"^\s*" + NUM + r"\s*$")
CSS_RULE = re.compile(r"([^{}]*)\{([^{}]*)\}")
CSS_COMMENT = re.compile(r"/\*.*?\*/", re.S)
ABSOLUTE_WIDTH = re.compile(r"(?<![\w-])width\s*:\s*" + NUM + r"\s*(px|pt|in|cm|mm|pc)", re.I)


class Node:
    def __init__(self, tag, attrs, parent, svg_root):
        self.tag = tag
        self.attrs = attrs
        self.parent = parent
        self.svg_root = svg_root  # outermost <svg> this node sits in, or None

    def classes(self):
        return (self.attrs.get("class") or "").split()


class PageParser(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.nodes = []
        self.stack = []
        self.styles = []
        self._in_style = False

    def _add(self, tag, attrs):
        parent = self.stack[-1] if self.stack else None
        svg_root = parent.svg_root if parent is not None else None
        node = Node(tag, {k: (v if v is not None else "") for k, v in attrs},
                    parent, svg_root)
        if tag == "svg" and svg_root is None:
            node.svg_root = node
        self.nodes.append(node)
        return node

    def handle_starttag(self, tag, attrs):
        node = self._add(tag, attrs)
        if tag == "style":
            self._in_style = True
            self.styles.append("")
        if tag not in VOID_TAGS:
            self.stack.append(node)

    def handle_startendtag(self, tag, attrs):
        self._add(tag, attrs)

    def handle_endtag(self, tag):
        if tag == "style":
            self._in_style = False
        for i in range(len(self.stack) - 1, -1, -1):
            if self.stack[i].tag == tag:
                del self.stack[i:]
                break

    def handle_data(self, data):
        if self._in_style:
            self.styles[-1] += data


def _inches(value):
    m = SIZE_VALUE.match(value)
    if not m:
        return None
    return float(m.group(1)) * TO_INCHES[m.group(2).lower()]


def _declarations(block):
    out = {}
    for part in block.split(";"):
        if ":" in part:
            name, value = part.split(":", 1)
            out[name.strip().lower()] = value.strip()
    return out


def _size_from(decls):
    w = _inches(decls.get(WIDTH_PROP, ""))
    h = _inches(decls.get(HEIGHT_PROP, ""))
    if w is None or h is None:
        return None
    return (w, h)


def _fmt(n):
    return f"{n:g}"


def _fmt_size(size):
    return f"{_fmt(round(size[0], 2))} x {_fmt(round(size[1], 2))} in"


def _font_findings(text, where, min_pt, unitless_scale=None):
    """Findings for every font-size (and font shorthand size) in a CSS text."""
    findings = []
    values = []
    for m in FONT_SIZE_DECL.finditer(text):
        values.append(m.group(1).strip())
    for m in FONT_SHORTHAND_DECL.finditer(text):
        u = UNIT_SIZE.search(m.group(1))
        if u:
            values.append(u.group(0))
    for value in values:
        findings.extend(_size_finding(value, where, min_pt, unitless_scale))
    return findings


def _size_finding(value, where, min_pt, unitless_scale=None, tag=None):
    u = UNIT_SIZE.match(value.strip())
    if u:
        number, unit = float(u.group(1)), u.group(2).lower()
        if unit in ("em", "rem", "%"):
            return [f"font-size {value.strip()} {where} cannot be verified for "
                    f"print; use pt or px."]
        pt = number * 0.75 if unit == "px" else number
        if pt < min_pt:
            return [f"Text size {u.group(0)} ({pt:.1f} pt) {where} is below "
                    f"the {_fmt(min_pt)} pt minimum."]
        return []
    if unitless_scale is not None:
        m = UNITLESS.match(value)
        if m:
            pt = float(m.group(1)) * 72 / unitless_scale
            if pt < min_pt:
                return [f"SVG text below the minimum: font-size "
                        f"{m.group(1)} is {pt:.1f} pt at "
                        f"{unitless_scale:.0f} units per inch on <{tag}>; "
                        f"the minimum is {_fmt(min_pt)} pt."]
    return []


def _is_external(url):
    u = url.strip().lower()
    return u.startswith(("http://", "https://", "//"))


def _layer_candidate(node):
    if node.tag != "svg" or node.svg_root is not node:
        return None
    nums = re.split(r"[\s,]+", (node.attrs.get("viewbox") or "").strip())
    if len(nums) != 4:
        return None
    try:
        box = [float(n) for n in nums]
    except ValueError:
        return None
    width = node.attrs.get("width")
    if width is not None and width.strip() != "100%":
        return None
    if ABSOLUTE_WIDTH.search(node.attrs.get("style") or ""):
        return None
    return (box[2], box[3])


def check_page(html, min_pt=6.0, max_in=44.0):
    """Return (findings, size, callouts).

    findings: list of sentences, empty when the page passes.
    size: (width, height) in inches, or None when it cannot be known.
    callouts: number of elements with the callout class.
    """
    parser = PageParser()
    parser.feed(html)
    parser.close()
    nodes = parser.nodes
    css = CSS_COMMENT.sub("", "\n".join(parser.styles))
    findings = []

    # Sheet size: root rules, then the inline style of the sheet.
    sheet = next((n for n in nodes
                  if n.tag == "main" and SHEET_CLASS in n.classes()), None)
    root_size = None
    for selectors, block in CSS_RULE.findall(css):
        names = {s.strip().lower() for s in selectors.split(",")}
        if names & {":root", "html"}:
            found = _size_from(_declarations(block))
            if found:
                root_size = found
    size = None
    if sheet is None:
        findings.append(f'No sheet element: the page needs <main class="'
                        f'{SHEET_CLASS}"> as its one printable surface.')
    else:
        inline = _size_from(_declarations(sheet.attrs.get("style") or ""))
        if root_size is None and inline is None:
            findings.append(f"The sheet size is not declared: set {WIDTH_PROP} "
                            f"and {HEIGHT_PROP} in inches on :root.")
        elif root_size is None:
            findings.append(f"The sheet size is set only inline on the sheet "
                            f"element; move {WIDTH_PROP} and {HEIGHT_PROP} to "
                            f":root, because the print page size reads them "
                            f"from there.")
        else:
            size = root_size
            if inline is not None and any(abs(a - b) > 1e-6
                                          for a, b in zip(root_size, inline)):
                findings.append(f"Conflicting sheet sizes: :root says "
                                f"{_fmt_size(root_size)} but the sheet element "
                                f"says {_fmt_size(inline)}. Keep the :root one "
                                f"only.")

    # Hidden content and content that leads elsewhere.
    for n in nodes:
        style = re.sub(r"\s+", "", (n.attrs.get("style") or "")).lower()
        if n.tag == "a" and (n.attrs.get("href") or "").strip():
            findings.append(f"Link to {n.attrs['href'].strip()}: a printed "
                            f"sheet cannot follow links; put the content on "
                            f"the page.")
        if n.tag == "details":
            findings.append("A <details> element is a collapsible section "
                            "that hides content; show it all on the sheet.")
        if n.tag == "iframe":
            findings.append("An <iframe> pulls in another page; everything "
                            "must be on this one sheet.")
        if "hidden" in n.attrs:
            findings.append(f"A <{n.tag}> has the hidden attribute: hidden "
                            f"content never reaches the printed sheet.")
        if "display:none" in style:
            findings.append(f"A <{n.tag}> has an inline display: none: hidden "
                            f"content never reaches the printed sheet.")
        if "visibility:hidden" in style:
            findings.append(f"A <{n.tag}> has an inline visibility: hidden: "
                            f"hidden content never reaches the printed sheet.")
        if n.tag in EXTERNAL_TAGS:
            for attr in ("href", "src"):
                url = n.attrs.get(attr) or ""
                if _is_external(url):
                    findings.append(f"External resource <{n.tag}> {url.strip()}"
                                    f": the page must be one self-contained "
                                    f"file.")
    for _ in re.finditer(r":hover\b", css, re.I):
        findings.append("A :hover rule shows content only on hover, and paper "
                        "has no hover; show it all the time.")
    for _ in re.finditer(r"(?<![\w-])display\s*:\s*none\b", css, re.I):
        findings.append("A style block sets display: none: hidden content "
                        "never reaches the printed sheet.")

    # Drawing layers and their text scale.
    layers = {}
    direct = [n for n in nodes if _layer_candidate(n) and sheet is not None
              and n.parent is sheet]
    if not direct:
        first = next((n for n in nodes if _layer_candidate(n)), None)
        direct = [first] if first else []
    for layer in direct:
        box_w, box_h = _layer_candidate(layer)
        scale = UNITS_PER_INCH
        if size:
            ux, uy = box_w / size[0], box_h / size[1]
            scale = max(ux, uy)
            if any(abs(u - UNITS_PER_INCH) / UNITS_PER_INCH > 0.01
                   for u in (ux, uy)):
                findings.append(
                    f"Drawing layer view box is {_fmt(box_w)} x {_fmt(box_h)}"
                    f"; for a {_fmt_size(size)} sheet at 100 units per inch "
                    f"it should be {_fmt(round(size[0] * 100, 1))} x "
                    f"{_fmt(round(size[1] * 100, 1))}.")
        layers[id(layer)] = scale

    # Text sizes.
    findings.extend(_font_findings(css, "in a style block", min_pt))
    for n in nodes:
        in_svg = n.svg_root is not None
        scale = layers.get(id(n.svg_root), UNITS_PER_INCH) if in_svg else None
        inline = n.attrs.get("style") or ""
        where = f"inline on <{n.tag}>"
        findings.extend(_font_findings(inline, where, min_pt))
        if in_svg:
            for m in FONT_SIZE_DECL.finditer(inline):
                if UNITLESS.match(m.group(1)):
                    findings.extend(_size_finding(m.group(1), where, min_pt,
                                                  scale, n.tag))
            attr = n.attrs.get("font-size")
            if attr is not None:
                findings.extend(_size_finding(attr, f"in the font-size "
                                              f"attribute of <{n.tag}>",
                                              min_pt, scale, n.tag))

    # Sheet too large.
    if size and max(size) > max_in:
        findings.append(f"The sheet is {_fmt_size(size)}: a side is longer than "
                        f"{_fmt(max_in)} in, a typical plotter roll width. The "
                        f"page probably holds more design than one sheet can "
                        f"carry; trim it or split it into more pages.")

    callouts = sum(1 for n in nodes if CALLOUT_CLASS in n.classes())
    if callouts > MAX_CALLOUTS:
        findings.append(f"{callouts} callouts is over the limit of "
                        f"{MAX_CALLOUTS}; combine them or move content into a "
                        f"card set.")
    return findings, size, callouts


def find_chrome():
    for name in CHROME_NAMES:
        path = shutil.which(name)
        if path:
            return path
    for path in MAC_CHROME:
        if os.path.isfile(path) and os.access(path, os.X_OK):
            return path
    return None


def make_preview(page, png, size):
    """Screenshot the page with headless Chrome. Return a line to print."""
    chrome = find_chrome()
    if chrome is None:
        return "preview: skipped, no headless Chrome was found"
    w, h = size or DEFAULT_SIZE
    width = round(w * PREVIEW_DPI + PREVIEW_DPI)
    height = round(h * PREVIEW_DPI + PREVIEW_DPI)
    out = Path(png).resolve()
    cmd = [chrome, "--headless", "--hide-scrollbars",
           f"--window-size={width},{height}", f"--screenshot={out}",
           Path(page).resolve().as_uri()]
    try:
        subprocess.run(cmd, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=120, check=False)
    except (OSError, subprocess.TimeoutExpired) as err:
        return f"preview: failed, {err}"
    if not out.exists():
        return f"preview: failed, Chrome wrote no file at {out}"
    return f"preview: {out}"


def main(argv=None):
    ap = argparse.ArgumentParser(
        prog="check_page.py",
        description="Check a one-page design HTML file without rendering it: "
                    "sheet size declared once in inches at :root, no hidden "
                    "or linked content, no external resources, no text under "
                    "the minimum size (SVG text at 100 units per inch), "
                    "drawing layer scale, sheet side limit and at most 8 "
                    "callouts. It cannot see clipping or overlap; use "
                    "--preview and look at the image.")
    ap.add_argument("page", help="path of the HTML file")
    ap.add_argument("--min-pt", type=float, default=6.0,
                    help="smallest allowed text size in points (default 6)")
    ap.add_argument("--max-in", type=float, default=44.0,
                    help="longest allowed sheet side in inches (default 44)")
    ap.add_argument("--preview", metavar="PNG",
                    help="write a preview PNG with headless Chrome")
    args = ap.parse_args(argv)
    try:
        html = Path(args.page).read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as err:
        print(f"{ap.prog}: cannot read {args.page}: {err}", file=sys.stderr)
        return 2
    findings, size, callouts = check_page(html, args.min_pt, args.max_in)
    for f in findings:
        print(f"- {f}")
    print(f"callouts: {callouts}")
    if size:
        print(f"size: {size[0]:.1f} x {size[1]:.1f} in")
    if args.preview:
        print(make_preview(args.page, args.preview, size))
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
