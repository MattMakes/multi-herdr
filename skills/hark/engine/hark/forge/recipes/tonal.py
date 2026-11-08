"""Tonal parts that the UI and tonal recipes build on.

The parts here are envelopes, pitch curves, sine voices with overtones, noise
that tracks a pitch, short noise hits, damped resonances, a small room and the
output chain. Interface sounds in film rise over 20 to 60 ms, die away in 0.2 to
0.4 s and sit a little wide in stereo, and the defaults follow that. All
randomness comes from the generator argument.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

import numpy as np
from numpy import (
    arange,
    argmax,
    array,
    asarray,
    broadcast_to,
    clip,
    column_stack,
    concatenate,
    convolve,
    cos,
    cumsum,
    exp,
    flatnonzero,
    hanning,
    linspace,
    log2,
    maximum,
    mean,
    median,
    minimum,
    ndarray,
    ones,
    pi,
    sin,
    sqrt,
    zeros,
)
from numpy.lib.stride_tricks import sliding_window_view

from hark.forge.engine import dsp, fx

SR = 48000
_FADE_TOP_HZ = 0.45 * SR
_CYCLE = 2.0 * pi
_TINY = 1e-12
_EMPTY = 1e-30
_SHORTEST = 1e-4  # seconds; keeps time constants above 0
_SHORTEST_HIT = 1e-5
_BYPASS = 0.005  # width, pan and wet at or under this do nothing
_SEMITONES = 12.0
_MS = 48  # samples in 1 ms
_HALF_SPAN = 120  # samples each side of a level window
_HOP = 240
_QUARTER_OCT = 2 ** 0.25


def _falloff(tt: ndarray, span: float, floor: float) -> ndarray:
    return exp(-tt / max(span, floor))


def _hann_up(u: Any) -> Any:
    return 0.5 - 0.5 * cos(pi * u)


def _unit_peak(sig: ndarray) -> ndarray:
    return sig / (np.max(np.abs(sig)) + _TINY)


def _phase(gen: np.random.Generator, hz_curve: Any) -> ndarray:
    return _CYCLE * cumsum(hz_curve) / SR + gen.uniform(0, _CYCLE)


def _shift_semis(base: float, semis: Any) -> Any:
    return base * 2.0 ** semis


def _soft_attack(tt: ndarray, onset: float, curve: float) -> ndarray:
    return _hann_up(clip(tt / max(onset, _SHORTEST), 0.0, 1.0) ** curve)


def env(t: ndarray, onset: float, tau: float, hold: float = 0, curve: float = 1,
        dcurve: float = 1) -> ndarray:
    """Rise on a half cosine for `onset` seconds, hold, then fall away."""
    lag = maximum(t - onset - hold, 0.0) / max(tau, _SHORTEST)
    if dcurve != 1:
        lag = lag ** dcurve
    return _soft_attack(t, onset, curve) * exp(-lag)


def two_rate(t: ndarray, onset: float, fast: float, tf: float, ts: float, curve: float = 1) -> ndarray:
    """Rise, then a quick fall weighted by `fast` over a slower ring."""
    since = maximum(t - onset, 0.0)
    quick = fast * _falloff(since, tf, _SHORTEST)
    slow = (1 - fast) * _falloff(since, ts, _SHORTEST)
    return _soft_attack(t, onset, curve) * (quick + slow)


def glide(t: ndarray, f0: float, bend_st: float = 0, ptau: float = 0.02) -> ndarray:
    """Pitch curve that begins bend_st semitones away and settles to f0."""
    return _shift_semis(f0, (bend_st / _SEMITONES) * _falloff(t, ptau, _SHORTEST))


def rise_track(t: ndarray, f0: float, rise_st: float, rtau: float) -> ndarray:
    """Pitch curve that climbs rise_st semitones above f0, as a bubble does."""
    return _shift_semis(f0, (rise_st / _SEMITONES) * (1.0 - _falloff(t, rtau, _SHORTEST)))


def nyquist_fade(f: Any, q: float = 1.0) -> Any:
    """Level for overtone q of pitch f, going to 0 across the 2.4 kHz under 0.45 SR."""
    return clip((_FADE_TOP_HZ - q * asarray(f, dtype=float)) / 2400.0, 0.0, 1.0)


def tone(f: Any, r: np.random.Generator, parts: Sequence[tuple[float, float, float]] = (),
         e: ndarray | None = None) -> ndarray:
    """Sine voice that follows pitch f, plus overtones given as (ratio, level, envelope power)."""
    hz_curve = asarray(f, dtype=float)
    shape = ones(len(hz_curve)) if e is None else asarray(e, dtype=float)
    base_ph = _phase(r, hz_curve)
    voice = sin(base_ph) * shape
    for mult, level, power in parts:
        if level > 0:
            over = level * nyquist_fade(hz_curve, mult) * shape ** power
            voice = voice + over * sin(mult * base_ph + r.uniform(0, _CYCLE))
    return voice


def follow_noise(n: int, r: np.random.Generator, fc: Any, q: float, e: ndarray) -> ndarray:
    """Noise band-passed at fc and scaled to the level of a unit sine where e is on."""
    shape = asarray(e, dtype=float)
    on = shape > 0.1 * (np.max(shape) + _TINY)
    cutoff = broadcast_to(minimum(asarray(fc, dtype=float), 16000.0), (n,)).copy()
    banded = dsp.svf(r.standard_normal(n), cutoff=cutoff, q=q, mode="band")
    level = sqrt(mean(banded[on] ** 2)) if on.any() else 0.0
    gain = sqrt(0.5) / (level + _TINY)
    return banded * gain * shape


def burst(n: int, r: np.random.Generator, lo: float, hi: float, tau: float, rise: float = 0.0003,
          order: int = 2) -> ndarray:
    """Brief band of noise for a contact, peak 1, padded with zeros to n samples."""
    used = min(n, dsp.n_of(max(6 * tau + rise, 0.002)))
    tt = dsp.t_of(used)
    noise = r.standard_normal(used)
    hit = dsp.butter(noise, kind="band", f=(max(lo, 20.0), min(hi, _FADE_TOP_HZ)), order=order)
    hit = hit * (1 - _falloff(tt, rise, _SHORTEST_HIT))
    hit = hit * _falloff(tt, tau, _SHORTEST_HIT)
    return concatenate([_unit_peak(hit), zeros(n - used)])


def thump(n: int, f: float, tau: float, rise: float = 0.002) -> ndarray:
    """Low soft sine knock, peak 1, uses no randomness."""
    tt = dsp.t_of(n)
    knock = sin(_CYCLE * f * tt) * (1 - _falloff(tt, rise, _SHORTEST)) * _falloff(tt, tau, _SHORTEST)
    return _unit_peak(knock)


def modes(n: int, r: np.random.Generator, freqs: Sequence[float], amps: Sequence[float], taus: Sequence[float],
          jitter: float = 0, bend: float = 0, bend_tau: float = 0.004) -> ndarray:
    """Damped sines added together; `bend` makes a hard hit begin a little sharp."""
    tt = dsp.t_of(n)
    sharpen = 1.0 + bend * _falloff(tt, bend_tau, _SHORTEST)
    total = zeros(n)
    for hz, level, ring in zip(freqs, amps, taus):
        if level > 0 and hz < _FADE_TOP_HZ:
            moved = hz * (1 + jitter * r.standard_normal())
            total += level * _falloff(tt, ring, _SHORTEST_HIT) * sin(_phase(r, moved * sharpen))
    return total


def strike(y: ndarray, r: np.random.Generator, contact_s: float, bright: float = 0.5) -> ndarray:
    """Excite resonator output y with a contact pulse that lasts contact_s seconds."""
    pulse = dsp.contact(seconds=contact_s, r=r, bright=bright)
    return dsp.excite(resonance=y, excitation=pulse)


def room(y: ndarray, r: np.random.Generator, wet: float, rt60: float, size: float = 0.15,
         width: float = 0.3) -> ndarray:
    """Small stereo space. At wet _BYPASS or less it passes y through and draws nothing."""
    if wet <= _BYPASS:
        return y
    space = {"rt60": rt60, "size": size, "predelay": 0.004, "damp": 0.6, "wet": wet, "stereo": True, "width": width}
    return fx.reverb(y, r, **space)


def _apply_gain(sig: ndarray, span: slice, gain: ndarray) -> ndarray:
    held = sig.copy()
    per_row = gain.reshape((-1,) + (1,) * (held.ndim - 1))
    held[span] = held[span] * per_row
    return held


def _taper_end(sig: ndarray, cnt: int) -> ndarray:
    if cnt <= 0:
        return sig
    gain = cos(linspace(0, pi / 2, cnt)) ** 2
    gain[-1] = 0.0  # the float cosine of pi/2 is a little above 0
    return _apply_gain(sig, slice(-cnt, None), gain)


def _taper_start(sig: ndarray) -> ndarray:
    cnt = min(len(sig), dsp.n_of(0.0003))
    if cnt <= 0:
        return sig
    return _apply_gain(sig, slice(None, cnt), _hann_up(linspace(0, 1, cnt)))


def _spread_pan(sig: ndarray, width: float, cut: float, pan: float) -> ndarray:
    if width > _BYPASS:
        pair = fx.decorrelate(sig, amount=width, cut=cut)
    else:
        pair = column_stack([sig, sig])
    if abs(pan) <= _BYPASS:
        return pair
    angle = (clip(pan, -1, 1) + 1) * pi / 4
    return pair * array([cos(angle), sin(angle)]) * sqrt(2)


def _mono_stage(sig: ndarray, hp: float, lp: float) -> ndarray:
    pre_n = min(len(sig), dsp.n_of(0.02), int(0.15 * len(sig)))
    sig = dsp.butter(_taper_end(sig, pre_n), kind="high", f=hp, order=4)
    if lp >= 17500:
        return sig
    return dsp.butter(sig, kind="low", f=lp, order=2)


def _close_edges(pair: ndarray, floor_db: float, fade_s: float) -> ndarray:
    pair = dsp.trim_tail(pair, floor_db=floor_db, keep=0.01)
    pair = _taper_end(pair, min(len(pair), dsp.n_of(fade_s)))
    return _taper_start(pair)


def finish(y: ndarray, r: np.random.Generator, hp: float = 80, lp: float = 12000, room_wet: float = 0,
           rt60: float = 0.15, width: float = 0, cut: float = 500, end_db: float = -60, fade: float = 0.006,
           pan: float = 0, size: float = 0.15) -> ndarray:
    """Takes mono and returns stereo: early fade, high and low cut, width, pan, room, trim, edge fades."""
    dry = _mono_stage(asarray(y, dtype=float), hp, lp)
    placed = _spread_pan(dry, width, cut, pan)
    spaced = room(placed, r, wet=room_wet, rt60=rt60, size=size, width=0.3 + width)
    return _close_edges(spaced, end_db, fade)


# Measures of a finished render. They need no model.

def _first_at(mask: ndarray) -> int | None:
    found = flatnonzero(mask)
    if len(found):
        return int(found[0])
    return None


def _power_curve(sig: ndarray) -> ndarray:
    size = len(sig)
    acc = concatenate([[0.0], cumsum(sig * sig)])
    mid = arange(0, size, _MS)
    left, right = (clip(mid + side, 0, size) for side in (-_HALF_SPAN, _HALF_SPAN))
    window = (acc[right] - acc[left]) / maximum(right - left, 1)
    return convolve(window, ones(10) / 10, mode="same")


# Power ratios for drops of 20, 30, 40 and 45 dB.
_DROP_20 = 1e-2
_DROP_30 = 1e-3
_DROP_40 = 1e-4
_DROP_45 = 10 ** -4.5


def _time_stats(sig: ndarray) -> dict:
    pw = _power_curve(sig)
    at = int(argmax(pw))
    top = float(pw[at])
    rising = flatnonzero(pw[: at + 1] >= top * _DROP_20)
    audible = flatnonzero(pw >= top * _DROP_45)
    begin = _first_at(pw >= top * _DROP_30) or 0
    last = int(audible[-1]) if len(audible) else len(sig) / _MS
    stats: dict = {"dur_s": round(last * 0.001 - begin * 0.001, 3),
                   "attack_ms": at - int(rising[0]) if len(rising) else None}
    after = pw[at:]
    for key, drop in (("t20_ms", _DROP_20), ("t40_ms", _DROP_40)):
        stats[key] = _first_at(after < top * drop)
    return stats


def _stft_power(sig: ndarray) -> tuple[ndarray, ndarray]:
    win = 2048
    if len(sig) < win:
        sig = concatenate([sig, zeros(win - len(sig))])
    frames = sliding_window_view(sig, win)[::_HOP]
    spec = np.abs(np.fft.rfft(frames * hanning(win), axis=1)) ** 2
    return spec, np.fft.rfftfreq(win, 1 / SR)


def _peak_path(spec: ndarray, axis_hz: ndarray) -> tuple[list[float], list[float]]:
    inside = (axis_hz >= 80) & (axis_hz <= 12000)
    sub = axis_hz[inside]
    pitch, focus = [], []
    for frame in spec[:, inside]:
        if frame.max() > 0:
            best = sub[int(argmax(frame))]
            near = (sub >= best / _QUARTER_OCT) & (sub <= best * _QUARTER_OCT)
            pitch.append(float(best))
            focus.append(float(frame[near].sum() / (frame.sum() + _EMPTY)))
    return pitch, focus


def _balance_into(stats: dict, loud: ndarray, axis_hz: ndarray) -> None:
    avg = loud.mean(axis=0)
    whole = avg.sum() + _EMPTY
    stats["centroid_hz"] = int(round(float((avg * axis_hz).sum() / whole)))
    stats["lo150"] = round(float(avg[axis_hz < 150].sum() / whole), 3)
    stats["hi4k"] = round(float(avg[axis_hz >= 4000].sum() / whole), 3)


def _pitch_into(stats: dict, loud: ndarray, axis_hz: ndarray) -> None:
    pitch, focus = _peak_path(loud, axis_hz)
    if len(focus) < 2:
        return
    ends = max(1, min(3, len(pitch) // 3))
    stats["tonal"] = round(float(mean(focus)), 2)
    shift = median(pitch[-ends:]) / median(pitch[:ends])
    stats["glide_st"] = round(float(12 * log2(shift)), 1)


def _spectrum_stats(sig: ndarray) -> dict:
    spec, axis_hz = _stft_power(sig)
    stats: dict = dict.fromkeys(("centroid_hz", "lo150", "hi4k", "tonal", "glide_st"))
    energy = spec.sum(axis=1)
    if energy.max() <= 0:
        return stats
    loud = spec[energy >= 0.01 * energy.max()]
    if len(loud):
        _balance_into(stats, loud, axis_hz)
        _pitch_into(stats, loud, axis_hz)
    return stats


def measure(x: ndarray) -> dict:
    """Catalogue measures for 1 render over silence. No model is used."""
    pair = dsp.stereo_of(asarray(x, dtype=float))
    mid = pair.mean(axis=1)
    found = _time_stats(mid)
    found.update(_spectrum_stats(mid))
    a, b = pair[:, 0], pair[:, 1]
    norm = sqrt(np.sum(a * a) * np.sum(b * b)) + _EMPTY
    found["corr"] = round(float(np.sum(a * b) / norm), 2)
    return found


def _range_miss(got: Any, lo: float, hi: float, scale: float) -> float:
    if got is None:
        return 1.0
    gap = lo - got if got < lo else (got - hi if got > hi else 0.0)
    return min(2.0, gap / scale) if gap else 0.0


def outside(m: dict, targets: dict) -> float:
    """How far measures m fall from target ranges {key: (lo, hi, scale)}. 0 means all are met."""
    miss = 0.0
    for want, bounds in targets.items():
        miss += _range_miss(m.get(want), *bounds)
    return float(miss)


def family_space(space: dict, params: dict, fine: Sequence[str]) -> dict:
    """Keep ranges for the keys in fine. Fix each other key at the value in params."""
    pinned = {}
    for key, rng in space.items():
        if key in params:
            pinned[key] = rng if key in fine else [params[key]]
    return pinned
