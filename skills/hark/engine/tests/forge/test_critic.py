"""Acceptance tests C1-C10 of forge-core part 8 (critic), with stub listen services."""

import math

import numpy as np
import pytest
import soundfile as sf

from hark.forge.engine import critic

SR = 48000


def _ping(f, seed, sec=0.3):
    t = np.arange(int(sec * SR)) / SR
    r = np.random.default_rng(seed)
    y = np.sin(2 * np.pi * f * t) * np.exp(-t / 0.06) + 0.05 * r.standard_normal(len(t)) * np.exp(-t / 0.03)
    y *= np.minimum(1, t / 0.002)
    y[-480:] *= np.linspace(1, 0, 480)
    return 0.5 * y


def _refs(tmp_path, n):
    paths = []
    for i in range(n):
        p = tmp_path / f"ref{i}.wav"
        sf.write(str(p), _ping(900 + 40 * i, i), SR, subtype="PCM_24")
        paths.append(str(p))
    return paths


def test_c1_resolve_mode():
    assert critic.resolve_mode("auto", "a glitchy digital stutter", ["r.wav"]) == "designed"
    assert critic.resolve_mode("auto", "a wooden knock", []) == "designed"
    assert critic.resolve_mode("auto", "a wooden knock", ["r.wav"]) == "real"
    assert critic.resolve_mode("real", "a wooden knock", []) == "real"


ROW = {"match_z": 4, "alt_margin": 1, "p_real": 0.9, "ref_typ_z": 1, "feat_pen": 0.5, "tone_pen": 0.2,
       "env_pen": 0.3, "art_pen": 0, "polish_pen": 0.4}


def test_c2_total_v2():
    t, ok = critic.total_v2(ROW, mode="real")
    assert abs(t - 4.95) < 1e-9 and ok
    t, ok = critic.total_v2(ROW, mode="designed")
    assert abs(t - 3.35) < 1e-9 and ok
    assert critic.total_v2({**ROW, "p_real": 0.5}, mode="designed")[1]


def test_c3_hard_never_passes():
    assert not critic.total_v2({**ROW, "hard": ["clicks"]})[1]


def test_c4_study_penalties():
    assert critic.study_penalties({"length": 0.4}, {"length": (0.8, 3.0)})[0]["env"] == pytest.approx(1.0)
    assert critic.study_penalties({"length": 0.1}, {"length": (0.8, 3.0)})[0]["env"] == 1.5
    assert critic.study_penalties({"corr": 0.2}, {"corr": (0.8, 1.0)})[0]["polish"] == 1.5


def test_c5_widen_targets():
    assert critic.widen_targets({"t40": (100, 200)}, [{"t40": 50}, {"t40": 300}]) == {"t40": (50, 300)}


def test_c6_score_refs_loo(tmp_path, stub_listen):
    c = critic.Critic("a short bright ping", ["a dull thud"], refs=_refs(tmp_path, 4))
    rows = c.score_refs()
    assert [r["left_out"] for r in rows] == [f"ref{i}.wav" for i in range(4)]
    assert all(r["ref_typ_z"] is not None for r in rows)
    (tmp_path / "b").mkdir()
    two = critic.Critic("a short bright ping", ["a dull thud"], refs=_refs(tmp_path / "b", 2))
    assert all(r["ref_typ_z"] is None for r in two.score_refs())


def test_c7_score_refs_memo(tmp_path, stub_listen):
    c = critic.Critic("a short bright ping", ["a dull thud"], refs=_refs(tmp_path, 4))
    a = c.score_refs()
    n = stub_listen.embeds
    b = c.score_refs()
    assert stub_listen.embeds == n
    assert [r["total"] for r in a] == [r["total"] for r in b]
    b[0]["total"] = 123
    assert c.score_refs()[0]["total"] != 123


def test_c8_holdout_cache(tmp_path, stub_listen, monkeypatch):
    monkeypatch.setenv("HARK_HOME", str(tmp_path))
    monkeypatch.setattr(critic, "_HOLD", {})
    c = critic.Critic("a short bright ping")
    x = _ping(700, 1)
    a = c.holdout([x])
    assert stub_listen.tag_calls == 1
    monkeypatch.setattr(critic, "_HOLD", {})  # force the file cache
    b = c.holdout([x])
    assert stub_listen.tag_calls == 1 and a == b
    assert isinstance(b[0]["audioset_top"][0], tuple)


def test_c9_extra_term(tmp_path, stub_listen):
    refs = _refs(tmp_path, 3)
    xs = [_ping(1000, 7), _ping(800, 8)]
    base = critic.Critic("a short bright ping", ["a dull thud"], refs=refs).score(xs)
    more = critic.Critic("a short bright ping", ["a dull thud"], refs=refs, extra=lambda x, row: -1.0).score(xs)
    for a, b in zip(base, more):
        assert b["total"] == pytest.approx(a["total"] - 1.0) and b["extra"] == -1.0


def test_c10_study_only_for_thin_refs(tmp_path, stub_listen):
    one = critic.Critic("a deep cinematic impact", refs=_refs(tmp_path, 1), category="impact")
    assert all("study" in r for r in one.score([_ping(100, 3), _ping(120, 4)]))
    (tmp_path / "four").mkdir()
    four = critic.Critic("a deep cinematic impact", refs=_refs(tmp_path / "four", 4), category="impact")
    assert all("study" not in r for r in four.score([_ping(100, 3)]))


def test_silent_row(stub_listen):
    r = critic.Critic("a ping").score([np.zeros(4800)])[0]
    assert r["total"] == -math.inf and r["summary"] == "silent" and not r["passes"]
