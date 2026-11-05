#!/usr/bin/env python3
"""Page checker for one-page designs.

The checker reads the HTML as text. It does not render the page, so it cannot
find clipped or overlapping parts: look at the preview image for those. It
uses the Python standard library only.

Module use:   findings, size, callouts = check_page(text)
Command use:  python3 check_page.py page.html [--min-pt 6]
              [--longest-side 44] [--preview out.png]
Exit codes:   0 = no findings, 1 = one or more findings,
              2 = the file cannot be read or the arguments are wrong.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
from html.parser import HTMLParser
from pathlib import Path

# Names that the starter page, the drawing parts and SKILL.md also use.
SURFACE_TAG = "main"
SURFACE_CLASS = "sheet"
NOTE_CLASS = "note"
SIZE_W = "--sheet-w"
SIZE_H = "--sheet-h"

CALLOUT_CAP = 8
LAYER_UNITS = 100.0  # drawing units per inch
SCREEN_PPI = 96
FALLBACK_SIZE = (17.0, 11.0)
INCHES_PER = {"in": 1.0, "cm": 1 / 2.54, "mm": 1 / 25.4}
NO_CLOSE = frozenset(
    "area base br col embed hr img input link meta param source track wbr"
    .split())
REMOTE_TAGS = frozenset(("img", "iframe", "link", "script"))
RELATIVE_UNITS = frozenset(("em", "rem", "%"))

_N = r"(\d*\.?\d+)"
_LENGTH = re.compile(r"^\s*" + _N + r"\s*(in|cm|mm)\s*$", re.I)
_FONT_SIZE = re.compile(r"(?<![\w-])font-size\s*:\s*([^;}\"]+)", re.I)
_FONT = re.compile(r"(?<![\w-])font\s*:\s*([^;}\"]+)", re.I)
_WITH_UNIT = re.compile(r"(?<![\w.])" + _N + r"(pt|px|em|rem|%)(?!\w)", re.I)
_BARE = re.compile(r"^\s*" + _N + r"\s*$")
_RULE = re.compile(r"([^{}]*)\{([^{}]*)\}")
_COMMENT = re.compile(r"/\*.*?\*/", re.S)
_HOVER = re.compile(r":hover\b", re.I)
_CSS_DISPLAY_NONE = re.compile(r"(?<![\w-])display\s*:\s*none\b", re.I)
_FIXED_WIDTH = re.compile(
    r"(?<![\w-])width\s*:\s*" + _N + r"\s*(px|pt|pc|in|cm|mm)", re.I)


# ---------------------------------------------------------------- pass 1: tree

class Element:
    def __init__(self, tag, attrs, parent):
        self.tag = tag
        self.attrs = attrs
        self.parent = parent
        self.children = []
        self.drawing = None  # the outermost <svg> around this element, if any

    def attr(self, name):
        return self.attrs.get(name) or ""

    def has_class(self, name):
        return name in self.attr("class").split()


class TreeBuilder(HTMLParser):
    """Builds an Element tree and keeps the text of every <style> element."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.top = Element("#document", {}, None)
        self.open = self.top
        self.css_parts = []

    def _attach(self, tag, attrs):
        values = {k: ("" if v is None else v) for k, v in attrs}
        child = Element(tag, values, self.open)
        self.open.children.append(child)
        return child

    def handle_starttag(self, tag, attrs):
        child = self._attach(tag, attrs)
        if tag not in NO_CLOSE:
            self.open = child

    def handle_startendtag(self, tag, attrs):
        self._attach(tag, attrs)

    def handle_endtag(self, tag):
        node = self.open
        while node is not self.top and node.tag != tag:
            node = node.parent
        if node is not self.top:
            self.open = node.parent

    def handle_data(self, data):
        if self.open.tag == "style":
            self.css_parts.append(data)


def walk(node):
    """Every element below node, in document order."""
    for child in node.children:
        yield child
        yield from walk(child)


def mark_drawings(top):
    for el in walk(top):
        outer = el.parent.drawing if el.parent is not None else None
        el.drawing = outer if outer is not None else (el if el.tag == "svg"
                                                      else None)


class Page:
    def __init__(self, text, min_pt, longest_side):
        builder = TreeBuilder()
        builder.feed(text)
        builder.close()
        mark_drawings(builder.top)
        self.elements = list(walk(builder.top))
        self.css = _COMMENT.sub("", "\n".join(builder.css_parts))
        self.min_pt = min_pt
        self.longest_side = longest_side
        self.surface = next((el for el in self.elements
                             if el.tag == SURFACE_TAG
                             and el.has_class(SURFACE_CLASS)), None)
        self.size = None
        self.scales = {}  # id(layer svg) -> units per inch for its text


# ------------------------------------------------------------- small helpers

def num(x):
    return f"{x:g}"


def inches_text(size):
    return f"{num(round(size[0], 2))} x {num(round(size[1], 2))} in"


def to_inches(value):
    hit = _LENGTH.match(value or "")
    return float(hit.group(1)) * INCHES_PER[hit.group(2).lower()] if hit else None


def declarations(block):
    pairs = (part.split(":", 1) for part in block.split(";") if ":" in part)
    return {k.strip().lower(): v.strip() for k, v in pairs}


def declared_size(block):
    decls = declarations(block)
    w, h = to_inches(decls.get(SIZE_W)), to_inches(decls.get(SIZE_H))
    return None if w is None or h is None else (w, h)


def root_size(css):
    """Size from the last :root rule that declares both properties."""
    found = None
    for selectors, block in _RULE.findall(css):
        if any(s.strip() == ":root" for s in selectors.split(",")):
            found = declared_size(block) or found
    return found


def is_remote(url):
    return url.strip().lower().startswith(("http://", "https://", "//"))


def unit_size_finding(raw, where, min_pt, relative_ok=False):
    """Finding for a size written with a unit, or None."""
    hit = _WITH_UNIT.match(raw.strip())
    if hit is None:
        return None
    value, unit = float(hit.group(1)), hit.group(2).lower()
    if unit in RELATIVE_UNITS:
        if relative_ok:
            return None
        return (f"font-size {raw.strip()} {where} cannot be verified for "
                f"print; use pt or px.")
    points = value * 72 / 96 if unit == "px" else value
    if points < min_pt:
        return (f"Text size {hit.group(0)} ({points:.1f} pt) {where} is below "
                f"the {num(min_pt)} pt minimum.")
    return None


def drawing_size_finding(raw, scale, tag, min_pt):
    """Finding for a unitless SVG size, or None."""
    hit = _BARE.match(raw)
    if hit is None:
        return None
    points = float(hit.group(1)) * 72 / scale
    if points >= min_pt:
        return None
    return (f"SVG text below the minimum: font-size {hit.group(1)} is "
            f"{points:.1f} pt at {scale:.0f} units per inch on <{tag}>; the "
            f"minimum is {num(min_pt)} pt.")


def css_font_values(css):
    """Every font-size value, plus the size part of each font shorthand."""
    values = [m.group(1).strip() for m in _FONT_SIZE.finditer(css)]
    for m in _FONT.finditer(css):
        part = _WITH_UNIT.search(m.group(1))
        if part:
            values.append(part.group(0))
    return values


def layer_view_box(el):
    """(width, height) of the view box when el can be a drawing layer."""
    if el.tag != "svg" or el.drawing is not el:
        return None
    parts = re.split(r"[\s,]+", el.attr("viewbox").strip())
    if len(parts) != 4:
        return None
    try:
        box = [float(p) for p in parts]
    except ValueError:
        return None
    width = el.attrs.get("width")
    if width is not None and width.strip() != "100%":
        return None
    if _FIXED_WIDTH.search(el.attr("style")):
        return None
    return box[2], box[3]


# ------------------------------------------------------ pass 2: one rule each

def rule_surface_and_size(page):
    out = []
    at_root = root_size(page.css)
    if page.surface is None:
        out.append(f'No sheet element: the page needs <{SURFACE_TAG} class="'
                   f'{SURFACE_CLASS}"> as its one printable surface.')
        on_surface = None
    else:
        on_surface = declared_size(page.surface.attr("style"))
    if at_root is not None:
        page.size = at_root
        if on_surface is not None and any(
                abs(a - b) > 1e-6 for a, b in zip(at_root, on_surface)):
            out.append(f"Conflicting sheet sizes: :root says "
                       f"{inches_text(at_root)} but the sheet element says "
                       f"{inches_text(on_surface)}. Keep the :root one only.")
    elif on_surface is not None:
        out.append(f"The sheet size is set only inline on the sheet element; "
                   f"move {SIZE_W} and {SIZE_H} to :root, because the print "
                   f"page size reads them from there.")
    else:
        out.append(f"The sheet size is not declared: set {SIZE_W} and "
                   f"{SIZE_H} in inches on :root.")
    return out


def rule_one_surface(page):
    """Content that is hidden, or that leads off the page."""
    out = []
    for el in page.elements:
        squeezed = re.sub(r"\s+", "", el.attr("style")).lower()
        href = el.attr("href").strip()
        if el.tag == "a" and href:
            out.append(f"Link to {href}: a printed sheet cannot follow links; "
                       f"put the content on the page.")
        if el.tag == "details":
            out.append("A <details> element is a collapsible section that "
                       "hides content; show it all on the sheet.")
        if el.tag == "iframe":
            out.append("An <iframe> pulls in another page; everything must "
                       "be on this one sheet.")
        if "hidden" in el.attrs:
            out.append(f"A <{el.tag}> has the hidden attribute: hidden content "
                       f"never reaches the printed sheet.")
        for decl, label in (("display:none", "display: none"),
                            ("visibility:hidden", "visibility: hidden")):
            if decl in squeezed:
                out.append(f"A <{el.tag}> has an inline {label}: hidden "
                           f"content never reaches the printed sheet.")
    if _HOVER.search(page.css):
        out.append("A :hover rule shows content only on hover, and paper has "
                   "no hover; show it all the time.")
    if _CSS_DISPLAY_NONE.search(page.css):
        out.append("A style block sets display: none: hidden content never "
                   "reaches the printed sheet.")
    return out


def rule_self_contained(page):
    out = []
    for el in page.elements:
        if el.tag not in REMOTE_TAGS:
            continue
        for name in ("href", "src"):
            url = el.attr(name)
            if is_remote(url):
                out.append(f"External resource <{el.tag}> {url.strip()}: the "
                           f"page must be one self-contained file.")
    return out


def rule_layers(page):
    """Pick the drawing layers, check their scale, record their text scale."""
    out = []
    picked = []
    for el in page.elements:
        box = layer_view_box(el)
        if box is None:
            continue
        if not picked or (page.surface is not None
                          and el.parent is page.surface):
            picked.append((el, box))
    for el, (bw, bh) in picked:
        scale = LAYER_UNITS
        if page.size is not None:
            per_x, per_y = bw / page.size[0], bh / page.size[1]
            scale = max(per_x, per_y)
            if max(abs(p - LAYER_UNITS) for p in (per_x, per_y)) \
                    > LAYER_UNITS * 0.01:
                out.append(
                    f"Drawing layer view box is {num(bw)} x {num(bh)}; for a "
                    f"{inches_text(page.size)} sheet at 100 units per inch it "
                    f"should be {num(round(page.size[0] * 100, 1))} x "
                    f"{num(round(page.size[1] * 100, 1))}.")
        page.scales[id(el)] = scale
    return out


def rule_text_size(page):
    out = []
    for raw in css_font_values(page.css):
        out.append(unit_size_finding(raw, "in a style block", page.min_pt))
    for el in page.elements:
        where = f"inline on <{el.tag}>"
        inline = el.attr("style")
        for raw in css_font_values(inline):
            out.append(unit_size_finding(raw, where, page.min_pt))
        if el.drawing is None:
            continue
        scale = page.scales.get(id(el.drawing), LAYER_UNITS)
        for m in _FONT_SIZE.finditer(inline):
            out.append(drawing_size_finding(m.group(1), scale, el.tag,
                                            page.min_pt))
        if "font-size" in el.attrs:
            raw = el.attrs["font-size"]
            out.append(unit_size_finding(
                raw, f"in the font-size attribute of <{el.tag}>",
                page.min_pt, relative_ok=True)
                or drawing_size_finding(raw, scale, el.tag, page.min_pt))
    return [f for f in out if f]


def rule_longest_side(page):
    if page.size is None or max(page.size) <= page.longest_side:
        return []
    return [f"The sheet is {inches_text(page.size)}: a side is longer than "
            f"{num(page.longest_side)} in, a typical plotter roll width. The "
            f"page probably holds more design than one sheet can carry; trim "
            f"it or split it into more pages."]


def count_callouts(page):
    return sum(1 for el in page.elements if el.has_class(NOTE_CLASS))


# The size rule runs first: the layer and side rules need the size.
RULES = (rule_surface_and_size, rule_one_surface, rule_self_contained,
         rule_layers, rule_text_size, rule_longest_side)


def check_page(text, min_pt=6.0, longest_side=44.0):
    """Check one page. Return (findings, size, callouts).

    findings -- list of sentences; empty when the page passes.
    size     -- (width, height) in inches, or None when it is unknown.
    callouts -- how many elements carry the callout class.
    """
    page = Page(text, min_pt, longest_side)
    findings = []
    for rule in RULES:
        findings += rule(page)
    callouts = count_callouts(page)
    if callouts > CALLOUT_CAP:
        findings.append(f"{callouts} callouts is over the limit of "
                        f"{CALLOUT_CAP}; combine them or move content into a "
                        f"card set.")
    return findings, page.size, callouts


# ------------------------------------------------------------- command line

def _chrome_candidates():
    for program in ("google-chrome", "google-chrome-stable", "chromium",
                    "chromium-browser", "chrome"):
        yield shutil.which(program)
    for app in ("Google Chrome", "Chromium"):
        bundle = f"/Applications/{app}.app/Contents/MacOS/{app}"
        if os.path.isfile(bundle) and os.access(bundle, os.X_OK):
            yield bundle


def find_chrome():
    """Path of a headless-capable Chrome: PATH first, then macOS apps."""
    return next((c for c in _chrome_candidates() if c), None)


def make_preview(page_path, png_path, size):
    """Ask headless Chrome for a screenshot. Return the line to print."""
    chrome = find_chrome()
    if not chrome:
        return "preview: skipped, no headless Chrome was found"
    w_in, h_in = size if size else FALLBACK_SIZE
    window = f"{round((w_in + 1) * SCREEN_PPI)},{round((h_in + 1) * SCREEN_PPI)}"
    target = Path(png_path).resolve()
    argv = [chrome, "--headless", "--hide-scrollbars",
            "--window-size=" + window, "--screenshot=" + str(target),
            Path(page_path).resolve().as_uri()]
    try:
        subprocess.run(argv, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=120, check=False)
    except (OSError, subprocess.TimeoutExpired) as err:
        return f"preview: failed, {err}"
    if target.exists():
        return f"preview: {target}"
    return f"preview: failed, Chrome wrote no file at {target}"


def build_parser():
    parser = argparse.ArgumentParser(
        prog="check_page.py",
        description=(
            "Check a one-page design HTML file without rendering it. The "
            "checks: the sheet size is declared once, in inches, on :root; "
            "nothing is hidden or linked away; no resource is loaded from "
            "the network; no text is under the minimum size (SVG text at "
            "100 units per inch); each drawing layer has the right scale; no "
            "side is over the limit; at most 8 callouts. Clipping and "
            "overlap need a look at the --preview image."))
    parser.add_argument("page", help="the HTML file to check")
    parser.add_argument("--min-pt", type=float, default=6.0, metavar="PT",
                        help="smallest text size allowed, in points "
                             "(default 6)")
    parser.add_argument("--longest-side", type=float, default=44.0,
                        metavar="IN",
                        help="longest sheet side allowed, in inches "
                             "(default 44)")
    parser.add_argument("--preview", metavar="PNG",
                        help="also save a preview PNG made by headless Chrome")
    return parser


def report_lines(findings, size, callouts):
    lines = ["- " + f for f in findings]
    lines.append(f"callouts: {callouts}")
    if size:
        lines.append("size: {:.1f} x {:.1f} in".format(*size))
    return lines


def main(argv=None):
    parser = build_parser()
    opts = parser.parse_args(argv)
    try:
        text = Path(opts.page).read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as err:
        sys.stderr.write(f"{parser.prog}: cannot read {opts.page}: {err}\n")
        return 2
    findings, size, callouts = check_page(text, opts.min_pt,
                                          opts.longest_side)
    lines = report_lines(findings, size, callouts)
    if opts.preview:
        lines.append(make_preview(opts.page, opts.preview, size))
    print("\n".join(lines))
    return 0 if not findings else 1


if __name__ == "__main__":
    sys.exit(main())
