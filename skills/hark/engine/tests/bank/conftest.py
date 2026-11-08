"""Fixtures of the bank tests: test recipes and a fixture bank.

Test recipes follow the recipe contract (ARCHITECTURE 8.2) and are built in
this file, never taken from the package. The fixture ``fixture_bank`` writes 4
small bank files into ``tmp_path`` and points the bank at them.
"""

from __future__ import annotations

import argparse
import json
import types
from pathlib import Path

import numpy as np
import pytest

SR = 48000


# ---------------------------------------------------------------- test recipes (ARCHITECTURE 8.2)


def _t(n: int) -> np.ndarray:
    return np.arange(n) / SR


def _fade_out(x: np.ndarray, ms: float = 5.0) -> np.ndarray:
    k = min(len(x), int(ms * 48))
    ramp = np.linspace(1, 0, k) ** 2
    x[-k:] *= ramp.reshape(-1, *([1] * (x.ndim - 1)))
    return x


def bell_render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A struck sine: 2 ms rise, exponential decay, tiny seeded noise; it dies out by its end (no cut tail)."""
    n = int(round(p["length"] * SR))
    t = _t(n)
    env = (1 - np.exp(-t / 0.002)) * np.exp(-t / p["tau"]) * (1 - t / p["length"]) ** 2
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
    _route_test_fit_kind(monkeypatch)
    return TEST_RECIPES


def _route_test_fit_kind(monkeypatch) -> None:
    """The real fit pipeline knows 6 kinds; teach its kind and knob lookups the test kind ``test_sine``."""
    from hark.fit.pipeline import auto

    real_kind, real_knobs = auto.kind_of, auto.knobs_of

    def kind_of(model):
        return "test_sine" if isinstance(model, dict) and model.get("kind") == "test_sine" else real_kind(model)

    def knobs_of(model):
        if not (isinstance(model, dict) and model.get("kind") == "test_sine"):
            return real_knobs(model)
        vary = {} if model.get("tones") is False else {"vary": ((0.0, 1.0), 0.0, "per-take jitter")}
        return {"gain_db": ((-12.0, 12.0), 0.0, "level"), **vary}

    monkeypatch.setattr(auto, "kind_of", kind_of)
    monkeypatch.setattr(auto, "knobs_of", knobs_of)


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
                             "model": {"kind": "test_sine", "f0": 5000.0}, "seed": 1, "vary": 0.3,
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
    # fit.json waits for the operator's CC0 decision (ARCHITECTURE 15, bank Q5); the other 3 files ship
    if not {"forge04.json", "forge_v2.json", "pluck.json"} <= {p.name for p in folder.glob("*.json")}:
        pytest.skip("needs-bank: the shipped bank files are built by integration (25b)")
    bank.reload()
    return bank


# Gaps of the shipped bank, each one open in the integration report (T-33). Remove an id once it ships clean.
# - forge04 members that fail their take audit, so build --forge04 leaves them out.
OPEN_FORGE04 = frozenset({"mallet/pluck", "whoosh_big/thump_deep"})
# - derived forge_v2 ids with no render whose default take passes sfx check (T-41): the file lists them under dropped.
DROPPED_DERIVED = frozenset({
    "pro_typing_run/blip", "pro_typing_run/double", "pro_typing_run/glass", "pro_typing_run/soft",
    "pro_typing_run/standard", "pro_typing_run/tick",
})


@pytest.fixture
def open_ids() -> dict:
    """The documented gaps of the shipped bank: forge04 members and derived ids left out."""
    return {"forge04": OPEN_FORGE04, "dropped": DROPPED_DERIVED}


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
