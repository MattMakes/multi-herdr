"""Acceptance tests Q-1 .. Q-9 of fit-quality.md, plus the optional model checks."""

from __future__ import annotations

import numpy as np
import pytest

from hark.fit.pipeline import quality as Q

SR = 48000


def _ring(dur: float = 0.3, f: float = 1000.0, tau: float = 0.08) -> np.ndarray:
    t = np.arange(int(dur * SR)) / SR
    y = 0.5 * np.exp(-t / tau) * np.sin(2 * np.pi * f * t)
    y[:24] += 0.4 * np.random.default_rng(0).standard_normal(24)
    return y


def test_q_1_self_distance_is_zero():
    x = _ring()
    q = Q.quality(x, x)
    assert q["mel_db"] == 0
    assert q["env_corr"] == pytest.approx(1.0, abs=1e-4)
    assert q["onset_err"] == 0
    assert q["tone_db"] == 0
    assert q["excess_db"] == 0 and q["missing_db"] == 0
    assert q["level_db"] == 0
    assert Q.score(q) < 1e-6


def test_q_2_scaled_copy_is_levelled():
    x = _ring()
    q = Q.quality(x, 0.5 * x)
    assert q["mel_db"] == pytest.approx(0, abs=1e-6)
    assert q["env_corr"] == pytest.approx(1.0, abs=1e-4)
    assert q["onset_err"] == pytest.approx(0, abs=1e-3)
    assert q["tone_db"] == pytest.approx(0, abs=1e-6)
    assert q["level_db"] == pytest.approx(-6.02, abs=0.01)


def test_q_3_delay_counts():
    x = _ring()
    late = np.concatenate([np.zeros(int(0.02 * SR)), x])
    q0, q = Q.quality(x, x), Q.quality(x, late)
    assert q["mel_db"] > q0["mel_db"]
    assert Q.score(q) > Q.score(q0)


def test_q_4_excess_and_missing():
    t = np.arange(int(0.5 * SR)) / SR
    tone = 0.5 * np.exp(-t / 0.05) * np.sin(2 * np.pi * 700 * t)
    noise = np.random.default_rng(1).standard_normal(len(t)) * 0.5 * 10 ** (-40 / 20)
    q = Q.quality(tone, tone + noise)
    assert q["excess_db"] > 0 and q["missing_db"] == 0
    r = Q.quality(tone + noise, tone)
    assert r["missing_db"] > 0 and r["excess_db"] == 0


def test_q_5_stereo_against_mono_sum():
    rng = np.random.default_rng(2)
    n = int(0.6 * SR)
    env = np.exp(-np.arange(n) / (0.1 * SR))
    st = np.column_stack([rng.standard_normal(n) * env, rng.standard_normal(n) * env]) * 0.3
    mono = st.mean(axis=1)
    q = Q.quality(st, np.column_stack([mono, mono]))
    assert q["side_db"][1] == -40
    assert q["side_db"][0] > -20
    assert q["width_db"] > 3
    assert q["tail_corr"][1] == pytest.approx(1.0, abs=1e-3)
    assert q["tail_corr_err"] > 0.3


def test_q_6_mono_pair_has_no_stereo_terms():
    x = _ring()
    q = Q.quality(x, _ring(tau=0.1))
    assert q["side_db"] == (-40.0, -40.0)
    assert q["width_db"] == 0
    assert q["tail_corr"] == (None, None)
    assert q["tail_corr_err"] == 0


def test_q_7_fade_in_lowers_sharpness():
    t = np.arange(int(0.3 * SR)) / SR
    step = np.concatenate([np.zeros(2400), 0.5 * np.exp(-t / 0.1) * np.sin(2 * np.pi * 500 * t)])
    fade = step.copy()
    k = int(0.005 * SR)
    fade[2400: 2400 + k] *= np.linspace(0, 1, k)
    q = Q.quality(step, fade)
    assert q["sharp_db_ms"][1] < q["sharp_db_ms"][0]
    assert q["onset_err"] > 1


def test_q_8_score_formula():
    q = {"mel_db": 2.0, "tone_db": 3.0, "env_corr": 0.9, "onset_err": 5.0}
    assert Q.score(q) == pytest.approx(2.0 + 0.9 + 8 * 0.1 + 2.0)
    assert Q.score(dict(q, clap_cos=0.9)) - Q.score(q) == pytest.approx(2.0)


def test_q_9_floor_uses_seeds_11_and_12():
    calls = []

    def fn(s: int) -> np.ndarray:
        calls.append(s)
        return np.random.default_rng(s).standard_normal(SR // 4) * 0.1

    fl = Q.floor(fn)
    assert calls == [11, 12]
    assert fl["mel_db"] > 0


def test_quality_fields_and_text():
    x = _ring()
    q = Q.quality(x, _ring(tau=0.1))
    for key in ("mel_db", "mel_256", "mel_1024", "mel_4096", "env_corr", "attack_ms", "sharp_db_ms", "onset_err",
                "tone_db", "excess_db", "missing_db", "t40_ms", "level_db", "len_s", "side_db", "width_db",
                "tail_corr", "tail_corr_err"):
        assert key in q
    line = Q.text(q, Q.quality(x, x))
    assert "mel" in line and "\n" not in line


def test_pair_png_writes_a_picture(tmp_path):
    x = _ring()
    path = Q.pair_png(str(tmp_path / "p.png"), x, _ring(tau=0.1), title="t", q=Q.quality(x, x),
                      extra=("third", _ring(f=800)))
    assert open(path, "rb").read(8) == b"\x89PNG\r\n\x1a\n"


@pytest.mark.ml
def test_clap_cos_of_a_sound_with_itself():
    x = _ring()
    assert Q.quality(x, x, clap=True)["clap_cos"] == pytest.approx(1.0, abs=1e-3)


@pytest.mark.ml
def test_describe_pair_fields():
    x = _ring()
    d = Q.quality(x, x, describe=True)["describe"]
    assert set(d) == {"original", "resynth"}
    assert {"tag", "z", "audioset", "p", "tags3"} <= set(d["original"])
