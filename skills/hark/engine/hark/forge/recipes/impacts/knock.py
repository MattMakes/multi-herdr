"""Recipe knock: a struck wood or plastic body (modal synthesis).

Spec: docs/specs/hark/recipes-impacts-hits.md section 4.
"""

from __future__ import annotations

from typing import Any

import numpy as np

from hark.forge.recipes import tonal

SR = 48000

TARGET = "a knock on a wooden block"
ALTS = ["a door knock", "a mouse click", "a hand clap", "a bubble pop", "a metal clank"]

# Mode tables: (ratio to f1, amplitude, decay multiplier).
MATERIALS: dict[str, tuple[tuple[float, float, float], ...]] = {
    "block": ((1.0, 1.0, 1.0), (1.48, 0.55, 0.6), (2.15, 0.45, 0.45), (3.4, 0.25, 0.3), (4.6, 0.12, 0.2)),
    "shell": ((1.0, 0.7, 1.0), (1.31, 1.0, 0.8), (1.77, 0.8, 0.6), (2.43, 0.6, 0.5), (3.1, 0.4, 0.35),
              (4.2, 0.25, 0.25), (5.6, 0.15, 0.2)),
    "tok": ((1.0, 1.0, 1.0), (2.7, 0.15, 0.3), (5.1, 0.05, 0.15)),
    "desk": ((1.0, 1.0, 1.0), (1.9, 0.6, 0.5), (2.8, 0.5, 0.4), (4.4, 0.3, 0.25), (6.3, 0.15, 0.15)),
}

# key: (range, default)
_TABLE: dict[str, tuple[Any, Any]] = {
    "mat": (["block", "shell", "tok", "desk"], "block"),
    "f1": ((250.0, 2600.0, "log"), 620.0),
    "decay": ((0.004, 0.12, "log"), 0.03),
    "slope": ((-0.4, 1.4), 0.6),
    "tilt": ((-1.0, 1.0), 0.0),
    "cloud": ((-60.0, 6.0), -60.0),
    "cloud_hi": ((2.0, 10.0, "log"), 4.0),
    "cloud_dec": ((0.3, 2.0, "log"), 0.8),
    "jitter": ((0.0, 0.02), 0.006),
    "bend": ((0.0, 4.0), 0.05),
    "bend_ms": ((1.0, 80.0, "log"), 4.0),
    "contact": ((0.0002, 0.004, "log"), 0.0012),
    "grit": ((0.0, 1.0), 0.4),
    "noise": ((-60.0, 6.0), -18.0),
    "noise_hz": ((300.0, 6000.0, "log"), 1800.0),
    "noise_ms": ((0.3, 20.0, "log"), 1.5),
    "flam": ((-60.0, -2.0), -60.0),
    "flam_ms": ((4.0, 70.0), 14.0),
    "bounce": ((-60.0, -2.0), -60.0),
    "bounce_ms": ((6.0, 60.0), 20.0),
    "body": ((-60.0, 6.0), -20.0),
    "body_f": ((50.0, 260.0, "log"), 140.0),
    "body_ms": ((4.0, 60.0, "log"), 15.0),
    "hp": ((40.0, 400.0, "log"), 70.0),
    "lp": ((2500.0, 16000.0, "log"), 8000.0),
    "room": ((0.0, 0.3), 0.08),
    "rt": ((0.05, 0.4, "log"), 0.12),
    "width": ((0.0, 0.6), 0.08),
    "jit": ((0.0, 1.0), 0.5),
}
SPACE: dict[str, Any] = {k: v[0] for k, v in _TABLE.items()}
DEFAULTS: dict[str, Any] = {k: v[1] for k, v in _TABLE.items()}

FINE: list[str] = [
    "decay", "slope", "tilt", "cloud", "cloud_hi", "cloud_dec", "jitter", "bend_ms", "contact", "grit", "noise",
    "noise_hz", "noise_ms", "flam", "flam_ms", "bounce", "bounce_ms", "body_f", "body_ms", "hp", "lp", "room",
    "rt", "width",
]

_ROWS: dict[str, tuple] = {
    "mat": ("block", "block", "block", "block", "tok"),
    "f1": (400, 589.7, 1055, 699.2, 537),
    "decay": (0.00898, 0.0042, 0.00446, 0.02025, 0.005),
    "slope": (1.4, 1.4, 0.9923, 0.552, -0.00403),
    "tilt": (0.4048, 0.2389, 0.4321, 0.08284, 0.5245),
    "cloud": (-9.5, -60, -53.72, -0.8436, -50.47),
    "cloud_hi": (6.712, 6.513, 10, 5.867, 3.995),
    "cloud_dec": (0.8552, 0.5565, 0.4418, 0.9966, 1.394),
    "jitter": (0.01331, 0.00101, 0.01216, 0.01775, 0.01402),
    "bend": (2.919, 3.485, 4.0, 2.157, 1.762),
    "bend_ms": (3.488, 5.989, 1.017, 71.36, 9.208),
    "contact": (0.00076, 0.004, 0.004, 0.00066, 0.00244),
    "grit": (0.6466, 0.9598, 1.0, 0.894, 0.6739),
    "noise": (0.01577, -29.01, -21.06, 3.033, -27.15),
    "noise_hz": (300, 304.7, 1316, 1279, 2665),
    "noise_ms": (0.3, 9.585, 11.51, 14.94, 3.251),
    "flam": (-42.5, -26.35, -15.46, -20.16, -2.088),
    "flam_ms": (12.92, 48.96, 38.32, 12.09, 55.14),
    "bounce": (-19.75, -10.06, -11.12, -47.54, -14.68),
    "bounce_ms": (44.74, 18.4, 21.61, 16.56, 28.8),
    "body": (-11.36, -48.31, -51.53, 4.088, -15.67),
    "body_f": (102, 177.7, 180.6, 95, 179.5),
    "body_ms": (4.521, 38.28, 23.32, 28.8, 4.703),
    "hp": (107.2, 40, 94.95, 70, 169.2),
    "lp": (11210, 3815, 5813, 4889, 7249),
    "room": (0.1648, 0.03, 0.03, 0.09192, 0.00579),
    "rt": (0.1519, 0.08539, 0.08, 0.276, 0.2958),
    "width": (0.2037, 0.2528, 0.2403, 0.5584, 0.3936),
    "jit": (0.1328, 0.2616, 0.2051, 0.8611, 0.8699),
}
FAMILY_NAMES: dict[str, list[str]] = {"knock": ["wood", "plastic", "snap", "desk", "double"]}
FAMILY: dict[str, list[dict]] = {
    "knock": [{**DEFAULTS, **{k: row[i] for k, row in _ROWS.items()}} for i in range(5)],
}

# T15 targets: key -> (lo, hi, scale), measured with tonal.measure.
STUDY: dict[str, tuple[float, float, float]] = {
    "dur_s": (0.11, 0.26, 0.08),
    "attack_ms": (10, 28, 10),
    "t20_ms": (12, 131, 30),
    "centroid_hz": (422, 1327, 300),
    "lo150": (0.02, 0.57, 0.1),
    "tonal": (0.42, 0.55, 0.1),
    "corr": (0.94, 0.98, 0.1),
}


def family_space(params: dict) -> dict[str, Any]:
    """The pinned family space around 1 member: fine keys keep their full range."""
    return tonal.family_space(SPACE, {**DEFAULTS, **params}, FINE)


def _on(db: float) -> bool:
    return db > -59


def _material(name: str) -> tuple[tuple[float, float, float], ...]:
    if name not in MATERIALS:
        raise ValueError(f"knock: unknown material {name!r}")
    return MATERIALS[name]


def _main_modes(p: dict) -> tuple[list[float], list[float], list[float]]:
    freqs, amps, taus = [], [], []
    for q, a, dm in _material(p["mat"]):
        f = p["f1"] * q
        if f >= 0.42 * SR:
            continue
        freqs.append(f)
        amps.append(a * q ** p["tilt"])
        taus.append(max(0.0008, p["decay"] * dm * q ** -p["slope"]))
    return freqs, amps, taus


def _peak(x: np.ndarray) -> float:
    return float(np.max(np.abs(x))) + 1e-12


def _cloud(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    child = np.random.default_rng(int(rng.integers(0, 2 ** 31)))
    f1 = p["f1"]
    hi = min(f1 * p["cloud_hi"], 0.4 * SR)
    fc = np.exp(child.uniform(np.log(1.1 * f1), np.log(max(hi, 1.2 * f1)), 28))
    tc = np.maximum(0.0008, p["decay"] * p["cloud_dec"] * (fc / f1) ** -p["slope"])
    ac = child.uniform(0.3, 1.0, 28) * (fc / f1) ** (p["tilt"] - 0.5)
    return tonal.modes(n, child, list(fc), list(ac), list(tc), 0, p["bend"], p["bend_ms"] / 1000)


def hit(p: dict, rng: np.random.Generator, n: int) -> np.ndarray:
    """One strike, mono, n samples, contact at sample 0."""
    freqs, amps, taus = _main_modes(p)
    y = tonal.modes(n, rng, freqs, amps, taus, p["jitter"], p["bend"], p["bend_ms"] / 1000)
    if _on(p["cloud"]):
        c = _cloud(p, n, rng)
        y = y / _peak(y) + 10 ** (p["cloud"] / 20) * c / _peak(c)
    y = tonal.strike(y, rng, p["contact"], p["grit"])[:n]
    y = y / _peak(y)
    if _on(p["noise"]):
        hz = p["noise_hz"]
        y = y + 10 ** (p["noise"] / 20) * tonal.burst(n, rng, hz / 2, 2 * hz, p["noise_ms"] / 1000, 0.0002)
    return y


def _spread(p: dict, rng: np.random.Generator) -> dict:
    j = p["jit"] * rng.standard_normal(3)
    return {**p, "f1": p["f1"] * np.exp(0.02 * j[0]), "decay": p["decay"] * np.exp(0.1 * j[1]),
            "contact": p["contact"] * np.exp(0.15 * j[2])}


def _ring(p: dict) -> float:
    ring = p["decay"] * max(dm * q ** -p["slope"] for q, _, dm in _material(p["mat"]))
    if _on(p["cloud"]):
        s = p["slope"]
        ring = max(ring, p["decay"] * p["cloud_dec"] * max(1.1 ** -s, p["cloud_hi"] ** -s))
    return ring


def length_of(p: dict) -> float:
    """Buffer seconds before the finish (at most 1 s)."""
    lead = p["flam_ms"] / 1000 if _on(p["flam"]) else 0.0
    bounce = p["bounce_ms"] / 1000 if _on(p["bounce"]) else 0.0
    body = p["body_ms"] / 1000 if _on(p["body"]) else 0.0
    return min(1.0, lead + bounce + 7 * max(_ring(p), body) + 0.02)


def _contacts(p: dict, rng: np.random.Generator, y: np.ndarray, k: int) -> None:
    n = len(y)
    if _on(p["flam"]):
        y += 10 ** (p["flam"] / 20) * hit({**p, "contact": p["contact"] * 1.5}, rng, n)
    if _on(p["bounce"]):
        kb = k + _n(p["bounce_ms"] / 1000)
        if kb < n - 8:
            y[kb:] += 10 ** (p["bounce"] / 20) * hit({**p, "contact": p["contact"] * 1.3}, rng, n - kb)
    if _on(p["body"]):
        f = p["body_f"] * (1 + 0.03 * rng.standard_normal())
        y[k:] += 10 ** (p["body"] / 20) * tonal.thump(n - k, f, p["body_ms"] / 1000, 0.001)


def _n(seconds: float) -> int:
    return max(1, int(round(seconds * SR)))


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take: stereo float64 after the T13 finish."""
    p = _spread({**DEFAULTS, **p}, rng)
    n = _n(length_of(p))
    k = _n(p["flam_ms"] / 1000) if _on(p["flam"]) else 0
    k = min(k, n - 1)
    y = np.zeros(n)
    y[k:] += hit(p, rng, n - k)
    _contacts(p, rng, y, k)
    return tonal.finish(y, rng, hp=p["hp"], lp=p["lp"], room_wet=p["room"], rt60=p["rt"], width=p["width"], cut=500)

