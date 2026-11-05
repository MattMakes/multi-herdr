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


def media_box_inches(pdf_bytes):
    boxes = re.findall(rb"/MediaBox\s*\[\s*([\d.\s]+)\]", pdf_bytes)
    return [tuple(round(float(v) / 72, 2) for v in b.split()[2:4])
            for b in boxes]


class StarterTests(unittest.TestCase):
    def setUp(self):
        self.html = STARTER.read_text(encoding="utf-8")

    def test_checker_passes(self):  # spec-page-starter t1
        findings, size, callouts = check_page.check_page(self.html)
        self.assertEqual(findings, [])
        self.assertEqual(size, (17.0, 11.0))
        self.assertEqual(callouts, 0)

    def test_text_level_rules(self):  # spec-page-starter t2
        html = re.sub(r"<!--.*?-->", "", self.html, flags=re.S)
        self.assertRegex(html, r"@page\s*\{[^}]*size:\s*var\(--sheet-w\)\s+"
                               r"var\(--sheet-h\)")
        self.assertRegex(html, r"@page\s*\{[^}]*margin:\s*0")
        self.assertRegex(html, r"html,\s*body\s*\{[^}]*overflow:\s*auto")
        self.assertNotRegex(html, r"(?i)<script|<link")
        self.assertNotRegex(html, r"(?i)https?://|@import|url\(")
        sheet = re.search(r'<main class="sheet"[^>]*>', html).group(0)
        self.assertEqual(sheet, '<main class="sheet">')

    def test_size_set_only_at_root(self):
        sizes = re.findall(r"--sheet-[wh]:\s*[\d.]+in", self.html)
        self.assertEqual(len(sizes), 2)
        self.assertRegex(self.html, r"\.sheet\s*\{[^}]*width:\s*var\(--sheet-w\)")
        self.assertRegex(self.html, r"\.sheet\s*\{[^}]*height:\s*var\(--sheet-h\)")

    def test_print_hides_backdrop(self):
        block = re.search(r"@media print\s*\{(.*?)\n  \}", self.html, re.S)
        self.assertIsNotNone(block)
        self.assertIn("box-shadow: none", block.group(1))
        self.assertIn("padding: 0", block.group(1))
        self.assertIn("background: none", block.group(1))

    def test_text_levels_and_styles(self):
        self.assertRegex(self.html, r"\.t1\s*\{[^}]*font-size:\s*28pt[^}]*bold")
        self.assertRegex(self.html, r"\.t2\s*\{[^}]*font-size:\s*11pt")
        self.assertRegex(self.html, r"\.t3\s*\{[^}]*font-size:\s*7pt")
        self.assertRegex(self.html, r"\.story\s*\{[^}]*italic")
        self.assertRegex(self.html, r"\.param\s*\{[^}]*italic")
        self.assertRegex(self.html, r"\.formula\s*\{[^}]*monospace")

    def test_head_comment_and_placeholders(self):
        head = self.html.split("</head>")[0]
        self.assertIn("<!--", head)
        self.assertIn("ONE place", head)
        self.assertRegex(self.html, r'class="title t1"')
        self.assertIn("CENTRE PICTURE", self.html)
        self.assertRegex(self.html, r'class="stamp">\w+ · \d{4}-\d{2}-\d{2}<')

    def test_changed_size_prints_at_that_size(self):  # spec-page-starter t3
        # Static half: the print size reads only the two root properties.
        resized = self.html.replace("--sheet-w: 17in", "--sheet-w: 30in") \
                           .replace("--sheet-h: 11in", "--sheet-h: 20in")
        findings, size, _ = check_page.check_page(resized)
        self.assertEqual((findings, size), ([], (30.0, 20.0)))
        # Rendered half: needs a local headless Chrome.
        chrome = check_page.find_chrome()
        if chrome is None:
            self.skipTest("no headless Chrome on this machine")
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / "page.html"
            pdf = Path(tmp) / "page.pdf"
            src.write_text(resized, encoding="utf-8")
            subprocess.run([chrome, "--headless", "--no-pdf-header-footer",
                            f"--print-to-pdf={pdf}", src.as_uri()],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           timeout=120, check=True)
            self.assertEqual(media_box_inches(pdf.read_bytes()), [(30.0, 20.0)])


if __name__ == "__main__":
    unittest.main()
