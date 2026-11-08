"""DES-1 .. DES-10, MAT-1 .. MAT-9, RNK-1 .. RNK-6: describe, match and rank.

Tests marked [model] in the spec run here with the stub CLAP and AST models for structure. A second
`ml` variant checks what only the real model can hear, and skips without the ML extras.
"""

from __future__ import annotations

import json
import os
import time

import numpy as np
import pytest

from hark.listen import describe as des
from hark.listen import match as mat
from hark.listen import search
from hark.listen import vocabulary as voc

SR = 48000


def _long_signal(seed=0):
    rng = np.random.default_rng(seed)
    rain = 0.1 * rng.standard_normal(15 * SR)
    t = np.arange(15 * SR) / SR
    notes = 440 * 2 ** (np.floor(t * 2) % 5 / 12)
    melody = 0.3 * np.sin(2 * np.pi * np.cumsum(notes) / SR)
    clicks = np.zeros(15 * SR)
    for k in range(30):
        i = int(k * 0.5 * SR)
        n = np.arange(int(0.05 * SR))
        clicks[i:i + len(n)] += 0.7 * np.sin(2 * np.pi * 3000 * n / SR) * np.exp(-n / (0.008 * SR))
    return np.concatenate([rain, melody, clicks])


@pytest.fixture
def files(write, sig):
    return {
        "click": write("click.wav", sig.click(dur=1.0, at=0.2)),
        "sine": write("sine.wav", sig.sine(dur=2.0)),
        "silence": write("silence.wav", np.zeros(SR)),
    }


# ---------------------------------------------------------------- describe


def test_des_1_too_short(write, no_models):
    r = des.describe(write("short.wav", 0.5 * np.ones(int(0.003 * SR))))
    assert r["top_tags"] == [] and r["measurements"] == {"duration": 0.003}
    assert "at least 5 ms" in r["summary"]


def test_des_2_near_silence(files, write, sig, no_models):
    quiet = write("quiet.wav", sig.sine(dur=1.0, dbfs=-80))
    for p in (files["silence"], quiet):
        r = des.describe(p)
        assert r["top_tags"] == [] and r["audioset"] == [] and r["tags"] == {}
        assert "near-silence" in r["summary"] and "peak" in r["summary"]
    assert "-80" in des.describe(quiet)["summary"]


def test_des_3_shape_of_a_click(files, fake_clap, fake_ast):
    shape = des.describe(files["click"])["shape"]
    assert shape.startswith("short transient")
    assert "attack" in shape and "decays in" in shape


def test_des_4_shape_of_a_tone():
    m = {"attack_ms": 5, "decay_ms": None, "onset_count": 1, "onsets_per_s": 0.5, "f0_hz": 440.0, "flatness": 0.0}
    assert "tonal, pitch ~440 Hz (A4)" in des.shape_words(m, 2.0)
    m2 = {"onset_count": 0, "onsets_per_s": 0.0, "flatness": 0.5}
    assert des.shape_words(m2, 5.0) == "sustained, noise-like"
    long = {"onset_count": 2, "onsets_per_s": 0.05, "lra_lu": 3.4}
    assert des.shape_words(long, 40.0) == "steady, few onsets, loudness range 3 LU"


def test_des_4_tone_file(files, fake_clap, fake_ast):
    r = des.describe(files["sine"])
    if r["measurements"].get("f0_hz"):
        assert "tonal, pitch ~440 Hz (A4)" in r["shape"]


def test_des_5_hides_unmeasurable(files, write, sig, fake_clap, fake_ast):
    short = des.describe(write("half.wav", sig.click(dur=0.5, at=0.1)))
    assert "noise_floor_dbfs" not in short["measurements"]
    full = des.describe(files["click"])
    assert full["measurements"]["width"] == "mono"
    assert all(v is not None for v in full["measurements"].values())
    assert list(full["measurements"]) == [k for k in des.SHOWN if k in full["measurements"]]


def _check_structure(r, top):
    assert len(r["top_tags"]) <= top
    for t in r["top_tags"]:
        assert t["verdict"] == voc.verdict(t["z"])
        assert t["category"] not in voc.UNVALIDATED + voc.NOT_SOUNDS_LIKE
    assert [t["z"] for t in r["top_tags"]] == sorted((t["z"] for t in r["top_tags"]), reverse=True)
    assert len(r["tags"]) == 16 and all(len(v) <= 3 for v in r["tags"].values())
    assert all(h["category"] in voc.UNVALIDATED for h in r["music_hints"])


def test_des_6_field_structure(files, fake_clap, fake_ast):
    _check_structure(des.describe(files["click"], top=5), 5)


@pytest.mark.ml
def test_des_6_field_structure_model(files):
    _check_structure(des.describe(files["click"]), 8)


def _check_windows(r):
    wins = r["distinct_windows"]
    assert len(wins) == 0 or 2 <= len(wins) <= 3
    assert [w["start"] for w in wins] == sorted(w["start"] for w in wins)
    for a, b in zip(wins, wins[1:]):
        assert a["end"] <= b["start"] + 1e-6
    assert len({w["top_tag"]["term"] for w in wins}) == len(wins)
    assert "read differently" in r["summary"] or "niform" in r["summary"]


def test_des_7_long_file_windows(write, fake_clap, fake_ast):
    r = des.describe(write("long.wav", _long_signal()))
    _check_windows(r)
    assert len(r["distinct_windows"]) >= 2


@pytest.mark.ml
def test_des_7_long_file_windows_model(write):
    _check_windows(des.describe(write("long.wav", _long_signal())))


def test_des_7_distinct_windows_rules():
    spans = [(0, 10), (5, 15), (10, 20), (15, 25)]
    W = np.eye(4, 8)
    assert des.distinct_windows(spans, W, ["a", "b", "c", "d"]) in ([0, 2], [1, 3], [0, 3])
    assert des.distinct_windows(spans, W, ["a"] * 4) == []
    assert des.distinct_windows(spans[:1], W[:1], ["a"]) == []


def test_des_8_range(write, fake_clap, fake_ast):
    r = des.describe(write("long.wav", _long_signal()), start=30, end=40)
    assert r["duration"] == 10.0 and "distinct_windows" not in r


def test_des_9_note_names():
    assert [des.note_name(f) for f in (261.63, 27.5, 4186, 466.16, 440)] == ["C4", "A0", "C8", "Bb4", "A4"]


def test_des_10_character_words():
    quality = [{"term": "a deep sound", "z": 3.1}, {"term": "a warm sound", "z": 2.6},
               {"term": "a bright sound", "z": 2.7}]
    assert des.character_words(quality) == ["deep", "warm"]
    assert des.character_words([{"term": "a dull sound", "z": 2.4}]) == []


def test_describe_cli_text(files, fake_clap, fake_ast, no_require, run_cli):
    code, out, _ = run_cli(des, ["describe", files["click"]])
    assert code == 0 and "measurements:" in out.splitlines()[-1]
    code, out, _ = run_cli(des, ["describe", files["click"], "--json"])
    assert code == 0 and json.loads(out)["path"] == files["click"]


# ---------------------------------------------------------------- match


def test_mat_1_empty_query(files, no_require, run_cli, no_models):
    for q in ([], "  ", ["ok", ""]):
        with pytest.raises(ValueError):
            mat.match(files["click"], q)
    code, _, err = run_cli(mat, ["match", files["click"], "  "])
    assert code == 1 and err.strip().startswith("hark match: error:") and len(err.strip().splitlines()) == 1


def test_mat_2_single_query(files, fake_clap):
    r = mat.match(files["click"], "a mouse click")
    assert r["p_vs_alts"] is None and len(r["results"]) == 1
    for key in ("query", "cosine", "z", "percentile", "rank_in_vocab", "verdict", "validated"):
        assert r[key] == r["results"][0][key]
    assert r["verdict_thresholds_z"] == {"strong": 3.0, "plausible": 2.5, "weak": 2.0}
    assert set(r["verdict_evidence"]) == {"strong", "plausible", "weak", "no match"}
    assert "recall_at_or_above" not in r["verdict_evidence"]["no match"]
    assert len(r["closest_vocab"]) == 4


def test_mat_3_several_queries(files, fake_clap):
    r = mat.match(files["click"], ["a mouse click", "heavy rain on a roof", "a dog barking"])
    zs = [row["z"] for row in r["results"]]
    assert zs == sorted(zs, reverse=True)
    assert sum(row["p_vs_alts"] for row in r["results"]) == pytest.approx(1, abs=0.002)
    assert "Forced choice among the queries" in r["summary"]


def test_mat_4_alternatives(files, fake_clap):
    r = mat.match(files["click"], "a mouse click", alts=["a door slam", "a bird chirp"])
    assert len(r["alts"]) == 2
    assert r["p_vs_alts"] + sum(a["p"] for a in r["alts"]) == pytest.approx(1, abs=0.002)
    assert set(r["alts"][0]) == {"query", "p", "z", "cosine"}


def test_mat_5_rank_and_percentile_agree(files, fake_clap):
    q = "a tiny plastic clip snapping"
    r = mat.match(files["click"], q)
    v = voc.get()
    a = voc.embed_file(files["click"]).astype(np.float64)
    u_t = a @ v.E.astype(np.float64).T - v.bE
    qe = v.embed_text([q])[0].astype(np.float64)
    u_q = a @ qe - v.bias(qe[None])[0]
    T = len(v.terms)
    assert r["rank_in_vocab"] == 1 + int((u_t > u_q).sum())
    assert r["percentile"] == round(100 * (u_t < u_q).sum() / T, 1)
    assert abs(r["rank_in_vocab"] - 1 + r["percentile"] * T / 100 - T) <= 1


def test_mat_5_query_equal_to_term_ties(files, fake_clap):
    term = voc.get().terms[0]
    r = mat.match(files["click"], term)
    assert term not in r["summary"].split("It sounds most like:")[-1] or "It sounds most like" not in r["summary"]


def test_mat_6_near_silent_warning(write, sig, fake_clap):
    r = mat.match(write("quiet.wav", sig.sine(dur=1.0, dbfs=-75)), "a hum")
    assert r["summary"].startswith("Warning: the input is near-silent")
    assert r["peak_dbfs"] == pytest.approx(-75, abs=0.6) and "z" in r


def test_mat_7_unvalidated(files, fake_clap, monkeypatch):
    monkeypatch.setattr(voc.Vocab, "nearest_category", lambda self, q: "music_mood")
    r = mat.match(files["click"], "uplifting and hopeful music")
    assert r["validated"] is False and "unvalidated" in r["summary"]
    assert "held-out library" not in r["summary"]


@pytest.mark.ml
def test_mat_7_unvalidated_model(files):
    r = mat.match(files["click"], "uplifting and hopeful music")
    assert r["validated"] is False and "held-out library" not in r["summary"]


def test_mat_8_evidence_sentence():
    rows = [{"query": "a click", "verdict": "strong", "validated": True, "z": 3.4, "rank_in_vocab": 1,
             "vocab_size": 322, "cosine": 0.4, "p_vs_alts": None}]
    text = mat.build_summary("x.wav", None, None, rows, [], -6.0)
    assert "98%" in text and "61%" in text
    rows[0]["verdict"] = "no match"
    assert "98%" not in mat.build_summary("x.wav", None, None, rows, [], -6.0)


def test_mat_8_segment_text():
    assert mat.segment_text(1.0, 2.5) == " [1.000-2.500 s]"
    assert mat.segment_text(None, 2.5) == " [0.000-2.500 s]"
    assert mat.segment_text(1.5, None) == " [from 1.500 s]"
    assert mat.segment_text(0.0, None) == ""


@pytest.mark.ml
def test_mat_9_real_match():
    path = os.environ.get("HARK_EVAL_KEYPRESS")
    if not path:
        pytest.skip("needs-data: a real recording of one mechanical key press (HARK_EVAL_KEYPRESS)")
    assert mat.match(path, "a single mechanical keyboard key press")["z"] >= 2.5
    assert mat.match(path, "heavy rain on a roof")["z"] < 2


# ---------------------------------------------------------------- rank


def test_rnk_1_bad_path(tmp_path, no_models):
    from hark.core import audio

    with pytest.raises(audio.AudioError, match="missing_dir"):
        search.rank([str(tmp_path / "missing_dir")], "x")


def test_rnk_2_unreadable_file(tmp_path, write, sig, fake_clap):
    write("pack/click.wav", sig.click())
    (tmp_path / "pack" / "bad.wav").write_bytes(np.random.default_rng(0).bytes(4000))
    r = search.rank([str(tmp_path / "pack")], "a click")
    assert r["n_files"] == 1 and len(r["skipped"]) == 1
    assert os.path.basename(r["skipped"][0]["path"]) == "bad.wav" and r["skipped"][0]["error"]


def test_rnk_3_silence_last(tmp_path, write, sig, fake_clap):
    write("pack/click.wav", sig.click())
    write("pack/silence.wav", np.zeros(SR))
    r = search.rank([str(tmp_path / "pack")], "a click")
    last = r["results"][-1]
    assert last["name"] == "silence.wav" and last["near_silent"] and last["verdict"] == "no match"
    assert "1 near-silent" in r["summary"]


def test_rnk_4_order_and_top(tmp_path, write, sig, fake_clap):
    for i in range(4):
        write(f"pack/c{i}.wav", sig.click(freq=800 + 700 * i))
    write("pack/zz.wav", np.zeros(SR))
    r = search.rank([str(tmp_path / "pack")], "a click")
    res = r["results"]
    flags = [x["near_silent"] for x in res]
    assert flags == sorted(flags)
    live = [x["z"] for x in res if not x["near_silent"]]
    assert live == sorted(live, reverse=True)
    one = search.rank([str(tmp_path / "pack")], "a click", top=1)
    assert len(one["results"]) == 1 and one["n_files"] == 5
    assert one["summary"].split("Bands:")[1] == r["summary"].split("Bands:")[1]


def test_rnk_5_duplicate_names(tmp_path, write, sig, fake_clap, no_require, run_cli):
    write("a/hit.wav", sig.click())
    write("b/hit.wav", sig.click(freq=900))
    code, out, _ = run_cli(search, ["rank", "a hit", str(tmp_path / "a"), str(tmp_path / "b")])
    assert code == 0 and "a/hit.wav" in out and "b/hit.wav" in out


def test_rnk_6_cached_rerank(tmp_path, write, sig, fake_clap):
    for i in range(3):
        write(f"pack/c{i}.wav", sig.click(dur=3.0, freq=900 + 400 * i))
    folder = str(tmp_path / "pack")
    search.rank([folder], "a click")
    windows = fake_clap.audio_windows
    assert windows > 0
    search.rank([folder], "a door knock")
    assert fake_clap.audio_windows == windows  # every span came from the cache


@pytest.mark.ml
def test_rnk_6_cached_rerank_speed(tmp_path, write, sig):
    for i in range(3):
        write(f"pack/c{i}.wav", sig.click(dur=3.0, freq=900 + 400 * i))
    folder = str(tmp_path / "pack")
    t0 = time.perf_counter()
    search.rank([folder], "a click")
    cold = time.perf_counter() - t0
    t1 = time.perf_counter()
    search.rank([folder], "a door knock")
    assert (time.perf_counter() - t1) * 5 <= cold
