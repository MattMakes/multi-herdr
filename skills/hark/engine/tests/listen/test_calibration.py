"""CAL-1 .. CAL-12: vocabulary file, windows, calibration math, verdicts and caches."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import numpy as np
import pytest

from hark.core import audio
from hark.listen import vocabulary as voc


def _stub_vocab(E, mean=None):
    """A Vocab object with hand-made term embeddings (no model)."""
    v = voc.Vocab.__new__(voc.Vocab)
    v.templates = ["{}"]
    v.categories = ["a", "b"]
    v.terms = [f"t{i}" for i in range(len(E))]
    v.term_category = ["a" if i % 2 == 0 else "b" for i in range(len(E))]
    v.E = np.asarray(E, np.float32)
    v.model_id = "stub"
    v.stats = None
    v.mean = None if mean is None else np.asarray(mean, np.float64)
    v.bE = v.bias(v.E)
    v.index = {t: i for i, t in enumerate(v.terms)}
    v._scale = 38.75
    return v


def test_cal_1_vocabulary_format():
    data = voc.read_vocab()
    assert voc.check_vocab(data) == []
    assert all(t.count("{}") == 1 for t in data["templates"])
    assert len(data["templates"]) == 5
    phrases = [p for v in data["categories"].values() for p in v]
    assert len(phrases) == len(set(phrases)) == 322
    assert set(voc.CATEGORY_NAMES) <= set(data["categories"])
    for name in voc.UNVALIDATED + voc.NOT_SOUNDS_LIKE:
        assert name in data["categories"]
    for q in data["categories"]["quality"]:
        assert q.startswith("a ") and q.endswith(" sound")


def test_cal_1_check_catches_problems():
    bad = {"templates": ["{} {}", "x"], "categories": {"ui": ["a", "a"]}}
    problems = voc.check_vocab(bad)
    assert any("exactly one" in p for p in problems)
    assert any("twice" in p for p in problems)
    assert any("missing" in p for p in problems)


def test_cal_2_statistics_match(tmp_path, fake_clap, write, sig):
    if voc.STATS_PATH.exists():
        stats = json.loads(voc.STATS_PATH.read_text())
        assert stats["model"] == voc.config.CLAP_MODEL
        assert len(stats["mean"]) == 512
        assert stats["templates"] == voc.read_vocab()["templates"]
    paths = [write(f"refs/x/a{i}.wav", sig.click(at=0.1 * (i + 1))) for i in range(3)]
    paths.append(write("refs/y/b.wav", sig.sine(dur=1.0)))
    out = tmp_path / "stats.json"
    stats = voc.build_stats([str(tmp_path / "refs")], out)
    loaded = json.loads(out.read_text())
    assert loaded["model"] == voc.config.CLAP_MODEL
    assert len(loaded["mean"]) == 512
    assert loaded["templates"] == voc.read_vocab()["templates"]
    assert set(loaded["term_cos"]) == set(voc.get().terms)
    assert loaded["n_ref"] == 4 and loaded["n_folders"] == 2
    text = out.read_text().splitlines()
    assert any(line.startswith('  "a fast whoosh passing by": [') for line in text)
    assert stats["logit_scale"] == 38.75


def test_cal_3_windows():
    assert voc.spans(0, 8) == [(0, 8)]
    s25 = voc.spans(0, 25)
    assert [round(a, 3) for a, _ in s25] == [0, 5, 10, 15] and s25[-1][1] == pytest.approx(25)
    assert [round(a, 3) for a, _ in voc.spans(0, 23)] == [0, 5, 10, 13]
    assert [round(a, 3) for a, _ in voc.spans(0, 100)] == [0, 20, 35, 55, 70, 90]
    assert [round(a, 3) for a, _ in voc.spans(30, 55)] == [30, 35, 40, 45]


def test_cal_4_folder_weights(tmp_path):
    paths = [str(tmp_path / "X" / f"{i}.wav") for i in range(3)] + [str(tmp_path / "Y" / "y.wav")]
    raw = voc.folder_weights(paths, normalise=False)
    assert raw.tolist() == pytest.approx([1 / 3, 1 / 3, 1 / 3, 1])
    w = voc.folder_weights(paths)
    assert w[:3].sum() == pytest.approx(w[3])
    assert w.sum() == pytest.approx(1.0)


def _four_terms():
    rng = np.random.default_rng(3)
    E = voc.unit(rng.standard_normal((4, 512)))
    mean = 0.2 * voc.unit(rng.standard_normal(512))
    a = voc.unit(rng.standard_normal(512))
    return E, mean, a


def test_cal_5_calibration_math():
    E, mean, a = _four_terms()
    v = _stub_vocab(E, mean)
    cal = v.calibrate(a[None, :], Q=E[[2]])
    z = cal.Z[0]
    assert z.mean() == pytest.approx(0, abs=1e-6)
    assert z.std() == pytest.approx(1, abs=1e-6)
    assert cal.z_q[0, 0] == pytest.approx(z[2])
    U = cal.U[0]
    assert cal.rank[0, 0] == 1 + int((U > U[2]).sum())
    assert cal.percentile[0, 0] == pytest.approx((U < U[2]).mean())
    best = voc.unit(a + 0.0)  # the sound's own direction beats every term
    top = v.calibrate(a[None, :], Q=best[None, :])
    assert top.percentile[0, 0] == pytest.approx(1.0)
    assert top.rank[0, 0] == 1


def test_cal_5_bias_is_q_dot_mean():
    E, mean, a = _four_terms()
    v = _stub_vocab(E, mean)
    cal = v.calibrate(a[None, :])
    assert cal.U[0] == pytest.approx(cal.C[0] - E @ mean)


def test_cal_6_raw_method():
    E, mean, a = _four_terms()
    v = _stub_vocab(E, mean)
    cal = v.calibrate(a[None, :], Q=E[[1]], method="raw")
    assert cal.Z[0] == pytest.approx(cal.C[0])
    assert cal.z_q[0, 0] == pytest.approx(cal.Cq[0, 0])


def test_cal_7_category_filter(fake_clap):
    v = voc.get()
    a = voc.unit(np.random.default_rng(5).standard_normal(512))
    kb = v.top_terms(a, k=3, cats=("keyboard",))
    assert len(kb) == 3 and all(c == "keyboard" for _, c, _, _ in kb)
    rest = v.top_terms(a, k=50, exclude=("quality", "production"))
    assert all(c not in ("quality", "production") for _, c, _, _ in rest)
    zs = [z for _, _, z, _ in rest]
    assert zs == sorted(zs, reverse=True)


def test_cal_8_verdict_bands():
    assert voc.verdict(3.0) == "strong"
    assert voc.verdict(2.999) == "plausible"
    assert voc.verdict(2.5) == "plausible"
    assert voc.verdict(2.0) == "weak"
    assert voc.verdict(1.99) == "no match"
    assert voc.verdict(-5) == "no match"
    t = list(voc.VERDICT_THRESHOLDS_Z.values())
    assert t == sorted(t, reverse=True) and len(set(t)) == len(t)


def test_cal_9_z_text():
    assert voc.zf(2.96) == "+2.9"
    assert voc.zf(3.0) == "+3.0"
    assert voc.zf(2.49) == "+2.4"
    assert voc.zf(-0.04) == "-0.1"
    for z in np.random.default_rng(0).uniform(-5, 8, 1000):
        assert voc.verdict(float(voc.zf(z))) == voc.verdict(z)


def test_cal_10_softmax():
    x = [0.31, 0.25, 0.12, 0.28]
    p = voc.softmax(x, 38.75)
    assert p.sum() == pytest.approx(1.0)
    assert ((p > 0) & (p < 1)).all()
    assert int(np.argmax(p)) == 0
    assert voc.softmax([v + 5 for v in x], 38.75) == pytest.approx(p)


def test_cal_11_range_errors(write, sig):
    path = write("one.wav", sig.sine(dur=1.0))
    with pytest.raises(ValueError, match="negative"):
        voc.check_range(path, -1)
    with pytest.raises(ValueError, match="past the end"):
        voc.check_range(path, 1.0)
    with pytest.raises(ValueError, match="end must be after start"):
        voc.check_range(path, 0.5, 0.5)
    assert voc.check_range(path, 0, 5) == pytest.approx(1.0)
    assert voc.clamp_range(1.0, 0, 5) == (0.0, 1.0)
    with pytest.raises(audio.AudioError):
        voc.check_range(str(Path(path).with_name("missing.wav")))


def test_cal_11_short_range_gets_one_span(write, sig, fake_clap):
    path = write("one.wav", sig.sine(dur=1.0))
    sp = voc.file_spans(path, 0.5, 0.503)
    assert len(sp) == 1 and sp[0][1] - sp[0][0] >= 0.01 - 1e-9
    e = voc.embed_file(path, 0.5, 0.503)
    assert e.shape == (512,) and np.linalg.norm(e) == pytest.approx(1, abs=1e-5)


def test_cal_12_text_cache_counts(fake_clap, listen_cache):
    templates = voc.read_vocab()["templates"]
    voc.embed_phrases([f"phrase {i}" for i in range(20)], templates)
    assert len(list(Path(listen_cache).rglob("tl_*.npy"))) == 1
    voc.embed_phrases(["one", "two", "three"], templates)
    assert len(list(Path(listen_cache).rglob("t_*.npy"))) == 3
    calls = fake_clap.text_calls
    voc.embed_phrases(["one", "two", "three"], templates)
    assert fake_clap.text_calls == calls


def test_cal_12_cache_survives_a_new_process(fake_clap, loads):
    voc.get().embed_text(["a brand new query"])
    assert loads.clap >= 1
    voc.reset()
    loads.clap = 0
    v = voc.get()
    v.embed_text(["a brand new query"])
    assert loads.clap == 0


def test_cal_12_bad_cache_file_is_recomputed(fake_clap, listen_cache):
    templates = ["{}"]
    first = voc.embed_phrases(["x"], templates)
    for f in Path(listen_cache).rglob("t_*.npy"):
        f.write_bytes(b"broken")
    again = voc.embed_phrases(["x"], templates)
    assert again == pytest.approx(first)


@pytest.mark.ml
def test_cal_12_text_cache_two_processes(tmp_path):
    run = "import hark.listen.vocabulary as v; v.get().embed_text(['a cache check query'])"
    guard = "import sys, hark.models.clap as c; c.get = lambda *a, **k: sys.exit(7); "
    env = {"HARK_HOME": str(tmp_path), "PYTHONPATH": str(Path(__file__).parents[2]), "HARK_OFFLINE": ""}
    first = subprocess.run([sys.executable, "-c", run], env=env, capture_output=True, text=True)
    second = subprocess.run([sys.executable, "-c", guard + run], env=env, capture_output=True, text=True)
    assert first.returncode == 0, first.stderr
    assert second.returncode == 0, "the second process loaded the CLAP model"


def test_vocabulary_cli_info(capsys):
    assert voc.main([]) == 0
    assert "322 terms in 16 categories" in capsys.readouterr().out


def test_scores_records(fake_clap):
    v = voc.get()
    a = voc.unit(np.random.default_rng(1).standard_normal(512))
    rec = v.scores(a[None, :], ["a glass ting", v.terms[0]])[0]
    assert set(rec[0]) == {"query", "cosine", "z", "percentile", "rank_in_vocab", "vocab_size"}
    assert rec[0]["vocab_size"] == 322
    cal = v.calibrate(a[None, :])
    assert rec[1]["z"] == pytest.approx(round(float(cal.Z[0, 0]), 2))


def test_padding_modes():
    short = np.ones(int(0.2 * voc.SR), np.float32)
    assert len(voc.prepare(short, "zero")) == int(voc.WIN_S * voc.SR)
    sp = voc.prepare(short, "spaced")
    assert len(sp) == int(voc.WIN_S * voc.SR)
    assert sp[int(1.0 * voc.SR)] == 1.0 and sp[int(0.5 * voc.SR)] == 0.0
    assert len(voc.prepare(short, "repeatpad")) == len(short)
    assert len(voc.prepare(np.ones(10, np.float32), "repeatpad")) == voc.MIN_SAMPLES
    assert voc.applied_pad(10.0, "spaced") == "-"
    assert voc.applied_pad(2.0, "spaced") == "repeatpad"
    assert voc.applied_pad(0.5, "spaced") == "spaced"
