"""Tests for scripts/check_page.py: the static rules and the command line."""

import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPTS = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(SCRIPTS))
import check_page  # noqa: E402

SCRIPT = SCRIPTS / "check_page.py"


def page(width="24in", height="16in", head="", body="", sheet_attrs="",
         root=True):
    """A minimal valid page; each test changes one part of it."""
    root_rule = (f":root {{ --sheet-w: {width}; --sheet-h: {height}; }}"
                 if root else "")
    return f"""<!doctype html>
<html><head><style>
{root_rule}
@page {{ size: var(--sheet-w) var(--sheet-h); margin: 0; }}
.sheet {{ width: var(--sheet-w); height: var(--sheet-h); }}
p {{ font-size: 9pt; }}
{head}
</style></head><body>
<main class="sheet"{sheet_attrs}>
<h1 class="title">The point of the page</h1>
<p>One paragraph.</p>
{body}
<p class="stamp">Draft 2026-01-01</p>
</main></body></html>"""


def layer(view_box, inner):
    return (f'<svg viewBox="{view_box}" style="position:absolute;inset:0">'
            f"{inner}</svg>")


def has(findings, *words):
    return [f for f in findings if all(w.lower() in f.lower() for w in words)]


class RuleTests(unittest.TestCase):
    def test_smallest_valid_page(self):  # spec-page-checks t1
        findings, size, callouts = check_page.check_page(page())
        self.assertEqual(findings, [])
        self.assertEqual(size, (24.0, 16.0))
        self.assertEqual(callouts, 0)

    def test_missing_sheet(self):
        html = page().replace('<main class="sheet">', "<main>")
        findings, size, _ = check_page.check_page(html)
        self.assertTrue(has(findings, "no sheet element"))
        self.assertIsNone(size)

    def test_units_converted_and_last_root_rule_wins(self):
        html = page(head=":root { --sheet-w: 508mm; --sheet-h: 30.48cm; }")
        _, size, _ = check_page.check_page(html)
        self.assertAlmostEqual(size[0], 20.0)
        self.assertAlmostEqual(size[1], 12.0)

    def test_other_units_not_accepted(self):
        findings, size, _ = check_page.check_page(page("2400px", "1600px"))
        self.assertIsNone(size)
        self.assertTrue(has(findings, "not declared"))

    def test_hidden_content(self):  # spec-page-checks t2
        body = ('<a href="appendix.html">more</a><details><summary>x</summary>'
                '</details><section style="Display : NONE">s</section>'
                '<p hidden>h</p>')
        findings, _, _ = check_page.check_page(page(body=body))
        self.assertTrue(has(findings, "appendix.html"))
        self.assertTrue(has(findings, "details", "collapsible"))
        self.assertTrue(has(findings, "<section>", "display: none"))
        self.assertTrue(has(findings, "<p>", "hidden attribute"))

    def test_empty_href_is_not_a_link(self):
        findings, _, _ = check_page.check_page(page(body='<a href=" ">x</a>'))
        self.assertEqual(findings, [])

    def test_inline_visibility_hidden_and_iframe(self):
        body = ('<span style="visibility:hidden">v</span>'
                '<iframe src="inner.html"></iframe>')
        findings, _, _ = check_page.check_page(page(body=body))
        self.assertTrue(has(findings, "<span>", "visibility"))
        self.assertTrue(has(findings, "iframe", "another page"))

    def test_display_none_in_style_block(self):
        findings, _, _ = check_page.check_page(page(head=".x { display:none }"))
        self.assertTrue(has(findings, "style block", "display: none"))

    def test_size_only_on_sheet(self):  # spec-page-checks t3
        html = page(root=False,
                    sheet_attrs=' style="--sheet-w: 24in; --sheet-h: 16in"')
        findings, size, _ = check_page.check_page(html)
        self.assertIsNone(size)
        self.assertTrue(has(findings, "only inline", ":root"))

    def test_conflicting_sizes(self):  # spec-page-checks t4
        html = page(sheet_attrs=' style="--sheet-w: 22in; --sheet-h: 14in"')
        findings, size, _ = check_page.check_page(html)
        self.assertEqual(size, (24.0, 16.0))
        self.assertTrue(has(findings, "conflicting", "24 x 16", "22 x 14"))

    def test_equal_sizes_no_finding(self):
        html = page(sheet_attrs=' style="--sheet-w: 24in; --sheet-h: 16in"')
        findings, _, _ = check_page.check_page(html)
        self.assertEqual(findings, [])

    def test_hover(self):  # spec-page-checks t5
        findings, _, _ = check_page.check_page(
            page(head=".note:hover .more { display: block; }"))
        self.assertEqual(len(findings), 1)
        self.assertTrue(has(findings, ":hover"))

    def test_small_text(self):  # spec-page-checks t6
        html = page(head=".tiny { font-size: 3pt; }",
                    body='<p style="font-size: 6px">x</p>')
        findings, _, _ = check_page.check_page(html)
        self.assertTrue(has(findings, "3pt", "3.0 pt", "style block", "6 pt"))
        self.assertTrue(has(findings, "6px", "4.5 pt", "inline on <p>"))

    def test_text_at_minimum_passes(self):
        html = page(head=".fine { font-size: 6pt; } .px { font-size: 8px; }")
        findings, _, _ = check_page.check_page(html)
        self.assertEqual(findings, [])

    def test_leading_decimal_and_shorthand(self):  # spec-page-checks t7
        html = page(head=".a { font-size: .3pt; } "
                         ".b { font: bold 4px/1.3 monospace; }")
        findings, _, _ = check_page.check_page(html)
        self.assertTrue(has(findings, ".3pt"))
        self.assertTrue(has(findings, "4px"))

    def test_relative_unit(self):  # spec-page-checks t8
        findings, _, _ = check_page.check_page(
            page(head=".a { font-size: 0.9em; }"))
        self.assertTrue(has(findings, "0.9em", "cannot be verified for print"))

    def test_two_external_resources(self):  # spec-page-checks t9
        body = ('<link rel="stylesheet" href="https://example.com/a.css">'
                '<img src="//cdn.example.com/b.png" alt="">'
                '<img src="data:image/png;base64,AAAA" alt="">')
        findings, _, _ = check_page.check_page(page(body=body))
        self.assertEqual(len(has(findings, "external resource")), 2)
        self.assertTrue(has(findings, "<link>", "https://example.com/a.css"))
        self.assertTrue(has(findings, "<img>", "//cdn.example.com/b.png"))

    def test_too_large(self):  # spec-page-checks t10, spec-page-contract t4
        findings, size, _ = check_page.check_page(page("48in", "16in"))
        self.assertEqual(size, (48.0, 16.0))
        self.assertTrue(has(findings, "44", "plotter"))

    def test_max_side_option(self):
        findings, _, _ = check_page.check_page(page(), max_in=20)
        self.assertTrue(has(findings, "20 in", "plotter"))

    def test_svg_text_too_small(self):  # spec-page-checks t11
        svg = layer("0 0 2400 1600", '<text font-size="6">a</text>')
        findings, _, _ = check_page.check_page(page(body=svg))
        hits = has(findings, "svg text below the minimum", "font-size 6",
                   "4.3 pt", "100 units per inch", "<text>")
        self.assertEqual(len(hits), 1)
        self.assertEqual(len(findings), 1)

    def test_svg_text_at_minimum(self):  # spec-page-checks t12
        svg = layer("0 0 2400 1600", '<text font-size="9">a</text>'
                                     '<text style="font-size: 9">b</text>')
        findings, _, _ = check_page.check_page(page(body=svg))
        self.assertEqual(findings, [])

    def test_svg_inline_unitless_too_small(self):
        svg = layer("0 0 2400 1600", '<text style="font-size:5">b</text>')
        findings, _, _ = check_page.check_page(page(body=svg))
        self.assertTrue(has(findings, "svg text below", "font-size 5"))

    def test_layer_wrong_scale(self):  # spec-page-checks t13
        svg = layer("0 0 4800 3200", '<text font-size="12">a</text>')
        findings, _, _ = check_page.check_page(page(body=svg))
        self.assertTrue(has(findings, "4800 x 3200", "2400 x 1600"))
        self.assertTrue(has(findings, "svg text below", "font-size 12",
                            "4.3 pt", "200 units per inch"))

    def test_layer_viewbox_for_30_by_20(self):  # spec-drawing-parts t1
        svg = layer("0 0 3000 2000", '<text font-size="9">a</text>')
        findings, size, _ = check_page.check_page(
            page("30in", "20in", body=svg))
        self.assertEqual(size, (30.0, 20.0))
        self.assertEqual(findings, [])

    def test_fixed_width_svg_is_not_a_layer(self):
        svg = ('<svg viewBox="0 0 100 100" width="200">'
               '<text font-size="9">a</text></svg>')
        findings, _, _ = check_page.check_page(page(body=svg))
        self.assertEqual(findings, [])

    def test_nested_layer_used_when_none_direct(self):
        svg = '<div>' + layer("0 0 4800 3200", "") + '</div>'
        findings, _, _ = check_page.check_page(page(body=svg))
        self.assertTrue(has(findings, "4800 x 3200"))

    def test_svg_attribute_with_px(self):
        svg = layer("0 0 2400 1600", '<text font-size="7px">a</text>')
        findings, _, _ = check_page.check_page(page(body=svg))
        self.assertTrue(has(findings, "7px", "5.2 pt"))

    def test_callout_limit(self):  # spec-page-checks t14, spec-page-contract t1
        nine = '<div class="callout left">c</div>' * 9
        findings, _, callouts = check_page.check_page(page(body=nine))
        self.assertEqual(callouts, 9)
        self.assertEqual(len(has(findings, "9 callouts", "limit of 8")), 1)
        eight = '<div class="left callout">c</div>' * 8
        findings, _, callouts = check_page.check_page(page(body=eight))
        self.assertEqual(callouts, 8)
        self.assertEqual(findings, [])


class CommandTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, html, name="page.html"):
        path = self.dir / name
        path.write_text(html, encoding="utf-8")
        return path

    def run_cli(self, *args):
        env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
        return subprocess.run([sys.executable, str(SCRIPT), *map(str, args)],
                              capture_output=True, text=True, env=env,
                              timeout=60)

    def test_clean_page(self):  # spec-checker-command t1
        r = self.run_cli(self.write(page()))
        self.assertEqual(r.returncode, 0, r.stdout)
        self.assertEqual(r.stdout.splitlines(),
                         ["callouts: 0", "size: 24.0 x 16.0 in"])

    def test_findings_set_exit_status(self):  # spec-checker-command t2
        r = self.run_cli(self.write(page("48in", "16in")))
        self.assertEqual(r.returncode, 1)
        lines = r.stdout.splitlines()
        self.assertTrue(lines[0].startswith("- "))
        self.assertEqual(lines[-2:], ["callouts: 0", "size: 48.0 x 16.0 in"])

    def test_callouts_at_limit(self):  # spec-checker-command t3
        body = '<div class="callout">c</div>' * 8
        r = self.run_cli(self.write(page(body=body)))
        self.assertEqual(r.returncode, 0)
        self.assertIn("callouts: 8", r.stdout.splitlines())

    def test_callouts_over_limit(self):  # spec-checker-command t4
        body = '<div class="callout">c</div>' * 9
        r = self.run_cli(self.write(page(body=body)))
        self.assertEqual(r.returncode, 1)
        self.assertIn("callouts: 9", r.stdout.splitlines())
        self.assertTrue(any(l.startswith("- ") and "limit of 8" in l
                            for l in r.stdout.splitlines()))

    def test_no_size_line_when_unknown(self):
        r = self.run_cli(self.write(page(root=False)))
        self.assertEqual(r.returncode, 1)
        self.assertFalse(any(l.startswith("size:")
                             for l in r.stdout.splitlines()))
        self.assertIn("callouts: 0", r.stdout.splitlines())

    def test_min_pt_option(self):
        r = self.run_cli(self.write(page()), "--min-pt", "10")
        self.assertEqual(r.returncode, 1)
        self.assertIn("9pt", r.stdout)

    def test_file_not_found(self):  # spec-checker-command t5
        r = self.run_cli(self.dir / "missing.html")
        self.assertEqual(r.returncode, 2)
        self.assertTrue(r.stderr.startswith("check_page.py: "))
        self.assertEqual(r.stdout, "")

    def test_bad_arguments(self):
        r = self.run_cli(self.write(page()), "--min-pt", "small")
        self.assertEqual(r.returncode, 2)
        self.assertEqual(self.run_cli().returncode, 2)

    def test_preview_without_chrome(self):  # spec-checker-command t6
        path = self.write(page())
        out = io.StringIO()
        with mock.patch.object(check_page, "find_chrome", return_value=None), \
                contextlib.redirect_stdout(out):
            code = check_page.main([str(path), "--preview",
                                    str(self.dir / "p.png")])
        self.assertEqual(code, 0)
        lines = out.getvalue().splitlines()
        self.assertIn("skipped", lines[-1])
        self.assertIn("no headless Chrome", lines[-1])

    def test_preview_runs_chrome_with_window_size(self):
        path = self.write(page())
        png = self.dir / "p.png"

        def fake_run(cmd, **kwargs):
            self.assertIs(kwargs["stdout"], subprocess.DEVNULL)
            self.assertIs(kwargs["stderr"], subprocess.DEVNULL)
            png.write_bytes(b"png")
            fake_run.cmd = cmd

        out = io.StringIO()
        with mock.patch.object(check_page, "find_chrome",
                               return_value="/opt/chrome"), \
                mock.patch.object(check_page.subprocess, "run", fake_run), \
                contextlib.redirect_stdout(out):
            code = check_page.main([str(path), "--min-pt", "10",
                                    "--preview", str(png)])
        self.assertEqual(code, 1)  # the preview never changes the status
        cmd = fake_run.cmd
        self.assertIn("--headless", cmd)
        self.assertIn("--hide-scrollbars", cmd)
        self.assertIn("--window-size=2400,1632", cmd)  # (24+1)*96, (16+1)*96
        self.assertIn(f"--screenshot={png.resolve()}", cmd)
        self.assertEqual(out.getvalue().splitlines()[-1],
                         f"preview: {png.resolve()}")

    def test_preview_default_size_when_unknown(self):
        path = self.write(page(root=False))
        cmds = []
        with mock.patch.object(check_page, "find_chrome",
                               return_value="/opt/chrome"), \
                mock.patch.object(check_page.subprocess, "run",
                                  lambda cmd, **k: cmds.append(cmd)), \
                contextlib.redirect_stdout(io.StringIO()):
            check_page.main([str(path), "--preview", str(self.dir / "p.png")])
        self.assertIn("--window-size=1728,1152", cmds[0])  # 18 x 12 in at 96

    def test_find_chrome_searches_path_first(self):
        with mock.patch.object(check_page.shutil, "which",
                               lambda n: "/x/chromium" if n == "chromium"
                               else None):
            self.assertEqual(check_page.find_chrome(), "/x/chromium")
        with mock.patch.object(check_page.shutil, "which", lambda n: None), \
                mock.patch.object(check_page.os.path, "isfile",
                                  lambda p: False):
            self.assertIsNone(check_page.find_chrome())


if __name__ == "__main__":
    unittest.main()
