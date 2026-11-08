"""Recipe drip: a water drop landing in water, modelled as a rising bubble.

Spec: docs/specs/hark/recipes-impacts-tuned.md section 2.
"""

from __future__ import annotations

from typing import Any

import numpy as np

from hark.forge.recipes import tonal

SR = 48000

TARGET = "a water drop falling into water, a drip"
ALTS = ["a bubble pop", "a mouse click", "a wooden knock", "a synth beep", "a finger snap"]

# key: (range, default)
_TABLE: dict[str, tuple[Any, float]] = {
    "f0": ((250.0, 1600.0, "log"), 520.0),
    "rise": ((0.0, 14.0), 6.0),
    "rtau": ((0.003, 0.03, "log"), 0.008),
    "onset": ((0.001, 0.06, "log"), 0.02),
    "curve": ((0.6, 2.5), 1.0),
    "tau": ((0.015, 0.12, "log"), 0.05),
    "h2": ((0.0, 0.6), 0.25),
    "hdec": ((1.0, 3.0), 1.6),
    "breath": ((-60.0, -2.0), -10.0),
    "bq": ((1.0, 8.0, "log"), 3.0),
    "daughter": ((-60.0, -3.0), -60.0),
    "d_ratio": ((1.3, 2.6), 1.8),
    "d_delay": ((0.004, 0.04), 0.012),
    "click": ((-60.0, -6.0), -24.0),
    "click_hz": ((800.0, 8000.0, "log"), 2500.0),
    "click_ms": ((0.2, 3.0, "log"), 0.6),
    "thump": ((-60.0, -6.0), -20.0),
    "thump_f": ((70.0, 220.0, "log"), 120.0),
    "hp": ((60.0, 300.0, "log"), 80.0),
    "lp": ((2500.0, 14000.0, "log"), 6000.0),
    "room": ((0.0, 0.35), 0.12),
    "rt": ((0.06, 0.5, "log"), 0.18),
    "width": ((0.0, 0.8), 0.15),
    "jit": ((0.0, 1.0), 0.5),
}
SPACE: dict[str, Any] = {k: v[0] for k, v in _TABLE.items()}
DEFAULTS: dict[str, Any] = {k: v[1] for k, v in _TABLE.items()}

FINE: list[str] = [
    "rtau", "onset", "curve", "tau", "h2", "hdec", "breath", "bq", "d_ratio", "d_delay", "click", "click_hz",
    "click_ms", "thump", "thump_f", "hp", "lp", "room", "rt", "width",
]

_ROWS: dict[str, tuple[float, ...]] = {
    "f0": (306.4, 254.5, 694.6, 420, 1150),
    "rise": (8.033, 9.767, 3.332, 10, 0.9682),
    "rtau": (0.00719, 0.00977, 0.01, 0.025, 0.004),
    "onset": (0.02135, 0.01237, 0.03436, 0.02, 0.006),
    "curve": (2.192, 1.123, 0.7727, 1.0, 1.296),
    "tau": (0.0411, 0.03743, 0.03, 0.05, 0.01727),
    "h2": (0.09568, 0.1299, 0.1039, 0.25, 0.3661),
    "hdec": (1.765, 1.755, 2.57, 1.6, 1.6),
    "breath": (-22.71, -26.28, -24.02, -10, -2),
    "bq": (6.109, 1.0, 1.697, 3, 3),
    "daughter": (-3, -3, -8.628, -6, -60),
    "d_ratio": (2.271, 1.976, 1.607, 2.2, 1.8),
    "d_delay": (0.03833, 0.02129, 0.04, 0.03, 0.012),
    "click": (-40.83, -51.96, -44.91, -24, -21.63),
    "click_hz": (2300, 7393, 1378, 2500, 4898),
    "click_ms": (2.804, 0.4349, 0.5007, 0.6, 0.6),
    "thump": (-23.28, -8.339, -39.42, -20, -40),
    "thump_f": (164.8, 155.3, 121.8, 120, 120),
    "hp": (183, 148, 97.33, 80, 200),
    "lp": (10010, 5147, 4560, 6000, 12230),
    "room": (0.2097, 0.1926, 0.07537, 0.12, 0.0),
    "rt": (0.08963, 0.1608, 0.1075, 0.18, 0.12),
    "width": (0.164, 0.297, 0.1842, 0.15, 0.2436),
    "jit": (0.2181, 0.7479, 0.225, 0.5, 0.3),
}
FAMILY_NAMES: dict[str, list[str]] = {"drip": ["drop", "bubble", "plink", "bloop", "tiny"]}
FAMILY: dict[str, list[dict]] = {
    "drip": [{**DEFAULTS, **{k: row[i] for k, row in _ROWS.items()}} for i in range(5)],
}

# T15 targets: key -> (lo, hi, scale), measured with tonal.measure.
STUDY: dict[str, tuple[float, float, float]] = {
    "dur_s": (0.11, 0.33, 0.08),
    "attack_ms": (16, 66, 15),
    "t20_ms": (90, 188, 30),
    "centroid_hz": (379, 1045, 200),
    "lo150": (0.02, 0.25, 0.1),
    "tonal": (0.48, 0.61, 0.1),
    "corr": (0.87, 0.99, 0.1),
}

# Audit allowance: the rising pitch of a bubble is intended.
RISES = True
RISE_MAX_ST = 14.0
RISE_MAX_S = 0.25
RISE_START_S = 0.06


def allows(glide: dict | None) -> bool:
    """True when a rising glide found by the forge audit is a bubble rise and not a synth sweep."""
    if not glide or glide.get("from_s") is None:
        return True
    return (glide["rise_st"] <= RISE_MAX_ST and glide["from_s"] <= RISE_START_S
            and glide["to_s"] - glide["from_s"] <= RISE_MAX_S)


def check(x: np.ndarray, role: str | None = None, sound: str | None = None) -> Any:
    """The forge audit of x for the category drip (the audit applies `allows` itself)."""
    from hark.forge.bank import sfx

    return sfx.check(x, category="drip", role=role, sound=sound)


_FAMILY_BOX = (SPACE, DEFAULTS, FINE)


def family_space(params: dict) -> dict[str, Any]:
    """The pinned family space around 1 member: fine keys keep their full range."""
    space, defaults, fine = _FAMILY_BOX
    return tonal.family_space(space, {**defaults, **params}, fine)


def _on(db: float) -> bool:
    return db > -59


def _n(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def bubble(p: dict, rng: np.random.Generator, n: int, f0: float, tau: float, rise: float) -> np.ndarray:
    """One bubble ring: a rising tone, its 2nd harmonic and the cavity hiss (mono)."""
    t = np.arange(n) / SR
    e = tonal.env(t, p["onset"], tau, 0, p["curve"])
    f = tonal.rise_track(t, f0, rise, p["rtau"])
    y = tonal.tone(f, rng, [(2.0, p["h2"], p["hdec"])], e)
    if _on(p["breath"]):
        y = y + 10 ** (p["breath"] / 20) * tonal.follow_noise(n, rng, f, p["bq"], e)
    return y


def _spread(p: dict, rng: np.random.Generator) -> dict:
    j = p["jit"] * rng.standard_normal(3)
    return {**p, "f0": p["f0"] * np.exp(0.03 * j[0]), "rise": p["rise"] * np.exp(0.1 * j[1]),
            "tau": p["tau"] * np.exp(0.08 * j[2])}


def length_of(p: dict) -> float:
    """Buffer seconds before the finish (at most 1 s)."""
    extra = p["d_delay"] if _on(p["daughter"]) else 0.0
    return min(1.0, p["onset"] + 7 * p["tau"] + extra + 0.02)


def _layers(p: dict, rng: np.random.Generator, y: np.ndarray) -> np.ndarray:
    n = len(y)
    pk = float(np.max(np.abs(y)))
    if _on(p["click"]):
        hz = p["click_hz"]
        y = y + pk * 10 ** (p["click"] / 20) * tonal.burst(n, rng, hz / 2, 2 * hz, p["click_ms"] / 1000, 0.0002)
    if _on(p["thump"]):
        y = y + pk * 10 ** (p["thump"] / 20) * tonal.thump(n, p["thump_f"], 0.012, 0.002)
    return y



def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take: stereo float64 after the T13 finish."""
    p = _spread({**DEFAULTS, **p}, rng)
    n = _n(length_of(p))
    y = bubble(p, rng, n, p["f0"], p["tau"], p["rise"])
    if _on(p["daughter"]):
        k = min(_n(p["d_delay"]), n - 1)
        y[k:] += 10 ** (p["daughter"] / 20) * bubble(p, rng, n - k, p["f0"] * p["d_ratio"], 0.6 * p["tau"],
                                                    0.6 * p["rise"])
    y = _layers(p, rng, y)
    return tonal.finish(y, rng, hp=p["hp"], lp=p["lp"], room_wet=p["room"], rt60=p["rt"], width=p["width"], cut=400)
