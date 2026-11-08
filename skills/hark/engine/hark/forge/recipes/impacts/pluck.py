"""Kalimba: a single plucked metal tine on a hollow wooden box, built from its physics.

Spec: docs/specs/hark/recipes-impacts-tuned.md section 3.
Two decisions from ARCHITECTURE.md section 15 apply. A partial parameter set is laid
over DEFAULTS (Q-PLUCK-1). Bending modes 2, 3 and 4 are all dropped when they reach
0.45 x 48000 Hz (Q-PLUCK-2).
"""

from __future__ import annotations

import math
from typing import Any

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.impacts import _common as cm

SR = cm.SR
_TAKE_S = 3.0
_HZ_LIMIT = 0.45 * SR

TARGET = "a kalimba note, a thumb piano pluck"
ALTS = ["a music box", "a xylophone note", "a glockenspiel", "an electric piano note", "a synthesizer pluck"]

REFS: list[str] = [cm.ref_root() + "/kalimba/kalimba_%02d.ogg" % k for k in (1, 2, 3, 4, 5)]
REF_F0: tuple[float, ...] = (392.0, 440.0, 493.9, 587.3, 659.3)

# The 17 tines: a C major scale from C4 upward.
_SCALE_STEPS = (0, 2, 4, 5, 7, 9, 11)
TINES: list[float] = [261.63 * 2 ** ((12 * octave + step) / 12)
                      for octave in range(3) for step in _SCALE_STEPS][:17]

SPACE: dict[str, Any] = cm.space_table("""
    p2_ratio 5.0 7.0
    p2_lvl 0.02 0.6 log
    p3_lvl 0.0 0.25
    tau1 0.35 2.4 log
    tau_exp 0.5 1.4
    beat_hz 0.2 4.0 log
    beat_lvl 0.0 0.6
    h2_lvl 0.0 0.25
    thumb_ms 0.1 4.0 log
    zing_lvl 0.0 0.4
    zing_f 2500.0 12000.0 log
    zing_tau 0.004 0.06 log
    skin_lvl 0.0 0.25
    skin_f 1500.0 9000.0 log
    helm_f 160.0 420.0 log
    helm_tau 0.008 0.06 log
    box_lvl 0.0 0.8
    box_lo 150.0 600.0 log
    box_tau 0.003 0.03 log
    knock_lvl 0.0 0.8
    knock_f 40.0 160.0 log
    knock_tau 0.008 0.08 log
    symp_lvl 0.0 0.25
    symp_rise 0.005 0.08 log
    room_wet 0.0 0.35
    rt60 0.15 0.9 log
    width 0.0 0.5
    air_db -9.0 0.0
    floor_db -88.0 -58.0
    rumble 0.0 0.8
""")

DEFAULTS: dict[str, Any] = cm.value_table("""
    p2_ratio 5.0  p2_lvl 0.386  p3_lvl 0.2038  tau1 0.3529  tau_exp 1.309  beat_hz 2.759
    beat_lvl 0.01985  h2_lvl 0.1751  thumb_ms 0.3572  zing_lvl 0.09041  zing_f 6908.0
    zing_tau 0.006356  skin_lvl 0.2027  skin_f 1500.0  helm_f 197.2  helm_tau 0.01108
    box_lvl 0.1924  box_lo 545.6  box_tau 0.01542  knock_lvl 0.5643  knock_f 77.94
    knock_tau 0.03731  symp_lvl 0.02239  symp_rise 0.07959  room_wet 0.2094  rt60 0.4881
    width 0.0  air_db -7.11  floor_db -64.71  rumble 0.3376
""")

_PITCH_CLASS = dict(zip("CDEFGAB", (0, 2, 4, 5, 7, 9, 11)))


def note(name: str, a4: float = 440.0) -> float:
    """Convert a note name ('C5', 'F#4', 'Bb3') to its frequency in Hz."""
    letter = name[:1].upper()
    if letter not in _PITCH_CLASS:
        raise ValueError(f"pluck: unknown note name {name!r}")
    semis = _PITCH_CLASS[letter]
    rest = name[1:]
    while rest[:1] in ("#", "b") and rest:
        semis += 1 if rest[0] == "#" else -1
        rest = rest[1:]
    try:
        octave = int(rest)
    except ValueError:
        raise ValueError(f"pluck: note name {name!r} has no octave") from None
    midi = semis + 12 * (octave + 1)
    return a4 * 2 ** ((midi - 69) / 12)


def _norm(sig: np.ndarray) -> float:
    """Peak magnitude plus a tiny guard, used as a divisor."""
    return float(np.max(np.abs(sig))) + 1e-12


def _pad_to(sig: np.ndarray, size: int) -> np.ndarray:
    out = np.zeros(size)
    out[: len(sig)] = sig[:size]
    return out


def mode(t: np.ndarray, f: float, tau: float, a: float, attack: float, rng: np.random.Generator) -> np.ndarray:
    """A decaying tine mode that starts from rest under a raised-cosine fade-in."""
    start = 0.15 * rng.uniform(0, cm.TWO_PI)
    out = a * cm.decay(t, tau) * cm.osc(f, t, start)
    ramp = max(2, cm.samples(attack))
    if ramp < len(out):
        out[:ramp] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, ramp))
    return out


def _tine_modes(p: dict, f0: float, tau1: float, lp: Any) -> list[tuple[float, float, float]]:
    """List (Hz, decay, level) of bending modes 2 to 4 that stay under the frequency limit."""
    second = p["p2_ratio"] * f0
    wanted = [(second, p["p2_lvl"])]
    if p["p3_lvl"] > 0.005:
        wanted += [(2.8 * second, p["p3_lvl"]), (5.5 * second, 0.5 * p["p3_lvl"])]
    slope = p["tau_exp"]
    return [(hz, tau1 * (hz / f0) ** -slope, level * lp(hz) / lp(f0))
            for hz, level in wanted if hz < _HZ_LIMIT]


def tine(p: dict, f0: float, n: int, rng: np.random.Generator) -> np.ndarray:
    """The metal tine: fundamental, a slow beat, the bending modes and a 2nd harmonic."""
    tt = cm.times(n)
    thumb_s = p["thumb_ms"] / 1000

    def soften(hz: float) -> float:
        # A soft thumb takes the top off the higher modes.
        return 1 / (1 + 0.5 * hz * thumb_s)

    base_tau = p["tau1"] * (f0 / 523.25) ** -0.5
    fade_in = max(0.35 * thumb_s, 0.0002)
    out = mode(tt, f0, base_tau, 1.0, fade_in, rng)
    if p["beat_lvl"] > 0.005:
        out += mode(tt, f0 + p["beat_hz"], 0.85 * base_tau, p["beat_lvl"], fade_in, rng)
    for hz, fall, level in _tine_modes(p, f0, base_tau, soften):
        out += mode(tt, hz, fall, level, fade_in, rng)
    if p["h2_lvl"] > 0.005:
        out += mode(tt, 2 * f0, base_tau / 2, p["h2_lvl"], fade_in, rng)
    return out


def box_ir(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Impulse response of the box: an air resonance and 18 short wood modes."""
    size = cm.samples(0.25)
    tt = cm.times(size)
    resp = np.zeros(size)
    resp[0] = 1.0
    air_hz = p["helm_f"]
    resp += 0.9 * cm.osc(air_hz, tt) * cm.decay(tt, p["helm_tau"]) * 2 * np.pi * air_hz / SR * 6
    low = p["box_lo"]
    for hz in np.exp(rng.uniform(np.log(low), np.log(2500.0), 18)):
        fall = p["box_tau"] * (hz / low) ** -0.5
        level = rng.uniform(0.2, 1.0)
        wave = cm.osc(hz, tt, rng.uniform(0, 6.3))
        resp += level * wave * cm.decay(tt, fall) * 2 * np.pi * hz / SR * 2
    return resp


def _cents_off(hz: float, f0: float) -> float:
    """Smallest distance in cents from ``hz`` to a low partial of ``f0``."""
    return min(abs(1200 * math.log2(hz / (k * f0))) for k in (0.5, 1, 1.5, 2, 3, 4))


def sympathetic(p: dict, f0: float, n: int, rng: np.random.Generator) -> np.ndarray:
    """The other tines ringing through the bridge, loudest near partials of the played note."""
    out = np.zeros(n)
    if p["symp_lvl"] < 0.003:
        return out
    tt = cm.times(n)
    swell = 1 - cm.decay(tt, p["symp_rise"])
    for hz in TINES:
        if abs(hz / f0 - 1) < 0.03:
            continue
        near = math.exp(-(_cents_off(hz, f0) / 30) ** 2)
        level = p["symp_lvl"] * (0.15 + near) * rng.uniform(0.5, 1.0)
        fall = 1.2 * p["tau1"] * (hz / 523.25) ** -0.5
        tuned = hz * (1 + 0.0015 * rng.standard_normal())
        out += level * cm.osc(tuned, tt, rng.uniform(0, 6.3)) * cm.decay(tt, fall) * swell
    return out


def skin(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    """Short band noise of the thumb or nail sliding off the tine."""
    span = cm.samples(0.02)
    burst = rng.standard_normal(span) * cm.decay(cm.times(span), 0.003)
    centre = p["skin_f"]
    burst = cm.bandpass(burst, centre / 2, min(2 * centre, _HZ_LIMIT))
    return _pad_to(burst / _norm(burst), n)


def thud(p: dict, n: int, rng: np.random.Generator) -> np.ndarray:
    """Low knock of the instrument body against whatever holds it."""
    span = min(n, cm.samples(0.4))
    tt = cm.times(span)
    hz = p["knock_f"] * (1 + 0.05 * rng.standard_normal())
    fall = p["knock_tau"]
    out = cm.osc(hz, tt) * cm.decay(tt, fall) * (1 - cm.decay(tt, 0.004))
    out += 0.5 * np.sin(cm.TWO_PI * 2.3 * hz * tt) * cm.decay(tt, 0.5 * fall) * (1 - cm.decay(tt, 0.002))
    return _pad_to(out / _norm(out), n)


def zing(p: dict, f0: float, n: int, rng: np.random.Generator) -> np.ndarray:
    """Buzz of the bridge: band noise opened once per period of the note."""
    span = min(n, cm.samples(0.4))
    tt = cm.times(span)
    centre = p["zing_f"]
    buzz = cm.bandpass(rng.standard_normal(span), 0.5 * centre, min(1.8 * centre, _HZ_LIMIT))
    opening = np.maximum(0, cm.osc(f0, tt)) ** 4
    out = buzz * (0.35 + opening) * cm.decay(tt, p["zing_tau"])
    return _pad_to(out / _norm(out), n)


def room_tone(n: int, rumble: float, rng: np.random.Generator) -> np.ndarray:
    """Pink background noise with some low rumble mixed in."""
    secs = n / SR
    pink = dsp.noise(secs, rng, "pink")[:n]
    low = cm.lowpass(dsp.noise(secs, rng, "brown")[:n], 120.0)
    low = low / (np.sqrt(np.mean(low ** 2)) + 1e-12)
    return (1 - rumble) * pink + 2 * rumble * low


def _choose_f0(p: dict, rng: np.random.Generator) -> float:
    given = p.get("f0")
    if given:
        return float(given)
    return float(REF_F0[int(rng.integers(len(REF_F0)))])


def _instrument(p: dict, f0: float, size: int, rng: np.random.Generator) -> np.ndarray:
    """Mono instrument sound before the room: tine, box, other tines and the noises."""
    raw = tine(p, f0, size, rng)
    level = _norm(raw)
    boxed = np.convolve(raw, box_ir(p, rng))[:size]
    boxed = boxed * level / _norm(boxed)
    share = p["box_lvl"]
    out = (1 - 0.5 * share) * raw + share * boxed
    out += level * sympathetic(p, f0, size, rng)
    out += p["skin_lvl"] * level * skin(p, size, rng)
    out += p.get("knock_lvl", 0.0) * level * thud(p, size, rng)
    if p.get("zing_lvl", 0.0) > 0.003:
        out += p["zing_lvl"] * level * zing(p, f0, size, rng)
    return out


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one note: stereo float64, at most 3 s; the caller sets the level."""
    p = {**DEFAULTS, **p}
    f0 = _choose_f0(p, rng)
    size = cm.samples(_TAKE_S)
    lead = cm.samples(0.004)
    out = np.concatenate([np.zeros(lead), _instrument(p, f0, size - lead, rng)])
    out = dsp.stereo(out, rng, width=p["width"], pan=0.0)
    out = dsp.room(out, rng, rt60=p["rt60"], size=0.3, damp=0.6, wet=p["room_wet"])
    out = dsp.mic(out, rng, low_cut=20.0, air_db=p["air_db"], floor_db=-140.0)[:size]
    noise_gain = _norm(out) * 10 ** (p["floor_db"] / 20)
    for ch in range(out.shape[1]):
        out[:, ch] += room_tone(size, p.get("rumble", 0.0), rng) * noise_gain
    out = dsp.trim_tail(out, p["floor_db"] - 2, 0.05)
    return dsp.fade(out, 0.0005, min(0.4, 0.3 * len(out) / SR))
