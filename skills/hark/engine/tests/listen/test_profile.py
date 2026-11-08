"""PRO-1 .. PRO-11: profile and per-film metrics."""

from __future__ import annotations

import json
import os

import numpy as np
import pytest

from hark.listen import profile as pr

SR = 48000
HITS = (5.0, 10.0, 15.0, 20.0, 25.0)


def _pink(n, seed=0):
    rng = np.random.default_rng(seed)
    spec = np.fft.rfft(rng.standard_normal(n))
    f = np.arange(len(spec))
    spec[1:] /= np.sqrt(f[1:])
    spec[0] = 0
    x = np.fft.irfft(spec, n)
    return x / np.sqrt(np.mean(x ** 2))


def _hit(seed=1):
    """A 5 ms click at -6 dBFS into a 0.4 s decaying pink burst that starts near -14 dBFS RMS."""
    n = int(0.4 * SR)
    t = np.arange(n) / SR
    burst = 10 ** (-14 / 20) * _pink(n, seed) * np.exp(-t / 0.08)
    k = int(0.005 * SR)
    burst[:k] = 10 ** (-6 / 20) * np.sign(np.sin(2 * np.pi * 3000 * t[:k]))
    return np.clip(burst, -0.99, 0.99)


def film_signal(dur=30.0, hits=HITS, dip=14.3, silence_from=28.0, seed=0):
    n = int(dur * SR)
    bed = 10 ** (-30 / 20) * np.column_stack([_pink(n, seed), _pink(n, seed + 7)])
    if dip is not None:
        a, b = int(dip * SR), int((dip + 0.5) * SR)
        bed[a:b] *= 10 ** (-20 / 20)
    for h in hits:
        i = int(h * SR)
        hit = _hit()
        bed[i:i + len(hit)] += hit[:, None]
    if silence_from is not None:
        bed[int(silence_from * SR):] = 0.0
    return bed


@pytest.fixture
def film(write):
    return write("film.wav", film_signal())


@pytest.fixture
def film_result(film):
    return pr.film_metrics(film, use_ast=False)


def test_pro_1_salient_hits(film_result):
    hits = film_result["events"]["hits"]
    for h in HITS:
        assert any(abs(t - h) <= 0.03 for t in hits), (h, hits)
    assert len(hits) == 5
    assert film_result["metrics"]["hit_rate"] == round(5 / 30, 2)


def test_pro_2_stopdown(film_result):
    stops = film_result["events"]["stopdowns"]
    near = [s for s in stops if abs(s[0] - 14.3) <= 0.1]
    assert len(near) == 1
    assert 0.4 <= near[0][1] <= 0.6 and near[0][2] >= 10


def test_pro_3_ending(film_result):
    m = film_result["metrics"]
    assert 1.8 <= m["end_silence_s"] <= 2.2
    assert m["first_sound_s"] < 0.1


def test_pro_4_arc(film_result):
    arc = film_result["arc"]
    assert len(arc) == 11 and all(v is not None for v in arc)
    assert arc[-1] == min(arc)


def test_pro_5_short_file(write):
    r = pr.film_metrics(write("short.wav", film_signal(6.0, hits=(2.0,), dip=None, silence_from=None)),
                        use_ast=False)
    assert r["short"] is True
    for k in ("lra", "crest_lu", "intro_lu", "end_lu", "final_lu"):
        assert r["metrics"][k] is None
    assert r["metrics"]["peak_pos"] is not None


def test_pro_6_near_and_chance():
    assert pr.near([1, 2, 3], [1.05, 2.5], 0.1) == pytest.approx(1 / 3)
    assert pr.near([], [1], 0.1) is None
    assert pr.near([1], [], 0.1) == 0
    d = 10.0
    expect = np.mean([pr.near([(5.0 + o) % d], [5.23], 0.01) for o in pr.CHANCE_OFFSETS])
    assert pr.chance([5.0], [5.23], 0.01, d) == pytest.approx(expect)
    assert pr.chance([9.9], [(9.9 + 0.23) % d], 0.001, d) == pytest.approx(1 / 16)


def test_pro_7_profile_of_two(tmp_path, write):
    a = write("refs/a.wav", film_signal(12.0, hits=(3.0, 8.0), dip=None, silence_from=None))
    b = write("refs/b.wav", film_signal(15.0, hits=(4.0, 9.0), dip=None, silence_from=None, seed=3))
    out = tmp_path / "prof" / "p.json"
    prof = pr.profile([a, b], use_ast=False, out=str(out))
    assert prof["n"] == 2
    for k, s in prof["metrics"].items():
        if s["n"]:
            assert len(s["values"]) == 2, k
    assert any("only 2 film(s)" in w for w in prof["warnings"])
    assert [f["id"] for f in prof["films"]] == ["ref_1", "ref_2"]
    text = out.read_text()
    assert "a.wav" not in text and "b.wav" not in text and str(tmp_path) not in text
    assert json.loads(text)["version"] == 2


def test_pro_8_skips(tmp_path, write, capsys):
    good = write("good.wav", film_signal(12.0, hits=(3.0,), dip=None, silence_from=None))
    short = write("short.wav", film_signal(2.0, hits=(), dip=None, silence_from=None))
    silent = write("silent.wav", np.zeros((10 * SR, 2)))
    prof = pr.profile([short, silent, str(tmp_path / "missing"), good], use_ast=False)
    err = capsys.readouterr().err
    assert prof["n"] == 1
    assert err.count("skipped") == 2 and "3 file(s) skipped" in prof["warnings"][0]
    assert any("missing" in w for w in [prof["summary"]]) or "3 file(s) skipped" in prof["summary"]


def test_pro_9_nothing_usable(write, run_cli):
    short = write("short.wav", film_signal(2.0, hits=(), dip=None, silence_from=None))
    with pytest.raises(pr.ProfileError, match="no usable reference films"):
        pr.profile([short], use_ast=False)
    code, _, err = run_cli(pr, ["profile", short, "--no-ast"])
    assert code == 1 and "hark profile: error: no usable reference films:" in err


def test_pro_10_styles(tmp_path, write):
    a = write("a.wav", film_signal(12.0, hits=(3.0,), dip=None, silence_from=None))
    b = write("b.wav", film_signal(12.0, hits=(4.0,), dip=None, silence_from=None, seed=5))
    prof = pr.profile([a, b], styles={"b.wav": "kinetic type"}, use_ast=False)
    assert [f["style"] for f in prof["films"]] == ["", "kinetic type"]
    sfile = tmp_path / "styles.json"
    sfile.write_text(json.dumps(["x"]))
    prof = pr.profile([a, b], styles=str(sfile), use_ast=False)
    assert [f["style"] for f in prof["films"]] == ["x", ""]
    with pytest.raises(pr.ProfileError):
        pr.read_styles(str(tmp_path / "nope.json"))


def _cut_video(monkeypatch, fps=25.0, dur=12.0, every=2.0):
    from hark.timeline import video

    n = int(dur * fps)
    d = np.full(n, 0.002)
    for k in range(1, int(dur / every)):
        d[int(k * every * fps)] = 0.6
    monkeypatch.setattr(video, "frame_diff", lambda path, cache=True: (fps, d))
    monkeypatch.setattr(video, "has_video", lambda path: True)


def test_pro_11_picture_sync(write, monkeypatch):
    _cut_video(monkeypatch)
    sig = film_signal(12.0, hits=(2.0, 4.0, 6.0, 8.0, 10.0), dip=None, silence_from=None)
    r = pr.film_metrics(write("cuts.wav", sig), use_ast=False)
    assert r["metrics"]["cut_rate"] == pytest.approx(5 / 12, abs=0.1)
    assert r["metrics"]["cuts_with_sound"] >= 0.8
    assert r["chance"]["cuts_with_sound"] < r["metrics"]["cuts_with_sound"]


def test_pro_11_few_events_no_sync(film):
    r = pr.film_metrics(film, use_ast=False)
    assert r["metrics"]["cuts_with_sound"] is None and r["fps"] is None and r["video"] is None


def test_music_presence(film, fake_ast):
    r = pr.film_metrics(film, use_ast=True)
    assert r["metrics"]["music_frac"] == 0.0 and len(r["events"]["music_windows"]) == 6


def test_fmt_value_and_tables():
    assert pr.fmt_value("music_frac", 0.42) == "42%"
    assert pr.fmt_value("peak_pos", 0.5) == "0.50"
    assert pr.fmt_value("lufs_i", -14.04) == "-14.0 LUFS"
    assert pr.fmt_value("tempo", None) == "n.a."
    assert len(pr.METRICS) == 33 and pr.MIN_FILM_S == 10.0 and pr.MIN_SHAPE_S == 3.0
    assert pr.unscorable({"duration": 2.0, "lufs": -20}) and pr.unscorable({"duration": 12, "lufs": -80}) == "silent"
    assert pr.unscorable({"duration": 12, "lufs": -20}) is None


def test_media_problem(tmp_path, write):
    assert pr.media_problem(str(tmp_path / "x.wav")) == "not found"
    assert "folder" in pr.media_problem(str(tmp_path))
    (tmp_path / "bad.wav").write_bytes(b"zz" * 50)
    assert "readable" in pr.media_problem(str(tmp_path / "bad.wav"))
    assert pr.media_problem(write("ok.wav", np.zeros(SR))) is None


def test_load_profile(tmp_path):
    p = tmp_path / "p.json"
    p.write_text(json.dumps({"version": 2, "n": 0}))
    assert pr.load_profile(str(p))["version"] == 2
    if pr.PROFILE_PATH.exists():
        assert "metrics" in pr.load_profile()
    else:
        assert not os.path.exists(pr.PROFILE_PATH)
