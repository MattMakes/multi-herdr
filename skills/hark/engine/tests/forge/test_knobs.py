"""Acceptance tests K1-K11 of forge-core part 5 (knobs), with hand-made entries and fake runs."""

import math

import numpy as np
import pytest

from hark.forge.engine import knobs

SR = 48000


def test_k1_hz():
    assert knobs.hz("A4") == 440.0
    assert abs(knobs.hz("Cs5") - 554.37) < 0.01 and knobs.hz("Cs5") == knobs.hz("C#5")
    assert knobs.hz("440hz") == 440.0 and knobs.hz(" 440 Hz ") == 440.0
    for bad in (True, "H2", 5):
        with pytest.raises(knobs.KnobError):
            knobs.hz(bad)


def test_k2_note_and_label():
    assert knobs.note(622.25) == "Eb5"
    assert knobs.note(452) == "A4 +47c"
    assert knobs.label("Db5") == "Cs5"
    assert knobs.label(445) == "445hz"


def test_k3_fit_length():
    x = np.ones(SR)
    y, note = knobs.fit_length(x, 0.5)
    assert len(y) == 24000 and y[-1] == 0 and note == "trim 500 ms"
    y, note = knobs.fit_length(np.ones(int(0.3 * SR)), 0.5)
    assert len(y) == 24000 and note == "pad 200 ms"


def test_k4_fit_length_keep():
    y, note = knobs.fit_length(np.ones(SR), 0.5, keep_s=0.49)
    k = int(note.split("fade ")[1].split(" ms")[0])
    assert 2 <= k <= 10
    assert np.all(y[: 24000 - 480] == 1.0)


def _whoosh_like(head_db):
    t = np.arange(int(0.6 * SR)) / SR
    env = np.where(t < 0.05, 10 ** (head_db / 20), np.exp(-((t - 0.3) / 0.08) ** 2))
    env[t >= 0.05] = np.maximum(env[t >= 0.05], 10 ** (head_db / 20))
    return np.random.default_rng(0).standard_normal(len(t)) * env * 0.5


def test_k5_place_apex():
    y, note = knobs.place_apex(_whoosh_like(-30), 0.30, 0.25)
    assert note.startswith("head trim") and len(y) == int(0.6 * SR) - 2400
    assert y[0] == 0
    x = _whoosh_like(-10)
    y, note = knobs.place_apex(x, 0.30, 0.25)
    assert y is x and note.startswith("head kept")


def test_k6_validate():
    with pytest.raises(knobs.KnobError):
        knobs.validate({"id": "r1", "category": "riser", "params": {}}, apex=0.5)
    with pytest.raises(knobs.KnobError):
        knobs.validate({"id": "w1", "category": "whoosh_fast", "params": {}}, dur=5.0)


def test_k7_describe_typing_key():
    d = knobs.describe({"id": "k1", "category": "typing_key", "recipe": "nope", "params": {"x": 1}})
    assert d["dur"] == "trim" and d["apex"] is False and d["pitch"] is None
    assert "not tuned" in d["why"]


def test_k8_sub_drop_pitch(monkeypatch):
    monkeypatch.setattr(knobs, "_estimate_pitch", lambda x, near, span=None: near)
    entry = {"id": "s1", "category": "sub_drop", "recipe": "nope", "params": {"f1": 40.0, "f0": 120.0}}
    seen = []

    def run(pe, fk=None):
        seen.append(dict(pe))
        return np.sin(2 * np.pi * 50 * np.arange(SR) / SR)

    x, edits = knobs.apply(entry, run, pitch="G1")
    assert abs(seen[-1]["f1"] - 49.0) < 0.01
    assert abs(seen[-1]["f0"] - 120.0 * seen[-1]["f1"] / 40.0) < 1e-9
    assert edits["pitch"]["ok"] is True


def test_k9_riser_dur():
    entry = {"id": "r1", "category": "riser", "recipe": "nope", "params": {"length": 3.0}}

    def run(pe, fk=None):
        L = pe.get("length", 3.0)
        return np.random.default_rng(1).standard_normal(int(L * 1.05 * SR)) * 0.1

    x, edits = knobs.apply(entry, run, dur=2.0)
    assert len(x) == 96000 and edits["dur"]["method"] == "param"


def test_k10_notify_decay():
    entry = {"id": "n1", "category": "notify", "recipe": "nope", "params": {"ring": 0.4, "f0": 880.0}}
    calls = []

    def run(pe, fk=None):
        calls.append(1)
        ring = pe.get("ring", 0.4)
        t = np.arange(int(2.5 * SR)) / SR
        return np.sin(2 * np.pi * 880 * t) * np.exp(-t / (0.3 * ring)) * np.minimum(1, t / 0.003)

    x, edits = knobs.apply(entry, run, decay=150)
    assert edits["decay"]["ok"] is True
    assert edits["decay"]["renders"] <= 5


def test_k11_pitch_warning():
    w = knobs.pitch_warning({"pitch": {"ok": False, "note": "C5", "measured_hz": 530.0, "cents_off": 23}})
    assert w["code"] == "pitch_off"
    assert knobs.pitch_warning({"pitch": {"ok": True}}) is None
    assert knobs.pitch_warning({"pitch": {"ok": None}}) is None


def test_whoosh_apex_lands(monkeypatch):
    def peak_s(x):
        m = np.abs(np.asarray(x))
        return float(np.argmax(m)) / SR

    monkeypatch.setattr(knobs, "_peak_s", peak_s)
    entry = {"id": "w1", "category": "whoosh_fast", "recipe": "nope",
             "params": {"rise": 0.2, "hold": 0.0, "fall": 0.3, "start_db": -30.0, "rise_k": 1.0}}

    def run(pe):
        p = {**entry["params"], **pe}
        n = int((p["rise"] + p["hold"] + p["fall"]) * SR) + 480
        y = np.zeros(n)
        y[int(p["rise"] * SR * 1.1)] = 1.0
        return y

    x, edits = knobs.apply(entry, run, apex=0.4, dur=1.0)
    assert len(x) == SR
    assert edits["apex"]["ok"] is True and abs(edits["apex"]["err_ms"]) <= 5
