"""FND-1 .. FND-14: find (candidates, windows, snapping, edges, selection, end to end)."""

from __future__ import annotations

import numpy as np
import pytest

from hark.listen import search

SR = 48000
CLICK_TIMES = (0.5, 1.5, 2.5, 3.5, 4.5)


def _click(level_db=-6.0, decay=0.04, length=0.3):
    n = np.arange(int(length * SR))
    burst = np.where(n < int(0.003 * SR), 1.0, 1.0) * np.exp(-n / (decay * SR))
    burst *= np.sign(np.sin(2 * np.pi * 2500 * n / SR) + 1e-9)
    return 10 ** (level_db / 20) * burst


def _room(dur, seed=0, rms_db=-60.0):
    return 10 ** (rms_db / 20) * np.random.default_rng(seed).standard_normal(int(dur * SR))


def _add(x, t, burst):
    i = int(round(t * SR))
    j = min(len(x), i + len(burst))
    x[i:j] += burst[:j - i]
    return x


def clicks_signal(times=CLICK_TIMES, dur=6.0, levels=None):
    x = _room(dur)
    for k, t in enumerate(times):
        _add(x, t, _click(levels[k] if levels else -6.0))
    return x


def _local(x):
    env = search.envelope(x)
    return env, search.noise_floor(env)


def _cands(x):
    env, floor = _local(x)
    return search.onset_candidates(x, env, floor, len(x) / SR), env, floor


def test_fnd_1_strokes_land_on_clicks():
    cands, _, _ = _cands(clicks_signal())
    # librosa can also flag the start of the room tone (a stroke of about 1 dB prominence); the spec
    # counts the 5 click strokes, so strokes under 20 dB prominence are left out of the count.
    strokes = [c for c in cands if c["kind"] == "stroke" and c["prom"] >= 20]
    assert len(strokes) == 5
    for s, t, nxt in zip(strokes, CLICK_TIMES, list(CLICK_TIMES[1:]) + [6.0]):
        assert t - 0.015 <= s["start"] <= t
        assert s["end"] < nxt
        assert s["priority"] >= s["prom"] + 10


def test_fnd_2_pairs():
    x = clicks_signal(times=(1.0, 1.12, 3.0, 3.12), dur=5.0)
    cands, _, _ = _cands(x)
    events = [c for c in cands if c["kind"] == "event"]
    assert len(events) == 2
    for e, (a, b) in zip(sorted(events, key=lambda c: c["start"]), ((1.0, 1.12), (3.0, 3.12))):
        assert e["start"] <= a and e["end"] > b
    assert not any(e["start"] < 1.1 and e["end"] > 3.1 for e in events)


def test_fnd_3_weak_lead():
    x = clicks_signal(times=(1.0, 1.1), dur=3.0, levels=[-40.0, -6.0])
    cands, _, _ = _cands(x)
    assert not any(c["kind"] == "event" and abs(c["start"] - 1.0) < 0.02 for c in cands)


def test_fnd_3_weak_lead_unit():
    st = [{"start": 1.0, "end": 1.05, "prom": 20.0}, {"start": 1.1, "end": 1.2, "prom": 50.0}]
    env = np.full(1000, -60.0)
    assert search.pairs_of(env, st) == []
    st[0]["prom"] = 40.0
    assert len(search.pairs_of(env, st)) == 1


def test_fnd_4_edit_gap():
    x = np.zeros(int(2 * SR))
    x[:int(0.98 * SR)] = _room(0.98)
    _add(x, 0.9, _click(length=0.08))
    x[int(1.0 * SR):] = _room(1.0, seed=1)
    x[int(0.98 * SR):int(1.0 * SR)] = 0.0
    _add(x, 1.0, _click(length=0.08))
    env = search.envelope(x)
    assert search.edit_gap(env, 0.9, 1.0)
    st = [{"start": 0.895, "end": 0.975, "prom": 50.0}, {"start": 0.995, "end": 1.08, "prom": 50.0}]
    assert search.pairs_of(env, st) == [] and search.chains_of(env, st) == []


def test_fnd_5_round_robin():
    rng = np.random.default_rng(0)
    starts = np.sort(rng.uniform(0, 20, 200))
    cands = [{"start": float(s), "priority": float(s) + rng.uniform(0, 0.1)} for s in starts]
    picked = search.round_robin(cands, 48)
    assert len(picked) == 48
    buckets = {int(c["start"] // 2) for c in cands}
    assert buckets == {int(c["start"] // 2) for c in picked}
    for b in buckets:
        mine = sorted((c["priority"] for c in cands if int(c["start"] // 2) == b), reverse=True)
        got = sorted((c["priority"] for c in picked if int(c["start"] // 2) == b), reverse=True)
        assert got == mine[:len(got)]


@pytest.mark.parametrize("span", [3, 7.5, 30, 61, 300, 900])
def test_fnd_6_windows_cover_everything(span):
    grid = search.window_grid(0.0, float(span))
    assert grid[0][0] == 0.0 and grid[-1][1] == pytest.approx(span)
    lengths = {round(b - a, 6) for a, b in grid}
    assert len(lengths) == 1 and max(lengths) <= 10
    for (a0, b0), (a1, _) in zip(grid, grid[1:]):
        assert a1 <= b0 + 1e-9
    if span <= 400:
        assert len(grid) <= 41
    if span == 900:
        assert len(grid) == 90 and grid[1][0] - grid[0][0] == pytest.approx(10)
    if span == 3:  # F3: win = clamp(3 / 6, 1, 5) = 1 s, so 5 windows (the spec text says 1; see the report)
        assert len(grid) == 5 and grid[0] == (0.0, 1.0)


def test_fnd_6_span_shorter_than_window():
    assert search.window_grid(0.0, 0.8) == [(0.0, 0.8)]
    assert search.window_grid(0.1, 3.0, win=5.0) == [(0.1, 3.0)]


def test_fnd_6_explicit_win_hop():
    assert [round(a, 3) for a, _ in search.window_grid(0.0, 7.5, 2.0, 2.0)] == [0, 2, 4, 5.5]


def test_fnd_7_dedupe():
    cands = [{"start": 0, "end": 1}, {"start": 0.02, "end": 1.01}, {"start": 2, "end": 3}]
    assert [(c["start"], c["end"]) for c in search.dedupe(cands)] == [(0, 1), (2, 3)]


def test_fnd_8_edge_levels():
    n = np.arange(SR)
    decay = np.exp(-n / SR * np.log(100) / 1.0) * np.sin(2 * np.pi * 300 * n / SR)
    env = search.envelope(decay)
    assert search.edge_levels(env, 0.0, 1.0)[1] <= -30
    sine = np.sin(2 * np.pi * 50 * np.arange(SR) / SR) * np.linspace(0.1, 1, SR)
    head, tail = search.edge_levels(search.envelope(sine), 0.0, 1.0)
    assert tail > -15
    assert search.edge_levels(env, 5.0, 5.0) == (0.0, 0.0)


def test_fnd_9_snapping():
    x = clicks_signal()
    env, floor = _local(x)
    a, b = search.snap_window(env, floor, 1.48, 2.0, 0.0, 6.0)
    assert 1.48 <= a <= 1.50
    decay_end = 1.5 + 0.04 * np.log(10 ** (35 / 20))
    assert abs(b - (decay_end + 0.02)) <= 0.3
    a2, _ = search.snap_window(env, floor, 1.53, 2.4, 0.0, 6.0)
    assert a2 > 1.53


def test_fnd_10_multiple_sounds():
    x = clicks_signal()
    env, floor = _local(x)
    assert search.multiple_sounds(env, floor, 0.4, 1.9)
    assert not search.multiple_sounds(env, floor, 1.4, 2.2)


def _contains_click(seg, times=CLICK_TIMES):
    return [t for t in times if seg["start"] <= t < seg["end"]]


def _end_to_end(path):
    r = search.find(path, "a click")
    assert len(r["segments"]) <= 5
    segs = sorted(r["segments"], key=lambda s: s["start"])
    for a, b in zip(segs, segs[1:]):
        assert a["end"] <= b["start"] + 0.001
    for s in segs:
        assert len(_contains_click(s)) == 1 or s["kind"] != "window"
        assert len(_contains_click(s)) <= 1 or s["kind"] != "window"
    return r


def test_fnd_11_end_to_end_stub(write, fake_clap, monkeypatch):
    monkeypatch.setattr(search, "query_kind", lambda v, q: ("event", 0.01))
    r = _end_to_end(write("clicks.wav", clicks_signal()))
    assert r["query_kind"] == "event" and r["n_events"] >= 5
    assert set(r["segments"][0]) == {"start", "end", "duration", "score", "z", "cosine", "verdict", "kind",
                                     "peak_dbfs", "head_db", "tail_db", "cut"}
    assert "edges snapped" in r["summary"] and "one stage" in r["summary"]


@pytest.mark.ml
def test_fnd_11_end_to_end(write):
    r = _end_to_end(write("clicks.wav", clicks_signal()))
    assert r["query_kind"] == "event"
    assert all(len(_contains_click(s)) == 1 for s in r["segments"])


def test_fnd_12_query_kind_thresholds(fake_clap, monkeypatch):
    from hark.listen import vocabulary as voc

    v = voc.get()
    e = np.asarray(v.embed_text(search.EVENT_ANCHORS), np.float64).mean(axis=0)
    t = np.asarray(v.embed_text(search.TEXTURE_ANCHORS), np.float64).mean(axis=0)
    kind, score = search.query_kind(v, e / np.linalg.norm(e))
    assert kind == "event" and score > 0
    kind, score = search.query_kind(v, t / np.linalg.norm(t))
    assert kind in ("texture", "either") and score < 0


@pytest.mark.ml
def test_fnd_12_query_kind():
    from hark.listen import vocabulary as voc

    v = voc.get()

    def read(text):
        return search.query_kind(v, v.embed_text([text])[0])[0]

    assert read("a single mechanical keyboard key press") == "event"
    assert read("heavy rain on a roof") in ("texture", "either")
    assert read("a piece of music") == "texture"


def _range(path):
    r = search.find(path, "a click", start=2.0, end=4.0)
    assert r["duration"] == 2.0
    for s in r["segments"]:
        assert 1.995 <= s["start"] < 4.0
    return r


def test_fnd_13_range_stub(write, fake_clap, monkeypatch):
    monkeypatch.setattr(search, "query_kind", lambda v, q: ("event", 0.01))
    r = _range(write("clicks.wav", clicks_signal()))
    found = {t for s in r["segments"] for t in _contains_click(s)}
    assert {2.5, 3.5} <= found


@pytest.mark.ml
def test_fnd_13_range(write):
    r = _range(write("clicks.wav", clicks_signal()))
    assert {2.5, 3.5} <= {t for s in r["segments"] for t in _contains_click(s)}


def _taps_file(write):
    x = _room(300.0, seed=4)
    rng = np.random.default_rng(5)
    for t in (150.0, 275.0):
        n = np.arange(int(0.6 * SR))
        tap = sum(np.sin(2 * np.pi * f * n / SR) for f in (2200, 3500, 5100)) * np.exp(-n / (0.12 * SR)) * 0.2
        _add(x, t, tap + 0.001 * rng.standard_normal(len(n)))
    return write("taps.wav", x)


@pytest.mark.slow
def test_fnd_14_two_stages_stub(write, fake_clap, monkeypatch):
    monkeypatch.setattr(search, "query_kind", lambda v, q: ("event", 0.01))
    r = search.find(_taps_file(write), "a glass ting", top=3)
    assert "two stages" in r["summary"]
    assert r["n_windows"] <= 41


@pytest.mark.ml
@pytest.mark.slow
def test_fnd_14_two_stages(write):
    r = search.find(_taps_file(write), "a glass ting", top=3)
    assert "two stages" in r["summary"]
    for t in (150.0, 275.0):
        assert any(s["start"] <= t < s["end"] and s["kind"] in ("stroke", "event") for s in r["segments"])


def test_find_errors(write, no_models, no_require, run_cli):
    p = write("clicks.wav", clicks_signal())
    with pytest.raises(ValueError):
        search.find(p, "  ")
    code, _, err = run_cli(search, ["find", p, "x", "--start", "9"])
    assert code == 1 and err.startswith("hark find: error:")


def test_find_texture_windows_only(write, fake_clap, monkeypatch):
    monkeypatch.setattr(search, "query_kind", lambda v, q: ("texture", -0.4))
    r = search.find(write("clicks.wav", clicks_signal()), "rain")
    assert r["n_events"] == 0 and all(s["kind"] == "window" and s["cut"] == "" for s in r["segments"])
    r2 = search.find(write("c2.wav", clicks_signal()), "rain", kind="event")
    assert r2["query_kind"] == "event" and all(s["kind"] != "window" for s in r2["segments"])
