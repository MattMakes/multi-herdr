"""Fixtures of the bank tests: stand-ins, test recipes and a fixture bank.

Stand-ins: a module of another unit that is not built yet (core, the forge
engine, the recipe registry, fit) gets a small stand-in here, and only while the
real module is absent (implementer-notes rule 2). A stand-in follows the
ARCHITECTURE.md signature and does just enough for the bank logic. Tests that
need the real knob engine check ``real_knobs()`` and skip on a stand-in.

Test recipes follow the recipe contract (ARCHITECTURE 8.2) and are built in
this file, never taken from the package. The fixture ``fixture_bank`` writes 4
small bank files into ``tmp_path`` and points the bank at them.
"""

from __future__ import annotations

import argparse
import importlib
import importlib.util
import json
import math
import re
import sys
import types
from pathlib import Path

import numpy as np
import pytest

SR = 48000
STAND_INS: list[str] = []


# ---------------------------------------------------------------- stand-in plumbing


def _absent(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is None
    except (ModuleNotFoundError, ValueError):
        return True


def _install(name: str, attrs: dict) -> types.ModuleType:
    mod = types.ModuleType(name)
    mod.__dict__.update(attrs)
    mod.STAND_IN = True
    parent_name, _, leaf = name.rpartition(".")
    if parent_name and parent_name not in sys.modules:
        if _absent(parent_name):
            _install(parent_name, {"__path__": []})
        else:
            importlib.import_module(parent_name)
    sys.modules[name] = mod
    if parent_name:
        setattr(sys.modules[parent_name], leaf, mod)
    STAND_INS.append(name)
    return mod


def _maybe(name: str, build) -> None:
    if _absent(name):
        _install(name, build())


# ---------------------------------------------------------------- core stand-ins


def _audio() -> dict:
    import soundfile
    import soxr

    class AudioError(RuntimeError):
        pass

    def resample(x, sr_from, sr_to):
        y = np.asarray(x, dtype=np.float32)
        return y if sr_from == sr_to else np.asarray(soxr.resample(y, sr_from, sr_to, quality="VHQ"), np.float32)

    def load(path, sr=48000, mono=False, start=None, end=None, cache=True):
        try:
            x, fs = soundfile.read(str(path), dtype="float32")
        except Exception as exc:  # noqa: BLE001
            raise AudioError(f"{path}: {exc}") from None
        x = resample(x, fs, sr)
        return x.mean(axis=1) if mono and x.ndim == 2 else x

    def write_wav(path, x, sr=48000, subtype="PCM_24"):
        Path(path).parent.mkdir(parents=True, exist_ok=True)
        soundfile.write(str(path), np.asarray(x), int(sr), subtype=subtype)
        return str(path)

    def to_mono(x):
        x = np.asarray(x)
        return x.mean(axis=1) if x.ndim == 2 else x

    return {"SR": SR, "Audio": np.ndarray, "AudioError": AudioError, "load": load, "resample": resample,
            "write_wav": write_wav, "to_mono": to_mono}


def _features() -> dict:
    from scipy.signal import lfilter

    shelf = ([1.53512485958697, -2.69169618940638, 1.19839281085285], [1.0, -1.69065929318241, 0.73248077421585])
    high = ([1.0, -2.0, 1.0], [1.0, -1.99004745483398, 0.99007225036621])

    def k_weight(x, sr=48000):
        return lfilter(*high, lfilter(*shelf, np.asarray(x, dtype=np.float64), axis=0), axis=0)

    return {"k_weight": k_weight}


def _report() -> dict:
    def add_common(parser):
        parser.add_argument("--json", action="store_true")
        parser.add_argument("--out", dest="out")
        parser.add_argument("--threads", type=int)

    def clean(obj):
        return json.loads(json.dumps(obj, default=lambda v: v.item() if hasattr(v, "item") else str(v)))

    def emit(result, args, text=None):
        body = json.dumps(clean(result), indent=1)
        print(body if getattr(args, "json", False) else (text or result.get("summary", "")))
        if getattr(args, "out", None):
            Path(args.out).write_text(body + "\n", encoding="utf-8")
        return result

    return {"add_common": add_common, "emit": emit, "clean": clean}


def _config() -> dict:
    import os

    return {"HOME": Path(os.environ.get("HARK_HOME") or Path.home() / ".cache" / "hark")}


# ---------------------------------------------------------------- forge engine stand-ins


def _dsp() -> dict:
    return {"rng": lambda seed: np.random.default_rng(int(seed) & 0xFFFFFFFF)}


def _db(v: float, ref: float) -> float:
    return round(20 * math.log10(v / ref + 1e-12), 1)


def _polish() -> dict:
    def peak_of(x):
        return float(np.max(np.abs(x))) or 1e-12

    def clicks(x, thr=18.0):
        x = np.asarray(x, dtype=np.float64)
        return {"clicks": 0, "click_times": [], "edge_db": _db(max(abs(x[0]), abs(x[-1])), peak_of(x))}

    def dc(x):
        return {"dc_db": _db(abs(float(np.mean(x))), peak_of(x)), "infra_db": None}

    def truncated(x):
        x = np.asarray(x, dtype=np.float64)
        tail = x[-96:]
        return {"trunc_db": _db(float(np.sqrt(np.mean(tail * tail))), peak_of(x)), "end_db": None}

    def silence(x):
        x = np.asarray(x, dtype=np.float64)
        live = np.nonzero(np.abs(x) >= peak_of(x) * 1e-3)[0]
        quiet = (np.abs(x[live[0]: live[-1] + 1]) < 1e-6).astype(int)
        edges = np.diff(np.concatenate([[0], quiet, [0]]))
        runs = np.nonzero(edges == -1)[0] - np.nonzero(edges == 1)[0]
        long = runs[runs >= 96]
        return {"silence_ms": round(float(long.sum()) / 48, 1), "silence_max_ms": round(float(runs.max(initial=0)) / 48, 1)}

    def clipping(x):
        x = np.asarray(x, dtype=np.float64).reshape(len(x), -1)
        runs = 0
        for ch in x.T:
            top = np.isclose(np.abs(ch), np.abs(ch).max()) & (np.abs(ch) >= 0.999)
            edges = np.diff(np.concatenate([[0], top.astype(int), [0]]))
            runs += int(np.sum((np.nonzero(edges == -1)[0] - np.nonzero(edges == 1)[0]) >= 3))
        return {"clip_runs": runs, "overs": int(np.sum(np.abs(x) > 1.0))}

    def attack(x):
        return {"attack_ms": round(float(np.argmax(np.abs(x))) / 48, 2), "onset_hf_db": None}

    def body(x):
        x = np.asarray(x, dtype=np.float64)
        return {"crest_db": _db(peak_of(x), float(np.sqrt(np.mean(x * x))) or 1e-12)}

    return {"clicks": clicks, "dc": dc, "truncated": truncated, "silence": silence, "clipping": clipping,
            "attack": attack, "body": body, "ring": lambda x, legacy=False: {"ring_z": 0.0, "ring_ms": 0.0},
            "tail": lambda x: {"bump_db": 0.0, "decay_db_s": None, "t40_ms": None},
            "resonances": lambda x, top=3: {"res_db": 0.0, "res_hz": None, "res_peaks": []}}


WHOOSH = ("whoosh_fast", "whoosh_big", "pro_whoosh_big", "swish", "swipe")
NOISE = ("whoosh_fast", "whoosh_big", "swish", "swipe", "glitch", "pro_glitch", "typing_key", "pro_typing_key",
         "pro_typing_run")
PITCH = {"notify": ("f0", 220.0, 3200.0), "pluck": ("f0", 130.0, 2100.0)}
NAMES = ("C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B")


def _knobs() -> dict:
    class KnobError(ValueError):
        pass

    def midi(p):
        m = re.fullmatch(r"([A-Ga-g])([#sb]{0,2})(-?\d+)", str(p).strip())
        if not m:
            return None
        pc = {"C": 0, "D": 2, "E": 4, "F": 5, "G": 7, "A": 9, "B": 11}[m.group(1).upper()]
        acc = m.group(2)
        return 12 * (int(m.group(3)) + 1) + pc + acc.count("#") + acc.count("s") - acc.count("b")

    def hz(p):
        if isinstance(p, bool):
            raise KnobError("a pitch must be a note or Hz")
        if isinstance(p, (int, float)):
            f = float(p)
        elif midi(p) is not None:
            f = 440.0 * 2 ** ((midi(p) - 69) / 12)
        else:
            m = re.fullmatch(r"\s*(\d+(?:\.\d+)?)\s*(hz)?\s*", str(p), re.I)
            if not m:
                raise KnobError(f"pitch {p!r}: give a note (E5, Fs4) or Hz (440, 440hz)")
            f = float(m.group(1))
        if not 8 <= f <= 20000:
            raise KnobError(f"pitch {p!r} is outside 8 Hz - 20 kHz")
        return f

    def note(f):
        n = 69 + 12 * math.log2(f / 440)
        k = round(n)
        cents = round(100 * (n - k))
        name = f"{NAMES[k % 12]}{k // 12 - 1}"
        return name + (f" {cents:+d}c" if abs(cents) >= 5 else "")

    def label(p):
        f = hz(p)
        n = 69 + 12 * math.log2(f / 440)
        if abs(n - round(n)) < 0.05:
            return note(440 * 2 ** ((round(n) - 69) / 12)).replace("#", "s")
        return f"{round(f, 2):g}hz"

    def describe(entry):
        cat = entry.get("category")
        spec = PITCH.get(cat)
        pitch = None if spec is None else {"param": spec[0], "native_hz": entry["params"][spec[0]],
                                           "note": note(entry["params"][spec[0]]), "range_hz": [spec[1], spec[2]]}
        out = {"dur": "shape" if cat in WHOOSH else "trim", "apex": cat in WHOOSH, "pitch": pitch,
               "tonal": {"decay": None, "onset": None, "bright": None, "width": None}}
        if pitch is None:
            out["why"] = "a noise sound has no pitch" if cat in NOISE else "this category has no pitch parameter"
        return out

    def validate(entry, dur=None, apex=None, pitch=None, **_):
        sid, cat = entry.get("id"), entry.get("category")
        if apex is not None and cat not in WHOOSH:
            raise KnobError(f"{sid}: apex works only on the whoosh family; use dur and align on end")
        if dur is not None and not 0.01 <= dur <= (4.4 if cat in WHOOSH else 60):
            raise KnobError(f"{sid}: dur {dur} is out of range")
        if apex is not None and not (0.01 <= apex <= 4.3 and (dur is None or apex < dur - 0.02)):
            raise KnobError(f"{sid}: apex {apex} is out of range")
        if pitch is not None:
            f = hz(pitch)
            if cat in NOISE:
                raise KnobError(f"{sid}: a noise sound has no pitch")
            spec = PITCH.get(cat)
            if spec is None:
                raise KnobError(f"{sid}: this category has no pitch parameter")
            if not spec[1] <= f <= spec[2]:
                raise KnobError(f"{sid}: pitch {note(f)} ({f:.1f} Hz) is outside {spec[1]:g}-{spec[2]:g} Hz")

    def fit_length(x, dur, keep_s=None):
        n = int(round(dur * SR))
        x = np.asarray(x, dtype=np.float64)
        if len(x) <= n:
            pad = np.zeros((n - len(x),) + x.shape[1:])
            return np.concatenate([x, pad]), f"pad {round((n - len(x)) / 48)} ms"
        y = x[:n].copy()
        k = min(n, max(480, min(7200, n // 4)))
        fade = 0.5 * (1 + np.cos(np.linspace(0, np.pi, k)))
        y[-k:] *= fade.reshape(-1, *([1] * (y.ndim - 1)))
        return y, f"trim {round((len(x) - n) / 48)} ms"

    def place_apex(x, pk, apex):
        shift = int(round((apex - pk) * 1000)) * 48
        x = np.asarray(x, dtype=np.float64)
        if shift >= 0:
            return np.concatenate([np.zeros((shift,) + x.shape[1:]), x]), f"head pad {shift // 48} ms"
        return x[-shift:], f"head trim {-shift // 48} ms"

    def apply(entry, run, dur=None, apex=None, pitch=None, **_):
        from hark.forge.bank import bank

        validate(entry, dur=dur, apex=apex, pitch=pitch)
        edits, edit = {}, {}
        if pitch is not None:
            param = PITCH[entry["category"]][0]
            edit[param] = hz(pitch)
        x = run(edit) if edit else run()
        if apex is not None:
            x, fit = place_apex(x, bank.times(x)["peak_s"], apex)
            edits["apex"] = {"target": apex, "fit": fit}
        if dur is not None:
            x, fit = fit_length(x, dur)
            edits["dur"] = {"target": dur, "method": describe(entry)["dur"], "renders": 1, "fit": fit}
        if apex is not None:
            m = bank.times(x)["peak_s"]
            edits["apex"].update(measured=m, err_ms=round(1000 * (m - apex), 1), ok=abs(m - apex) <= 0.005)
        if pitch is not None:
            f = hz(pitch)
            got = bank.estimate_pitch(x, near_hz=f, span_st=2.0)
            cents = None if got is None else round(1200 * math.log2(got / f))
            edits["pitch"] = {"target_hz": f, "note": note(f), "param": param, "params": edit, "measured_hz": got,
                              "cents_off": cents, "ok": cents is not None and abs(cents) <= 50}
        return x, edits

    def pitch_warning(edits):
        p = (edits or {}).get("pitch") or {}
        if p.get("ok") is not False:
            return None
        if p.get("measured_hz") is None:
            return {"code": "pitch_off", "msg": f"pitch {p.get('note')} missed: the render has no clear partial"}
        return {"code": "pitch_off", "msg": f"pitch {p.get('note')} missed: measured {p['measured_hz']:.1f} Hz"}

    return {"KnobError": KnobError, "midi": midi, "hz": hz, "note": note, "label": label, "describe": describe,
            "validate": validate, "apply": apply, "fit_length": fit_length, "place_apex": place_apex,
            "pitch_warning": pitch_warning}


def _schema() -> dict:
    class NotReproducible(ValueError):
        pass

    def resolve(b):
        from hark.forge import recipes

        return getattr(recipes.load(b["recipe"]), b.get("fn", "render"))

    def raw(b):
        from hark.forge.engine import dsp

        x = np.asarray(resolve(b)(dict(b["params"]), dsp.rng(b.get("seed", 0))), dtype=np.float64)
        return None if not x.size or float(np.max(np.abs(x))) < 1e-7 else x

    def level(x, b):
        gain = b.get("gain", 10 ** (b.get("gain_db", 0.0) / 20))
        return (x * (10 ** (b.get("peak_db", -1.0) / 20) / np.max(np.abs(x))) * gain).astype(np.float32)

    def rerender(b):
        x = raw(b)
        if x is None:
            raise NotReproducible("the render failed")
        return level(x, b)

    def compare(x, y):
        x, y = np.asarray(x), np.asarray(y)
        if x.ndim == 1:
            x = x[:, None]
        if y.ndim == 1:
            y = y[:, None]
        return float(np.max(np.abs(x - y))) if x.shape == y.shape else math.inf

    def block(report, pick):
        return {**pick["render"]}

    def render_ref(fn):
        return None

    return {"NotReproducible": NotReproducible, "resolve": resolve, "raw": raw, "level": level,
            "rerender": rerender, "compare": compare, "block": block, "render_ref": render_ref, "TOL": 1e-6}


def _recipes() -> dict:
    class UnknownRecipe(KeyError):
        pass

    def load(name):
        raise UnknownRecipe(name)

    def seed_path(name):
        return Path("/nonexistent") / f"{name}.json"

    return {"FOLDERS": ("ui", "impacts", "motion", "risers"), "UnknownRecipe": UnknownRecipe, "load": load,
            "names": lambda: [], "seed_path": seed_path}


def _fit() -> dict:
    def render(model, seed=0, take=None, bed=True, **knobs):
        raise ValueError(f"no fit renderer for kind {model.get('kind')!r}")

    def load(path):
        return json.loads(Path(path).read_text(encoding="utf-8"))

    return {"render": render, "load": load}


def _fit_auto() -> dict:
    return {"kind_of": lambda model: model.get("kind"),
            "knobs_of": lambda model: {"vary": 0.0} if model.get("tones", True) else {}}


_maybe("hark.core.audio", _audio)
_maybe("hark.core.features", _features)
_maybe("hark.core.report", _report)
_maybe("hark.core.config", _config)
_maybe("hark.forge.engine.dsp", _dsp)
_maybe("hark.forge.engine.polish", _polish)
_maybe("hark.forge.engine.knobs", _knobs)
_maybe("hark.forge.engine.schema", _schema)
_maybe("hark.forge.recipes", _recipes)
_maybe("hark.forge.recipes.tonal", lambda: {"measure": lambda x: {"dur_s": len(x) / SR, "corr": 1.0}})
_maybe("hark.fit", _fit)
_maybe("hark.fit.pipeline.auto", _fit_auto)


def real_knobs() -> bool:
    """True when the knob engine is the real T-16 module."""
    from hark.forge.engine import knobs

    return not getattr(knobs, "STAND_IN", False)


# ---------------------------------------------------------------- test recipes (ARCHITECTURE 8.2)


def _t(n: int) -> np.ndarray:
    return np.arange(n) / SR


def _fade_out(x: np.ndarray, ms: float = 5.0) -> np.ndarray:
    k = min(len(x), int(ms * 48))
    ramp = np.linspace(1, 0, k) ** 2
    x[-k:] *= ramp.reshape(-1, *([1] * (x.ndim - 1)))
    return x


def bell_render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A struck sine: 2 ms rise, exponential decay, tiny seeded noise."""
    n = int(round(p["length"] * SR))
    t = _t(n)
    env = (1 - np.exp(-t / 0.002)) * np.exp(-t / p["tau"])
    x = np.sin(2 * np.pi * p["f0"] * t + rng.uniform(0, 0.2)) * env + 1e-4 * rng.standard_normal(n) * env
    return _fade_out(x)


def air_render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A stereo noise pass that rises to its peak at ``rise`` s, then falls."""
    n = int(round((p["rise"] + p["fall"]) * SR))
    t = _t(n)
    env = np.where(t < p["rise"], (t / p["rise"]) ** 2, np.exp(-(t - p["rise"]) / (p["fall"] / 5)))
    x = rng.standard_normal((n, 2)) * env[:, None] * 0.3
    x[:, 1] = 0.7 * x[:, 1] + 0.3 * x[:, 0]
    return _fade_out(x)


def double_render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Two noise bursts ``gap`` s apart."""
    n = int(round(p["length"] * SR))
    t = _t(n)
    x = 1e-4 * rng.standard_normal(n) * np.exp(-t / 0.2)
    for start in (0.01, 0.01 + p["gap"]):
        on = t >= start
        x[on] += rng.standard_normal(int(on.sum())) * np.exp(-(t[on] - start) / 0.006)
    return _fade_out(x)


def _recipe(render, defaults: dict) -> types.SimpleNamespace:
    space = {k: (v * 0.5, v * 2.0) for k, v in defaults.items()}
    return types.SimpleNamespace(SPACE=space, DEFAULTS=dict(defaults), TARGET="a test sound", ALTS=["noise"],
                                 render=render, __name__="test_recipe")


TEST_RECIPES = {
    "t_bell": _recipe(bell_render, {"f0": 1000.0, "tau": 0.08, "length": 0.4}),
    "t_air": _recipe(air_render, {"rise": 0.12, "fall": 0.25}),
    "t_double": _recipe(double_render, {"gap": 0.12, "length": 0.4}),
}


TEST_RECIPES["t_bell"].FAMILIES = {"notify": [("m9", {"f0": 880.0, "tau": 0.1}), ("bad", {"length": 0.0})]}


def fit_render(model: dict, seed: int = 0, take=None, bed=True, **knobs) -> np.ndarray:
    """The test fit kind ``test_sine``: a short struck sine; ``vary`` jitters it per seed."""
    rng = np.random.default_rng(seed)
    p = {"f0": model["f0"] * (1 + knobs.get("vary", 0.0) * 0.01 * rng.uniform(-1, 1)), "tau": 0.03, "length": 0.15}
    return bell_render(p, rng)


@pytest.fixture(autouse=True)
def test_recipes(monkeypatch):
    """Route the test recipe names and the test fit kind to the renders of this file."""
    from hark.forge import recipes

    import hark.fit

    real_load, real_fit = recipes.load, hark.fit.render
    monkeypatch.setattr(recipes, "load", lambda name: TEST_RECIPES[name] if name in TEST_RECIPES else real_load(name))

    def fit(model, seed=0, *a, **kw):
        return fit_render(model, seed, **kw) if model.get("kind") == "test_sine" else real_fit(model, seed, *a, **kw)

    monkeypatch.setattr(hark.fit, "render", fit)
    return TEST_RECIPES


# ---------------------------------------------------------------- the fixture bank


def forge_entry(cat: str, member: str, recipe: str, seed: int, **params) -> tuple[str, dict]:
    base = dict(TEST_RECIPES[recipe].DEFAULTS)
    base.update(params)
    return f"{cat}/{member}", {"type": "forge", "category": cat, "member": member, "recipe": recipe, "fn": "render",
                               "params": base, "seed": seed, "peak_db": -1.0, "gain_db": 0.0, "mode": "designed"}


FIXTURE_FILES = {
    "forge_v2": ("fv2t", [
        forge_entry("notify", "m0", "t_bell", 11, f0=1046.5, tau=0.12, length=0.6),
        forge_entry("notify", "low", "t_bell", 12, f0=523.25, tau=0.12, length=0.6),
        forge_entry("click", "soft", "t_bell", 13, f0=2900.0, tau=0.012, length=0.08),
        forge_entry("pro_click", "glass", "t_bell", 14, f0=3000.0, tau=0.015, length=0.1),
        forge_entry("whoosh_fast", "crisp", "t_air", 15),
        forge_entry("whoosh_fast", "tight", "t_air", 16, rise=0.08, fall=0.2),
        forge_entry("whoosh_big", "deep", "t_air", 17, rise=0.3, fall=0.5),
        forge_entry("pro_whoosh_big", "swell", "t_air", 18, rise=0.25, fall=0.4),
        forge_entry("impact", "double", "t_double", 19),
    ]),
    "forge04": ("f04t", [
        forge_entry("ui_pop", "soft", "t_bell", 21, f0=550.0, tau=0.03, length=0.14),
        forge_entry("knock", "wood", "t_bell", 22, f0=400.0, tau=0.02, length=0.15),
    ]),
    "fit": ("fit1t", [
        ("fit/tick/domino", {"type": "fit", "category": "fit", "member": "tick/domino",
                             "model": {"kind": "test_sine", "f0": 5000.0}, "seed": 0, "vary": 0.3,
                             "source": {"id": "t", "title": "t", "author": "t", "license": "CC0", "url": "t"},
                             "fit": {"kind": "test_sine", "chosen": None, "class": "struck"}, "kinds": ["tick"],
                             "notes": "a test fit", "pitch_hz": None}),
    ]),
    "pluck": ("pluck1t", [forge_entry("pluck", "kalimba", "t_bell", 8001, f0=523.2511306011972, length=0.5)]),
}


def write_fixture_bank(folder: Path) -> None:
    """Render every fixture entry, store its meta, and write the 4 files."""
    from hark.forge.bank import bank

    folder.mkdir(parents=True, exist_ok=True)
    for name, (tag, entries) in FIXTURE_FILES.items():
        doc = {"bank": 1, "name": name, "version": tag, "hark": "0.1.0", "note": "test bank", "categories": {},
               "entries": {}}
        for sound_id, entry in entries:
            entry = json.loads(json.dumps(entry))
            raw = bank.renderer({**entry, "id": sound_id})()
            entry["meta"] = bank.meta_of(bank.normalize(raw, -1.0).astype(np.float32))
            doc["entries"][sound_id] = entry
        bank.write_bank(folder / f"{name}.json", doc)


@pytest.fixture
def fixture_bank(tmp_path, monkeypatch, hark_home):
    """The bank reads only the 4 fixture files of ``tmp_path/bankdata``."""
    from hark.forge.bank import bank

    folder = tmp_path / "bankdata"
    monkeypatch.setattr(bank, "BANK_DIR", folder)
    monkeypatch.delenv("HARK_BANK", raising=False)
    bank.reload()
    write_fixture_bank(folder)
    bank.reload()
    yield bank
    bank.reload()


@pytest.fixture
def shipped():
    """The package bank, or a ``needs-bank`` skip until integration builds it (ARCHITECTURE 12.1)."""
    from hark.forge.bank import bank

    folder = Path(bank.PKG_DIR) / "data"
    if sorted(p.name for p in folder.glob("*.json")) != ["fit.json", "forge04.json", "forge_v2.json", "pluck.json"]:
        pytest.skip("needs-bank: the shipped bank files are built by integration (25b)")
    return bank


@pytest.fixture
def cli():
    """Run ``hark sfx`` / ``hark kit`` through their own ``add_cli`` parsers: returns (exit code, stdout)."""
    from hark.forge.bank import kit, sfx

    parser = argparse.ArgumentParser(prog="hark")
    sub = parser.add_subparsers(dest="command")
    sfx.add_cli(sub)
    kit.add_cli(sub)

    def run(argv: list[str]) -> int:
        args = parser.parse_args(argv)
        code = args.func(args)
        return 0 if code is None else code

    return run
