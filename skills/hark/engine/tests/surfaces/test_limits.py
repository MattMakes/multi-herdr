"""Acceptance tests L-T1 .. L-T5 of limits and film-pipeline integration (surfaces-limits.md L4).

L-T1 reads the hark docs (unit skills-and-docs), L-T2 and L-T3 render kits (unit bank-kit-sfx), L-T4 reads cues
(unit timeline). Each one skips while its unit is absent. L-T5 always checks the texts of this unit.
"""

from __future__ import annotations

import json
import os
import re
from pathlib import Path

import pytest

from hark.surfaces import gallery, gallery_page, llm, mcp_server

SKILLS = Path(__file__).resolve().parents[4]
DOC_ROOTS = (SKILLS / "hark", SKILLS / "sound-sourcing")

# Distinctive L2 numbers: a doc line that quotes one must say it is a source measurement.
L2_NUMBERS = ("0.966", "0.951", "1289", "1484", "0.958", "0.948", "0.909", "0.858", "93.2%", "96.4%", "89.1%",
              "120 of 140", "128 passed", "8 times in 15", "0.945", "0.881", "168 pass")
SOURCE_LABEL = re.compile(r"source", re.IGNORECASE)

# Sentences that state 1 user's taste as an instruction (M1.9 rule).
PREFERENCE_PHRASES = ("the user rejects", "the user prefers", "the user hates", "the user dislikes",
                      "the user likes", "the user wants real", "the user wants no", "never use synthetic",
                      "always use real", "the operator prefers", "the operator rejects", "we prefer real")


def doc_files() -> list[Path]:
    files: list[Path] = []
    for root in DOC_ROOTS:
        if root.is_dir():
            files += [p for p in root.rglob("*.md") if "engine" not in p.relative_to(root).parts]
    return files


def paragraphs(text: str) -> list[str]:
    return [p for p in re.split(r"\n\s*\n", text) if p.strip()]


def test_l_t1_doc_numbers_carry_source_label():
    files = doc_files()
    if not files:
        pytest.skip("needs unit skills-and-docs (T-22): no hark docs yet")
    bad = []
    for path in files:
        for para in paragraphs(path.read_text(encoding="utf-8")):
            hits = [n for n in L2_NUMBERS if n in para]
            if hits and not SOURCE_LABEL.search(para):
                bad.append(f"{path.name}: {hits}")
    assert bad == []


def _kit():
    kit = pytest.importorskip("hark.forge.bank.kit", reason="needs unit bank-kit-sfx (T-19)")
    bank = pytest.importorskip("hark.forge.bank.bank", reason="needs unit bank-kit-sfx (T-19)")
    if not list(bank.ids()):
        pytest.skip("needs-bank: no shipped bank files yet (25b builds them)")
    return kit, bank


def test_l_t2_kit_render_cached(tmp_path):
    kit, bank = _kit()
    kit_json = str(tmp_path / "audio" / "kit.json")
    kit.init(preset="calm", out=kit_json)
    takes = str(tmp_path / "takes")
    first = kit.render(kit_json, takes)
    assert first["stats"]["rendered"] > 0
    second = kit.render(kit_json, takes)
    assert second["stats"]["rendered"] == 0 and second["stats"]["cached"] == first["stats"]["takes"]
    lock = json.loads(Path(takes, "kit.lock.json").read_text())
    assert lock["bank"] == bank.VERSION


def test_l_t3_null_role_has_no_take(tmp_path):
    kit, _ = _kit()
    kit_json = tmp_path / "kit.json"
    kit.init(preset="calm", out=str(kit_json))
    doc = json.loads(kit_json.read_text())
    doc["roles"]["riser"] = None
    kit_json.write_text(json.dumps(doc))
    takes = tmp_path / "takes"
    kit.render(str(kit_json), str(takes))
    assert not [p for p in takes.iterdir() if p.name.startswith("riser")]
    lock = json.loads((takes / "kit.lock.json").read_text())
    assert lock["roles"].get("riser") is None
    cue = {"fps": 30, "events": [{"f": 45, "kind": "riser", "weight": 0, "pan": 0}]}
    placed = [e for e in cue["events"] if lock["roles"].get(e["kind"])]
    assert placed == []


def test_l_t4_cue_frames_to_seconds(tmp_path):
    tl = pytest.importorskip("hark.timeline.timeline", reason="needs unit timeline (T-15)")
    doc = {"fps": 30, "bpm": 120, "total": 300, "acts": [], "cue": "x",
           "events": [{"f": 90, "kind": "hit", "weight": 1, "pan": 0}]}
    cues, fps = tl.read_cues(doc)
    assert fps == 30 and cues[0]["t"] == 3.0
    path = tmp_path / "cues.json"
    path.write_text(json.dumps(doc))
    cues, _ = tl.read_cues(str(path))
    assert cues[0]["t"] == 3.0


def own_texts() -> dict[str, str]:
    texts = {"mcp instructions": mcp_server.INSTRUCTIONS}
    texts.update({f"tool {t.name}": mcp_server.description(t.name) for t in mcp_server.TOOLS})
    texts["llm rubric"] = llm.build_prompt("ctx", question="q", synth_intended=True)
    texts["gallery pro texts"] = " ".join(gallery.PRO_TEXT.values())
    texts["gallery legend"] = " ".join(t for _, t in gallery_page.LEGEND + gallery_page.LEGEND_BENCH)
    return texts


def test_l_t5_no_user_preference_as_instruction():
    texts = own_texts()
    texts.update({str(p.relative_to(SKILLS)): p.read_text(encoding="utf-8") for p in doc_files()})
    bad = [(name, phrase) for name, text in texts.items() for phrase in PREFERENCE_PHRASES
           if phrase in " ".join(text.lower().split())]
    assert bad == []


def test_l_t5_phrase_list_catches_a_bad_sentence():
    text = "Use realness first, because the user rejects synthetic sound."
    assert any(p in text.lower() for p in PREFERENCE_PHRASES)


def test_doc_roots_point_at_skills():
    assert SKILLS.name == "skills" and os.path.isdir(SKILLS / "hark")
