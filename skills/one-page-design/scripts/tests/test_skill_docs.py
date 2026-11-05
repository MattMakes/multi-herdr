"""The agent-facing scenarios cannot run as code. These tests check that the
skill text states each rule the scenarios depend on."""

import re
import sys
import unittest
from pathlib import Path

SKILL = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(SKILL / "scripts"))
import check_page  # noqa: E402


def read(rel):
    return (SKILL / rel).read_text(encoding="utf-8")


def front_matter():
    text = read("SKILL.md")
    m = re.match(r"---\n(.*?)\n---\n", text, re.S)
    fields = {}
    for line in m.group(1).splitlines():
        key, _, value = line.partition(":")
        fields[key.strip()] = value.strip()
    return fields


def squash(text):
    return re.sub(r"\s+", " ", text).lower()


class SkillTextTests(unittest.TestCase):
    def test_front_matter(self):
        fm = front_matter()
        self.assertEqual(fm["name"], "one-page-design")
        self.assertIn("one-page design", fm["description"])
        self.assertIn("Stone Librande", read("SKILL.md"))

    def test_trigger_match(self):  # spec-skill-workflow t1
        desc = front_matter()["description"].lower()
        for phrase in ("too long", "nobody reads", "wall", "one sheet",
                       "poster", "table-mat", "meeting"):
            self.assertIn(phrase, desc)

    def test_trigger_ticket_deliverable(self):  # spec-skill-trigger-p2 t1
        desc = front_matter()["description"].lower()
        self.assertIn("orchestrator ticket", desc)
        self.assertIn("deliverable", desc)
        body = squash(read("SKILL.md"))
        self.assertIn("a team or fleet task asks for a one-page design as its "
                      "output", body)

    def test_trigger_one_pager(self):  # spec-skill-trigger-p2 t2
        desc = front_matter()["description"].lower()
        self.assertIn("one-pager", desc)
        self.assertIn("must fit on one sheet", desc)

    def test_scope_when_to_use(self):  # spec-skill-trigger-p2 b2
        body = squash(read("SKILL.md"))
        self.assertIn("keep on a wall or a table and read it there", body)
        self.assertIn("grown too long for the people who must read it", body)

    def test_trigger_exclusion(self):  # spec-skill-workflow t2, spec-skill-trigger-p2 t3
        desc = front_matter()["description"].lower()
        self.assertIn("skip it when the output is a dashboard, a report or a "
                      "slide deck", desc)
        body = squash(read("SKILL.md"))
        self.assertIn("leave it out for slide decks, dashboards, status "
                      "reports and api reference documents", body)

    def test_no_point_sentence(self):  # spec-skill-workflow t3
        body = squash(read("SKILL.md"))
        self.assertIn("do not make a page", body)
        self.assertIn("there is not yet a page to make", body)
        self.assertIn("ask them for the one point", body)

    def test_drawn_way_without_image_source(self):  # spec-skill-workflow t4
        body = squash(read("SKILL.md"))
        self.assertIn("nothing on hand can make images", body)
        self.assertIn("exact and open to later edits", body)
        self.assertIn("inline svg", body)
        self.assertIn("svg or html that sits on top of the picture", body)

    def test_supporting_files_named_and_present(self):
        body = read("SKILL.md")
        for rel in ("references/page-parts.md", "references/drawing-parts.md",
                    "references/illustrator-request.md", "assets/starter.html",
                    "references/page-contract.md", "references/principles.md",
                    "scripts/check_page.py"):
            self.assertIn(rel, body)
            self.assertTrue((SKILL / rel).is_file(), rel)

    def test_common_errors_listed(self):
        body = squash(read("SKILL.md"))
        for error in ("no date", "smaller to fit", "standard paper size",
                      "words inside a generated image", "defined but not used",
                      "picture share rule", "boxes and arrows",
                      "headings and bullet lists", "numbered circles",
                      "prose box"):
            self.assertIn(error, body)

    def test_print_instructions(self):
        body = squash(read("SKILL.md"))
        self.assertIn("print to pdf at the declared size", body)
        self.assertIn("plotter", body)
        self.assertIn("poster or tiling", body)


class ContractTests(unittest.TestCase):
    def setUp(self):
        self.text = squash(read("references/page-contract.md"))

    def test_picture_too_small(self):  # spec-page-contract t2
        self.assertIn("covers at least half of the sheet", self.text)
        self.assertIn("35 percent", self.text)

    def test_one_text_style_fails(self):  # spec-page-contract t3
        self.assertIn("one text style only fails", self.text)
        self.assertIn("flavour italic, the rules upright and the numbers as a "
                      "small italic line", self.text)

    def test_limits(self):  # spec-page-contract t1, t4
        self.assertIn("at most 8 callouts", self.text)
        self.assertIn("at most 3 card sets", self.text)
        self.assertIn("never make fine print smaller than 6 pt", self.text)

    def test_no_external_loads(self):  # spec-reword-doc-phrases-p3 b3
        self.assertIn("fetches no style sheet, script or font from outside "
                      "its own file", self.text)


class PartsTests(unittest.TestCase):
    def setUp(self):
        self.parts = squash(read("references/page-parts.md"))
        self.drawing = squash(read("references/drawing-parts.md"))

    def test_timed_route(self):  # spec-visual-grammar t1
        self.assertIn("map with numbered stops", self.parts)
        self.assertIn("minute ruler along the bottom edge", self.parts)

    def test_crossed_attributes(self):  # spec-visual-grammar t2
        self.assertIn("an x in each unused cell", self.parts)

    def test_screen_flow(self):  # spec-visual-grammar t3
        self.assertIn("how does one screen or menu lead to the next? "
                      "(only this) | a flowchart", self.parts)
        self.assertIn("only case for a flowchart", self.parts)

    def test_wrong_ring(self):  # spec-visual-grammar t4
        self.assertIn("use a ring only when that is true", self.parts)
        self.assertIn("refuse that shape and use a matrix or cards", self.parts)

    def test_stacked_blocks(self):  # spec-drawing-parts t2
        self.assertIn("move each layer up by one block height", self.drawing)
        self.assertIn("draw the lowest block first and the highest block "
                      "last", self.drawing)
        self.assertIn("draw from back to front", self.drawing)

    def test_leader_line_shape(self):  # spec-drawing-parts t3
        self.assertIn("it is straight or it has one bend", self.drawing)
        self.assertIn("it is never curved", self.drawing)
        self.assertIn("two leader lines never cross", self.drawing)
        self.assertIn("ends in a small dot on the target itself", self.drawing)
        self.assertIn("100 drawing units for each inch", self.drawing)
        self.assertIn("all cards on one page use one layout", self.drawing)

    def test_layer_view_box_rule(self):  # spec-drawing-parts t1 (text half)
        self.assertIn('viewbox="0 0 3000 2000"', self.drawing)


class SnippetTests(unittest.TestCase):
    """spec-rewrite-drawing-parts-p2 t2: the parts still pass the checker."""

    def snippet(self, heading):
        section = read("references/drawing-parts.md").split(
            "## " + heading + "\n")[1].split("\n## ")[0]
        return re.search(r"```html\n(.*?)```", section, re.S).group(1)

    def test_layer_route_and_legend_in_starter(self):
        layer = self.snippet("The drawing layer")
        route = self.snippet("Route with stops")
        legend = self.snippet("Legend")
        self.assertIn("<!-- shapes first, leader lines last -->", layer)
        layer = layer.replace("<!-- shapes first, leader lines last -->",
                              route)
        starter = read("assets/starter.html")
        mark = '<p class="dateline">'
        self.assertIn(mark, starter)
        page = starter.replace(mark, layer + legend + mark, 1)
        findings, size, _ = check_page.check_page(page)
        self.assertEqual((findings, size), ([], (17.0, 11.0)))
        self.assertIn("<svg", page)


class RequestTests(unittest.TestCase):
    def setUp(self):
        self.text = read("references/illustrator-request.md")

    def test_completed_form(self):  # spec-illustrator-request t1
        example = self.text.split("## Filled example")[1].split("## When")[0]
        self.assertIn("View type: high oblique", example)
        for part in ("gears", "workbench", "pendulum"):
            self.assertIn(part, example)
        path = re.search(r"Output file: (\S+)", example).group(1)
        self.assertTrue(path.startswith("/"))
        self.assertIn("NO TEXT", example)
        self.assertIn("2048 pixels", example)
        self.assertIn("accent colour only: orange", example.replace("\n", " "))
        self.assertNotIn("<", example)  # every field is filled

    def test_view_types_and_embed_step(self):  # spec-rewrite-picture-request-p4 t1
        form = self.text.split("## Form")[1].split("## Filled example")[0]
        field = re.search(r"View type: <choose one: (.*)>", form).group(1)
        names = [part.split(" (")[0] for part in field.split(" | ")]
        self.assertEqual(names, ["top-down", "high oblique", "eye level",
                                 "equal-axis", "single frame"])
        step = squash(self.text.split("## When the picture comes back")[1])
        for need in ("base64", "data:image/png;base64", "<img>",
                     "no separate image file"):
            self.assertIn(need, step)

    def test_text_in_result(self):  # spec-illustrator-request t2
        text = squash(self.text)
        self.assertIn("open", text)
        self.assertIn("refuse it and send the same form again", text)
        self.assertIn("never edit or paint out the text", text)


class PrinciplesTests(unittest.TestCase):
    def setUp(self):
        self.text = read("references/principles.md")

    def test_mission_principle(self):  # spec-reference-principles t1
        section = squash(self.text.split("## A mission or quest")[1]
                         .split("\n## ")[0])
        for need in ("thumbnail", "italic story line", "central map",
                     "numbers and letters", "column on one side", "legend",
                     "date", "flooded city"):
            self.assertIn(need, section)

    def test_trade_off_regions(self):  # spec-reword-doc-phrases-p3 b4
        section = squash(self.text.split("## Roles that trade off")[1]
                         .split("\n## ")[0])
        for need in ("edges are the axes", "each region holds a single unit "
                     "group", "figure", "traits", "continuous trade-offs"):
            self.assertIn(need, section)

    def test_credit_and_no_images(self):  # spec-reference-principles t2
        self.assertIn("Librande", self.text)
        self.assertNotRegex(self.text, r"!\[|<img|\.png|\.jpe?g")


if __name__ == "__main__":
    unittest.main()
