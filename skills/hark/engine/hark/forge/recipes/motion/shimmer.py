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
SPACE: dict[str, Any] = {kn: spec for kn, spec, _ in _TABLE}
DEFAULTS: dict[str, Any] = {kn: v for kn, _, v in _TABLE}


def _m(**kw: Any) -> dict[str, Any]:
    return {**DEFAULTS, **kw}


_RING = {"style": "run", "ring": 1, "tone_db": -40}
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


def family_space(pset: dict, style: str | None = None) -> dict:
    """SPACE with every key outside FINE[style] pinned to the member's value."""
    free = _free_keys(pset, style)
    return {kn: spec if kn in free else [pset.get(kn, DEFAULTS[kn])] for kn, spec in SPACE.items()}


def _free_keys(pset: dict, style: str | None) -> tuple[str, ...]:
    return FINE.get(style or pset.get("style") or "sparkle", ())


def scale_freqs(lo: float, hiv: float, scale: str, key: int) -> list[float]:
    """The scale notes between lo and hi Hz, sorted; [sqrt(lo hi)] when none."""
    fs = sorted(C0 * 2 ** (o + (sv + key) / 12.0) for o in range(11) for sv in SCALES[scale])
    kept = [freq for freq in fs if lo <= freq <= hiv]
    return kept if kept else [float(np.sqrt(lo * hiv))]


def envelope(cfg: dict, ns: int) -> np.ndarray:
    """The linear level curve: a swell from start_db, a hold, then a straight dB fade."""
    tax = u.times(ns)
    up = np.clip(tax / cfg["swell"], 0, 1)
    db = cfg["start_db"] * (1 - up ** (1 / max(cfg["rise_curve"], 0.001)))
    after = tax > cfg["swell"] + cfg["hold"]
    db = np.where(after, -45.0 * np.maximum((tax - cfg["swell"] - cfg["hold"]) / cfg["fade"], 0), db)
    return 10 ** (np.maximum(db, -90.0) / 20.0)


def _tilt_gain(cfg: dict, freq: float) -> float:
    return 10 ** (cfg["tilt"] * np.log2(freq / 2000.0) / 20.0)


def _wobble(tax: np.ndarray, swing: float, gen: np.random.Generator) -> np.ndarray | None:
    if swing <= 0.05:
        return None
    return 2 ** ((swing / 1200.0) * np.sin(2 * np.pi * gen.uniform(0.7, 2.0) * tax + gen.uniform(0, 6.3)))


def _partial(freq: float, tax: np.ndarray, wob, beat: float, qv: float, gen) -> np.ndarray:
    phase = gen.uniform(0, 6.3)
    if wob is None:
        z = np.sin(2 * np.pi * freq * tax + phase)
    else:
        z = np.sin(2 * np.pi * freq * np.cumsum(wob) / SR + phase)
    if beat > 0.02:
        fb = freq + beat * gen.uniform(0.5, 1.5) * np.sqrt(qv)
        z = (z + 0.7 * np.sin(2 * np.pi * fb * tax + phase)) / 1.7
    return z


def _strike_noise(freq: float, ns: int, kn: int, hard: float, sizzle, gen) -> np.ndarray:
    sig = np.zeros(ns)
    if hard > 0.05:
        mv = min(ns, u.samples(0.003))
        sig[:mv] += _mallet(freq, mv, hard, gen)
    if sizzle is not None and sizzle > -59:
        mv = min(ns, u.samples(0.08))
        sig[:mv] += _sizzle(freq, mv, kn, sizzle, gen)
    return sig


def _mallet(freq: float, mv: int, hard: float, gen: np.random.Generator) -> np.ndarray:
    """Step 4 of note: a 3 ms high-passed noise click."""
    wv = dsp.butter(gen.standard_normal(mv), "high", min(2 * freq, 12000.0), 2)
    return 0.25 * hard * u.unit_peak(wv * np.exp(-u.times(mv) / 0.0006))


def _sizzle(freq: float, mv: int, kn: int, sizzle: float, gen: np.random.Generator) -> np.ndarray:
    """Step 5 of note: a 5 to 18 kHz strike noise, level sizzle dB re the note."""
    wv = dsp.butter(gen.standard_normal(mv), "band", (max(5000.0, 1.5 * freq), 18000.0), 2)
    wv = wv * np.exp(-u.times(mv) / gen.uniform(0.005, 0.016))
    wv[:kn] *= u.raised_cos_in(min(kn, mv))
    ref = u.rms(wv[: u.samples(0.01)]) + u.EPS
    return 10 ** (sizzle / 20.0) * 0.35 * wv / ref


def note(freq: float, ns: int, tc: float, partials, hard: float, bell: float,
         gen: np.random.Generator, beat: float = 0.0, swing: float = 0.0,
         sizzle: float | None = None) -> np.ndarray:
    """A struck note of n samples: damped partials, a mallet click and optional sizzle."""
    tax = u.times(ns)
    sig = _partials(freq, tax, tc, partials, (hard, bell, beat), _wobble(tax, swing, gen), gen)
    kn = min(ns, u.samples(0.0006))
    sig[:kn] *= u.raised_cos_in(kn)
    return sig + _strike_noise(freq, ns, kn, hard, sizzle, gen)


def _partials(freq: float, tax: np.ndarray, tc: float, partials, tone: tuple[float, float, float],
              wob, gen: np.random.Generator) -> np.ndarray:
    """Step 2 of note: the damped partials; tone = (hard, bell, beat)."""
    hard, bell, beat = tone
    sig = np.zeros(len(tax))
    for qv, amp in zip(*partials):
        fqv = freq * qv
        if (qv > 1 and bell <= 0) or fqv > 18000:
            continue
        aa = amp * (bell if qv > 1 else 1.0) * qv ** (1.2 * (hard - 0.5))
        env = np.exp(-tax / max(tc * qv**-0.9, 0.002))
        sig += aa * env * _partial(fqv, tax, wob, beat, qv, gen)
    return sig


def _pan_add(bus: np.ndarray, sig: np.ndarray, gain: float, pos: float, at: int) -> None:
    gl, gr = u.pan_gains(pos)
    u.add_at(bus[:, 0], gain * gl * sig, at)
    u.add_at(bus[:, 1], gain * gr * sig, at)


def _grain_times(cfg: dict, env: np.ndarray, gen: np.random.Generator) -> list[float]:
    ns = len(env)
    dur, res, tax = ns / SR, [], 0.0
    while tax < dur:
        tax += gen.exponential(1.0 / cfg["rate"])
        e = env[min(ns - 1, int(tax * SR))]
        dv = cfg["dens_floor"] + (1 - cfg["dens_floor"]) * min(1.0, e)
        if gen.random() < dv and tax < dur - 0.01:
            res.append(tax)
    return res


def _grain(cfg: dict, notes: list[float], mv: int, gen: np.random.Generator) -> np.ndarray:
    sig = np.zeros(mv)
    freq = notes[0]
    for _ in range(int(cfg["cluster"])):
        freq = notes[int(gen.integers(len(notes)))]
        sig += _cluster_note(cfg, freq, mv, gen) * _tilt_gain(cfg, freq)
    if cfg["octave"] > 0.01 and 2 * freq < 0.42 * SR:
        sig += _octave_echo(cfg, freq, mv, gen)
    return sig


def _cluster_note(cfg: dict, freq: float, mv: int, gen: np.random.Generator) -> np.ndarray:
    """Step 4 of sparkle: one grain note, part bar note and part band noise."""
    yc = (1 - cfg["g_noise"]) * note(freq, mv, cfg["g_tau"], BAR, 0.0, cfg["g_bell"], gen)
    if cfg["g_noise"] > 0.01:
        wv = dsp.butter(gen.standard_normal(mv), "band", (freq / 1.25, min(1.25 * freq, 20000.0)), 2)
        yc = yc + cfg["g_noise"] * u.unit_peak(wv * np.exp(-u.times(mv) / cfg["g_tau"]))
    return yc


def _octave_echo(cfg: dict, freq: float, mv: int, gen: np.random.Generator) -> np.ndarray:
    """Step 6 of sparkle: a sine an octave up, 6 ms late, cut to m."""
    y2 = note(2 * freq, mv, 0.7 * cfg["g_tau"], BAR, 0.0, 0.0, gen)
    return cfg["octave"] * np.concatenate([np.zeros(u.samples(0.006)), y2])[:mv]


def _decor_bus(bus: np.ndarray, amount: float, cut: float) -> np.ndarray:
    mid = (bus[:, 0] + bus[:, 1]) / 2
    sidev = (bus[:, 0] - bus[:, 1]) / 2
    return fx.decorrelate(mid, amount, cut=cut)[: len(mid)] + np.column_stack([sidev, -sidev])


def sparkle(cfg: dict, ns: int, gen: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """The grain cloud: (stereo grains, envelope)."""
    env = envelope(cfg, ns)
    notes = scale_freqs(cfg["g_lo"], cfg["g_hi"], cfg["scale"], int(cfg["key"]))
    gl = u.samples(min(0.35, 7 * cfg["g_tau"]))
    bus = np.zeros((ns, 2))
    for tax in _grain_times(cfg, env, gen):
        ix = int(tax * SR)
        mv = min(gl, ns - ix)
        if mv < 16:
            continue
        sig = _grain(cfg, notes, mv, gen)
        g = env[ix] * 10 ** (gen.uniform(-1, 1) * cfg["g_jit_db"] / 20.0) / np.sqrt(cfg["cluster"])
        sign = 1.0 if gen.random() < 0.5 else -1.0
        _pan_add(bus, sig, g, sign * gen.uniform(0.35, 1) * cfg["spread"], ix)
    bus *= 0.25
    if cfg["decor"] > 0.01:
        bus = _decor_bus(bus, cfg["decor"], 400.0)
    return bus, env


def _r2(cfg: dict) -> bool:
    return bool(cfg.get("r2_path"))


def _run_index(direction: str, count: int, room: int) -> np.ndarray:
    if direction == "down":
        idx = np.linspace(room - 1, 0, count)
    elif direction == "arc":
        idx = (room - 1) * np.sin(np.linspace(0, np.pi, count)) ** 0.8
    else:
        idx = np.linspace(0, room - 1, count)
    return np.round(idx).astype(int)


def _struck(cfg: dict, freq: float, gen: np.random.Generator) -> tuple[float, float]:
    """(pitch with the per-strike detune, decay time) of one run note or chime strike."""
    if not _r2(cfg):
        freq = freq * 2 ** (gen.uniform(-1, 1) * cfg["detune"] / 1200.0)
    return freq, cfg["n_decay"] * (freq / 1000.0) ** -cfg["f_decay"]


def run(cfg: dict, ns: int, gen: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """A run of bar notes, alternating left and right: (stereo x 0.3, envelope)."""
    notes = scale_freqs(cfg["g_lo"], cfg["g_hi"], cfg["scale"], int(cfg["key"]))
    count = int(cfg["notes"])
    idx = _run_index(cfg["direction"], count, len(notes))
    bus = np.zeros((ns, 2))
    for kn in range(count):
        ix = _run_start(cfg, kn, count, gen)
        mv = min(u.samples(min(6 * cfg["n_decay"], _cap(cfg))), ns - ix)
        if mv >= 16:
            _run_note(cfg, bus, notes[idx[kn]], (kn, ix, mv), gen)
    return bus * 0.3, envelope(cfg, ns)


def _cap(cfg: dict) -> float:
    """The longest note in s: 4 on the round-2 path, else 6."""
    return 4.0 if _r2(cfg) else 6.0


def _run_start(cfg: dict, kn: int, count: int, gen: np.random.Generator) -> int:
    """Step 2 of run: the start sample of note k, with a 4 ms jitter after the first."""
    tkv = cfg["run_t"] * (kn / max(count - 1, 1)) ** cfg["accel"]
    tkv += gen.uniform(-0.004, 0.004) if kn > 0 else 0.0
    return int(max(tkv, 0.0) * SR)


def _run_note(cfg: dict, bus: np.ndarray, fa: float, where: tuple[int, int, int],
              gen: np.random.Generator) -> None:
    """Render run note k of m samples at sample i (where = (k, i, m)) and pan it into bus."""
    kn, ix, mv = where
    freq, tc = _struck(cfg, fa, gen)
    sig = note(freq, mv, tc, BAR, cfg["hard"], 0.8 * cfg["g_bell"] + 0.2, gen, cfg["beat"], cfg["swing"],
             cfg["sizzle"])
    g = _tilt_gain(cfg, freq) * 10 ** (gen.uniform(-1, 1) * cfg["g_jit_db"] / 40.0)
    pos = (1.0 if kn % 2 else -1.0) * cfg["spread"] * gen.uniform(0.6, 1)
    _pan_add(bus, sig, g, pos, ix)


def chimes(cfg: dict, ns: int, gen: np.random.Generator) -> tuple[np.ndarray, np.ndarray]:
    """Tuned tubes struck at random, the strikes thinning out: (stereo x 0.3, ones)."""
    tubes = _tubes(cfg, gen)
    bus, tax = np.zeros((ns, 2)), 0.0
    for kn in range(int(cfg["strikes"])):
        ix = int(tax * SR)
        if ix >= ns - 16:
            break
        _strike(cfg, bus, tubes[int(gen.integers(len(tubes)))], (kn, ix), gen)
        tax += gen.exponential(0.06 + 0.012 * kn)
    return bus * 0.3, np.ones(ns)


def _tubes(cfg: dict, gen: np.random.Generator) -> list[tuple[float, float]]:
    """Step 1 of chimes: (pitch, pan) of each tube, spread over the scale notes."""
    notes = scale_freqs(cfg["g_lo"], cfg["g_hi"], cfg["scale"], int(cfg["key"]))
    pick = np.round(np.linspace(0, len(notes) - 1, min(int(cfg["tubes"]), len(notes)))).astype(int)
    return [(notes[nth], gen.uniform(-1, 1) * cfg["spread"]) for nth in pick]


def _strike(cfg: dict, bus: np.ndarray, tube: tuple[float, float], when: tuple[int, int],
            gen: np.random.Generator) -> None:
    """Strike k of a tube at sample i (when = (k, i)) and pan it into bus."""
    kn, ix = when
    freq, tc = _struck(cfg, tube[0], gen)
    mv = min(u.samples(min(5 * cfg["n_decay"], _cap(cfg))), len(bus) - ix)
    sig = note(freq, mv, tc, TUBE, cfg["hard"], 0.8, gen, cfg["beat"], cfg["swing"], cfg["sizzle"])
    g = 10 ** (gen.uniform(-1, 1) * cfg["g_jit_db"] / 20.0) * 0.92**kn
    _pan_add(bus, sig, g, tube[1], ix)


def bed(cfg: dict, ns: int, benv: np.ndarray, gen: np.random.Generator) -> np.ndarray:
    """The tone bed on the scale root and 2 independent air washes, stereo."""
    tone = _bed_tone(cfg, ns, gen)
    if cfg["tone_db"] <= -39 and not _r2(cfg):
        tone = np.zeros(ns)
    res = fx.decorrelate(tone * benv, cfg["tone_w"], cut=200.0)[:ns]
    return res + np.column_stack([_air_wash(cfg, ns, gen) * benv for _ in range(2)])


def _bed_tone(cfg: dict, ns: int, gen: np.random.Generator) -> np.ndarray:
    """Steps 1 and 2 of bed: the tone on the scale root with vibrato, RMS 0.05 x tone level."""
    root = C0 * 2 ** (int(cfg["key"]) / 12.0)
    freq = root * 2 ** round(np.log2(cfg["tone_hz"] / root))
    vib = 1 + cfg["vib"] * np.sin(2 * np.pi * 5.1 * u.times(ns) + gen.uniform(0, 6.3))
    ph = 2 * np.pi * np.cumsum(freq * vib) / SR
    tone = np.sin(ph) + cfg["tone_h"] * (0.5 * np.sin(2 * ph) + 0.3 * np.sin(3 * ph))
    return tone * 0.05 * 10 ** (cfg["tone_db"] / 20.0) / (u.rms(tone) + u.EPS)


def _air_wash(cfg: dict, ns: int, gen: np.random.Generator) -> np.ndarray:
    """Step 5 of bed: one band of white noise, RMS 0.05 x air level."""
    amp = dsp.butter(dsp.butter(gen.standard_normal(ns), "high", cfg["air_hp"], 2), "low", 14000.0, 2)
    return amp * 0.05 * 10 ** (cfg["air_db"] / 20.0) / (u.rms(amp) + u.EPS)


def _bed_ref(sig: np.ndarray, ns: int) -> float:
    hv = u.samples(0.2)
    kn = max(1, ns // hv)
    power = np.mean(sig * sig, axis=1)
    blocks = [power[nth * hv : (nth + 1) * hv].mean() for nth in range(kn) if power[nth * hv : (nth + 1) * hv].size]
    return float(np.sqrt(max(blocks))) + 1e-9


def _ring_env(sig: np.ndarray, ns: int) -> np.ndarray:
    hv = u.samples(0.02)
    kk = max(1, ns // hv)
    power = np.mean(sig * sig, axis=1)
    e = np.array([np.sqrt(power[nth * hv : (nth + 1) * hv].mean()) for nth in range(kk)])
    centres = (np.arange(kk) + 0.5) * hv
    env = np.interp(np.arange(ns), centres, e)
    return env / (e.max() + u.EPS)


def _with_bed(cfg: dict, sig: np.ndarray, env: np.ndarray, ns: int, gen) -> np.ndarray:
    if not (cfg["tone_db"] > -39 or cfg["air_db"] > -49):
        return sig
    ref = _bed_ref(sig, ns)
    if cfg["style"] == "sparkle":
        benv = env
    elif cfg["ring"]:
        benv = _ring_env(sig, ns)
    else:
        benv = envelope(cfg, ns)
    return sig + bed(cfg, ns, benv, gen) * ref / 0.05


def _end(cfg: dict, sig: np.ndarray, ns: int) -> np.ndarray:
    if cfg["gate_end"]:
        kn = u.samples(0.03)
    elif cfg["ring"]:
        kn = u.samples(min(cfg["tail_fade"], 0.4 * cfg["length"]))
    else:
        kn = u.samples(min(0.3, 0.15 * cfg["length"]))
    kn = min(kn, ns)
    if kn > 1:
        sig[-kn:] *= (np.cos(0.5 * np.pi * np.arange(kn) / (kn - 1)) ** 2)[:, None]
    if cfg["ring"]:
        r = min(ns, u.samples(0.0005))
        sig[:r] *= u.raised_cos_in(r)[:, None]
    return sig


_LAYERS: dict[str, Callable] = {"sparkle": sparkle, "run": run}


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One take of the shimmer (section 13.7), float64 stereo at 48 kHz."""
    qv = {**DEFAULTS, **cfg}
    ns = u.samples(qv["length"])
    sig, env = _LAYERS.get(qv["style"], chimes)(qv, ns, gen)
    sig = _with_bed(qv, sig, env, ns, gen)
    if qv["ring"] and qv["style"] != "sparkle" and qv["decor"] > 0.01:
        sig = _decor_bus(sig, qv["decor"], 500.0)
    sig = dsp.butter(sig, "high", qv["hp"], 4)
    if qv["deess"] > 0.2:
        sig = dsp.peq(sig, 9000.0, -qv["deess"], 0.8)
    if qv["verb"] > 0.01:
        sig = fx.reverb(sig, gen, rt60=qv["rt60"], size=0.7, predelay=0.015, damp=0.45, wet=qv["verb"],
                      stereo=True, width=1.0)
    sig = np.array(sig[:ns], dtype=float)
    if qv["style"] != "sparkle" and not qv["ring"]:
        sig = sig * envelope(qv, ns)[:, None]
    return u.below_full_scale(_end(qv, sig, ns))


def _low_db(mv: dict) -> float:
    shares = mv.get("shares")
    first = list(shares.values())[0] if isinstance(shares, dict) else shares[0]
    return float(10 * np.log10(10 ** (first / 10.0) + 1e-12))


def pro_penalty(wav: np.ndarray, cat: str) -> float:
    """The summed outside distance of the measures from TARGETS[cat] (0 for an unknown cat)."""
    tgts = TARGETS.get(cat)
    if not tgts:
        return 0.0
    mv = dict(measure(wav))
    mv["low_db"] = _low_db(mv)
    return float(sum(min(2.0, outside(mv.get(kn), lo, hiv, sv)) for kn, lo, hiv, sv in tgts))


def extra_for(cat: str, weight: float = 1.0) -> Callable:
    """A critic extra check: (x, row) -> -weight x pro_penalty(x, cat)."""
    def extra(wav: np.ndarray, row: Any = None) -> float:
        return -weight * pro_penalty(wav, cat)
    return extra
