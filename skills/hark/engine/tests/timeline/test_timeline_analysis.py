"""Acceptance tests T1-T19 of docs/specs/hark/timeline-analysis-tests.md, plus the package contract (ARCHITECTURE 7)."""

from __future__ import annotations

import json
import subprocess
import sys

import numpy as np
import pytest

SR = 48000


def _near(events, t, tol):
    return [e for e in events if abs(e["t"] - t) <= tol]


def test_t1_click_track_hits(click_track):
    from hark.timeline.timeline import detect_events

    times = [0.5, 1.25, 2.0, 2.6, 3.33]
    events = detect_events(click_track(times, 4.0, seed=0))
    hits = [e for e in events if e["shape"] == "hit"]
    for t in times:
        assert _near(hits, t, 0.003), t
    assert len(hits) <= 6
    e = _near(hits, 1.25, 0.003)[0]
    assert e["prominence_db"] > 20
    assert -12 <= e["peak_dbfs"] <= 6
    assert e["length_ms"] < 200


def test_t2_prominence_of_quiet_click(click_track):
    from hark.timeline.timeline import detect_events

    x = click_track([1.0], 2.0, amp=0.5, bed=0.0) + click_track([1.5], 2.0, amp=0.05, bed=0.0, seed=1)
    events = detect_events(x)
    loud, quiet = _near(events, 1.0, 0.005)[0], _near(events, 1.5, 0.005)[0]
    assert loud["prominence_db"] - quiet["prominence_db"] > 10


def _sine(dur, amp=0.5, freq=440.0):
    t = np.arange(int(round(dur * SR))) / SR
    return (amp * np.sin(2 * np.pi * freq * t)).astype(np.float32)


def test_t3_gap_and_clipping():
    from hark.timeline.timeline import loudness

    x = np.concatenate([_sine(3), np.zeros(SR, dtype=np.float32), _sine(2)])
    L = loudness(x)
    assert len(L["silences"]) == 1
    g = L["silences"][0]
    assert g["kind"] == "digital silence"
    assert abs(g["start"] - 3.0) <= 0.15 and abs(g["end"] - 4.0) <= 0.15
    assert L["clipping"]["runs"] == 0
    hot = np.clip(5 * _sine(6), -1, 1)
    assert loudness(hot)["clipping"]["runs"] > 10


def _beat_track(click_track):
    beats = np.arange(0.25, 12.5, 0.5)
    x = click_track(beats, 12.5, amp=0.3)
    t = np.arange(int(0.3 * SR)) / SR
    thump = 0.8 * np.sin(2 * np.pi * 60 * t) * np.exp(-t / 0.08)
    for b in beats[::4]:
        i = int(b * SR)
        x[i: i + t.size] += thump[: x.size - i]
    return x, beats


def test_t4_beat_grid(click_track):
    from hark.timeline.timeline import music_grid

    x, beats = _beat_track(click_track)
    g = music_grid(x)
    assert abs(g["tempo"] - 120) <= 2
    assert g["confidence"] == "clear"
    found = [b for b in g["beats"] if 1.0 <= b <= 11.5]
    assert np.median([np.min(np.abs(beats - b)) for b in found]) < 0.015
    accented = beats[::4]
    assert np.min(np.abs(accented - g["downbeats"][1])) <= 0.03


def test_t5_noise_has_no_grid():
    from hark.timeline.timeline import music_grid

    n = np.random.default_rng(0).uniform(-0.1, 0.1, 8 * SR)
    assert music_grid(n)["confidence"] == "none"


def test_t6_cue_formats(tmp_path):
    from hark.timeline._dsp import TimelineError
    from hark.timeline.timeline import read_cues

    f = tmp_path / "cues.json"
    f.write_text(json.dumps({"fps": 60, "events": [{"f": 120, "kind": "hit", "id": "a"}, {"f": 30, "kind": "tick"}]}))
    cues, fps = read_cues(str(f))
    assert fps == 60 and [c["t"] for c in cues] == [0.5, 2.0] and cues[1]["id"] == "a"
    assert [c["t"] for c in read_cues("1.5, 3")[0]] == [1.5, 3.0]
    assert [c["t"] for c in read_cues('[{"t": 2.25}]')[0]] == [2.25]
    assert read_cues([4, 1])[0][0]["t"] == 1
    assert read_cues(None) == ([], None)
    assert read_cues({"fps": 50, "events": [{"frame": 100}]})[0][0]["t"] == 2.0
    assert read_cues([{"seconds": 1.25}])[0][0]["t"] == 1.25
    with pytest.raises(TimelineError, match="fps"):
        read_cues([{"f": 10}])
    with pytest.raises(TimelineError, match="without a time"):
        read_cues([{"kind": "x"}])
    with pytest.raises(TimelineError):
        read_cues("one, two")


def test_t7_sync_statuses_and_stats():
    from hark.timeline.timeline import sync

    events = [{"t": t, "prominence_db": p, "shape": "hit"} for t, p in ((1.0, 12), (2.03, 12), (3.3, 2), (5.0, 12))]
    beats = list(np.arange(1.0, 4.0001, 0.5))
    r = sync([1.004, 2.0, 3.25, 4.0], events, beats=beats, frame_s=1 / 60)
    items, st = r["items"], r["stats"]
    assert [i["status"] for i in items] == ["OK", "near", "near", "no sound"]
    assert items[0]["offset_ms"] == -4.0 and items[1]["offset_ms"] == 30.0 and items[3]["offset_ms"] is None
    assert st["hit_within_1_frame"] == 0.25
    assert st["no_sound"] == [4.0]
    assert 0 <= st["chance_within_1_frame"] <= 0.1
    assert st["on_beat"] == 0.75 and st["on_beat_chance"] == 0.2
    assert st["accent_min_db"] == 12.0
    assert [i["accent_offset_ms"] for i in items] == [-4.0, 30.0, None, None]
    assert st["accent_within_1_frame"] == 0.25 and st["accent_within_100ms"] == 0.5
    assert st["accent_chance_within_1_frame"] < st["chance_within_1_frame"]


def test_t8_level_peak():
    from hark.timeline.timeline import level_envs, level_peak, sync

    t = np.arange(6 * SR) / SR
    amp = 0.01 + 0.2 * np.exp(-0.5 * ((t - 3.0) / 0.25) ** 2)
    x = (amp * np.random.default_rng(0).uniform(-1, 1, t.size)).astype(np.float32)
    envs = level_envs(x)
    pk = level_peak(envs, 3.05)
    assert pk is not None and abs(pk["t"] - 3.0) <= 0.06 and pk["rise_db"] > 10
    assert level_peak(envs, 1.0) is None
    r = sync([3.05, 1.0], [], levels=envs)
    assert [i["status"] for i in r["items"]] == ["peak", "no sound"]


@pytest.fixture
def six_s_clicks(wav, click_track):
    return wav(click_track([0.5 + k for k in range(6)], 6.0, seed=0), "clicks6.wav")


def test_t9_full_run(six_s_clicks, stub_models, hark_home):
    from hark.timeline.timeline import timeline

    r = timeline(six_s_clicks, cues=[1.5, 2.52, 5.0], detail="low")
    assert r["text"].startswith(r["summary"])
    r["text"].encode("ascii")
    # The bed makes the file start with sound, so the lead event (starts_file, analysis 2.2 step 7) comes first.
    clicks = [e for e in r["events"] if e["shape"] == "hit" and e["prominence_db"] > 10 and not e.get("starts_file")]
    assert len(clicks) == 6
    st = r["sync"]["cues"]["stats"]
    assert st["n"] == 3 and st["no_sound"] == [5.0]
    assert "NO HIT" in r["text"] and "cue -20 ms OK" in r["text"]
    json.dumps(r)
    assert r["cached"] is False
    assert timeline(six_s_clicks, cues=[1.5, 2.52, 5.0], detail="low")["cached"] is True
    z = timeline(six_s_clicks, start=2, end=4, detail="low", cache=False)
    assert z["start"] == 2.0 and z["end"] == 4.0
    strong = [e["t"] for e in z["events"] if e["prominence_db"] >= 10]
    assert strong == [2.5, 3.5]


def test_t10_zoom_matches_full_run(wav, click_track, stub_models, hark_home):
    from hark.timeline.timeline import timeline

    path = wav(click_track(np.arange(0.5, 12, 0.5), 12.0, seed=0), "clicks12.wav")
    cues = [2.0, 4.02, 6.5, 7.97]
    full = timeline(path, cues=cues, detail="low", cache=False)
    zoom = timeline(path, cues=cues, start=4, end=8, detail="low", cache=False)
    assert zoom["context"] == [0.0, 12.0]
    by_t = {i["t"]: i for i in full["sync"]["cues"]["items"]}
    for item in zoom["sync"]["cues"]["items"]:
        assert item["status"] == by_t[item["t"]]["status"]
        assert item["offset_ms"] == by_t[item["t"]]["offset_ms"]
    assert all(4 <= e["t"] <= 8 for e in zoom["events"])
    assert all(4 <= b <= 8 for b in zoom["music"]["beats"])


def test_t11_tag_budget_spread():
    from hark.timeline._tags import pick

    events = [{"t": 1.0 + 0.06 * k, "band": "high", "prominence_db": 30 - 0.1 * k, "contrast_db": 20}
              for k in range(10)]
    events += [{"t": float(t), "band": "full", "prominence_db": 20, "contrast_db": 20} for t in (3, 5, 7, 9)]
    events.sort(key=lambda e: e["t"])
    chosen, _ = pick(events, 6, [], 0.0, 10.0)
    times = [events[i]["t"] for i in chosen]
    assert sum(1 for t in times if t < 2) <= 2
    assert {3.0, 5.0, 7.0, 9.0} <= set(times)


def test_t12_cover_art_is_not_video():
    from hark.timeline.timeline import is_picture

    cover = {"has_video": True, "video": {"fps": 90000.0}}
    film = {"has_video": True, "video": {"fps": 30.0}}
    assert not is_picture("song.mp3", cover)
    assert not is_picture("song.mp4", cover)
    assert is_picture("film.mp4", film)
    assert not is_picture("song.m4a", film)


def test_t13_silence_and_gap_edges():
    from hark.timeline.timeline import loudness

    L = loudness(np.zeros((5 * SR, 2), dtype=np.float32))
    assert L["digital_silence"] and L["integrated_lufs"] is None and L["true_peak_dbtp"] is None
    assert len(L["silences"]) == 1 and L["silences"][0]["where"] == "whole file"
    assert min(L["momentary"]["lufs"]) >= -70
    z = np.zeros(SR, dtype=np.float32)
    x = np.concatenate([z, _sine(3), z, _sine(2), z])
    gaps = loudness(x)["silences"]
    assert [g.get("where") for g in gaps] == ["lead-in", None, "tail"]
    assert gaps[0]["start"] == 0.0
    assert all("where" not in g for g in loudness(x, at_start=False, at_end=False)["silences"])


def test_t14_video_cuts_sync(tmp_path, mp4, click_track, stub_models, hark_home):
    from hark.timeline.timeline import timeline

    path = mp4(tmp_path / "film.mp4", audio=click_track([0.6, 1.5, 2.4], 3.0, seed=0))
    r = timeline(path, detail="low", cache=False)
    assert r["video"] == path and r["fps"] == 30
    assert r["cuts"] == [1.5]
    item = r["sync"]["cuts"]["items"][0]
    assert item["status"] in ("OK", "near") and abs(item["offset_ms"]) < 50


def test_t15_picture_only_video(tmp_path, mp4, stub_models, hark_home):
    from hark.timeline.timeline import timeline

    r = timeline(mp4(tmp_path / "mute.mp4"), detail="low", cache=False)
    assert r["has_audio"] is False and r["cuts"] == [1.5]
    assert r["sync"]["cuts"]["items"][0]["status"] == "no sound"
    assert "NO AUDIO STREAM" in r["summary"]


def test_t16_phrase_table():
    from hark.timeline._tags import BACKGROUND, PHRASE_LIST

    assert len(PHRASE_LIST) == 49 and len(set(PHRASE_LIST)) == 49
    assert set(PHRASE_LIST) == set(BACKGROUND)


def test_t17_ascii_rule():
    from hark.timeline._dsp import ascii_safe

    assert ascii_safe("t\xf6n.wav") == "t\\xf6n.wav"


def test_t18_cli_parse(cli_parser):
    from hark.timeline.timeline import add_cli

    a = cli_parser(add_cli).parse_args(["timeline", "x.wav", "--cues", "1,2", "--from", "1", "--to", "3",
                                        "--detail", "low"])
    assert (a.start, a.end, a.detail, a.cues) == (1.0, 3.0, "low", "1,2")


@pytest.mark.ml
@pytest.mark.slow
def test_t19_real_models(six_s_clicks, hark_home):
    from hark.timeline._tags import KINDS
    from hark.timeline.timeline import timeline

    r = timeline(six_s_clicks, detail="low", cache=False)
    tagged = [e for e in r["events"] if e.get("kind")]
    assert tagged and all(e["kind"] in KINDS for e in tagged)
    assert r["sections"] and all(s.get("label") for s in r["sections"])
    assert r["model_calls"]["clap_event_windows"] >= len(tagged)


# ---------------------------------------------------------------- errors (analysis 4) and the package contract

def test_errors_detail_window_and_files(six_s_clicks, wav, stub_models, hark_home, tmp_path):
    from hark.timeline._dsp import TimelineError
    from hark.timeline.timeline import timeline

    with pytest.raises(TimelineError, match="low, normal, high"):
        timeline(six_s_clicks, detail="max")
    with pytest.raises(FileNotFoundError):
        timeline(str(tmp_path / "nope.wav"))
    with pytest.raises(TimelineError, match="no picture"):
        timeline(six_s_clicks, video=six_s_clicks)
    with pytest.raises(TimelineError, match="nothing to analyse"):
        timeline(six_s_clicks, start=7, cache=False)
    with pytest.raises(TimelineError, match="0.1 s"):
        timeline(wav(np.zeros(1000, dtype=np.float32), "tiny.wav"), cache=False)


def test_digital_silence_file(wav, stub_models, hark_home):
    from hark.timeline.timeline import timeline

    r = timeline(wav(np.zeros((3 * SR, 2), dtype=np.float32), "zeros.wav"), cache=False)
    assert "DIGITAL SILENCE" in r["summary"]
    assert r["sections"] == [{"start": 0.0, "end": 3.0, "windows": 0, "label": "digital silence", "lufs": None,
                              "digital_silence": True}]
    assert r["events"] == [] and r["model_calls"]["clap_event_windows"] == 0


def test_package_is_callable(six_s_clicks, stub_models, hark_home):
    import hark.timeline

    r = hark.timeline(six_s_clicks, detail="low", cache=False)
    assert r["file"] == six_s_clicks
    assert callable(hark.timeline.detect_events)


def test_package_import_is_light():
    code = ("import sys, hark.timeline; heavy = [m for m in ('numpy', 'scipy', 'matplotlib', 'hark.timeline.timeline')"
            " if m in sys.modules]; print(heavy)")
    out = subprocess.run([sys.executable, "-c", code], capture_output=True, text=True, check=True).stdout
    assert out.strip() == "[]"



def test_cli_handler_json(six_s_clicks, stub_models, hark_home, capsys, cli_parser, tmp_path):
    from hark.timeline.timeline import add_cli

    out = tmp_path / "res" / "r.json"
    args = cli_parser(add_cli).parse_args(["timeline", six_s_clicks, "--detail", "low", "--json", "--out", str(out)])
    assert args.func(args) == 0
    printed = json.loads(capsys.readouterr().out)
    assert printed["detail"] == "low" and json.loads(out.read_text())["summary"] == printed["summary"]
