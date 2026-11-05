"""Tests for scripts/check_page.py: each page rule and the command line."""

import contextlib
import io
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(HERE))
import check_page  # noqa: E402

CHECKER = HERE / "check_page.py"


def sheet_html(size=("24in", "16in"), css="", inside="", surface_style=None,
               root_rule=True):
    """A small page that passes every rule. A test changes one piece."""
    root = (f":root {{ --sheet-h: {size[1]}; --sheet-w: {size[0]}; }}"
            if root_rule else "")
    style = f' style="{surface_style}"' if surface_style is not None else ""
    return "\n".join([
        "<!DOCTYPE html>",
        '<html lang="en">',
        "<head>",
        '<meta charset="utf-8">',
        "<style>",
        root,
        ".sheet { height: var(--sheet-h); width: var(--sheet-w); }",
        "@page { margin: 0; size: var(--sheet-w) var(--sheet-h); }",
        ".body-copy { font-size: 10pt; }",
        css,
        "</style>",
        "</head>",
        "<body>",
        f'<main class="sheet"{style}>',
        '  <h1 class="headline far">Ferry routes meet at the lighthouse</h1>',
        '  <section class="body-copy">Three boats, one dock.</section>',
        inside,
        '  <div class="dateline">Proposal 2026-03-14</div>',
        "</main>",
        "</body>",
        "</html>",
    ])


def svg_layer(box, content, where="position:absolute;inset:0"):
    return f'<svg style="{where}" viewBox="{box}">{content}</svg>'


def matching(findings, *needles):
    """The findings that contain every needle, ignoring case."""
    return [f for f in findings
            if all(n.lower() in f.lower() for n in needles)]


def check(html, **kw):
    return check_page.check_page(html, **kw)


class SizeRules(unittest.TestCase):
    def test_smallest_valid_page(self):  # spec-page-checks t1
        self.assertEqual(check(sheet_html()), ([], (24.0, 16.0), 0))

    def test_metric_units_and_last_root_rule(self):
        html = sheet_html(css=":root { --sheet-h: 254mm; --sheet-w: 45.72cm; }")
        _, size, _ = check(html)
        self.assertAlmostEqual(size[0], 18.0)
        self.assertAlmostEqual(size[1], 10.0)

    def test_pixel_size_is_not_accepted(self):
        findings, size, _ = check(sheet_html(("1800px", "1200px")))
        self.assertIsNone(size)
        self.assertTrue(matching(findings, "not declared"))

    def test_size_only_on_surface(self):  # spec-page-checks t3
        html = sheet_html(root_rule=False,
                          surface_style="--sheet-w:24in;--sheet-h:16in")
        findings, size, _ = check(html)
        self.assertIsNone(size)
        self.assertEqual(len(matching(findings, "only inline", ":root")), 1)

    def test_two_different_sizes(self):  # spec-page-checks t4
        html = sheet_html(surface_style="--sheet-w: 22in; --sheet-h: 14in")
        findings, size, _ = check(html)
        self.assertEqual(size, (24.0, 16.0))
        self.assertTrue(matching(findings, "conflicting", "24 x 16",
                                 "22 x 14"))

    def test_same_size_twice_is_fine(self):
        html = sheet_html(surface_style="--sheet-w: 24in; --sheet-h: 16in")
        self.assertEqual(check(html)[0], [])

    def test_too_large(self):  # spec-page-checks t10, spec-page-contract t4
        findings, size, _ = check(sheet_html(("48in", "16in")))
        self.assertEqual(size, (48.0, 16.0))
        self.assertTrue(matching(findings, "44", "plotter"))

    def test_longest_side_parameter(self):
        findings, _, _ = check(sheet_html(), longest_side=20)
        self.assertTrue(matching(findings, "20 in", "plotter"))


class SurfaceRules(unittest.TestCase):
    def test_no_surface_size_at_root(self):  # spec-checker-rule-fixes-p2 t1
        html = sheet_html(("48in", "10in")).replace('class="sheet"', "")
        findings, size, _ = check(html)
        self.assertEqual(size, (48.0, 10.0))
        self.assertEqual(len(findings), 2, findings)
        self.assertTrue(matching(findings, "no sheet element"))
        self.assertTrue(matching(findings, "44 in", "plotter"))

    def test_no_surface_no_size(self):  # spec-checker-rule-fixes-p2 t2
        findings, size, _ = check("<html><body><p>Just a note.</p>"
                                  "</body></html>")
        self.assertIsNone(size)
        self.assertEqual(len(findings), 2, findings)
        self.assertTrue(matching(findings, "no sheet element"))
        self.assertTrue(matching(findings, "not declared", ":root"))

    def test_size_on_html_element_only(self):  # spec-checker-rule-fixes-p2 t3
        html = sheet_html(root_rule=False,
                          css="html { --sheet-w: 18in; --sheet-h: 9in; }")
        findings, size, _ = check(html)
        self.assertIsNone(size)
        self.assertEqual(len(findings), 1, findings)
        self.assertTrue(matching(findings, "not declared", ":root"))

    def test_no_surface_layer_scale_still_checked(self):
        html = sheet_html(inside=svg_layer("0 0 1200 800", "")) \
            .replace('class="sheet"', 'class="paper"')
        findings, _, _ = check(html)
        self.assertTrue(matching(findings, "1200 x 800", "2400 x 1600"))


class HiddenContentRules(unittest.TestCase):
    def test_hidden_content(self):  # spec-page-checks t2
        inside = ('<a href="appendix.html">see appendix</a>'
                  '<details><summary>Tide tables</summary>low, high</details>'
                  '<aside style="DISPLAY:  None">crew list</aside>'
                  '<span hidden>spare text</span>')
        findings, _, _ = check(sheet_html(inside=inside))
        self.assertTrue(matching(findings, "appendix.html"))
        self.assertTrue(matching(findings, "<details>", "collapsible"))
        self.assertTrue(matching(findings, "<aside>", "display: none"))
        self.assertTrue(matching(findings, "<span>", "hidden attribute"))
        self.assertEqual(len(findings), 4, findings)

    def test_blank_href_is_not_a_link(self):
        self.assertEqual(check(sheet_html(inside='<a href="  ">x</a>'))[0], [])

    def test_inline_visibility_and_iframe(self):
        inside = ('<em style="Visibility : Hidden">ghost</em>'
                  '<iframe src="harbour-map.html"></iframe>')
        findings, _, _ = check(sheet_html(inside=inside))
        self.assertTrue(matching(findings, "<em>", "visibility"))
        self.assertTrue(matching(findings, "iframe", "another page"))

    def test_display_none_in_style_block(self):
        findings, _, _ = check(sheet_html(css=".spare { display: none; }"))
        self.assertTrue(matching(findings, "style block", "display: none"))

    def test_hover(self):  # spec-page-checks t5
        findings, _, _ = check(
            sheet_html(css=".dock:hover .timetable { display: block; }"))
        self.assertEqual(len(findings), 1, findings)
        self.assertTrue(matching(findings, ":hover"))

    def test_repeats_give_one_finding(self):  # spec-checker-rule-fixes-p2 t4
        css = (".buoy:hover { color: red; } .pier:hover { color: blue; }"
               " .old { display: none; } .older { display:none }")
        findings, _, _ = check(sheet_html(css=css))
        self.assertEqual(len(matching(findings, ":hover")), 1)
        self.assertEqual(len(matching(findings, "style block",
                                      "display: none")), 1)
        self.assertEqual(len(findings), 2, findings)

    def test_inline_findings_stay_per_element(self):
        inside = '<b style="display:none">a</b><i style="display:none">b</i>'
        findings, _, _ = check(sheet_html(inside=inside))
        self.assertEqual(len(matching(findings, "inline display: none")), 2)

    def test_visibility_in_style_block(self):  # spec-checker-rule-fixes-p2 t5
        findings, _, _ = check(sheet_html(css=".dimmed { visibility: hidden; }"))
        self.assertEqual(findings, [])


class ExternalRules(unittest.TestCase):
    def test_two_external_resources(self):  # spec-page-checks t9
        inside = ('<link href="https://styles.example.org/harbour.css" '
                  'rel="stylesheet">'
                  '<img alt="" src="//pics.example.net/boat.png">'
                  '<img alt="" src="data:image/png;base64,iVBOR">')
        findings, _, _ = check(sheet_html(inside=inside))
        self.assertEqual(len(matching(findings, "external resource")), 2)
        self.assertTrue(matching(findings, "<link>",
                                 "https://styles.example.org/harbour.css"))
        self.assertTrue(matching(findings, "<img>",
                                 "//pics.example.net/boat.png"))


class TextSizeRules(unittest.TestCase):
    def test_small_text(self):  # spec-page-checks t6
        html = sheet_html(css=".footnote { font-size: 3pt; }",
                          inside='<small style="font-size:6px">tiny</small>')
        findings, _, _ = check(html)
        self.assertTrue(matching(findings, "3pt", "3.0 pt", "style block",
                                 "6 pt"))
        self.assertTrue(matching(findings, "6px", "4.5 pt", "inline on <small>"))

    def test_at_the_minimum(self):
        html = sheet_html(css=".a { font-size: 6pt } .b { font-size: 8px }")
        self.assertEqual(check(html)[0], [])

    def test_leading_point_and_shorthand(self):  # spec-page-checks t7
        html = sheet_html(css=".a { font-size: .3pt; } "
                              ".b { font: bold 4px/1.3 monospace; }")
        findings, _, _ = check(html)
        self.assertTrue(matching(findings, ".3pt"))
        self.assertTrue(matching(findings, "4px"))

    def test_relative_unit(self):  # spec-page-checks t8
        findings, _, _ = check(sheet_html(css=".a { font-size: 0.9em; }"))
        self.assertTrue(matching(findings, "0.9em",
                                 "cannot be verified for print"))

    def test_relative_inline_on_svg_still_found(self):
        svg = svg_layer("0 0 2400 1600", '<text style="font-size:2rem">r</text>')
        findings, _, _ = check(sheet_html(inside=svg))
        self.assertTrue(matching(findings, "2rem", "cannot be verified"))

    def test_min_pt_parameter(self):
        findings, _, _ = check(sheet_html(), min_pt=12)
        self.assertTrue(matching(findings, "10pt", "12 pt"))


class DrawingLayerRules(unittest.TestCase):
    def test_svg_text_too_small(self):  # spec-page-checks t11
        svg = svg_layer("0 0 2400 1600", '<text font-size="6">a</text>')
        findings, _, _ = check(sheet_html(inside=svg))
        self.assertEqual(len(findings), 1, findings)
        self.assertTrue(matching(findings, "svg text below the minimum",
                                 "font-size 6", "4.3 pt",
                                 "100 units per inch", "<text>"))

    def test_svg_text_at_minimum(self):  # spec-page-checks t12
        svg = svg_layer("0 0 2400 1600", '<text font-size="9">a</text>'
                                         '<text style="font-size: 9">b</text>')
        self.assertEqual(check(sheet_html(inside=svg))[0], [])

    def test_svg_inline_unitless_too_small(self):
        svg = svg_layer("0 0 2400 1600", '<tspan style="font-size:5">b</tspan>')
        findings, _, _ = check(sheet_html(inside=svg))
        self.assertTrue(matching(findings, "svg text below", "font-size 5",
                                 "<tspan>"))

    def test_layer_wrong_scale(self):  # spec-page-checks t13
        svg = svg_layer("0 0 4800 3200", '<text font-size="12">a</text>')
        findings, _, _ = check(sheet_html(inside=svg))
        self.assertTrue(matching(findings, "4800 x 3200", "2400 x 1600"))
        self.assertTrue(matching(findings, "svg text below", "font-size 12",
                                 "4.3 pt", "200 units per inch"))

    def test_view_box_for_30_by_20(self):  # spec-drawing-parts t1
        svg = svg_layer("0 0 3000 2000", '<text font-size="9">a</text>')
        self.assertEqual(check(sheet_html(("30in", "20in"), inside=svg)),
                         ([], (30.0, 20.0), 0))

    def test_fixed_width_svg_is_not_a_layer(self):
        svg = ('<svg width="300" viewBox="0 0 50 50">'
               '<text font-size="9">a</text></svg>')
        self.assertEqual(check(sheet_html(inside=svg))[0], [])

    def test_px_attribute_checked_as_text(self):
        svg = svg_layer("0 0 2400 1600", '<text font-size="7px">a</text>')
        findings, _, _ = check(sheet_html(inside=svg))
        self.assertTrue(matching(findings, "7px", "5.2 pt"))

    def test_first_candidate_inside_a_div(self):  # spec-checker-rule-fixes-p2 t6
        first = ('<div class="inset">'
                 + svg_layer("0 0 4800 2400", '<text font-size="10">a</text>')
                 + "</div>")
        second = svg_layer("0 0 1600 800", "")
        findings, _, _ = check(sheet_html(("16in", "8in"),
                                          inside=first + second))
        self.assertEqual(len(findings), 2, findings)
        self.assertTrue(matching(findings, "4800 x 2400", "1600 x 800"))
        self.assertTrue(matching(findings, "svg text below", "font-size 10",
                                 "2.4 pt", "300 units per inch"))

    def test_later_nested_candidate_is_not_a_layer(self):
        first = svg_layer("0 0 2400 1600", "")
        later = '<div>' + svg_layer("0 0 900 900", "") + '</div>'
        self.assertEqual(check(sheet_html(inside=first + later))[0], [])

    def test_relative_svg_attribute(self):  # spec-checker-rule-fixes-p2 t7
        svg = svg_layer("0 0 2400 1600", '<text font-size="150%">a</text>')
        self.assertEqual(check(sheet_html(inside=svg))[0], [])


class StyleTextParity(unittest.TestCase):
    """spec-checker-css-parity-p3: size tokens and the :root selector."""

    def page(self, **kw):
        return sheet_html(("20in", "12in"), **kw)

    def test_small_size_token(self):  # t1
        findings, size, _ = check(self.page(
            css=".tide-table { --caption-font-size: 5pt; }"))
        self.assertEqual(size, (20.0, 12.0))
        self.assertEqual(len(findings), 1, findings)
        self.assertTrue(matching(findings, "5pt", "in a style block", "6 pt"))

    def test_legible_size_token(self):  # t2
        html = self.page(css=".tide-table { --caption-font-size: 12pt; }")
        self.assertEqual(check(html), ([], (20.0, 12.0), 0))

    def test_font_token_small_px(self):  # t3
        html = self.page().replace(
            "--sheet-w: 20in;", "--sheet-w: 20in; --body-font: italic 4.5px serif;")
        findings, _, _ = check(html)
        self.assertEqual(len(findings), 1, findings)
        self.assertTrue(matching(findings, "4.5px", "3.4 pt"))

    def test_relative_size_token(self):
        findings, _, _ = check(self.page(
            css=".tide-table { --caption-font-size: 0.8rem; }"))
        self.assertTrue(matching(findings, "cannot be verified"))

    def test_root_must_be_last_selector(self):  # t4
        rule = "{ --sheet-w: 20in; --sheet-h: 12in; }"
        page_a = sheet_html(root_rule=False, css="html, :root " + rule)
        self.assertEqual(check(page_a), ([], (20.0, 12.0), 0))
        findings, size, _ = check(sheet_html(root_rule=False,
                                             css=":root, html " + rule))
        self.assertIsNone(size)
        self.assertEqual(len(findings), 1, findings)
        self.assertTrue(matching(findings, "not declared", ":root"))

    def test_root_after_statement_at_rule(self):  # t5
        html = self.page().replace("<style>\n", "<style>\n@layer base, page;\n", 1)
        self.assertIn("@layer base, page;\n:root", html)
        self.assertEqual(check(html), ([], (20.0, 12.0), 0))

    def test_size_token_inline_and_in_svg(self):  # t6
        inside = ('<p style="--note-font-size: 4pt">Tide tables</p>'
                  + svg_layer("0 0 2000 1200",
                              '<text style="--tag-font-size: 6" x="40" '
                              'y="90">Pier</text>'))
        findings, size, _ = check(self.page(inside=inside))
        self.assertEqual(size, (20.0, 12.0))
        self.assertEqual(len(findings), 2, findings)
        self.assertTrue(matching(findings, "4pt", "inline on <p>"))
        self.assertTrue(matching(findings, "svg text", "4.3 pt",
                                 "100 units per inch"))


class CalloutRules(unittest.TestCase):
    def test_callout_limit(self):  # spec-page-checks t14, spec-page-contract t1
        nine = '<div class="note west">n</div>' * 9
        findings, _, count = check(sheet_html(inside=nine))
        self.assertEqual(count, 9)
        self.assertEqual(len(matching(findings, "9 callouts", "limit of 8")), 1)
        eight = '<div class="east note">n</div>' * 8
        self.assertEqual(check(sheet_html(inside=eight)),
                         ([], (24.0, 16.0), 8))


class CommandLine(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.folder = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def save(self, text, name="design.html"):
        target = self.folder / name
        target.write_text(text, encoding="utf-8")
        return target

    def invoke(self, *argv):
        env = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
        return subprocess.run([sys.executable, str(CHECKER)]
                              + [str(a) for a in argv],
                              capture_output=True, text=True, env=env,
                              timeout=60)

    def test_clean_page(self):  # spec-checker-command t1
        done = self.invoke(self.save(sheet_html()))
        self.assertEqual(done.returncode, 0, done.stdout)
        self.assertEqual(done.stdout.splitlines(),
                         ["callouts: 0", "size: 24.0 x 16.0 in"])

    def test_findings_set_exit_status(self):  # spec-checker-command t2
        done = self.invoke(self.save(sheet_html(("48in", "16in"))))
        self.assertEqual(done.returncode, 1)
        out = done.stdout.splitlines()
        self.assertTrue(out[0].startswith("- "))
        self.assertEqual(out[-2:], ["callouts: 0", "size: 48.0 x 16.0 in"])

    def test_callouts_at_limit(self):  # spec-checker-command t3
        done = self.invoke(self.save(sheet_html(
            inside='<p class="note">n</p>' * 8)))
        self.assertEqual(done.returncode, 0)
        self.assertIn("callouts: 8", done.stdout.splitlines())

    def test_callouts_over_limit(self):  # spec-checker-command t4
        done = self.invoke(self.save(sheet_html(
            inside='<p class="note">n</p>' * 9)))
        self.assertEqual(done.returncode, 1)
        out = done.stdout.splitlines()
        self.assertIn("callouts: 9", out)
        self.assertTrue([l for l in out
                         if l.startswith("- ") and "limit of 8" in l])

    def test_size_line_left_out_when_unknown(self):
        done = self.invoke(self.save(sheet_html(root_rule=False)))
        self.assertEqual(done.returncode, 1)
        out = done.stdout.splitlines()
        self.assertFalse([l for l in out if l.startswith("size:")])
        self.assertIn("callouts: 0", out)

    def test_min_pt_option(self):
        done = self.invoke(self.save(sheet_html()), "--min-pt", "12")
        self.assertEqual(done.returncode, 1)
        self.assertIn("10pt", done.stdout)

    def test_longest_side_option(self):
        done = self.invoke(self.save(sheet_html()), "--longest-side", "20")
        self.assertEqual(done.returncode, 1)
        self.assertIn("plotter", done.stdout)

    def test_file_not_found(self):  # spec-checker-command t5
        done = self.invoke(self.folder / "absent.html")
        self.assertEqual(done.returncode, 2)
        self.assertTrue(done.stderr.startswith("check_page.py: "))
        self.assertEqual(done.stdout, "")

    def test_bad_arguments(self):
        self.assertEqual(self.invoke(self.save(sheet_html()), "--min-pt",
                                     "tiny").returncode, 2)
        self.assertEqual(self.invoke().returncode, 2)

    def test_preview_without_chrome(self):  # spec-checker-command t6
        target = self.save(sheet_html())
        buffer = io.StringIO()
        with mock.patch.object(check_page, "find_chrome", return_value=None), \
                contextlib.redirect_stdout(buffer):
            status = check_page.main([str(target), "--preview",
                                      str(self.folder / "shot.png")])
        self.assertEqual(status, 0)
        last = buffer.getvalue().splitlines()[-1]
        self.assertIn("skipped", last)
        self.assertIn("no headless Chrome", last)

    def test_preview_command_and_window(self):
        target = self.save(sheet_html())
        shot = self.folder / "shot.png"
        seen = {}

        def pretend_chrome(argv, **options):
            self.assertIs(options["stdout"], subprocess.DEVNULL)
            self.assertIs(options["stderr"], subprocess.DEVNULL)
            seen["argv"] = argv
            shot.write_bytes(b"\x89PNG")

        buffer = io.StringIO()
        with mock.patch.object(check_page, "find_chrome",
                               return_value="/usr/local/bin/chromium"), \
                mock.patch.object(check_page.subprocess, "run",
                                  pretend_chrome), \
                contextlib.redirect_stdout(buffer):
            status = check_page.main([str(target), "--min-pt", "12",
                                      "--preview", str(shot)])
        self.assertEqual(status, 1)  # the preview never changes the status
        argv = seen["argv"]
        self.assertIn("--headless", argv)
        self.assertIn("--hide-scrollbars", argv)
        self.assertIn("--window-size=2400,1632", argv)  # (24+1)*96, (16+1)*96
        self.assertIn(f"--screenshot={shot.resolve()}", argv)
        self.assertEqual(buffer.getvalue().splitlines()[-1],
                         f"preview: {shot.resolve()}")

    def test_preview_window_when_size_unknown(self):
        target = self.save(sheet_html(root_rule=False))
        calls = []
        with mock.patch.object(check_page, "find_chrome",
                               return_value="/usr/local/bin/chromium"), \
                mock.patch.object(check_page.subprocess, "run",
                                  lambda argv, **o: calls.append(argv)), \
                contextlib.redirect_stdout(io.StringIO()):
            check_page.main([str(target), "--preview",
                             str(self.folder / "shot.png")])
        self.assertIn("--window-size=1728,1152", calls[0])  # 18 x 12 in

    def test_chrome_search_order(self):
        with mock.patch.object(check_page.shutil, "which",
                               lambda p: "/opt/bin/chromium"
                               if p == "chromium" else None):
            self.assertEqual(check_page.find_chrome(), "/opt/bin/chromium")
        with mock.patch.object(check_page.shutil, "which", lambda p: None), \
                mock.patch.object(check_page.os.path, "isfile",
                                  lambda p: False):
            self.assertIsNone(check_page.find_chrome())


if __name__ == "__main__":
    unittest.main()
