"""Blind listening test: BT1-BT11, on a synthetic tree of finals and bench refs made in tmp_path."""

from __future__ import annotations

import json
import os
import subprocess
import sys

import numpy as np
import pytest
import soundfile as sf

from hark.forge.engine.eval import blind_sheet, blind_trim as bt, blind_v2
from hark.forge.engine.eval.blind_progress import verdict

SR = 48000


def _sound(seed: int, dur: float, f: float, noise: float) -> np.ndarray:
    """10 ms of digital silence, a 3 ms attack and a decay to about -70 dB at the end."""
    rng = np.random.default_rng(seed)
    n = int(dur * SR)
    t = np.arange(n) / SR
    tau = dur / 8.5
    body = (np.sin(2 * np.pi * f * t) + noise * rng.standard_normal(n)) * np.exp(-t / tau)
    att = int(0.003 * SR)
    body[:att] *= np.linspace(0, 1, att)
    body[-48:] *= np.linspace(1, 0, 48)
    x = np.concatenate([np.zeros(int(0.01 * SR)), body])
    return 0.5 * np.column_stack([x, 0.97 * x + 0.03 * np.roll(x, 3)])


def _clear_caches():
    bt.facts.cache_clear()
    bt.source_floor.cache_clear()
    bt._manifest_index.cache_clear()


@pytest.fixture
def tree(tmp_path, monkeypatch):
    """Finals (3 per category) and bench refs (3 per category) for every PLAN category."""
    finals, refs, cats = tmp_path / "forge_v2", tmp_path / "refs", {}
    for ci, (cat, *_rest) in enumerate(blind_v2.PLAN):
        (finals / cat / "final").mkdir(parents=True)
        (refs / cat / "bench").mkdir(parents=True)
        bench = []
        for k in range(3):
            dur = 0.2 + 0.03 * k + 0.01 * ci
            sf.write(str(finals / cat / "final" / f"{cat}_{k + 1:02d}.wav"),
                     _sound(100 * ci + k, dur, 500 + 90 * k, 0.2), SR, subtype="PCM_24")
            p = refs / cat / "bench" / f"{cat}_ref{k}.wav"
            sf.write(str(p), _sound(5000 + 100 * ci + k, dur * 1.05, 450 + 80 * k, 0.6), SR, subtype="PCM_24")
            bench.append(str(p))
        cats[cat] = {"forge": [], "bench": bench}
    (tmp_path / "refsets.json").write_text(json.dumps({"version": 1, "categories": cats}))
    monkeypatch.setenv("HARK_REFSETS", str(tmp_path / "refsets.json"))
    monkeypatch.setattr(blind_v2, "FINALS", finals)
    monkeypatch.setattr(blind_v2, "MANIFEST", tmp_path / "no_manifest.json")
    _clear_caches()
    yield tmp_path
    _clear_caches()


@pytest.fixture
def built(tree):
    out = tree / "blind"
    res = blind_v2.build_blind(out, 20, 2026, quiet=True)
    return out, res


# ---------------------------------------------------------------- picks

def test_BT1_allocate():
    a = blind_v2.allocate(20)
    assert sum(a.values()) == 20 and sorted(a.values()).count(2) == 6 and sorted(a.values()).count(1) == 8
    assert a == {c: t for c, _, t, _ in blind_v2.PLAN}
    assert set(blind_v2.allocate(14).values()) == {1} and len(blind_v2.allocate(14)) == 14


def test_BT2_pick_trials_deterministic(tree):
    a = blind_v2.pick_trials(20, 2026)
    b = blind_v2.pick_trials(20, 2026)
    assert [(t["made"], t["real"], t["real_is"]) for t in a] == [(t["made"], t["real"], t["real_is"]) for t in b]
    assert sum(1 for t in a if t["real_is"] == "A") == 10
    assert all(x["category"] != y["category"] for x, y in zip(a, a[1:]))
    digests = [blind_v2._digest(t["real"]) for t in a]
    assert len(set(digests)) == len(digests)


def test_BT3_answer_sheet_chance_line():
    assert blind_sheet.k05(20) == (15, pytest.approx(0.0207, abs=1e-4))
    sheet = blind_sheet.answer_sheet([], {"status": "PASS", "warnings": []}, 100.0, 20)
    assert "15 or more right is unlikely by luck (p = 0.021)" in " ".join(sheet.split())


# ---------------------------------------------------------------- trim and level

def test_BT5_match_pair():
    real, made = _sound(1, 0.3, 600, 0.6), 0.2 * _sound(2, 0.3, 900, 0.1)
    g, gc, over = bt.match_pair(real, made, "short")
    r, m = real * 10 ** (gc / 20), made * 10 ** ((g + gc) / 20)
    assert abs(bt.loud_max(r, 0.1) - bt.loud_max(m, 0.1)) < 0.01
    assert max(bt.true_peak(r), bt.true_peak(m)) <= -1.0 + 1e-6


def test_BT6_cut_struck_sine():
    t = np.arange(int(2.0 * SR)) / SR
    tau = 0.2
    x = np.sin(2 * np.pi * 440 * t) * np.exp(-t / tau)
    x = np.concatenate([np.zeros(int(0.05 * SR)), x])
    x = np.column_stack([x, x])
    info = bt.analyse(x, "long", bt.MADE_FADE_GUARD, None, cut_file=False)
    clip, f = bt.cut(x, info, -40.0, -40.0, False)
    assert f["onset_s"] == pytest.approx(0.05, abs=0.002)
    assert f["start_s"] == pytest.approx(f["onset_s"] - 0.004, abs=1e-4)
    assert np.all(np.abs(clip[:2]) < 1e-3)
    t40 = 0.05 + tau * np.log(100)  # power -40 dB = amplitude 1/100
    assert f["end_s"] == pytest.approx(t40, abs=0.05)
    assert f["fade_out_ms"] >= 5.0 and clip[-1, 0] == pytest.approx(0, abs=1e-6)


def test_BT7_pair_levels():
    loud = {"reach": 0.0, "head": 0.0, "floor": None}
    assert bt.pair_levels(loud, loud) == (-6.0, -6.0)
    quiet = {"reach": -80.0, "head": -90.0, "floor": None}
    assert bt.pair_levels(quiet, {"reach": -70.0, "head": None, "floor": None}) == (-50.0, -50.0)
    floor = {"reach": -80.0, "head": -90.0, "floor": -45.0}
    assert bt.pair_levels(floor, quiet) == (-35.0, -35.0)


# ---------------------------------------------------------------- build

def _keys_of(obj):
    if isinstance(obj, dict):
        yield from obj
        for v in obj.values():
            yield from _keys_of(v)
    elif isinstance(obj, list):
        for v in obj:
            yield from _keys_of(v)


def test_BT8_built_folder(built):
    out, res = built
    assert res["code"] == 0 and res["summary"]["status"] == "PASS", res["summary"]["fails"]
    for name in ("blind_test.wav", "answer_sheet.txt", "cleanliness.json", "KEY_open_after.json", "score_answers.py"):
        assert (out / name).is_file(), name
    assert len(list(out.glob("t*_A.wav"))) == 20 and len(list(out.glob("t*_B.wav"))) == 20
    key = json.loads((out / "KEY_open_after.json").read_text())
    reel = sf.read(str(out / "blind_test.wav"), always_2d=True)[0]
    mask = np.zeros(len(reel), dtype=bool)
    for t in key["trials"]:
        for side in "AB":
            i = int(round(t[f"{side}_start"] * SR))
            mask[i:i + int(round(t[f"{side}_length_s"] * SR)) + 1] = True
    assert np.all(reel[~mask] == 0)
    clean = json.loads((out / "cleanliness.json").read_text())
    assert not {"real", "made", "real_is", "real_file", "made_file"} & set(_keys_of(clean))


def test_BT9_written_files_hold_levels(built):
    v = built[1]["summary"]["verified_from_written_files"]
    assert v["max_loud_diff_lu"] <= 0.5 and v["max_true_peak_dbtp"] <= -0.95


def test_BT4_score_answers(built):
    out, _ = built
    key = json.loads((out / "KEY_open_after.json").read_text())
    right = [t["real"] for t in key["trials"]]
    wrong = ["B" if a == "A" else "A" for a in right]

    def run(args):
        return subprocess.run([sys.executable, str(out / "score_answers.py"), *args], capture_output=True, text=True,
                              cwd=str(out), env={k: v for k, v in os.environ.items() if k != "PYTHONPATH"})

    r = run(right)
    assert r.returncode == 0 and "20 of 20 right" in r.stdout and "p = 0.000 (9.5e-7)" in r.stdout
    w = run(wrong)
    assert "0 of 20 right" in w.stdout and "So few right is itself unlikely" in w.stdout
    q = run(["?"] + right[1:])
    assert "19 of 19 right" in q.stdout
    bad = run(right[:19])
    assert bad.returncode == 2 and bad.stdout.startswith("usage:")


def test_BT10_changed_input_stops_build(tree, monkeypatch):
    real_pair = blind_v2._pair
    calls = []

    def touching(t):
        if not calls:
            st = os.stat(t["made"])
            os.utime(t["made"], ns=(st.st_atime_ns, st.st_mtime_ns + 5_000_000_000))
        calls.append(1)
        return real_pair(t)

    monkeypatch.setattr(blind_v2, "_pair", touching)
    out = tree / "blind"
    with pytest.raises(SystemExit) as e:
        blind_v2.build_blind(out, 20, 2026, quiet=True)
    assert "input files changed while building" in str(e.value)
    assert not out.exists()


def test_dry_run_writes_nothing(tree, capsys):
    out = tree / "dry"
    res = blind_v2.build_blind(out, 20, 2026, dry_run=True)
    assert res["code"] == 0 and not out.exists() and capsys.readouterr().out.startswith("blind test PASS")


# ---------------------------------------------------------------- progress verdicts

def test_BT11_verdict():
    assert verdict("low", 10, 8, [9, 12], 0.3) == "better"
    assert verdict("low", 8, 7, [9, 12], 0.3) == "same (both within the real rows)"
    assert verdict("model", 2.0, 1.0, [0.5], 0.05) == "down (v1 sat above the real rows)"
    assert verdict("one", 2.0, 1.1, [0.9, 1.2], 0.03) == "better"
    assert verdict("span", 1, 2, [None], 0.1) == "" and verdict("low", None, 1, [1], 0.1) == ""
