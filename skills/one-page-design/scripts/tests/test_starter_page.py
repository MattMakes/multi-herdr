"""Tests for assets/starter.html."""

import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SKILL = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(SKILL / "scripts"))
import check_page  # noqa: E402

STARTER = SKILL / "assets" / "starter.html"

# Names from the first build. spec-rewrite-starter-p2 t2: none may remain.
OLD_NAMES = ("--ink", "--paper", "--rule", "--accent", "--wash-", "--muted",
             "t1", "t2", "t3", "story", "param", "formula", "title",
             "callout", "card", "sub", "row", "legend", "outline", "lead",
             "stamp")


def rule_body(css, selector):
    hit = re.search(r"(?:^|[\s}])" + re.escape(selector) + r"\s*\{([^}]*)\}",
                    css)
    return hit.group(1) if hit else None


def page_sizes_in_pdf(data):
    boxes = re.findall(rb"/MediaBox\s*\[\s*([\d.\s]+)\]", data)
    return [tuple(round(float(v) / 72, 2) for v in box.split()[2:4])
            for box in boxes]


class Starter(unittest.TestCase):
    def setUp(self):
        self.text = STARTER.read_text(encoding="utf-8")
        self.code = re.sub(r"<!--.*?-->", "", self.text, flags=re.S)
        self.css = re.search(r"<style>(.*)</style>", self.code, re.S).group(1)

    def test_checker_passes(self):  # spec-page-starter t1, spec-rewrite-starter-p2 t1
        self.assertEqual(check_page.check_page(self.text), ([], (17.0, 11.0), 0))

    def test_file_level_rules(self):  # spec-page-starter t2
        page = rule_body(self.css, "@page")
        self.assertRegex(page, r"size:\s*var\(--sheet-w\)\s+var\(--sheet-h\)")
        self.assertRegex(page, r"margin:\s*0")
        self.assertRegex(rule_body(self.css, "html"), r"overflow:\s*auto")
        self.assertNotRegex(self.code, r"(?i)<script|<link")
        self.assertNotRegex(self.code, r"(?i)https?://|@import|url\(")
        surface = re.search(r'<main class="sheet"[^>]*>', self.code).group(0)
        self.assertEqual(surface, '<main class="sheet">')

    def test_size_only_on_root(self):
        self.assertEqual(len(re.findall(r"--sheet-[wh]:\s*[\d.]+in",
                                        self.text)), 2)
        sheet = rule_body(self.css, ".sheet")
        self.assertIn("width: var(--sheet-w)", sheet)
        self.assertIn("height: var(--sheet-h)", sheet)

    def test_print_drops_the_desk(self):
        printed = re.search(r"@media print\s*\{(.*)\}\s*$", self.css, re.S)
        self.assertIsNotNone(printed)
        body = printed.group(1)
        for needed in ("background: none", "padding: 0", "box-shadow: none"):
            self.assertIn(needed, body)

    def test_levels_and_voices(self):
        self.assertRegex(rule_body(self.css, ".far"), r"font-size:\s*28pt")
        self.assertRegex(rule_body(self.css, ".far"), r"font-weight:\s*700")
        self.assertRegex(rule_body(self.css, ".near"), r"font-size:\s*11pt")
        self.assertRegex(rule_body(self.css, ".fine"), r"font-size:\s*7pt")
        self.assertIn("italic", rule_body(self.css, ".flavor"))
        self.assertIn("normal", rule_body(self.css, ".mechanic"))
        self.assertIn("italic", rule_body(self.css, ".tuning"))
        self.assertIn("monospace", rule_body(self.css, ".math"))

    def test_tokens(self):
        root = rule_body(self.css, ":root")
        for token in ("--c-text", "--c-sheet", "--c-line", "--c-hot",
                      "--c-fill-a", "--c-fill-b", "--c-fill-c"):
            self.assertIn(token + ":", root)

    def test_head_comment_and_placeholders(self):
        head = self.text.split("</head>")[0]
        self.assertIn("<!--", head)
        self.assertIn("only place", head)
        self.assertIn('class="headline far"', self.text)
        self.assertIn("before you write any prose", self.text)
        self.assertRegex(self.text,
                         r'class="dateline">\w+ · \d{4}-\d{2}-\d{2}<')

    def test_old_names_gone(self):  # spec-rewrite-starter-p2 t2
        for path in SKILL.rglob("*"):
            if not path.is_file() or path.suffix not in (".md", ".html",
                                                         ".py"):
                continue
            if path.suffix == ".py" and path.name != "check_page.py":
                continue  # tests use these words as prose, not as names
            text = path.read_text(encoding="utf-8")
            for old in OLD_NAMES:
                if old.startswith("--"):
                    pattern = re.escape(old)
                elif path.suffix == ".py":
                    # In code an old class name would be a string literal.
                    pattern = r"[\"']" + re.escape(old) + r"[\"']"
                else:
                    pattern = (r'class="[^"]*\b' + re.escape(old) + r'\b'
                               r'|\.' + re.escape(old) + r'\b(?![-.])')
                hit = re.search(pattern, text)
                self.assertIsNone(hit, f"old name {old} in {path.name}")

    def test_new_size_prints_at_that_size(self):  # spec-page-starter t3
        bigger = (self.text.replace("--sheet-w: 17in", "--sheet-w: 30in")
                  .replace("--sheet-h: 11in", "--sheet-h: 20in"))
        self.assertEqual(check_page.check_page(bigger), ([], (30.0, 20.0), 0))
        chrome = check_page.find_chrome()
        if chrome is None:
            self.skipTest("no headless Chrome on this machine")
        with tempfile.TemporaryDirectory() as tmp:
            source = Path(tmp) / "big.html"
            pdf = Path(tmp) / "big.pdf"
            source.write_text(bigger, encoding="utf-8")
            subprocess.run([chrome, "--headless", "--no-pdf-header-footer",
                            f"--print-to-pdf={pdf}", source.as_uri()],
                           stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL, timeout=120, check=True)
            self.assertEqual(page_sizes_in_pdf(pdf.read_bytes()), [(30.0, 20.0)])


if __name__ == "__main__":
    unittest.main()
