# HTML to text script

Usage: `python3 html_to_text.py PAGE.html [PAGE.html ...] [--out DIR]`

The script converts saved HTML pages, for example store documentation, to plain text, so you can search them and quote exact numbers. It drops script, style and nav content and puts table cells on 1 line with ` | ` between them. Copy the block into a file named `html_to_text.py` before you run it. It uses the Python standard library only.

```python
#!/usr/bin/env python3
"""Usage: python3 html_to_text.py PAGE.html [PAGE.html ...] [--out DIR]

Convert saved HTML pages (for example store documentation) to plain text, so you
can search them and quote exact numbers. Script, style and nav content is dropped.
Block elements become new lines; table cells are separated by ' | '.
Without --out, the text goes to stdout. With --out, each page goes to DIR/<name>.txt.
Standard library only.
"""
import argparse
import os
import re
import sys
from html.parser import HTMLParser

SKIP = {"script", "style", "noscript", "svg", "nav", "head"}
BLOCK = {"p", "div", "br", "li", "ul", "ol", "tr", "table", "section", "article",
         "h1", "h2", "h3", "h4", "h5", "h6", "pre", "blockquote", "dd", "dt"}


class TextParser(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.parts = []
        self.skip_depth = 0

    def handle_starttag(self, tag, attrs):
        if tag in SKIP:
            self.skip_depth += 1
        elif tag in BLOCK:
            self.parts.append("\n")
            if tag.startswith("h") and len(tag) == 2:
                self.parts.append("#" * int(tag[1]) + " ")
            elif tag == "li":
                self.parts.append("- ")
        elif tag in ("td", "th"):
            self.parts.append(" | ")

    def handle_endtag(self, tag):
        if tag in SKIP and self.skip_depth:
            self.skip_depth -= 1
        elif tag in BLOCK:
            self.parts.append("\n")

    def handle_data(self, data):
        if not self.skip_depth:
            self.parts.append(data)

    def text(self):
        raw = "".join(self.parts)
        lines = [re.sub(r"[ \t\r\f\v]+", " ", ln).strip() for ln in raw.split("\n")]
        out = "\n".join(lines)
        return re.sub(r"\n{3,}", "\n\n", out).strip() + "\n"


def convert(path):
    with open(path, encoding="utf-8", errors="replace") as f:
        p = TextParser()
        p.feed(f.read())
        return p.text()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("pages", nargs="+")
    ap.add_argument("--out", help="output folder for .txt files")
    args = ap.parse_args()
    if args.out:
        os.makedirs(args.out, exist_ok=True)
    for path in args.pages:
        text = convert(path)
        if args.out:
            name = os.path.splitext(os.path.basename(path))[0] + ".txt"
            with open(os.path.join(args.out, name), "w", encoding="utf-8") as f:
                f.write(text)
            print(f"{path} -> {name} ({len(text)} chars)")
        else:
            sys.stdout.write(f"==> {path} <==\n{text}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```
