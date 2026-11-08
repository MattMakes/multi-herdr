"""Acceptance tests G-T1 .. G-T20 of the gallery (surfaces-gallery.md G5).

Every fixture is a synthetic WAV written under tmp_path. The forge reference-set module (unit forge-core) is
replaced by a fake when it is absent; when it is present, the test sets its kit-bench stems to table G4.4.
"""

from __future__ import annotations

import json
import math
import os
import re
import shutil
import time
from pathlib import Path

import numpy as np
import pytest
import soundfile as sf

from hark.surfaces import gallery as g
from hark.surfaces import gallery_inputs as gi
from hark.surfaces import gallery_page as gp
from hark.surfaces import gallery_sound as gs

KIT_BENCH = ["ui_notify_02", "whoosh_med_01", "whoosh_med_02", "whoosh_big_02", "swipe_01", "riser_02",
             "impact_03", "shimmer_01"]
K1 = ([1.53512485958697, -2.69169618940638, 1.19839281085285], [1.0, -1.69065929318241, 0.73248077421585])
K2 = ([1.0, -2.0, 1.0], [1.0, -1.99004745483398, 0.99007225036621])


def sine(freq=800.0, dur=0.3, amp=0.3, sr=48000, decay=None) -> np.ndarray:
    t = np.arange(int(round(dur * sr))) / sr
    x = amp * np.sin(2 * np.pi * freq * t)
    return x * np.exp(-t / decay) if decay else x


def write(path: Path, x: np.ndarray, sr: int = 48000) -> str:
    path.parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(path), np.asarray(x, dtype=np.float32), sr, subtype="PCM_24")
    return str(path)


def build(tmp_path: Path, **kw) -> tuple[dict, str]:
    out = str(tmp_path / "site" / "gallery.html")
    res = g.gallery(out=out, **kw)
    return res, Path(out).read_text(encoding="utf-8")


def article(page: str, cat: str) -> str:
    m = re.search(rf'<article class="card" id="{re.escape(cat)}">(.*?)</article>', page, re.S)
    assert m, f"no card {cat}"
    return m.group(1)


@pytest.fixture
def tree(tmp_path) -> Path:
    """out/click/final/{click_01,click_02}.wav, out/pop/final/pop_01.wav, out/_judge/final/x.wav."""
    out = tmp_path / "out"
    write(out / "click/final/click_01.wav", sine(2000, 0.08, decay=0.02))
    write(out / "click/final/click_02.wav", sine(2400, 0.1, decay=0.02))
    write(out / "pop/final/pop_01.wav", sine(600, 0.25, decay=0.08))
    write(out / "_judge/final/x.wav", sine(300, 0.2))
    return out


@pytest.fixture
def refsets(fake_module, monkeypatch):
    """The forge reference-set module with the G4.4 kit-bench stems and an empty registry."""
    try:
        import hark.forge.engine.refsets as mod  # noqa: F401
    except ImportError:
        mod = None
    if mod is None:
        def held_out(paths, kit=True, path=None):
            return [p for p in paths if os.path.splitext(os.path.basename(p))[0] in KIT_BENCH]

        def load(path=None):
            raise FileNotFoundError("no registry")

        return fake_module("hark.forge.engine.refsets", held_out=held_out, load=load, KIT_BENCH=list(KIT_BENCH),
                           CATEGORIES={"click": "a short UI click"},
                           refset=lambda cat, split="forge", path=None: [])
    monkeypatch.setattr(mod, "KIT_BENCH", list(KIT_BENCH), raising=False)
    return mod


# ---------------------------------------------------------------- loudness


def by_hand_lufs(x: np.ndarray) -> float:
    from scipy.signal import lfilter
    y = lfilter(*K2, lfilter(*K1, x.astype(np.float64)))
    p = y ** 2
    n = 19200
    if len(p) < n:
        return -0.691 + 10 * math.log10(p.sum() / n + 1e-24)
    c = np.concatenate([[0.0], np.cumsum(p)])
    return -0.691 + 10 * math.log10((c[n:] - c[:-n]).max() / n + 1e-24)


def test_g_t1_lufs_and_measure(tmp_path):
    x = sine(997.0, 1.0, amp=0.1)
    assert abs(gs.lufs_max(x, 48000) - by_hand_lufs(x)) <= 0.2
    m = gs.measure(write(tmp_path / "s.wav", x))
    assert m["dur_s"] == 1.0 and m["peak_dbfs"] == -20.0 and m["channels"] == 1


def test_g_t2_short_sound_zero_padded():
    short = gs.lufs_max(sine(997.0, 0.1, amp=0.1), 48000)
    long = gs.lufs_max(sine(997.0, 0.4, amp=0.1), 48000)
    assert short < long
    assert abs(short - by_hand_lufs(sine(997.0, 0.1, amp=0.1))) <= 0.1


def test_g_t3_resampled_to_48k():
    a = gs.lufs_max(sine(997.0, 1.0, amp=0.1, sr=44100), 44100)
    b = gs.lufs_max(sine(997.0, 1.0, amp=0.1), 48000)
    assert abs(a - b) <= 0.1
    assert gs.lufs_max(np.zeros((0, 1)), 48000) is None


# ---------------------------------------------------------------- inputs


def test_g_t4_discover_and_order(tree, tmp_path):
    found = gi.discover(str(tree))
    assert sorted(found) == ["click", "pop"]
    assert g.card_order(["pop", "click"]) == ["click", "pop"]
    assert g.family_of("click") == "ui" and g.family_of("pop") == "tonal"
    _, page = build(tmp_path, folder=str(tree))
    assert page.index('id="fam-ui"') < page.index('id="fam-tonal"')
    assert page.index('id="click"') < page.index('id="pop"') and 'id="_judge"' not in page


def test_family_rules():
    assert g.family_of("pro_whoosh_fast") == "whoosh"
    assert g.family_of("big_boom") == "impact" and g.family_of("door_creak") == "other"
    assert g.card_order(["zeta", "alpha", "swipe", "whoosh_fast"]) == ["whoosh_fast", "swipe", "alpha", "zeta"]
    assert g.display_name("pro_logo_sting") == "Logo sting"


def test_g_t5_final_folder_takes_parent(tmp_path):
    write(tmp_path / "out/click/final_r2/a.wav", sine())
    assert list(gi.discover(str(tmp_path / "out/click/final_r2"))) == ["click"]


def test_discover_errors(tmp_path):
    with pytest.raises(FileNotFoundError):
        gi.discover(str(tmp_path / "missing"))
    (tmp_path / "empty").mkdir()
    with pytest.raises(FileNotFoundError, match=r"<category>/final/\*\.wav"):
        gi.discover(str(tmp_path / "empty"))
    with pytest.raises(ValueError, match="folder or --compare"):
        g.gallery(out=str(tmp_path / "x.html"))


def test_g_t6_drive_letter_is_no_label(tmp_path):
    with pytest.raises(ValueError, match="LABEL:GLOB"):
        g.gallery(compare=["D:/x"], out=str(tmp_path / "x.html"))
    with pytest.raises(ValueError, match="OLD=NEW"):
        gi.parse_alias(["broken"])


def test_g_t7_compare_with_alias(tmp_path, monkeypatch):
    write(tmp_path / "old/keypress/final/keypress_01.wav", sine(3000, 0.05, decay=0.01))
    write(tmp_path / "new/typing_key/final/typing_key_01.wav", sine(3200, 0.06, decay=0.01))
    monkeypatch.chdir(tmp_path)
    res, page = build(tmp_path, compare=["v1:old/*/final", "v2:new/*/final"], alias=["keypress=typing_key"])
    card = article(page, "typing_key")
    assert "keypress_01" in card and "typing_key_01" in card
    assert 'id="keypress"' not in page
    assert page.count('href="#typing_key"') == 1
    assert res["versions"] == {"v1": 1, "v2": 1} and res["categories"] == 1


def test_g_t8_identical_file_marked_and_embedded_once(tmp_path, monkeypatch):
    write(tmp_path / "v1/click/final/click_01.wav", sine(2000, 0.08, decay=0.02))
    write(tmp_path / "v1/click/final/click_02.wav", sine(2500, 0.08, decay=0.02))
    write(tmp_path / "v2/click/final/click_03.wav", sine(2700, 0.09, decay=0.02))
    shutil.copy(tmp_path / "v1/click/final/click_01.wav", tmp_path / "v2/click/final/click_01.wav")
    monkeypatch.chdir(tmp_path)
    res, page = build(tmp_path, compare=["v1:v1/*/final", "v2:v2/*/final"], embed=True)
    assert '<span class="pill same" title="same file as in that version">= v1</span>' in page
    assert page.count("data-src-of=") == 1
    assert res["embedded"] is True and res["codec"] == "flac"
    sources = re.findall(r'<audio[^>]*\bsrc="(data:audio/flac;base64,[^"]+)"', page)
    assert len(sources) == 3 and len(set(sources)) == 3


def test_g_t9_variant_alignment():
    def sounds(*variants):
        return [{"variant": v, "name": v} for v in variants]

    orders = gi.align([sounds("crisp", "soft", "deep"), sounds("deep", "crisp", "new")])
    assert [s["variant"] for s in orders[0]] == ["crisp", "soft", "deep"]
    assert [s["variant"] if s else None for s in orders[1]] == ["crisp", "new", "deep"]
    assert gi.align([None, sounds("a")])[0] is None


# ---------------------------------------------------------------- benchmark results


def _stamp(ts: float) -> str:
    return time.strftime("%Y-%m-%d %H:%M", time.localtime(ts))


def _results(tmp_path: Path, made: list[dict], created: str) -> str:
    path = tmp_path / "results.json"
    path.write_text(json.dumps({"tag": "bench1", "created": created, "categories": {
        "click": {"made": made, "summary": {"made": {"var_cos": 0.81, "n": len(made)}, "real_cross": {},
                                            "real_loo": {}}, "inside": {"per_sound": [3, 4], "n_checks": 5},
                  "n_bench": 4, "n_forge": 6}}}))
    return str(path)


def test_g_t10_rename_match(tmp_path):
    path = write(tmp_path / "out/click/final/click_01.wav", sine(2000, 0.2, amp=0.5))
    created = _stamp(time.time() + 86400)
    made = [{"file": "click_01.wav", "length_s": 0.25, "tone_db": 9.9, "t40_ratio": 7.77,
             "artifact_detail": {"peak_dbfs": -6.0}},
            {"file": "click_old.wav", "length_s": 0.2005, "tone_db": 3.3, "t40_ratio": 1.25,
             "artifact_detail": {"peak_dbfs": -6.1}}]
    res, page = build(tmp_path, folder=str(tmp_path / "out"), results=_results(tmp_path, made, created))
    card = article(page, "click")
    assert "3.3 dB" in card and "1.25 x" in card and "9.9 dB" not in card
    notes = [w for w in res["warnings"] if "renamed" in w]
    assert notes and "click_01.wav" in notes[0] and "click_old.wav" in notes[0]
    assert os.path.exists(path)


def test_g_t11_file_newer_than_run(tmp_path):
    path = write(tmp_path / "out/click/final/click_01.wav", sine(2000, 0.2, amp=0.5))
    created = _stamp(os.path.getmtime(path) - 3600)
    made = [{"file": "click_01.wav", "length_s": 0.2, "tone_db": 4.4, "artifact_detail": {"peak_dbfs": -6.0}}]
    res, page = build(tmp_path, folder=str(tmp_path / "out"), results=_results(tmp_path, made, created))
    card = article(page, "click")
    assert "benchmark numbers withheld" in card and "4.4 dB" not in card
    assert any("changed after the run" in w and "file newer" in w for w in res["warnings"])


def test_stale_reason_rules(tmp_path):
    sound = {"path": write(tmp_path / "a.wav", sine()), "dur_s": 0.3, "peak_dbfs": -10.5}
    future = _stamp(time.time() + 86400)
    assert gi.stale_reason(None, sound, future) == ""
    assert gi.stale_reason({"length_s": 0.35}, sound, future) == "length 0.300 s, benchmark 0.350 s"
    assert gi.stale_reason({"artifact_detail": {"peak_dbfs": -9.0}}, sound, future) == "peak -10.5 dBFS, benchmark -9.0"


def test_results_errors(tmp_path, tree):
    bad = tmp_path / "bad.json"
    bad.write_text("[]")
    with pytest.raises(ValueError, match="bad.json"):
        g.gallery(str(tree), out=str(tmp_path / "x.html"), results=str(bad))
    good = _results(tmp_path, [], "2020-01-01 00:00")
    with pytest.raises(ValueError, match="final"):
        g.gallery(str(tree), out=str(tmp_path / "x.html"), results=f"nolabel:{good}")


# ---------------------------------------------------------------- references


def test_g_t12_refs_bench_refused(tree, tmp_path):
    out = tmp_path / "site" / "g.html"
    with pytest.raises(ValueError, match="bench"):
        g.gallery(str(tree), out=str(out), refs="bench")
    assert not out.exists() and not (tmp_path / "site").exists()


def test_g_t13_held_out_reference_dropped(tree, tmp_path, refsets):
    write(tmp_path / "refs/click/forge/click_ref_a.wav", sine(1800, 0.1, decay=0.02))
    write(tmp_path / "refs/click/forge/impact_03.wav", sine(90, 0.3, decay=0.1))
    res, page = build(tmp_path, folder=str(tree), refs=str(tmp_path / "refs"))
    card = article(page, "click")
    assert card.count('class="tile ref"') == 1 and "click_ref_a" in card and "impact_03" not in card
    assert any(w == "click: 1 held-out reference(s) dropped" for w in res["warnings"])
    assert res["refs"] >= 1


def test_guard_unavailable_drops_refs(tree, tmp_path, fake_module):
    def broken(paths, kit=True, path=None):
        raise RuntimeError("guard down")

    fake_module("hark.forge.engine.refsets", held_out=broken, KIT_BENCH=[], CATEGORIES={})
    write(tmp_path / "refs/click/forge/click_ref_a.wav", sine(1800, 0.1, decay=0.02))
    res, page = build(tmp_path, folder=str(tree), refs=str(tmp_path / "refs"))
    assert 'class="tile ref"' not in article(page, "click")
    assert any("held-out check could not run" in w for w in res["warnings"])


def test_g_t14_audition_without_index(tree, tmp_path, refsets):
    write(tree / "click/final/audition.wav", sine(1000, 0.5))
    write(tmp_path / "refs/click/forge/click_ref_a.wav", sine(1800, 0.1, decay=0.02))
    res, page = build(tmp_path, folder=str(tree), refs=str(tmp_path / "refs"))
    assert "audition (made" not in article(page, "click")
    assert any("audition" in w and "no index" in w for w in res["warnings"])
    assert "audition" not in " ".join(re.findall(r'class="name"[^>]*>([^<]*)', page))


def test_audition_with_clean_index(tree, tmp_path, refsets):
    write(tree / "click/final/audition.wav", sine(1000, 0.5))
    (tree / "click/final/audition.txt").write_text("0.0 s ref click_ref_a.wav\n1.2 s ref other.wav\n")
    write(tmp_path / "refs/click/forge/click_ref_a.wav", sine(1800, 0.1, decay=0.02))
    _, page = build(tmp_path, folder=str(tree), refs=str(tmp_path / "refs"))
    assert "audition (made, nearest ref, alternating)" in article(page, "click")


# ---------------------------------------------------------------- loudness switch


def test_g_t15_loudness_target():
    t = gp.loudness_target([-12.0, -18.0, -24.0])
    assert t == -22.8
    assert gp.loudness_target([-40.0, -41.0]) == -30.0 and gp.loudness_target([-5.0]) == -14.0
    assert gp.loudness_target([]) is None
    assert math.isclose(min(1, 10 ** ((t - -12.0) / 20)), 10 ** (-10.8 / 20))
    assert min(1, 10 ** ((t - -24.0) / 20)) == 1
    assert "Math.min(1,Math.pow(10,(T-l)/20))" in gp.SCRIPT


def test_g_t15_page_switch(tree, tmp_path):
    _, page = build(tmp_path, folder=str(tree))
    lufs = [float(v) for v in re.findall(r'data-lufs="(-?[0-9.]+)"', page)]
    assert len(lufs) == 3
    target = re.search(r'id="match"[^>]*data-target="(-?[0-9.]+)"', page)
    assert target and float(target.group(1)) == gp.loudness_target(lufs)
    assert re.search(r'<input[^>]*id="match"[^>]*checked', page)


# ---------------------------------------------------------------- embedding


def test_g_t16_embed_compressed(tree, tmp_path):
    if shutil.which("ffmpeg") is None:
        pytest.skip("needs ffmpeg on PATH")
    (tree / "click/final/report.md").write_text("# report")
    res, page = build(tmp_path, folder=str(tree), compressed=True)
    assert res["codec"] == "mp3 192 kbps" and res["embedded"] is True
    sources = re.findall(r'<audio[^>]*\bsrc="([^"]+)"', page)
    assert sources and all(s.startswith("data:audio/mpeg;base64,") for s in sources)
    assert "report.md" not in page
    enc = tmp_path / "site" / "gallery_files" / "enc"
    assert len(list(enc.glob("*.mp3"))) >= 2
    again, _ = build(tmp_path, folder=str(tree), compressed=True)
    assert again["encoded"] == 0


def test_report_link_when_not_embedded(tree, tmp_path):
    (tree / "click/final/report.md").write_text("# report")
    _, page = build(tmp_path, folder=str(tree))
    assert "report.md" in article(page, "click") and "report.md" not in article(page, "pop")


def test_g_t17_prune_after_category_removed(tree, tmp_path):
    first, _ = build(tmp_path, folder=str(tree))
    assert first["images"]["made"] == 3
    shutil.rmtree(tree / "pop")
    second, _ = build(tmp_path, folder=str(tree))
    assert second["images"]["pruned"] >= 1 and second["images"]["reused"] == 2
    assets = Path(second["assets"])
    assert not [p for p in assets.glob("*.png") if "_pop_" in p.name]


def test_g_t18_kbps_out_of_range(tree, tmp_path):
    with pytest.raises(ValueError, match="16 to 320"):
        g.gallery(str(tree), out=str(tmp_path / "x.html"), compressed=True, kbps=12)
    with pytest.raises(ValueError, match="lossy"):
        g.gallery(str(tree), out=str(tmp_path / "x.html"), kbps=128)
    with pytest.raises(ValueError, match="codecs"):
        g.gallery(str(tree), out=str(tmp_path / "x.html"), embed=True, codec="aiff")


def test_g_t19_no_outside_resources(tree, tmp_path):
    for kw in ({}, {"embed": True}):
        _, page = build(tmp_path, folder=str(tree), **kw)
        assert "<script src" not in page and "<link" not in page
        for src in re.findall(r'\bsrc="([^"]*)"', page):
            assert not src.startswith("http")
        assert page.startswith("<!doctype html>") and 'lang="en"' in page


def test_g_t20_card_span():
    assert gs.card_span([2.0] + [0.1] * 5) == pytest.approx(0.25)
    assert gs.card_span([]) == 1.0 and gs.card_span([None, 0]) == 1.0
    assert gs.card_span([0.01]) == 0.05


# ---------------------------------------------------------------- page details


def test_summary_text_and_result_keys(tree, tmp_path):
    res, _ = build(tmp_path, folder=str(tree))
    keys = {"html", "assets", "url", "versions", "categories", "sounds", "refs", "images", "embedded", "codec",
            "size_mb", "critic_pass", "seconds", "warnings", "text"}
    assert keys <= set(res)
    lines = res["text"].splitlines()
    assert lines[0].startswith("gallery: 2 categories, 3 sounds -> ")
    assert lines[1] == "  versions: final (2)" and lines[-1].startswith("  open: file://")
    assert res["url"].startswith("file://") and "\\" not in res["html"]


def test_thumbnail_png(tmp_path):
    src = write(tmp_path / "a.wav", sine(1000, 0.3, decay=0.1))
    out = tmp_path / "thumb.png"
    assert gs.write_thumb(src, str(out), 0.3) is True
    from PIL import Image
    with Image.open(out) as im:
        assert im.size == (520, 132)
    name = gs.thumb_name("v 1", "click", src, 0.3)
    assert re.fullmatch(r"v-1_click_a_[0-9a-f]{8}\.png", name)


def test_param_text_formats():
    assert gp.param_text(True) == "yes" and gp.param_text(3) == "3"
    assert gp.param_text(1234.5) == "1234" or gp.param_text(1234.5) == "1235"
    assert gp.param_text(0.123456) == "0.1235"
    assert gp.param_text(list(range(10))) == "0, 1, 2, 3, 4, 5, 6, 7 ..."


def test_report_data_on_tile(tmp_path):
    folder = tmp_path / "out/click/final"
    write(folder / "click_01.wav", sine(2000, 0.1, decay=0.02))
    (folder / "report.json").write_text(json.dumps({
        "target": "a crisp UI click", "mode": "designed", "recipe": "click",
        "picks": [{"file": "click_01.wav", "member": "crisp", "params": {"freq": 2000.0, "snap": True}, "seed": 7,
                   "score": {"total": 0.8765, "passes": True, "tone_db": 2.5}, "measured": {"t40_ms": 45.0}}]}))
    _, page = build(tmp_path, folder=str(tmp_path / "out"))
    card = article(page, "click")
    assert 'critic target "a crisp UI click"' in card and "designed mode" in card
    assert ">pass<" in card and "0.88" in card and "45 ms" in card
    assert "Recipe and parameters (1 sounds)" in card and "freq" in card and ">yes<" in card
