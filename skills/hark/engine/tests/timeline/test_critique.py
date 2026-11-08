"""Acceptance tests C1-C14 of docs/specs/hark/timeline-critique.md (critique scorecard and the launch profile)."""

from __future__ import annotations

import json
import os
import re

import numpy as np
import pytest

SR = 48000
WEIGHTS = ("critical", "major", "minor", "info")
VERDICTS = ("launch-grade", "close", "needs work", "not launch-grade", "n.a.")


def prof(**ranges) -> dict:
    """Profile helper of critique 5: n 7, min lo, max hi, median the mean, p10 lo, p90 hi."""
    metrics = {k: {"n": 7, "min": lo, "max": hi, "median": (lo + hi) / 2, "p10": lo, "p90": hi}
               for k, (lo, hi) in ranges.items()}
    return {"n": 7, "metrics": metrics}


def soundtrack(dur, hits=(), seed=0, mono=False):
    """Stereo noise bed (0.05) with decaying noise hits (0.15 s, 0.8, decay 30 ms); R = 0.9 L + 0.1 noise."""
    r = np.random.default_rng(seed)
    n = int(round(dur * SR))
    left = 0.05 * r.uniform(-1, 1, n)
    k = int(0.15 * SR)
    env = np.exp(-np.arange(k) / SR / 0.03)
    for t in hits:
        i = int(t * SR)
        j = min(n, i + k)
        left[i:j] += 0.8 * r.uniform(-1, 1, j - i) * env[: j - i]
    right = left.copy() if mono else 0.9 * left + 0.1 * 0.05 * r.uniform(-1, 1, n)
    return np.column_stack([left, right]).astype(np.float32)


def rows(card) -> dict:
    return {r["metric"]: r for r in card["metrics"]}


@pytest.fixture
def thresholds(monkeypatch):
    """Fix the realness thresholds (lo 0.4, hi 0.6) for the scorecard."""
    from hark.timeline import critique

    th = {"lo": 0.4, "hi": 0.6}
    monkeypatch.setattr(critique, "real_thresholds", lambda file=True: dict(th))
    return th


C1_PROFILE = prof(onset_rate=(2.5, 8.0), music_frac=(0.67, 1.0), first_sound_s=(0.0, 0.5))


def test_c1_ranges_and_statuses(thresholds):
    from hark.timeline.critique import scorecard

    values = {"lufs_i": -14.0, "true_peak": -0.5, "onset_rate": 1.0, "music_frac": 0.3, "first_sound_s": 0.0}
    r = rows(scorecard(values, C1_PROFILE))
    assert r["lufs_i"]["status"] == "ok" and r["lufs_i"]["basis"] == "web"
    assert r["true_peak"]["status"] == "high"
    assert rows(scorecard(values, C1_PROFILE, lossy=True))["true_peak"]["status"] == "ok"
    assert r["onset_rate"]["status"] == "low" and r["onset_rate"]["range"] == [2.2, 8.3]
    assert r["music_frac"]["status"] == "low" and r["music_frac"]["weight"] == "critical"
    assert r["first_sound_s"]["range"][0] is None and r["first_sound_s"]["status"] == "ok"
    assert r["band_sub"]["status"] == "n.a."
    assert r["tempo"]["weight"] == "info"


def test_c2_near_miss_of_a_critical_rule(thresholds):
    from hark.timeline.critique import scorecard

    assert rows(scorecard({"lufs_i": -17.5}, C1_PROFILE))["lufs_i"]["weight"] == "major"
    assert rows(scorecard({"lufs_i": -25.0}, C1_PROFILE))["lufs_i"]["weight"] == "critical"


def test_c3_launch_grade(thresholds):
    from hark.timeline.critique import scorecard

    card = scorecard({"lufs_i": -14.0, "onset_rate": 5.0, "music_frac": 0.9}, C1_PROFILE)
    assert card["verdict"] == "launch-grade" and card["score"] == 1.0


C4_PROFILE = prof(hit_rate=(0.1, 1.66), music_frac=(0.67, 1.0))


def test_c4_realness_is_critical(thresholds):
    from hark.timeline.critique import scorecard

    row = rows(scorecard({"p_real": 0.3}, C4_PROFILE))["p_real"]
    assert row["status"] == "low" and row["weight"] == "critical" and row["range"][0] == thresholds["hi"]
    card = scorecard({"p_real": 0.3, "lufs_i": -14.0, "music_frac": 1.0}, C4_PROFILE)
    assert card["verdict"] == "needs work"


def test_c5_realness_near_the_line(thresholds):
    from hark.timeline.critique import scorecard

    assert thresholds["hi"] > 0.55
    assert rows(scorecard({"p_real": 0.52}, C4_PROFILE))["p_real"]["weight"] == "minor"
    assert rows(scorecard({"p_real": 0.99}, C4_PROFILE))["p_real"]["status"] == "ok"


def test_c6_density_floor(thresholds):
    from hark.timeline.critique import scorecard

    row = rows(scorecard({"hit_rate": 0.0}, C4_PROFILE))["hit_rate"]
    assert row["status"] == "low" and row["range"][0] == 0.05


def test_c7_bundled_profile():
    from hark.core import config

    path = os.path.join(config.DATA_DIR, "timeline", "launch_profile.json")
    with open(path, encoding="utf-8") as fh:
        text = fh.read()
    p = json.loads(text)
    assert p["n"] == 7
    # Seed profile (timeline A1, decision Q1): per-film values are not shipped, so films is empty.
    assert [f["id"] for f in p["films"]] == [f"ref_{k}" for k in range(1, len(p["films"]) + 1)]
    assert not re.search(r"\.(mp4|mov|wav|m4a)\b|/Users/|[A-Za-z]:\\\\|~/", text)
    from hark.timeline.critique import SPEC

    for key, spec in SPEC.items():
        if key != "p_real" and spec[0] > 0:
            assert p["metrics"][key]["n"] >= 6, key


def test_c7_seed_ranges_match_the_spec(thresholds):
    from hark.listen.profile import load_profile
    from hark.timeline.critique import window

    p = load_profile()["metrics"]
    expect = {"onset_rate": (2.16, 8.52), "hit_rate": (0.05, 1.86), "music_frac": (0.62, None),
              "cuts_with_sound": (0.45, None), "first_sound_s": (None, 0.58), "end_silence_s": (None, 2.15),
              "max_gap_s": (None, 1.91), "fade_s": (None, 3.04), "lra": (0.7, 13.2), "crest_lu": (0.6, 4.0),
              "final_lu": (-3.7, 7.1)}
    for key, (lo, hi) in expect.items():
        got_lo, got_hi, _ = window(key, p[key], False)
        assert (None if got_lo is None else round(got_lo, 3)) == lo, key
        assert (None if got_hi is None else round(got_hi, 3)) == hi, key


def test_c8_late_first_sound(wav, hark_home):
    from hark.timeline.critique import critique

    x = soundtrack(12, hits=(2, 6, 9))
    x[: int(1.5 * SR)] = 0
    res = critique(wav(x, "late.wav"), use_ast=False, use_realness=False)
    json.dumps(res)
    assert res["verdict"] in VERDICTS
    r = rows(res)
    assert r["first_sound_s"]["status"] == "high"
    assert any(a["metric"] == "first_sound_s" and "0:01.5" in a["action"] for a in res["actions"])
    assert r["cuts_with_sound"]["status"] == "n.a." and "cuts_with_sound" in res["summary"]
    res["text"].encode("ascii")


def test_c9_nothing_to_score(wav, hark_home, monkeypatch):
    import hark.listen.profile as listen_profile
    from hark.timeline.critique import critique

    def no_heavy_pass(*a, **kw):
        raise AssertionError("film_metrics must not run for an unscorable file")

    monkeypatch.setattr(listen_profile, "film_metrics", no_heavy_pass)
    for x in (np.zeros((8 * SR, 2), dtype=np.float32), soundtrack(1.0)):
        res = critique(wav(x, "short.wav"), use_ast=False, use_realness=False)
        assert res["verdict"] == "n.a." and res["actions"] == []
        assert "nothing to score" in res["summary"]
        assert res["summary"] == res["text"]


def test_c10_mono_mix_and_short_clip(wav, hark_home):
    from hark.timeline.critique import critique

    res = critique(wav(soundtrack(20, mono=True), "mono.wav"), use_ast=False, use_realness=False)
    row = rows(res)["side_to_mid_db"]
    assert row["value"] == -40 and row["status"] == "low"
    assert any(a["metric"] == "side_to_mid_db" and "mono" in a["action"] for a in res["actions"])
    short = critique(wav(soundtrack(6, hits=(1, 3, 5)), "six.wav"), use_ast=False, use_realness=False)
    r = rows(short)
    assert all(r[k]["status"] == "n.a." for k in ("lra", "crest_lu", "intro_lu", "end_lu", "final_lu"))
    assert "clip under 10 s" in short["summary"]
    assert short["partial"] == ["realness", "music presence"]
    assert "partial" in short["text"].splitlines()[1]


def test_c11_input_errors(wav, tmp_path, hark_home, capsys, cli_parser):
    from hark.listen.profile import ProfileError
    from hark.timeline.critique import add_cli, critique

    audio = wav(soundtrack(4, hits=(1, 2, 3)), "a.wav")
    with pytest.raises(ProfileError, match="file not found"):
        critique(str(tmp_path / "nope.wav"))
    with pytest.raises(ProfileError, match="folder"):
        critique(str(tmp_path))
    with pytest.raises(ProfileError, match="has no picture"):
        critique(audio, video=wav(soundtrack(4), "b.wav"))
    assert critique(audio, video=audio, use_ast=False, use_realness=False)["video"] is None
    with pytest.raises(ProfileError, match="profile not found"):
        critique(audio, profile=str(tmp_path / "none.json"))
    bad = tmp_path / "bad.json"
    bad.write_text('{"n": 1}')
    with pytest.raises(ProfileError, match="not a profile"):
        critique(audio, profile=str(bad))
    args = cli_parser(add_cli).parse_args(["critique", "nope.wav", "--fast"])
    assert args.func(args) == 1
    assert capsys.readouterr().err == "hark critique: error: file not found: nope.wav\n"


def test_c12_profile_of_two_soundtracks(wav, tmp_path, hark_home, thresholds):
    from hark.listen.profile import profile
    from hark.timeline.critique import critique, scorecard

    paths = [wav(soundtrack(12, hits=(2, 5, 8), seed=s), f"ref{s}.wav") for s in (1, 2)]
    out = str(tmp_path / "two.json")
    p = profile(paths, use_ast=False, out=out)
    assert any("2" in w for w in p["warnings"])
    res = critique(paths[0], profile=out, use_ast=False, use_realness=False)
    r = rows(res)
    assert res["profile_n"] == 2
    assert r["music_frac"]["status"] == "n.a."
    assert r["onset_rate"]["status"] not in ("n.a.", "info")
    row = rows(scorecard({"cuts_with_sound": 0.1}, p))["cuts_with_sound"]
    assert row["status"] == "n.a." and row["basis"] == "no reference data"


def test_c13_cli_json(wav, hark_home, capsys, cli_parser):
    from hark.timeline.critique import add_cli

    path = wav(soundtrack(8, hits=(1, 3, 5, 7)), "eight.wav")
    args = cli_parser(add_cli).parse_args(["critique", path, "--fast", "--json"])
    assert args.func(args) == 0
    res = json.loads(capsys.readouterr().out)
    assert res["scored"] > 10 and res["summary"] and res["partial"] == ["realness", "music presence"]


def test_c14_spec_keys_in_listen_table():
    from hark.listen.profile import METRICS
    from hark.timeline.critique import SPEC

    keys = set(METRICS) if isinstance(METRICS, dict) else {m["key"] for m in METRICS}
    assert {k for k in SPEC if k != "p_real"} <= keys


# ---------------------------------------------------------------- more behaviour of critique 2

def test_reference_film_is_scored_leave_one_out(wav, hark_home):
    from hark.listen.profile import aggregate, film_metrics
    from hark.timeline.critique import critique

    paths = [wav(soundtrack(12, hits=(2, 5, 8), seed=s), f"r{s}.wav") for s in (1, 2, 3)]
    films = []
    for k, p in enumerate(paths, 1):
        fm = film_metrics(p, use_ast=False)
        films.append({"id": f"ref_{k}", "style": "", "duration": fm["duration"], "has_video": False,
                      "metrics": fm["metrics"], "chance": fm["chance"], "arc": fm["arc"]})
    res = critique(paths[1], profile=aggregate(films), use_ast=False, use_realness=False)
    assert res["reference_match"] == {"id": "ref_2", "scored_against": 2}
    assert res["profile_n"] == 2 and res["profile"] == "custom"
    assert "reference ref_2" in res["summary"]


def test_realness_thresholds_fall_back(monkeypatch):
    import hark.listen.realness as realness
    from hark.timeline.critique import real_thresholds

    def broken(path=None):
        raise FileNotFoundError("no model")

    monkeypatch.setattr(realness, "load_model", broken)
    real_thresholds.cache_clear()
    try:
        assert real_thresholds(True) == {"lo": 0.5, "hi": 0.5}
        assert real_thresholds(False) == {"lo": 0.5, "hi": 0.5}
    finally:
        real_thresholds.cache_clear()


def test_p_real_format_avoids_false_threshold(thresholds):
    from hark.timeline.critique import fmt_p

    assert fmt_p(0.398) == "0.398"
    assert fmt_p(0.4) == "0.40"
    assert fmt_p(0.73) == "0.73"


def test_realness_action_lists_flagged_windows(thresholds):
    from hark.timeline.critique import Film, act_realness

    segs = [{"start": s, "end": s + 4, "p_real": p} for s, p in ((0, 0.2), (4, 0.3), (8, 0.9), (12, 0.35))]
    f = Film("x.wav", None, np.zeros(1), 16.0, -14.0, realness={"p_real": 0.3, "segments": segs})
    text = act_realness(f, "p_real", 0.3, "low")
    assert "likely code-synthesized" in text and "0:00.00-0:04.00 (0.20)" in text and "sound-sourcing" in text
    assert "hark realness stem.wav" in text
    quiet = Film("x.wav", None, np.zeros(1), 16.0, -14.0,
                 realness={"p_real": 0.55, "segments": [{"start": 0, "end": 4, "p_real": 0.7}]})
    assert act_realness(quiet, "p_real", 0.55, "low") is None
