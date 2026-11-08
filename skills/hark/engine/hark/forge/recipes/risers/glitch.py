"""glitch: a digital stutter that opens with crackle and accelerates into the cut.

Two paths: version 2 (``render``) and version 6 (``render6``), chosen by the presence of ``roll0``. A pattern
generator, seeded from ``pattern``, lays out the gesture; the render generator draws every noise sample.
"""

from __future__ import annotations

import types

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.risers import _glitchkit as gk
from hark.forge.recipes.risers._common import SR, bw, crush, hann, headroom, n_of, t_of

TARGET = "a short digital glitch stutter with bit-crushed noise bursts that speed up and cut off"
ALTS = ["radio static", "a helicopter", "a woodpecker", "rain on a tin roof"]
REFSET = "pro_glitch"
TARGETS = {"pro_glitch": TARGET, "glitch": TARGET}
PRO_SHARES = (-21.7, -24.8, -17.0, -1.2, -6.6)
FM_RATIOS = (0.5, 1.0, 1.41, 2.0, 3.0, 7.13)

_V2 = [
    ("dur", 0.35, 0.85, None, 0.65), ("crackle", 0.12, 0.5, None, 0.35),
    ("crackle_rate", 40.0, 300.0, "log", 200.0), ("crackle_hp", 1500.0, 6000.0, "log", 3500.0),
    ("crackle_db", -30.0, -6.0, None, -14.0), ("grid0", 0.026, 0.046, None, 0.032),
    ("grid1", 0.015, 0.028, None, 0.022), ("accel", 0.6, 2.5, "log", 1.2), ("gate", 0.35, 0.95, None, 0.7),
    ("swell", 0.0, 10.0, None, 4.0), ("bits", 3.0, 10.0, None, 5.0), ("srate", 1500.0, 16000.0, "log", 6000.0),
    ("tonal", 0.0, 1.0, None, 0.35), ("tone_f", 500.0, 5000.0, "log", 1800.0), ("fm", 0.0, 1.0, None, 0.3),
    ("slices", 1, 5, "int", 3), ("repeat", 0.0, 0.9, None, 0.5), ("rev", 0.0, 0.5, None, 0.15),
    ("pitch_step", -2.0, 2.0, None, 0.0), ("low_db", -50.0, -14.0, None, -34.0),
    ("decay", 0.15, 3.0, "log", 0.6), ("lvl_jit", 0.0, 8.0, None, 4.0), ("floor_db", -72.0, -36.0, None, -52.0),
    ("width", 0.0, 1.0, None, 0.9), ("hp", 300.0, 7000.0, "log", 4500.0), ("bright", -6.0, 9.0, None, 3.0),
    ("drive", 0.0, 12.0, None, 4.0), ("end_tail", 0.004, 0.025, "log", 0.012), ("pattern", 0, 999, "int", 11),
    ("skip", 0.0, 0.5, None, 0.0), ("accent_db", 0.0, 12.0, None, 0.0),
]
_V6 = [
    ("dur", 0.22, 0.85, None, 0.55), ("crackle", 0.0, 0.35, None, 0.12), ("crackle_db", -30.0, -6.0, None, -18.0),
    ("grid0", 0.03, 0.11, None, 0.06), ("grid1", 0.012, 0.045, None, 0.022), ("accel", 0.6, 2.5, "log", 1.2),
    ("gate", 0.45, 1.0, None, 0.8), ("roll0", 0.006, 0.07, "log", 0.03), ("roll1", 0.0025, 0.03, "log", 0.006),
    ("srate0", 2500.0, 24000.0, "log", 16000.0), ("srate1", 700.0, 12000.0, "log", 3000.0),
    ("sdrop", 0.0, 0.8, None, 0.3), ("bits0", 4.0, 12.0, None, 8.0), ("bits1", 2.0, 8.0, None, 4.0),
    ("tone", 0.0, 1.0, None, 0.6), ("tone_f", 110.0, 1400.0, "log", 330.0), ("notes", 1, 3, "int", 2),
    ("chirp", 0.0, 1.0, None, 0.3), ("fm", 0.0, 1.0, None, 0.3), ("noise", 0.0, 1.0, None, 0.3),
    ("jump", 0.0, 1.0, None, 0.5), ("rev", 0.0, 0.5, None, 0.15), ("pitch_step", -2.0, 2.0, None, 0.0),
    ("drop", 0.0, 0.4, None, 0.12), ("width", 0.0, 1.0, None, 0.8), ("pan_jump", 0.0, 1.0, None, 0.3),
    ("decay", 0.3, 6.0, "log", 2.0), ("swell", 0.0, 10.0, None, 4.0), ("lvl_jit", 0.0, 8.0, None, 3.0),
    ("low_db", -60.0, -10.0, None, -30.0), ("hp", 60.0, 2000.0, "log", 150.0),
    ("lp", 5000.0, 18000.0, "log", 14000.0), ("bright", -6.0, 6.0, None, 0.0), ("drive", 0.0, 12.0, None, 4.0),
    ("end_tail", 0.004, 0.03, "log", 0.01), ("floor_db", -72.0, -40.0, None, -56.0),
    ("pattern", 0, 999, "int", 601),
]


def _space(spec_rows: list) -> dict:
    return {kn: ((lo, hiv) if scale is None else (lo, hiv, scale)) for kn, lo, hiv, scale, _ in spec_rows}


SPACE = _space(_V2)
DEFAULTS = {kn: dv for kn, _, _, _, dv in _V2 if kn not in ("skip", "accent_db")}
SPACE6 = _space(_V6)
DEFAULTS6 = {kn: dv for kn, _, _, _, dv in _V6}

FAMILY6 = [
    ("stutter", {"roll0": 0.035, "roll1": 0.004, "grid0": 0.07, "grid1": 0.02, "gate": 0.9, "srate0": 16000,
                 "srate1": 2500,
                     "bits0": 8, "bits1": 4, "tone": 0.5, "tone_f": 280, "chirp": 0.2, "fm": 0.3, "noise": 0.6,
                     "jump": 0.25, "drop": 0.08,
                     "swell": 8, "lvl_jit": 2, "hp": 250, "bright": 3, "dur": 0.6, "pattern": 611}),
    ("crush", {"roll0": 0.02, "roll1": 0.011, "grid0": 0.055, "grid1": 0.025, "gate": 0.75, "srate0": 12000,
               "srate1": 1500,
                   "sdrop": 0.6, "bits0": 6, "bits1": 3, "tone": 0.25, "chirp": 0.1, "fm": 0.4, "noise": 0.9,
                   "jump": 0.8, "pan_jump": 0.6,
                   "swell": 8, "lvl_jit": 2, "hp": 300, "bright": 3, "dur": 0.5, "pattern": 612}),
    ("data", {"roll0": 0.012, "roll1": 0.005, "grid0": 0.045, "grid1": 0.018, "gate": 0.6, "srate0": 20000,
              "srate1": 6000,
                  "bits0": 7, "bits1": 5, "tone": 0.0, "chirp": 0.8, "fm": 0.6, "noise": 0.5, "jump": 0.9,
                  "drop": 0.05, "width": 1.0,
                  "pan_jump": 0.5, "swell": 9, "lvl_jit": 2, "hp": 500, "crackle": 0.0, "dur": 0.45, "pattern": 613}),
    ("dropout", {"roll0": 0.06, "roll1": 0.012, "grid0": 0.09, "grid1": 0.03, "gate": 0.7, "srate0": 24000,
                 "srate1": 1200,
                     "sdrop": 0.15, "bits0": 10, "bits1": 3, "tone": 0.35, "tone_f": 180, "notes": 2, "chirp": 0.0,
                     "fm": 0.5, "noise": 0.7,
                     "jump": 0.1, "drop": 0.15, "swell": 8, "lvl_jit": 2, "low_db": -24, "hp": 120, "crackle": 0.0,
                     "dur": 0.7,
                     "pattern": 614}),
    ("scramble", {"roll0": 0.025, "roll1": 0.006, "grid0": 0.05, "grid1": 0.02, "gate": 0.85, "srate0": 8000,
                  "srate1": 2000,
                      "sdrop": 0.5, "bits0": 6, "bits1": 4, "tone": 0.3, "tone_f": 600, "chirp": 0.3, "fm": 0.8,
                      "noise": 0.6, "jump": 1.0,
                      "rev": 0.45, "pitch_step": 1.0, "drop": 0.05, "pan_jump": 0.4, "swell": 9, "lvl_jit": 2,
                      "dur": 0.55,
                      "pattern": 615}),
    ("hit", {"roll0": 0.015, "roll1": 0.003, "grid0": 0.035, "grid1": 0.014, "gate": 0.75, "srate0": 10000,
             "srate1": 2000,
                 "bits0": 6, "bits1": 3, "tone": 0.3, "tone_f": 220, "chirp": 0.2, "fm": 0.3, "noise": 0.8,
                 "jump": 0.5, "drop": 0.0, "swell": 6,
                 "low_db": -22, "crackle": 0.0, "dur": 0.3, "pattern": 616}),
]


def full6(label: str) -> dict:
    """A FAMILY6 member as a full parameter set (DEFAULTS6 plus its overrides)."""
    cfg = dict(DEFAULTS6)
    cfg.update(dict(FAMILY6)[label])
    return {kn: (float(v) if not isinstance(v, str) else v) for kn, v in cfg.items()}


# ------------------------------------------------------------------------------------------------ version 2


def _fm_tone(prng: np.random.Generator, ns: int, fc_lo: float, fc_hi: float, ix_lo: float,
             ix_hi: float) -> np.ndarray:
    cut = prng.uniform(fc_lo, fc_hi)
    ratio = FM_RATIOS[int(prng.integers(len(FM_RATIOS)))]
    index = prng.uniform(ix_lo, ix_hi)
    tax = t_of(ns)
    return np.sin(2 * np.pi * cut * tax + index * np.sin(2 * np.pi * cut * ratio * tax))


def _buffer_v2(cfg: dict, room: int, gen, prng) -> np.ndarray:
    noise = crush(gen.standard_normal(room), cfg["bits"] + 2, cfg["srate"])
    wav = noise / (np.std(noise) + 1e-12) * (1 - 0.6 * max(cfg["tonal"], cfg["fm"]))
    if cfg["tonal"] > 0.02:
        basev = cfg["tone_f"] * 2.0 ** prng.uniform(-1, 1)
        steps = [int(kn) for kn in prng.integers(-5, 6, int(prng.integers(1, 4)))]
        shape = "square" if prng.random() < 0.7 else "saw"
        wav = wav + cfg["tonal"] * 1.4 * dsp.osc(gk.stepped_track(room, basev, steps), shape)
    if cfg["fm"] > 0.02:
        wav = wav + cfg["fm"] * 1.2 * _fm_tone(prng, room, 800, 6000, 1.5, 8)
    return crush(wav / (gk.peak(wav) + 1e-12), cfg["bits"], cfg["srate"])


def _choices_v2(cfg: dict, bv: types.SimpleNamespace, prev: list[int], nbuf: int, prng) -> tuple[tuple, tuple]:
    """(buffer, reversed, offset) for left and right; with chance 1 - width the right copies the left."""
    left = _pick_v2(cfg, prev[0], nbuf, 0, prng)
    if prng.random() > cfg["width"]:
        return left, left
    off = 0 if bv.last else n_of(prng.uniform(0, 0.003))
    return left, _pick_v2(cfg, prev[1], nbuf, off, prng)


def _pick_v2(cfg: dict, prev_idx: int, nbuf: int, offset: int, prng) -> tuple[int, bool, int]:
    idx = prev_idx if prng.random() < cfg["repeat"] else int(prng.integers(nbuf))
    return idx, bool(prng.random() < cfg["rev"]), offset


def _burst_env_v2(cfg: dict, mv: int, dv: float, last: bool, tail_n: int) -> np.ndarray:
    if not last:
        return gk.decay_env(mv, cfg["decay"] * dv, 0.0004)
    hold_tau = max(cfg["decay"] * dv, 0.5 * (mv - 1) / SR)
    return gk.last_env(mv - 1, hold_tau, tail_n + 1, cfg["end_tail"])[:mv + tail_n]


def _level_v2(cfg: dict, kn: int, n_body: int, last: bool, prng) -> float:
    jit = abs(cfg["lvl_jit"] * prng.standard_normal())
    lvl0 = 10 ** ((cfg["swell"] * (kn / max(1, n_body - 1) - 1) - jit * (not last)) / 20)
    if "accent_db" in cfg and not last and prng.random() > 0.35:
        lvl0 *= 10 ** (-cfg["accent_db"] / 20)
    return lvl0


def _low_pulse(mv: int, fa: float, tc: float, gain: float) -> np.ndarray:
    tax = t_of(mv)
    return np.sin(2 * np.pi * fa * tax) * np.exp(-tax / tc) * gk.box(mv, 0.001) * gain


def _layout(cfg: dict, t_st: float) -> types.SimpleNamespace:
    """Sizes, the slot grid from t_st to the cut, and the buffer length (largest slot + 4 ms)."""
    n_end, tail_n = n_of(cfg["dur"]), n_of(4 * cfg["end_tail"])
    slots = gk.grid(t_st, cfg["dur"], cfg["grid0"], cfg["grid1"], cfg["accel"])
    return types.SimpleNamespace(n_end=n_end, tail_n=tail_n, n=n_end + tail_n, t_st=t_st, slots=slots,
                                 nb=len(slots), size=n_of(max(dv for _, dv in slots) + 0.004))


def _burst_v2(cfg: dict, kn: int, slot: tuple, ctx: types.SimpleNamespace, prng) -> types.SimpleNamespace:
    """Start, level, speed and envelope of burst k; the last burst runs to the cut."""
    onset, dv = slot
    last = kn == ctx.nb - 1
    start = gk.start_of(onset)
    g = max(16, ctx.n_end - start) if last else max(16, n_of(dv * cfg["gate"]))
    mv = g if last else min(g, ctx.size)
    return types.SimpleNamespace(k=kn, d=dv, last=last, start=start, speed=2.0 ** (kn * cfg["pitch_step"] / 12),
                                 level=_level_v2(cfg, kn, ctx.nb, last, prng),
                                 env=_burst_env_v2(cfg, mv, dv, last, ctx.tail_n))


def _skipped(cfg: dict, bv: types.SimpleNamespace, prng) -> bool:
    """Round 3: with chance skip, a slot that is not the first or the last stays empty."""
    return "skip" in cfg and 0 < bv.k and not bv.last and prng.random() < cfg["skip"]


def _write_v2(sig: np.ndarray, bv: types.SimpleNamespace, bufs, choices, gen) -> None:
    for cv, (idx, rev, off) in enumerate(choices):
        seg = gk.body_segment(gk.read_slice(bufs[idx], rev, bv.speed), len(bv.env))
        gk.add_clip(sig[:, cv], seg * bv.env * bv.level * np.exp(0.12 * gen.standard_normal()), bv.start + off)


def _low_v2(cfg: dict, low: np.ndarray, bv: types.SimpleNamespace, prng) -> None:
    mv = min(n_of(bv.d * cfg["gate"] + 0.006), len(low) - bv.start)
    fa = prng.uniform(55, 110)
    if mv > 1:
        gk.add_clip(low, _low_pulse(mv, fa, 0.012, 10 ** (cfg["low_db"] / 20) * bv.level), bv.start)


def _bursts_v2(cfg: dict, ctx: types.SimpleNamespace, bufs, gen, prng) -> tuple[np.ndarray, np.ndarray]:
    sig, low = np.zeros((ctx.n, 2)), np.zeros(ctx.n)
    prev = [0, 0]
    for kn, slot in enumerate(ctx.slots):
        bv = _burst_v2(cfg, kn, slot, ctx, prng)
        if not _skipped(cfg, bv, prng):
            choices = _choices_v2(cfg, bv, prev, len(bufs), prng)
            prev = [choices[0][0], choices[1][0]]
            _write_v2(sig, bv, bufs, choices, gen)
            if cfg["low_db"] > -39.5:
                _low_v2(cfg, low, bv, prng)
    return sig, low


def _crackle_grain_v2(cfg: dict, gen, round3: bool):
    def grain(prog: float) -> np.ndarray:
        mv = max(2, n_of(gen.uniform(0.0015, 0.006)))
        tax = t_of(mv)
        if gen.random() < 0.2:
            g = 1.5 * np.sin(2 * np.pi * gen.uniform(150, 900) * tax) * hann(mv)
        else:
            wv = hann(mv) if round3 else np.sqrt(hann(mv))
            g = gen.standard_normal(mv) * wv * np.exp(-tax / (0.5 * mv / SR))
        return g * 10 ** ((cfg["crackle_db"] + 18 * np.log10(gen.uniform(0.05, 1)) + 6 * prog) / 20)
    return grain


def _crackle_v2(cfg: dict, ns: int, t_st: float, gen) -> np.ndarray:
    grain = _crackle_grain_v2(cfg, gen, "skip" in cfg)
    pair = []
    for _ in range(2):
        wav = gk.grain_train(ns, t_st, gk.accel_rate(cfg["crackle_rate"]), gen, grain)
        pair.append(0.85 * bw(wav, "high", cfg["crackle_hp"], 2) + 0.15 * bw(wav, "high", 120, 2))
    return np.column_stack(pair)


def _bed(ns: int, n_end: int, hpv: float, rate: float, gen) -> np.ndarray:
    bed = bw(gen.standard_normal((ns, 2)), "high", hpv, 2)
    bed = np.column_stack([crush(bed[:, cv], 6, rate) for cv in range(2)])
    bed = bed / (4 * np.std(bed) + 1e-12)
    bed[n_end:] = 0.0
    return bed


def _finish(sig: np.ndarray, drive: float) -> np.ndarray:
    if drive > 0.2:
        sig = fx.saturate(sig / (gk.peak(sig) + 1e-12), drive, os=2)
    sig = bw(sig, "low", 18000, 8)
    sig[:2] = 0.0
    return dsp.fade(sig, 0.0005, 0.003)


def _merged_v2(cfg: dict) -> dict:
    qv = dict(DEFAULTS)
    qv.update(cfg or {})
    return qv


def _texture_v2(cfg: dict, ctx: types.SimpleNamespace, sig: np.ndarray, gen) -> np.ndarray:
    """The crackle lead-in and the crushed bed under the stutter, both scaled to the current peak."""
    if cfg["crackle"] > 0.02 and ctx.t_st > 0.02:
        sig = sig + _crackle_v2(cfg, ctx.n, ctx.t_st, gen) * gk.peak(sig)
    if cfg["floor_db"] > -71.5:
        ramp = np.minimum(1.0, t_of(ctx.n) / max(0.02, ctx.t_st + 0.02))[:, None]
        sig = sig + 10 ** (cfg["floor_db"] / 20) * gk.peak(sig) * _bed(ctx.n, ctx.n_end, 3000, cfg["srate"], gen) * ramp
    return sig


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One take. A set with roll0 renders with version 6; else version 2 (not peak-normalised below 0 dBFS)."""
    if "roll0" in (cfg or {}):
        return render6(cfg, gen)
    cfg = _merged_v2(cfg)
    prng = gk.pattern_rng(cfg["pattern"], 13)
    ctx = _layout(cfg, cfg["dur"] * cfg["crackle"])
    bufs = [_buffer_v2(cfg, ctx.size, gen, prng) for _ in range(max(1, int(round(cfg["slices"]))))]
    sig, low = _bursts_v2(cfg, ctx, bufs, gen, prng)
    sig = bw(sig, "high", cfg["hp"], 4)
    sig = _texture_v2(cfg, ctx, sig + (low * gk.peak(sig))[:, None], gen)
    return headroom(_finish(dsp.peq(sig, 6000, cfg["bright"], 0.5), cfg["drive"]))


# ------------------------------------------------------------------------------------------------ version 6

_NS = n_of(0.6)
_INTERVALS = (0, 7, 12, 3, 10)


def _chord6(cfg: dict, prng) -> np.ndarray:
    track = gk.held_track(_NS, prng, lambda: cfg["tone_f"] * 2.0 ** (int(prng.integers(-7, 8)) / 12), 0.04, 0.12)
    shape = "square" if prng.random() < 0.5 else "saw"
    notes = _INTERVALS[:max(1, int(round(cfg["notes"])))]
    return sum(dsp.osc(track * 2.0 ** (iv / 12) * (1 + 0.004 * nth), shape) for nth,
               iv in enumerate(notes)) / len(notes)


def source6(cfg: dict, gen, prng) -> np.ndarray:
    """The 0.6 s source the bursts cut from: chord, chirps, FM and noise, peak 1."""
    sv = np.zeros(_NS)
    if cfg["tone"] > 0.01:
        sv += cfg["tone"] * _chord6(cfg, prng)
    if cfg["chirp"] > 0.01:
        track = gk.held_track(_NS, prng, lambda: 1000 * 2.0 ** prng.uniform(0, 2), 0.005, 0.03)
        sv += cfg["chirp"] * 0.8 * dsp.osc(track, "square")
    if cfg["fm"] > 0.01:
        sv += cfg["fm"] * _fm_tone(prng, _NS, 500, 3000, 2, 9)
    if cfg["noise"] > 0.01:
        sv += cfg["noise"] * 0.5 * gen.standard_normal(_NS)
    return sv / (gk.peak(sv) + 1e-12)


def _crackle6(cfg: dict, ns: int, t_end: float, gen) -> np.ndarray:
    def grain(prog: float) -> np.ndarray:
        mv = max(2, n_of(gen.uniform(0.0015, 0.006)))
        g = crush(gen.standard_normal(mv), 4, (4000, 8000, 16000)[int(gen.integers(3))]) * hann(mv)
        return g * 10 ** ((cfg["crackle_db"] + 14 * np.log10(gen.uniform(0.05, 1)) + 6 * prog) / 20)
    pair = [bw(gk.grain_train(ns, t_end, gk.accel_rate(220.0), gen, grain), "high", 1500, 2) for _ in range(2)]
    return np.column_stack(pair)


def _rate_drop(cfg: dict, prng) -> float:
    """With chance sdrop the sample rate drops 1 or 2 more octaves."""
    if prng.random() < cfg["sdrop"]:
        return 2.0 if prng.random() < 0.5 else 4.0
    return 1.0


def _draw6(cfg: dict, bv: types.SimpleNamespace, pos: list[int], cv: int, prng) -> tuple[int, bool, float, int]:
    """Read start, reverse flag, rate and bits of one channel's segment; a jump moves the read position."""
    if prng.random() < cfg["jump"]:
        pos[cv] = int(prng.integers(0, max(1, _NS - bv.ls - 1)))
    rev = bool(prng.random() < cfg["rev"])
    srk = bv.sr0 / _rate_drop(cfg, prng)
    bk = int(round(bv.b0 + prng.uniform(-1, 1)))
    return max(0, min(pos[cv], _NS - bv.ls - 1)), rev, srk, bk


def _segment6(cfg: dict, srcv, pos: list[int], cv: int, bv: types.SimpleNamespace, prng) -> np.ndarray:
    amp, rev, srk, bk = _draw6(cfg, bv, pos, cv, prng)
    sl = gk.read_slice(srcv[amp:amp + bv.ls], rev, bv.speed)
    pos[cv] = (pos[cv] + n_of(bv.d)) % max(1, _NS - bv.ls - 1)
    seg = gk.body_segment(sl, bv.m) / (gk.peak(sl) + 1e-12)
    return crush(seg, max(2, bk), min(srk, SR / 2))


def _env6(cfg: dict, g: int, dv: float, last: bool, tail_n: int) -> np.ndarray:
    if not last:
        return gk.decay_env(g, cfg["decay"] * dv, 0.0003)
    return gk.last_env(g, g / SR / 3.45, tail_n, cfg["end_tail"])


def _drop_run(state: dict, want: bool) -> bool:
    """A drop-out happens when wanted and fewer than 2 drop-outs came in a row; else the run resets."""
    dropped = want and state["run"] < 2
    state["run"] = state["run"] + 1 if dropped else 0
    return dropped


def _level6(cfg: dict, prog: float, last: bool, prng) -> float:
    jit = abs(cfg["lvl_jit"] * prng.standard_normal())
    return 10 ** ((cfg["swell"] * (prog - 1) - jit * (not last)) / 20)


def _sides6(cfg: dict, last: bool, prng) -> tuple[float, float]:
    """With chance pan_jump (never the last burst) the burst plays hard left or hard right."""
    if prng.random() < cfg["pan_jump"] and not last:
        return (1.0, 0.12) if prng.random() < 0.5 else (0.12, 1.0)
    return 1.0, 1.0


def _slice6(cfg: dict, kn: int, prog: float) -> dict:
    """Slice length, speed, sample rate and bits of burst k at progress u."""
    return {"ls": max(24, n_of(cfg["roll0"] * (cfg["roll1"] / cfg["roll0"]) ** prog)),
            "speed": 2.0 ** (kn * cfg["pitch_step"] / 12),
            "sr0": cfg["srate0"] * (cfg["srate1"] / cfg["srate0"]) ** prog,
            "b0": cfg["bits0"] + (cfg["bits1"] - cfg["bits0"]) * prog}


def _burst6(cfg: dict, kn: int, slot: tuple, ctx: types.SimpleNamespace, prng) -> types.SimpleNamespace:
    onset, dv = slot
    prog = kn / max(1, ctx.nb - 1)
    last = kn == ctx.nb - 1
    start = gk.start_of(onset)
    g = max(32, ctx.n_end - start) if last else max(32, n_of(dv * cfg["gate"]))
    return types.SimpleNamespace(d=dv, last=last, inner=kn > 0 and not last, start=start, g=g,
                                 m=g + (ctx.tail_n if last else 0), level=_level6(cfg, prog, last, prng),
                                 sides=_sides6(cfg, last, prng), **_slice6(cfg, kn, prog))


def _thump6(cfg: dict, low: np.ndarray, bv: types.SimpleNamespace, prng) -> None:
    mv = min(n_of(min(0.05, bv.d)), len(low) - bv.start)
    freq = prng.uniform(50, 90)
    if mv > 1:
        gk.add_clip(low, _low_pulse(mv, freq, 0.015, 10 ** (cfg["low_db"] / 20) * bv.level), bv.start)


def _write6(cfg: dict, sig, bv: types.SimpleNamespace, srcv, pos: list[int], prng) -> None:
    env = _env6(cfg, bv.g, bv.d, bv.last, bv.m - bv.g) * bv.level
    own = prng.random() < cfg["width"]
    seg_l = _segment6(cfg, srcv, pos, 0, bv, prng)
    seg_r = _segment6(cfg, srcv, pos, 1, bv, prng) if own else seg_l
    off = n_of(prng.uniform(0, 0.002)) if (own and not bv.last) else 0
    gk.add_clip(sig[:, 0], seg_l * env * bv.sides[0], bv.start)
    gk.add_clip(sig[:, 1], seg_r * env * bv.sides[1], bv.start + off)


def _bursts6(cfg: dict, ctx: types.SimpleNamespace, srcv, prng) -> tuple[np.ndarray, np.ndarray]:
    pos = [int(v) for v in prng.integers(0, _NS // 2, 2)]
    state = {"run": 0}
    sig, low = np.zeros((ctx.n, 2)), np.zeros(ctx.n)
    for kn, slot in enumerate(ctx.slots):
        bv = _burst6(cfg, kn, slot, ctx, prng)
        if not _drop_run(state, bv.inner and prng.random() < cfg["drop"]):
            _write6(cfg, sig, bv, srcv, pos, prng)
            if cfg["low_db"] > -55 and bv.level > 0.5:
                _thump6(cfg, low, bv, prng)
    return sig, low


def _post6(cfg: dict, ctx: types.SimpleNamespace, sig: np.ndarray, low: np.ndarray, gen) -> np.ndarray:
    """10.6 steps 5 to 10: high-pass, low thumps, crackle lead-in, bed, EQ, drive, the final low-pass and peak."""
    sig = bw(sig, "high", cfg["hp"], 2)
    pk = gk.peak(sig)
    sig = sig + (low * 3 * pk)[:, None]
    if ctx.t_st > 0.02:
        sig = sig + _crackle6(cfg, ctx.n, ctx.t_st, gen) * 2 * pk
    if cfg["floor_db"] > -71.5:
        sig = sig + 10 ** (cfg["floor_db"] / 20) * pk * _bed(ctx.n, ctx.n_end, 2000, 8000, gen)
    sig = _finish(dsp.peq(bw(bw(sig, "high", 25, 2), "low", cfg["lp"], 4), 6000, cfg["bright"], 0.5), cfg["drive"])
    return 0.89 * sig / (gk.peak(sig) + 1e-12)


def render6(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Version 6: a beat-repeat roll cut from a pitched source, normalised to an absolute peak of 0.89."""
    cfg = {**DEFAULTS6, **(cfg or {})}
    prng = gk.pattern_rng(cfg["pattern"], 61)
    ctx = _layout(cfg, cfg["dur"] * cfg["crackle"] if cfg["crackle"] > 0.02 else 0.0)
    srcv = source6(cfg, gen, prng)
    sig, low = _bursts6(cfg, ctx, srcv, prng)
    return _post6(cfg, ctx, sig, low, gen)


# ------------------------------------------------------------------------------------------------ measures


def _stereo(wav: np.ndarray) -> np.ndarray:
    return wav if wav.ndim == 2 else np.column_stack([wav, wav])


def _block_db(mv: np.ndarray, hv: int) -> np.ndarray:
    kn = max(1, len(mv) // hv)
    seg = mv[:kn * hv].reshape(kn, hv) if len(mv) >= hv else mv.reshape(1, -1)
    return 10 * np.log10(np.mean(seg ** 2, axis=1) + 1e-14)


def _shares(mv: np.ndarray) -> tuple[float, float, list[float]]:
    P = np.abs(np.fft.rfft(mv)) ** 2
    freq = np.fft.rfftfreq(len(mv), 1 / SR)
    tot = P.sum() + 1e-14
    cen = float((P * freq).sum() / tot)
    s412 = float(10 * np.log10(P[(freq >= 4000) & (freq < 12000)].sum() / tot + 1e-14))
    edges = (0, 150, 1000, 4000, 12000, 24001)
    shares = [round(float(10 * np.log10(P[(freq >= amp) & (freq < bv)].sum() / tot + 1e-14)), 1)
              for amp, bv in zip(edges[:-1], edges[1:])]
    return cen, s412, shares


def _onsets(mv: np.ndarray) -> list[int]:
    e = _block_db(mv, 48)
    e = e - e.max()
    above = e > -14
    kept: list[int] = []
    for ix in range(1, len(e)):
        if above[ix] and not above[ix - 1] and (not kept or ix - kept[-1] >= 12):
            kept.append(ix)
    return kept


def _gap_medians(onsets: list[int]) -> tuple[float | None, float | None]:
    gaps = np.diff(onsets).astype(float)
    if len(gaps) < 4:
        return None, None
    half = len(gaps) // 2
    return float(np.median(gaps[:half])), float(np.median(gaps[half:]))


def glitch_stats(wav: np.ndarray) -> dict:
    """Model-free glitch measurements (10.7)."""
    wav = _stereo(np.asarray(wav, float))
    mv = wav.mean(axis=1)
    diff = wav[:, 0] - wav[:, 1]
    corr = 1.0 if np.std(diff) <= 1e-9 else float(np.corrcoef(wav[:, 0], wav[:, 1])[0, 1])
    cen, s412, shares = _shares(mv)
    end_db = float(10 * np.log10(np.mean(mv[-480:] ** 2) + 1e-14) - _block_db(mv, 240).max())
    onv = _onsets(mv)
    early, late = _gap_medians(onv)
    return {"corr": corr, "centroid_hz": cen, "s412_db": s412, "shares": shares, "end_db": end_db,
            "bursts": len(onv), "early_ms": early, "late_ms": late,
            "med2_db": float(np.median(_block_db(mv, 96)) - _block_db(mv, 96).max())}


def _accel_term(sv: dict) -> float:
    if sv["early_ms"] is None:
        return 0.5
    return min(1.0, max(0.0, sv["late_ms"] - sv["early_ms"] + 2) / 8)


def study_extra(wav: np.ndarray, row=None) -> float:
    """The pro glitch box: a penalty against the measured pro example, returned as -min(3, 0.6 sum)."""
    sv = glitch_stats(wav)
    share_err = sum(max(0.0, abs(amp - bv) - 6) / 12 for amp, bv in zip(sv["shares"], PRO_SHARES))
    terms = [
        min(1.0, max(0.0, sv["corr"] - 0.3) / 0.4),
        min(1.0, max(0.0, 6500 - sv["centroid_hz"], sv["centroid_hz"] - 11000) / 2500),
        min(1.0, max(0.0, -4 - sv["s412_db"]) / 6),
        min(1.0, max(0.0, sv["end_db"] + 30) / 10),
        0.5 if sv["bursts"] < 6 else 0.0,
        _accel_term(sv),
        min(1.5, share_err),
        min(1.0, max(0.0, sv["med2_db"] + 29) / 6),
    ]
    return -min(3.0, 0.6 * sum(terms))


def generic_extra(wav: np.ndarray, row=None) -> float:
    """Gesture rules only (no references): width, a stop on the cut, enough bursts, acceleration."""
    sv = glitch_stats(wav)
    total = (min(1.0, max(0.0, sv["corr"] - 0.5) / 0.4) + min(1.0, max(0.0, sv["end_db"] + 30) / 10)
             + (0.5 if sv["bursts"] < 6 else 0.0) + _accel_term(sv))
    return -min(3.0, 0.6 * total)
