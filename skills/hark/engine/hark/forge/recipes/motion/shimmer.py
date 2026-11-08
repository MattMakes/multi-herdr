"""shimmer: sparkle, chime-run and wind-chime textures (recipes-motion part 4, section 13).

Styles: ``sparkle`` (scale-tuned grains on a rise, hold and fall curve, with a tone bed,
an air wash and a wide plate), ``run`` (bar notes up, down or in an arc, panned note by
note) and ``chimes`` (tuned tubes struck at random, the strikes thinning out).
"""

from __future__ import annotations

from typing import Any, Callable

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.motion import _util as u
from hark.forge.recipes.risers.notify import measure, outside

SR = u.SR
C0 = 16.3516
TARGET = "a sparkling magical shimmer"
ALTS = ["a xylophone run", "a piano trill", "rain on a window", "jingling keys",
        "a synthesizer pad"]
REFSET = "pro_shimmer"

SCALES: dict[str, tuple[int, ...]] = {
    "penta": (0, 2, 4, 7, 9), "major": (0, 2, 4, 5, 7, 9, 11), "lydian": (0, 2, 4, 6, 7, 9, 11),
}
BAR: tuple[tuple[float, ...], tuple[float, ...]] = ((1.0, 2.756, 5.404), (1.0, 0.32, 0.1))
TUBE: tuple[tuple[float, ...], tuple[float, ...]] = ((1.0, 2.76, 5.40, 8.93), (0.5, 1.0, 0.6, 0.25))

_TABLE: tuple[tuple[str, Any, Any], ...] = (
    ("style", ["sparkle", "run", "chimes"], "sparkle"), ("length", (1.0, 3.0), 1.95),
    ("start_db", (-50.0, -20.0), -42.0), ("swell", (0.2, 1.1), 0.85),
    ("rise_curve", (0.6, 2.5), 1.2), ("hold", (0.0, 0.3), 0.1), ("fade", (0.4, 1.4), 0.85),
    ("gate_end", [0, 1], 0), ("rate", (20.0, 75.0), 55.0), ("dens_floor", (0.1, 0.7), 0.5),
    ("g_lo", (350.0, 3500.0, "log"), 450.0), ("g_hi", (4500.0, 11000.0, "log"), 9000.0),
    ("scale", ["penta", "major", "lydian"], "penta"), ("key", (0, 11, "int"), 2),
    ("g_tau", (0.0015, 0.06, "log"), 0.004), ("g_noise", (0.0, 1.0), 0.6),
    ("cluster", (1, 4, "int"), 2), ("g_bell", (0.0, 1.0), 0.3), ("g_jit_db", (0.0, 9.0), 6.0),
    ("tilt", (-3.0, 6.0), 1.5), ("spread", (0.3, 1.0), 0.95), ("decor", (0.0, 1.5), 1.2),
    ("octave", (0.0, 0.5), 0.15), ("tone_hz", (250.0, 800.0, "log"), 500.0),
    ("tone_db", (-40.0, -6.0), -10.0), ("tone_h", (0.0, 0.5), 0.2), ("vib", (0.0, 0.012), 0.004),
    ("tone_w", (0.2, 1.2), 1.0), ("air_db", (-50.0, -16.0), -30.0),
    ("air_hp", (4500.0, 9000.0, "log"), 6500.0), ("verb", (0.0, 0.7), 0.3),
    ("rt60", (0.5, 2.8, "log"), 1.2), ("hp", (150.0, 1200.0, "log"), 250.0),
    ("deess", (0.0, 9.0), 3.0), ("notes", (6, 24, "int"), 12), ("run_t", (0.4, 1.6), 1.2),
    ("direction", ["up", "down", "arc"], "up"), ("accel", (0.6, 1.6), 1.0),
    ("n_decay", (0.1, 2.5, "log"), 0.5), ("hard", (0.0, 1.0), 0.5), ("tubes", (4, 9, "int"), 6),
    ("strikes", (4, 30, "int"), 14), ("ring", [0, 1], 0), ("beat", (0.0, 3.0), 0.0),
    ("detune", (0.0, 12.0), 0.0), ("swing", (0.0, 8.0), 0.0), ("sizzle", (-60.0, -8.0), -60.0),
    ("f_decay", (0.0, 0.8), 0.0), ("tail_fade", (0.2, 1.2), 0.3),
)
SPACE: dict[str, Any] = {k: spec for k, spec, _ in _TABLE}
DEFAULTS: dict[str, Any] = {k: v for k, _, v in _TABLE}


def _m(**kw: Any) -> dict[str, Any]:
    return {**DEFAULTS, **kw}


_RING = dict(style="run", ring=1, tone_db=-40)
FAMILY: dict[str, list[dict[str, Any]]] = {
    "pro_shimmer": [
        _m(),
        _m(length=1.4, swell=0.35, fade=0.7, rate=65, g_lo=1200, tone_db=-18, g_tau=0.003,
           cluster=3, hold=0.05, air_db=-26),
        _m(style="run", notes=16, run_t=1.4, direction="arc", gate_end=1, n_decay=0.09, hard=0.95,
           length=1.65, start_db=-18, swell=0.9, hold=0.3, fade=2.0, tone_db=-22, tone_hz=270,
           verb=0.2, rt60=0.6, g_lo=400, g_hi=6000, air_db=-34),
        _m(style="run", notes=18, run_t=0.75, direction="down", accel=0.8, gate_end=1,
           n_decay=0.08, hard=0.8, length=1.15, start_db=-4, swell=0.1, hold=0.2, fade=1.4,
           tone_db=-40, scale="major", key=7, g_lo=800, g_hi=7000, verb=0.25, rt60=0.9),
        _m(length=2.4, swell=1.0, fade=1.0, rate=34, g_hi=6000, g_tau=0.012, g_noise=0.15,
           cluster=1, tone_hz=400, tone_db=-12, verb=0.5, rt60=2.0, g_bell=0.7, scale="lydian",
           key=5),
    ],
    "shimmer": [
        _m(**_RING, notes=22, run_t=0.9, direction="up", accel=0.9, n_decay=0.8, f_decay=0.3,
           length=4.2, tail_fade=0.7, scale="major", g_lo=1800, g_hi=9000, hard=0.65, g_bell=0.5,
           beat=1.2, detune=3, swing=1, sizzle=-3, verb=0.45, rt60=3.2, spread=0.95, air_db=-30,
           air_hp=3500, deess=2, decor=0.8),
        _m(**_RING, notes=14, run_t=0.8, direction="down", accel=0.8, n_decay=0.6, f_decay=0.25,
           length=4.0, tail_fade=0.7, g_lo=1500, g_hi=8000, g_bell=0.15, hard=0.55, beat=1.8,
           detune=5, swing=1.5, sizzle=-5, verb=0.5, rt60=3.2, spread=0.9, air_db=-32,
           air_hp=4500, decor=0.8),
        _m(style="chimes", ring=1, tubes=6, strikes=12, n_decay=0.7, f_decay=0.2, length=4.2,
           tail_fade=0.9, tone_db=-40, g_lo=900, g_hi=3200, hard=0.75, beat=0.9, detune=4,
           swing=4, sizzle=-6, verb=0.42, rt60=3.0, spread=1.0, air_db=-30, air_hp=4000, tilt=3,
           decor=1.0),
        _m(**_RING, notes=8, run_t=0.7, direction="up", accel=1.0, n_decay=0.55, f_decay=0.3,
           length=3.6, tail_fade=0.6, scale="major", key=0, g_lo=1000, g_hi=4200, hard=0.8,
           g_bell=0.6, beat=0.8, detune=2, swing=1, sizzle=-9, verb=0.42, rt60=2.9, spread=0.9,
           air_db=-32, air_hp=3000, decor=0.8),
        _m(length=4.2, swell=0.6, fade=3.4, rate=40, g_tau=0.06, g_noise=0, cluster=1, g_bell=1.0,
           verb=0.5, rt60=3.0, tone_db=-30, octave=0.15),
        _m(**_RING, notes=16, run_t=1.1, direction="arc", accel=1.0, n_decay=0.75, f_decay=0.35,
           length=4.4, tail_fade=0.8, scale="penta", key=9, g_lo=1300, g_hi=7000, hard=0.5,
           g_bell=0.8, beat=1.4, detune=6, swing=2, sizzle=-5, verb=0.48, rt60=3.2, spread=1.0,
           air_db=-30, air_hp=4000, decor=0.8),
    ],
}

FINE: dict[str, tuple[str, ...]] = {
    "sparkle": ("start_db", "rise_curve", "dens_floor", "g_tau", "g_noise", "g_bell", "g_jit_db",
                "tilt", "spread", "decor", "octave", "tone_db", "tone_h", "vib", "tone_w", "air_db",
                "air_hp", "verb", "rt60", "hp", "deess"),
    "run": ("accel", "hard", "g_bell", "g_jit_db", "tilt", "spread", "octave", "verb", "rt60", "hp",
            "deess", "air_db", "beat", "detune", "sizzle"),
    "chimes": ("hard", "g_jit_db", "tilt", "verb", "rt60", "hp", "deess", "air_db", "beat",
               "detune", "swing", "sizzle"),
}

TARGETS: dict[str, tuple[tuple[str, float, float, float], ...]] = {
    "pro_shimmer": (("length_s", 1.4, 2.6, 0.4), ("corr", -0.2, 0.45, 0.2), ("tilt", 2, 7, 3),
                    ("low_db", -200, -30, 6)),
    "shimmer": (("length_s", 1.5, 5.0, 0.5), ("low_db", -200, -25, 6)),
}


def family_space(params: dict, style: str | None = None) -> dict:
    """SPACE with every key outside FINE[style] pinned to the member's value."""
    style = style or params.get("style") or "sparkle"
    free = FINE.get(style, ())
    return {k: spec if k in free else [params.get(k, DEFAULTS[k])] for k, spec in SPACE.items()}


def scale_freqs(lo: float, hi: float, scale: str, key: int) -> list[float]:
    """The scale notes between lo and hi Hz, sorted; [sqrt(lo hi)] when none."""
    fs = sorted(C0 * 2 ** (o + (s + key) / 12.0) for o in range(11) for s in SCALES[scale])
    kept = [f for f in fs if lo <= f <= hi]
    return kept if kept else [float(np.sqrt(lo * hi))]


def envelope(p: dict, n: int) -> np.ndarray:
    """The linear level curve: a swell from start_db, a hold, then a straight dB fade."""
    t = u.times(n)
    up = np.clip(t / p["swell"], 0, 1)
    db = p["start_db"] * (1 - up ** (1 / max(p["rise_curve"], 0.001)))
    after = t > p["swell"] + p["hold"]
    db = np.where(after, -45.0 * np.maximum((t - p["swell"] - p["hold"]) / p["fade"], 0), db)
    return 10 ** (np.maximum(db, -90.0) / 20.0)


def _tilt_gain(p: dict, f: float) -> float:
    return 10 ** (p["tilt"] * np.log2(f / 2000.0) / 20.0)


def _wobble(t: np.ndarray, swing: float, rng: np.random.Generator) -> np.ndarray | None:
    if swing <= 0.05:
        return None
    return 2 ** ((swing / 1200.0) * np.sin(2 * np.pi * rng.uniform(0.7, 2.0) * t + rng.uniform(0, 6.3)))


def _partial(f: float, t: np.ndarray, wob, beat: float, q: float, rng) -> np.ndarray:
    phase = rng.uniform(0, 6.3)
    if wob is None:
        z = np.sin(2 * np.pi * f * t + phase)
    else:
        z = np.sin(2 * np.pi * f * np.cumsum(wob) / SR + phase)
    if beat > 0.02:
        fb = f + beat * rng.uniform(0.5, 1.5) * np.sqrt(q)
        z = (z + 0.7 * np.sin(2 * np.pi * fb * t + phase)) / 1.7
    return z


def _strike_noise(f: float, n: int, k: int, hard: float, sizzle, rng) -> np.ndarray:
    y = np.zeros(n)
    if hard > 0.05:
        m = min(n, u.samples(0.003))
        w = dsp.butter(rng.standard_normal(m), "high", min(2 * f, 12000.0), 2)
        y[:m] += 0.25 * hard * u.unit_peak(w * np.exp(-u.times(m) / 0.0006))
    if sizzle is not None and sizzle > -59:
        m = min(n, u.samples(0.08))
        w = dsp.butter(rng.standard_normal(m), "band", (max(5000.0, 1.5 * f), 18000.0), 2)
        w = w * np.exp(-u.times(m) / rng.uniform(0.005, 0.016))
        w[:k] *= u.raised_cos_in(min(k, m))
        ref = u.rms(w[: u.samples(0.01)]) + u.EPS
        y[:m] += 10 ** (sizzle / 20.0) * 0.35 * w / ref
    return y


def note(f: float, n: int, tau: float, partials, hard: float, bell: float,
         rng: np.random.Generator, beat: float = 0.0, swing: float = 0.0,
         sizzle: float | None = None) -> np.ndarray:
    """A struck note of n samples: damped partials, a mallet click and optional sizzle."""
    t = u.times(n)
    wob = _wobble(t, swing, rng)
    y = np.zeros(n)
    for q, a in zip(*partials):
        fq = f * q
        if (q > 1 and bell <= 0) or fq > 18000:
            continue
        aa = a * (bell if q > 1 else 1.0) * q ** (1.2 * (hard - 0.5))
        env = np.exp(-t / max(tau * q**-0.9, 0.002))
        y += aa * env * _partial(fq, t, wob, beat, q, rng)
    k = min(n, u.samples(0.0006))
    y[:k] *= u.raised_cos_in(k)
    return y + _strike_noise(f, n, k, hard, sizzle, rng)


def _pan_add(bus: np.ndarray, y: np.ndarray, gain: float, pos: float, at: int) -> None:
    gl, gr = u.pan_gains(pos)
    u.add_at(bus[:, 0], gain * gl * y, at)
    u.add_at(bus[:, 1], gain * gr * y, at)


def _grain_times(p: dict, env: np.ndarray, rng: np.random.Generator) -> list[float]:
    n = len(env)
    dur, out, t = n / SR, [], 0.0
    while t < dur:
        t += rng.exponential(1.0 / p["rate"])
        e = env[min(n - 1, int(t * SR))]
        d = p["dens_floor"] + (1 - p["dens_floor"]) * min(1.0, e)
        if rng.random() < d and t < dur - 0.01:
            out.append(t)
    return out


def _grain(p: dict, notes: list[float], m: int, rng: np.random.Generator) -> np.ndarray:
    y = np.zeros(m)
    f = notes[0]
    for _ in range(int(p["cluster"])):
        f = notes[int(rng.integers(len(notes)))]
        yc = (1 - p["g_noise"]) * note(f, m, p["g_tau"], BAR, 0.0, p["g_bell"], rng)
        if p["g_noise"] > 0.01:
            w = dsp.butter(rng.standard_normal(m), "band", (f / 1.25, min(1.25 * f, 20000.0)), 2)
            yc = yc + p["g_noise"] * u.unit_peak(w * np.exp(-u.times(m) / p["g_tau"]))
        y += yc * _tilt_gain(p, f)
    if p["octave"] > 0.01 and 2 * f < 0.42 * SR:
        y2 = note(2 * f, m, 0.7 * p["g_tau"], BAR, 0.0, 0.0, rng)
        lag = u.samples(0.006)
        y += p["octave"] * np.concatenate([np.zeros(lag), y2])[:m]
    return y


def _decor_bus(bus: np.ndarray, amount: float, cut: float) -> np.ndarray:
    mid = (bus[:, 0] + bus[:, 1]) / 2
    side = (bus[:, 0] - bus[:, 1]) / 2
    return fx.decorrelate(mid, amount, cut=cut)[: len(mid)] + np.column_stack([side, -side])


def sparkle(p: dict, n: int, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """The grain cloud: (stereo grains, envelope)."""
    env = envelope(p, n)
    notes = scale_freqs(p["g_lo"], p["g_hi"], p["scale"], int(p["key"]))
    gl = u.samples(min(0.35, 7 * p["g_tau"]))
    bus = np.zeros((n, 2))
    for t in _grain_times(p, env, rng):
        i = int(t * SR)
        m = min(gl, n - i)
        if m < 16:
            continue
        y = _grain(p, notes, m, rng)
        g = env[i] * 10 ** (rng.uniform(-1, 1) * p["g_jit_db"] / 20.0) / np.sqrt(p["cluster"])
        sign = 1.0 if rng.random() < 0.5 else -1.0
        _pan_add(bus, y, g, sign * rng.uniform(0.35, 1) * p["spread"], i)
    bus *= 0.25
    if p["decor"] > 0.01:
        bus = _decor_bus(bus, p["decor"], 400.0)
    return bus, env


def _r2(p: dict) -> bool:
    return bool(p.get("r2_path"))


def _run_index(direction: str, count: int, size: int) -> np.ndarray:
    if direction == "down":
        idx = np.linspace(size - 1, 0, count)
    elif direction == "arc":
        idx = (size - 1) * np.sin(np.linspace(0, np.pi, count)) ** 0.8
    else:
        idx = np.linspace(0, size - 1, count)
    return np.round(idx).astype(int)


def _struck(p: dict, f: float, rng: np.random.Generator) -> tuple[float, float]:
    """(pitch with the per-strike detune, decay time) of one run note or chime strike."""
    if not _r2(p):
        f = f * 2 ** (rng.uniform(-1, 1) * p["detune"] / 1200.0)
    return f, p["n_decay"] * (f / 1000.0) ** -p["f_decay"]


def run(p: dict, n: int, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """A run of bar notes, alternating left and right: (stereo x 0.3, envelope)."""
    notes = scale_freqs(p["g_lo"], p["g_hi"], p["scale"], int(p["key"]))
    count = int(p["notes"])
    idx = _run_index(p["direction"], count, len(notes))
    cap = 4.0 if _r2(p) else 6.0
    bus = np.zeros((n, 2))
    for k in range(count):
        tk = p["run_t"] * (k / max(count - 1, 1)) ** p["accel"]
        tk += rng.uniform(-0.004, 0.004) if k > 0 else 0.0
        i = int(max(tk, 0.0) * SR)
        m = min(u.samples(min(6 * p["n_decay"], cap)), n - i)
        if m < 16:
            continue
        f, tau = _struck(p, notes[idx[k]], rng)
        y = note(f, m, tau, BAR, p["hard"], 0.8 * p["g_bell"] + 0.2, rng, p["beat"], p["swing"],
                 p["sizzle"])
        g = _tilt_gain(p, f) * 10 ** (rng.uniform(-1, 1) * p["g_jit_db"] / 40.0)
        pos = (1.0 if k % 2 else -1.0) * p["spread"] * rng.uniform(0.6, 1)
        _pan_add(bus, y, g, pos, i)
    return bus * 0.3, envelope(p, n)


def chimes(p: dict, n: int, rng: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """Tuned tubes struck at random, the strikes thinning out: (stereo x 0.3, ones)."""
    notes = scale_freqs(p["g_lo"], p["g_hi"], p["scale"], int(p["key"]))
    pick = np.round(np.linspace(0, len(notes) - 1, min(int(p["tubes"]), len(notes)))).astype(int)
    tubes = [(notes[j], rng.uniform(-1, 1) * p["spread"]) for j in pick]
    cap = 4.0 if _r2(p) else 6.0
    bus, t = np.zeros((n, 2)), 0.0
    for k in range(int(p["strikes"])):
        i = int(t * SR)
        if i >= n - 16:
            break
        f0, pos = tubes[int(rng.integers(len(tubes)))]
        f, tau = _struck(p, f0, rng)
        m = min(u.samples(min(5 * p["n_decay"], cap)), n - i)
        y = note(f, m, tau, TUBE, p["hard"], 0.8, rng, p["beat"], p["swing"], p["sizzle"])
        g = 10 ** (rng.uniform(-1, 1) * p["g_jit_db"] / 20.0) * 0.92**k
        _pan_add(bus, y, g, pos, i)
        t += rng.exponential(0.06 + 0.012 * k)
    return bus * 0.3, np.ones(n)


def bed(p: dict, n: int, benv: np.ndarray, rng: np.random.Generator) -> np.ndarray:
    """The tone bed on the scale root and 2 independent air washes, stereo."""
    root = C0 * 2 ** (int(p["key"]) / 12.0)
    f = root * 2 ** round(np.log2(p["tone_hz"] / root))
    t = u.times(n)
    vib = 1 + p["vib"] * np.sin(2 * np.pi * 5.1 * t + rng.uniform(0, 6.3))
    ph = 2 * np.pi * np.cumsum(f * vib) / SR
    tone = np.sin(ph) + p["tone_h"] * (0.5 * np.sin(2 * ph) + 0.3 * np.sin(3 * ph))
    tone = tone * 0.05 * 10 ** (p["tone_db"] / 20.0) / (u.rms(tone) + u.EPS)
    if p["tone_db"] <= -39 and not _r2(p):
        tone = np.zeros(n)
    out = fx.decorrelate(tone * benv, p["tone_w"], cut=200.0)[:n]
    airs = []
    for _ in range(2):
        a = dsp.butter(dsp.butter(rng.standard_normal(n), "high", p["air_hp"], 2), "low", 14000.0, 2)
        airs.append(a * 0.05 * 10 ** (p["air_db"] / 20.0) / (u.rms(a) + u.EPS) * benv)
    return out + np.column_stack(airs)


def _bed_ref(y: np.ndarray, n: int) -> float:
    h = u.samples(0.2)
    k = max(1, n // h)
    power = np.mean(y * y, axis=1)
    blocks = [power[j * h : (j + 1) * h].mean() for j in range(k) if power[j * h : (j + 1) * h].size]
    return float(np.sqrt(max(blocks))) + 1e-9


def _ring_env(y: np.ndarray, n: int) -> np.ndarray:
    h = u.samples(0.02)
    kk = max(1, n // h)
    power = np.mean(y * y, axis=1)
    e = np.array([np.sqrt(power[j * h : (j + 1) * h].mean()) for j in range(kk)])
    centres = (np.arange(kk) + 0.5) * h
    env = np.interp(np.arange(n), centres, e)
    return env / (e.max() + u.EPS)


def _with_bed(p: dict, y: np.ndarray, env: np.ndarray, n: int, rng) -> np.ndarray:
    if not (p["tone_db"] > -39 or p["air_db"] > -49):
        return y
    ref = _bed_ref(y, n)
    if p["style"] == "sparkle":
        benv = env
    elif p["ring"]:
        benv = _ring_env(y, n)
    else:
        benv = envelope(p, n)
    return y + bed(p, n, benv, rng) * ref / 0.05


def _end(p: dict, y: np.ndarray, n: int) -> np.ndarray:
    if p["gate_end"]:
        k = u.samples(0.03)
    elif p["ring"]:
        k = u.samples(min(p["tail_fade"], 0.4 * p["length"]))
    else:
        k = u.samples(min(0.3, 0.15 * p["length"]))
    k = min(k, n)
    if k > 1:
        y[-k:] *= (np.cos(0.5 * np.pi * np.arange(k) / (k - 1)) ** 2)[:, None]
    if p["ring"]:
        r = min(n, u.samples(0.0005))
        y[:r] *= u.raised_cos_in(r)[:, None]
    return y


_LAYERS: dict[str, Callable] = {"sparkle": sparkle, "run": run}


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One take of the shimmer (section 13.7), float64 stereo at 48 kHz."""
    q = {**DEFAULTS, **p}
    n = u.samples(q["length"])
    y, env = _LAYERS.get(q["style"], chimes)(q, n, rng)
    y = _with_bed(q, y, env, n, rng)
    if q["ring"] and q["style"] != "sparkle" and q["decor"] > 0.01:
        y = _decor_bus(y, q["decor"], 500.0)
    y = dsp.butter(y, "high", q["hp"], 4)
    if q["deess"] > 0.2:
        y = dsp.peq(y, 9000.0, -q["deess"], 0.8)
    if q["verb"] > 0.01:
        y = fx.reverb(y, rng, rt60=q["rt60"], size=0.7, predelay=0.015, damp=0.45, wet=q["verb"],
                      stereo=True, width=1.0)
    y = np.array(y[:n], dtype=float)
    if q["style"] != "sparkle" and not q["ring"]:
        y = y * envelope(q, n)[:, None]
    return u.below_full_scale(_end(q, y, n))


def _low_db(m: dict) -> float:
    shares = m.get("shares")
    first = list(shares.values())[0] if isinstance(shares, dict) else shares[0]
    return float(10 * np.log10(10 ** (first / 10.0) + 1e-12))


def pro_penalty(x: np.ndarray, cat: str) -> float:
    """The summed outside distance of the measures from TARGETS[cat] (0 for an unknown cat)."""
    targets = TARGETS.get(cat)
    if not targets:
        return 0.0
    m = dict(measure(x))
    m["low_db"] = _low_db(m)
    return float(sum(min(2.0, outside(m.get(k), lo, hi, s)) for k, lo, hi, s in targets))


def extra_for(cat: str, weight: float = 1.0) -> Callable:
    """A critic extra check: (x, row) -> -weight x pro_penalty(x, cat)."""
    def extra(x: np.ndarray, row: Any = None) -> float:
        return -weight * pro_penalty(x, cat)
    return extra
