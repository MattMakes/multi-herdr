"""Generated test fixtures for the --self-test of each script.

Test fixtures may be made in code. The recordings-only rule applies to films,
not to these files. Nothing here touches the network.
"""
from __future__ import annotations

import json
import os

import numpy as np

SR = 48000


class Checker:
    def __init__(self):
        self.ok = True

    def __call__(self, name: str, cond) -> bool:
        cond = bool(cond)
        self.ok &= cond
        print(("ok   " if cond else "FAIL ") + name)
        return cond

    def code(self) -> int:
        return 0 if self.ok else 1


def write(path: str, x: np.ndarray, sr: int = SR, subtype: str = "PCM_24") -> str:
    import soundfile as sf
    os.makedirs(os.path.dirname(path), exist_ok=True)
    sf.write(path, np.asarray(x, np.float32), sr, subtype=subtype)
    return path


def put_json(path: str, obj) -> str:
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w") as fh:
        json.dump(obj, fh, indent=1)
    return path


def burst(dur=0.1, f=900.0, decay=40.0, seed=1, amp=0.8, lead=0.0) -> np.ndarray:
    """A decaying tone plus noise burst, with optional leading silence."""
    rng = np.random.default_rng(seed)
    t = np.arange(int(dur * SR)) / SR
    x = amp * np.exp(-decay * t) * (0.7 * np.sin(2 * np.pi * f * t) + 0.3 * rng.standard_normal(len(t)))
    x[: int(0.0005 * SR)] *= np.linspace(0, 1, int(0.0005 * SR))
    x = np.concatenate([np.zeros(int(lead * SR)), x])
    return np.stack([x, x], 1)


def noise_bed(dur=10.0, level=0.1, seed=3) -> np.ndarray:
    rng = np.random.default_rng(seed)
    return level * rng.standard_normal((int(dur * SR), 2))


def hits_track(n=8, gap=0.25, seed=5) -> np.ndarray:
    """A recording of n separate hits, gap seconds apart, each slightly different."""
    parts = []
    for k in range(n):
        b = burst(0.08, 700 + 40 * k, 50, seed + k, 0.5 + 0.04 * k)
        parts += [b, np.zeros((int((gap - 0.08) * SR), 2))]
    return np.concatenate([np.zeros((int(0.05 * SR), 2))] + parts)


def click_track(bpm=120.0, bars=20, beats=4, louder_from=None, sr=SR, seed=9) -> np.ndarray:
    """Clicks on every beat, accented downbeats, a steady chord pad, optionally louder later."""
    rng = np.random.default_rng(seed)
    T = 60.0 / bpm
    n = int(bars * beats * T * sr) + sr
    x = np.zeros(n)
    t = np.arange(n) / sr
    x += 0.02 * (np.sin(2 * np.pi * 220 * t) + np.sin(2 * np.pi * 277.2 * t))
    for k in range(bars * beats):
        s = int(round(k * T * sr))
        down = k % beats == 0
        m = int(0.03 * sr)
        env = np.exp(-np.arange(m) / sr * (60 if down else 120))
        tone = np.sin(2 * np.pi * (60 if down else 1800) * np.arange(m) / sr)
        amp = (0.9 if down else 0.3) * (2.0 if louder_from is not None and k >= louder_from * beats else 1.0)
        x[s:s + m] += amp * env * (tone + 0.2 * rng.standard_normal(m))
    x = np.clip(x, -0.99, 0.99)
    return np.stack([x, x], 1)


def project(d: str) -> dict:
    """A small film project: 2 fragments, a bed, music, cues. Returns the useful paths."""
    a = os.path.join(d, "audio", "assets")
    files = {
        "tap": write(os.path.join(a, "ui", "fx-1_tap.wav"), burst(0.12, 1200, 45, 1, 0.8, 0.02)),
        "pop": write(os.path.join(a, "ui", "fx-2_pop.wav"), burst(0.15, 600, 30, 2, 0.7, 0.02)),
        "keys": write(os.path.join(a, "keyboard", "fx-3_keys.wav"), hits_track(8)),
        "hit": write(os.path.join(a, "impacts", "fx-4_hit.wav"), burst(0.6, 80, 6, 4, 0.9, 0.03)),
        "room": write(os.path.join(a, "ambience", "fx-5_room.wav"), noise_bed(4.0, 0.05)),
        "music": write(os.path.join(a, "music", "fx-6_track.wav"), click_track(120, 8, louder_from=4)),
    }
    events = [{"f": 30, "kind": "click", "id": "demo:cursor:0", "pan": 0.2}]
    events += [{"f": 60 + 9 * k, "kind": "key", "id": f"demo:type:key:{k}", "variant": "word"} for k in range(6)]
    events += [{"f": 180 + 20 * k, "kind": "pop", "id": f"demo:card:{k}", "pan": -0.5 + 0.5 * k} for k in range(3)]
    events += [{"f": 300, "kind": "hit", "id": "demo:reveal:0"}]
    cues = put_json(os.path.join(d, "out", "cues.json"), {"fps": 60, "total": 480, "events": events})
    ui = {"samples": {"ui.tap": {"file": "audio/assets/ui/fx-1_tap.wav", "mono": True, "eq": {"hp": 120}},
                      "ui.pop": {"file": "audio/assets/ui/fx-2_pop.wav", "pitch": 2},
                      "ui.pop2": {"file": "audio/assets/ui/fx-2_pop.wav", "pitch": 3, "eq": {"highshelf": [6000, -3]}}},
          "pools": {"ui.tap": ["ui.tap"], "ui.pop": ["ui.pop", "ui.pop2"]},
          "rules": [{"name": "ui.click", "match": {"kind": "click"}, "pool": "ui.tap", "mode": "fixed", "gain_db": -6},
                    {"name": "ui.pops", "match": {"kind": "pop"}, "pool": "ui.pop", "pitch_ramp": [0, 2],
                     "jitter_db": 1, "pan_scale": 0.3}]}
    kb = {"slices": {"kb.keys": {"file": "audio/assets/keyboard/fx-3_keys.wav", "mono": True}},
          "samples": {"kb.hit": {"file": "audio/assets/impacts/fx-4_hit.wav"}},
          "pools": {"kb.keys": ["kb.keys#*"], "kb.hit": ["kb.hit"]},
          "rules": [{"name": "kb.type", "match": {"kind": "key"}, "pool": "kb.keys", "jitter_db": 1.5,
                     "pitch": [-0.2, 0.2]},
                    {"name": "kb.hit", "match": {"kind": "hit"}, "pool": "kb.hit", "gain_db": -2}],
          "place": [{"sample": "kb.keys#2", "at": 7.0, "gain_db": -8}]}
    amb = {"ambience": [{"name": "am.room", "file": "audio/assets/ambience/fx-5_room.wav", "at": 0.0, "until": 8.0,
                         "gain_db": -12, "width": 0.8}]}
    mu = {"music": {"file": "audio/assets/music/fx-6_track.wav", "src_bpm": 120, "target_bpm": 120,
                    "stretch": "none", "gain_db": -10,
                    "segments": [{"label": "bars 1-4, one take to the end", "src_from": 0.0, "src_to": 8.0,
                                  "at": 0.0, "tail_fade": 1.0}]}}
    paths = {"ui": put_json(os.path.join(d, "audio", "design", "ui.json"), ui),
             "kb": put_json(os.path.join(d, "audio", "design", "kb.json"), kb),
             "amb": put_json(os.path.join(d, "audio", "design", "amb.json"), amb),
             "mu": put_json(os.path.join(d, "audio", "design", "mu.json"), mu)}
    top = {"duration": 8.0, "cues": "out/cues.json",
           "include": ["audio/design/mu.json", "audio/design/amb.json", "audio/design/ui.json", "audio/design/kb.json"],
           "buses": {"_doc": "a comment", "music": {"auto": [[0, 0], [4.8, 0], [5.0, -3]]}, "sfx": {"gain_db": 0}},
           "master": {"lufs": -14, "ceiling": 0.77, "no_glue": True}}
    paths["top"] = put_json(os.path.join(d, "audio", "sounddesign.json"), top)
    paths["cues"] = cues
    paths.update(files)
    paths["root"] = d
    return paths
