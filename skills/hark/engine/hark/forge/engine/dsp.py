"""Sound building blocks for forge recipes.

Every function computes its samples. Nothing reads recorded audio. The random
generator is always passed in, and the order of its draws is fixed, so one
seed gives one exact render.
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
    cos,
    cumsum,
    exp,
    full,
    interp,
    isscalar,
    linspace,
    log,
    log2,
    log10,
    maximum,
    mean,
    ndarray,
    nonzero,
    pi,
    ptp,
    sin,
    sort,
    sqrt,
    square,
    tan,
    where,
    zeros,
    zeros_like,
)
from numpy.random import Generator, default_rng
from scipy import signal
from scipy.signal import fftconvolve, lfilter, sosfilt, sosfiltfilt, ss2tf

SR = 48000
NYQ = SR / 2
_TWO_PI = 2 * pi

BELLS: dict[str, tuple[tuple[float, ...], tuple[float, ...]]] = {
    "church": ((0.5, 1.0, 1.183, 1.506, 2.0, 2.514, 2.662, 3.011, 4.166, 5.433, 6.796, 8.215),
               (0.55, 0.8, 0.7, 0.35, 1.0, 0.45, 0.4, 0.3, 0.25, 0.18, 0.12, 0.08)),
    "tubular": ((1.0, 2.756, 5.404, 8.933, 13.34), (0.35, 1.0, 0.7, 0.4, 0.2)),
    "bar": ((1.0, 2.756, 5.404, 8.933), (1.0, 0.3, 0.12, 0.06)),
    "glass": ((1.0, 2.32, 4.25, 6.63, 9.38), (1.0, 0.45, 0.25, 0.12, 0.06)),
    "bowl": ((1.0, 2.71, 5.13, 8.27), (1.0, 0.6, 0.3, 0.12)),
}


# ---------------------------------------------------------------- basics

def rng(seed: int) -> Generator:
    """The forge generator for one seed."""
    return default_rng(int(seed) & 0xFFFFFFFF)


def n_of(seconds: float) -> int:
    """Sample count of a duration, at least 1."""
    return max(1, int(round(float(seconds) * SR)))


def t_of(n: int) -> ndarray:
    """Sample times in seconds."""
    return arange(int(n)) / SR


def _db_gain(db: Any) -> Any:
    """Linear amplitude of a level in dB."""
    return 10 ** (db / 20)


def _decay(times: ndarray, tau: Any, floor: float) -> ndarray:
    """exp(-t / tau), with tau held above floor."""
    return exp(-times / max(tau, floor))


def _grid(seconds: float) -> tuple[int, ndarray]:
    """Sample count and sample times of a duration."""
    num = n_of(seconds)
    return num, t_of(num)


def _energy(src: ndarray) -> float:
    return float(sqrt(mean(square(src)))) + 1e-12


def _absmax(src: ndarray) -> float:
    return float(np.max(np.abs(src))) if np.size(src) else 0.0


def _abs_level(src: ndarray) -> ndarray:
    """Per-sample absolute level, the channel maximum for stereo."""
    amp = np.abs(asarray(src, float))
    return amp.max(axis=1) if amp.ndim == 2 else amp


def stereo_of(x: ndarray) -> ndarray:
    """Mono to 2 equal channels; stereo is returned unchanged."""
    sig = asarray(x, float)
    return column_stack([sig, sig]) if sig.ndim == 1 else sig


def _half_cos_ramp(num: int) -> ndarray:
    """Half raised cosine 0..1 over n points."""
    return 0.5 - 0.5 * cos(linspace(0.0, pi, num))


# ---------------------------------------------------------------- noise, envelopes

def _tilt_fft(src: ndarray, expo: float) -> ndarray:
    bins = np.fft.rfft(src)
    hz = np.fft.rfftfreq(len(src), 1.0 / SR)
    if len(hz) > 1:
        hz[0] = hz[1]
    else:
        hz[0] = 1.0
    return np.fft.irfft(bins / hz ** expo, len(src))


def noise(seconds: float, r: Generator, color: str = "white") -> ndarray:
    """Unit-RMS noise; white, pink or brown."""
    src = r.standard_normal(n_of(seconds))
    if color != "white":
        src = _tilt_fft(src, 0.5 if color == "pink" else 1.0)
    return src / (sqrt(mean(src * src)) + 1e-12)


def env_exp(seconds: float, tau: float, attack: float = 0.0005) -> ndarray:
    """Exponential decay with a short linear attack."""
    num = n_of(seconds)
    envl = exp(-t_of(num) / max(tau, 1e-5))
    amp = min(n_of(attack), num)
    if amp > 1:
        envl[:amp] *= linspace(0.0, 1.0, amp)
    return envl


def env_curve(points: Sequence[tuple[float, float]], seconds: float, shape: str = "lin") -> ndarray:
    """Breakpoint envelope; ``exp`` interpolates in dB."""
    knots = sorted((float(when), float(level)) for when, level in points)
    knots_t = array([p[0] for p in knots])
    knots_v = array([p[1] for p in knots])
    times = t_of(n_of(seconds))
    if shape == "exp":
        db_vals = 20 * log10(maximum(knots_v, 1e-6))
        return _db_gain(interp(times, knots_t, db_vals))
    return interp(times, knots_t, knots_v)


def contact(seconds: float, r: Generator, bright: float = 0.7) -> ndarray:
    """A raised-cosine contact pulse with noisy grit."""
    num = max(2, n_of(seconds))
    bump = 0.5 - 0.5 * cos(linspace(0.0, 2 * pi, num))
    hiss = r.standard_normal(num) * bump * bright
    return bump + hiss


# ---------------------------------------------------------------- resonators

def modal(freqs: Sequence[float], decays: Sequence[float], amps: Sequence[float], seconds: float,
          r: Generator, jitter: float = 0.003, glide: float = 0.0) -> ndarray:
    """Add up exponentially decaying sines, one per (frequency, decay, level) triple."""
    num, times = _grid(seconds)
    total = zeros(num)
    for hz, tau, level in zip(freqs, decays, amps):
        if level == 0 or hz >= 0.95 * NYQ:
            continue
        detuned = hz * (1 + jitter * r.standard_normal())
        bent = detuned * (1 - glide * (1 - _decay(times, tau, 1e-4)))
        start = r.uniform(0, _TWO_PI)
        total += level * _decay(times, tau, 1e-5) * sin(_TWO_PI * cumsum(bent) / SR + start)
    return total


def modal_cloud(f_lo: float, f_hi: float, count: int, decay_lo: float, decay_hi: float, seconds: float,
                r: Generator, tilt: float = -3.0) -> ndarray:
    """Many modes spread on a log scale; decay moves from decay_lo to decay_hi with pitch."""
    return _log_spread_modes(r, seconds, int(count), (f_lo, f_hi), (decay_lo, decay_hi), tilt)


def _log_spread_modes(gen: Generator, dur: float, k: int, band: tuple, taus_at_edges: tuple,
                      db_per_oct: float) -> ndarray:
    bottom, top = band
    hz = sort(exp(gen.uniform(log(bottom), log(top), k)))
    pos = log(hz / bottom) / max(log(top / bottom), 1e-9)
    first_tau, last_tau = (log(v) for v in taus_at_edges)
    taus = exp(first_tau + pos * (last_tau - first_tau))
    shades = gen.uniform(0.3, 1.0, len(hz))
    return modal(hz, taus, shades * _db_gain(db_per_oct * log2(hz / bottom)), dur, gen, jitter=0.0)


def excite(resonance: ndarray, excitation: ndarray) -> ndarray:
    """Convolve an excitation with a resonance, cut to the resonance length."""
    sig = fftconvolve(asarray(excitation, float), asarray(resonance, float))
    return sig[: len(resonance)]


# ---------------------------------------------------------------- filters

def _safe_hz(hz: Any) -> Any:
    return clip(asarray(hz, float), 10.0, 0.95 * NYQ)


def butter(x: ndarray, kind: str, f: Any, order: int = 2) -> ndarray:
    """Causal Butterworth filter along axis 0 (low, high, band, bandstop)."""
    band_kind = {"low": "lowpass", "high": "highpass", "band": "bandpass", "bandstop": "bandstop"}[kind]
    cut = _safe_hz(f)
    edges = cut.tolist() if cut.ndim else float(cut)
    sec_coefs = signal.butter(order, edges, btype=band_kind, fs=SR, output="sos")
    return sosfilt(sec_coefs, asarray(x, float), axis=0)


def peq(x: ndarray, f0: float, gain_db: float, q: float = 0.9) -> ndarray:
    """RBJ peaking biquad."""
    lin_gain = 10 ** (gain_db / 40)
    omega = 2 * pi * min(f0, 0.95 * NYQ) / SR
    alf = sin(omega) / (2 * q)
    bq = array([1 + alf * lin_gain, -2 * cos(omega), 1 - alf * lin_gain])
    amp = array([1 + alf / lin_gain, -2 * cos(omega), 1 - alf / lin_gain])
    return lfilter(bq / amp[0], amp / amp[0], asarray(x, float), axis=0)


def _tpt_gains(cut: ndarray, res: ndarray) -> tuple[ndarray, ...]:
    gain = tan(pi * cut / SR)
    kn = 1.0 / maximum(res, 0.05)
    h1 = 1.0 / (1 + gain * (gain + kn))
    h2 = gain * h1
    return kn, h1, h2, gain * h2


def _tpt_static(src: ndarray, kn: float, h1: float, h2: float, h3: float, tap: str) -> ndarray:
    mat_a = array([[2 * h1 - 1, -2 * h2], [2 * h2, 1 - 2 * h3]])
    mat_b = array([[2 * h2], [2 * h3]])
    taps = {"band": ([h1, -h2], h2), "low": ([h2, 1 - h3], h3)}
    if tap in taps:
        ctl, dv = taps[tap]
    elif tap == "bandn":
        ctl, dv = [kn * h1, -kn * h2], kn * h2
    else:
        ctl, dv = [-kn * h1 - h2, kn * h2 - (1 - h3)], 1 - kn * h2 - h3
    bq, amp = ss2tf(mat_a, mat_b, array([ctl]), array([[dv]]))
    return lfilter(bq[0], amp, src)


def _tpt_run(samples: list, g1: list, g2: list, g3: list) -> tuple[ndarray, ndarray]:
    """Band (v1) and low (v2) outputs of the TPT state-variable filter, sample by sample."""
    band_hist, low_hist = [], []
    s1 = s2 = 0.0
    for ix, sample in enumerate(samples):
        inp_v = sample - s2
        band_v = g1[ix] * s1 + g2[ix] * inp_v
        low_v = s2 + g2[ix] * s1 + g3[ix] * inp_v
        s1 = 2 * band_v - s1
        s2 = 2 * low_v - s2
        band_hist.append(band_v)
        low_hist.append(low_v)
    return array(band_hist, float), array(low_hist, float)


_TPT_TAPS = {"low": lambda src, kn, band_v, low_v: low_v, "band": lambda src, kn, band_v, low_v: band_v,
            "bandn": lambda src, kn, band_v, low_v: kn * band_v}


def _tpt_highpass(src: ndarray, kn: ndarray, band_v: ndarray, low_v: ndarray) -> ndarray:
    return src - kn * band_v - low_v


def _tpt_varying(src: ndarray, kn: ndarray, h1: ndarray, h2: ndarray, h3: ndarray,
              tap: str) -> ndarray:
    band_v, low_v = _tpt_run(src.tolist(), h1.tolist(), h2.tolist(), h3.tolist())
    return _TPT_TAPS.get(tap, _tpt_highpass)(src, asarray(kn, float), band_v, low_v)


def svf(x: ndarray, cutoff: Any, q: Any = 0.7, mode: str = "band") -> ndarray:
    """Time-varying TPT state-variable filter (low, band, bandn, high)."""
    sig = asarray(x, float)
    if sig.ndim == 2:
        return column_stack([svf(sig[:, ctl], cutoff, q, mode) for ctl in range(sig.shape[1])])
    num = len(sig)
    cut = clip(broadcast_to(asarray(cutoff, float), (num,)), 20.0, 0.45 * SR)
    res_arr = broadcast_to(asarray(q, float), (num,))
    if num and ptp(cut) == 0 and ptp(res_arr) == 0:
        kn, h1, h2, h3 = (float(arr[0]) for arr in _tpt_gains(cut[:1], res_arr[:1]))
        return _tpt_static(sig, kn, h1, h2, h3, mode)
    return _tpt_varying(sig, *_tpt_gains(cut, res_arr), mode)


# ---------------------------------------------------------------- room, mic, stereo

def _first_order(src: ndarray, ftype: str, hz: float) -> ndarray:
    return butter(src, ftype, hz, order=1)


def room_ir(r: Generator, rt60: float = 0.25, size: float = 0.5, damp: float = 0.6,
            seconds: float | None = None) -> ndarray:
    """Impulse response of a small room: unit spike, 6 random echoes, darkening noise tail."""
    if seconds is None:
        seconds = min(2.0, 1.2 * rt60 + 0.05)
    num, times = _grid(seconds)
    hiss_tail = r.standard_normal(num) * exp(-6.91 * times / max(rt60, 0.02))
    dark = _first_order(hiss_tail, "low", 1500 + (1 - damp) * 9000)
    mixw = (times / seconds) ** 0.7 * damp
    resp = 0.3 * ((1 - mixw) * hiss_tail + mixw * dark)
    resp[0] += 1.0
    for _ in range(6):
        at = int(r.uniform(0.002, 0.004 + 0.02 * size) * SR)
        level = r.uniform(0.15, 0.45)
        polarity = r.choice([-1.0, 1.0])
        if at < num:
            resp[at] += level * polarity
    return resp


def room(x: ndarray, r: Generator, rt60: float = 0.25, size: float = 0.5, damp: float = 0.6,
         wet: float = 0.12) -> ndarray:
    """Return the input plus a reverberant copy scaled by wet (energy-normalised response)."""
    resp = room_ir(r, rt60, size, damp)
    resp[0] = 0.0
    dry = asarray(x, float)
    scale = sqrt(np.sum(resp * resp)) + 1e-9
    if dry.ndim == 2:
        tail = fftconvolve(dry, resp[:, None], axes=0)
    else:
        tail = fftconvolve(dry, resp)
    out = wet * tail / scale
    out[: dry.shape[0]] += dry
    return out


def mic(x: ndarray, r: Generator, low_cut: float = 35.0, air_db: float = -2.0,
        floor_db: float = -66.0, color: tuple[float, float] | None = None) -> ndarray:
    """Pretend capture: high-pass, top shelf, optional bump, and pink hiss below the peak."""
    out = _first_order(asarray(x, float), "high", low_cut)
    out = peq(out, 10000.0, air_db, 0.6)
    if color is not None:
        out = peq(out, color[0], color[1], 0.8)
    hiss_level = (_absmax(out) + 1e-12) * _db_gain(floor_db)
    length = out.shape[0]
    dur = length / SR
    channels = 1 if out.ndim == 1 else out.shape[1]
    beds = [noise(dur, r, "pink")[:length] for _ in range(channels)]
    bed = beds[0] if out.ndim == 1 else column_stack(beds)
    return out + hiss_level * bed


def _shift_right(src: ndarray, dv: int) -> ndarray:
    buf = zeros_like(src)
    if dv < len(src):
        buf[dv:] = src[: len(src) - dv]
    return buf


def _equal_power(pos_lr: Any) -> tuple[Any, Any]:
    angle = (clip(pos_lr, -1, 1) + 1) * pi / 4
    return cos(angle) * sqrt(2), sin(angle) * sqrt(2)


def stereo(x: ndarray, r: Generator, width: float = 0.3, pan: float = 0.0) -> ndarray:
    """Widen mono into 2 channels from a filtered difference of 2 tiny delays, then pan."""
    mono = asarray(x, float)
    lags = [int(r.uniform(0.2, 0.9) * 1e-3 * SR) for _ in range(2)]
    side = butter(_shift_right(mono, lags[0]) - _shift_right(mono, lags[1]), "high", 300.0, 2)
    gl, gr = _equal_power(pan)
    spread = width * side
    return column_stack([(mono + spread) * gl, (mono - spread) * gr])


# ---------------------------------------------------------------- finishing

def fade(x: ndarray, fin: float = 0.001, fout: float = 0.02) -> ndarray:
    """Raised-cosine fade in and fade out."""
    sig = array(x, dtype=float, copy=True)
    num = len(sig)
    amp, bq = min(num, n_of(fin)), min(num, n_of(fout))
    if amp > 1:
        win = 0.5 - 0.5 * cos(linspace(0, pi, amp))
        sig[:amp] *= win if sig.ndim == 1 else win[:, None]
    if bq > 1:
        win = 0.5 + 0.5 * cos(linspace(0, pi, bq))
        sig[num - bq:] *= win if sig.ndim == 1 else win[:, None]
    return sig


def place(dst: ndarray, x: ndarray, t: float, gain: float = 1.0) -> ndarray:
    """Mix x into dst starting at t seconds; writes into dst and returns it."""
    start = n_of(t) if t > 0 else 0
    free = len(dst) - start
    if free <= 0:
        return dst
    m = min(len(x), free)
    dst[start:start + m] += gain * asarray(x[:m])
    return dst


def trim_tail(x: ndarray, floor_db: float = -70.0, keep: float = 0.01) -> ndarray:
    """Drop everything after the last sample that reaches floor_db below the peak, plus keep seconds."""
    sig = asarray(x)
    if not len(sig):
        return sig
    level = np.abs(sig)
    if level.ndim == 2:
        level = level.max(axis=1)
    above = nonzero(level >= level.max() * _db_gain(floor_db))[0]
    if above.size == 0:
        return sig[:]
    return sig[: min(len(sig), int(above[-1]) + n_of(keep))]


def normalize(x: ndarray, peak_db: float = -1.0) -> ndarray:
    """Scale to a peak level in dBFS."""
    sig = asarray(x, float)
    return sig * _db_gain(peak_db) / (_absmax(sig) + 1e-12)


# ---------------------------------------------------------------- curves, oscillators

def _stretch_to(amp: ndarray, num: int) -> ndarray:
    return amp if len(amp) == num else interp(linspace(0, len(amp) - 1, num), arange(len(amp)), amp)


def curve(spec: Any, n: int, shape: str = "lin") -> ndarray:
    """Per-sample control track built from a number, a breakpoint list or an array."""
    if isscalar(spec):
        return full(n, float(spec))
    if _is_breakpoints(spec):
        return env_curve(spec, n / SR, shape)[:n]
    return _stretch_to(asarray(spec, float), n)


def _is_breakpoints(obj: Any) -> bool:
    seq = (list, tuple)
    return isinstance(obj, seq) and len(obj) > 0 and isinstance(obj[0], seq)


def sweep(f0: float, f1: float, seconds: float, curve: str = "exp", power: float = 1.0) -> ndarray:
    """A frequency track from f0 to f1."""
    pos = linspace(0.0, 1.0, n_of(seconds))
    if curve == "lin":
        return f0 + (f1 - f0) * pos
    if curve == "ease":
        pos = pos ** power
    return f0 * (f1 / f0) ** pos


def _poly_blep(times: ndarray, inc: ndarray) -> ndarray:
    buf = zeros_like(times)
    below = times < inc
    pos = times[below] / inc[below]
    buf[below] = 2 * pos - pos * pos - 1
    above = (~below) & (times > 1 - inc)
    pos = (times[above] - 1) / inc[above]
    buf[above] = pos * pos + 2 * pos + 1
    return buf


def _pulse_wave(ang: ndarray, inc: ndarray) -> ndarray:
    sqw = where(ang < 0.5, 1.0, -1.0)
    return sqw + _poly_blep(ang, inc) - _poly_blep((ang + 0.5) % 1.0, inc)


def _leaky_tri(hz: ndarray, sqw: ndarray, start_ph: float) -> ndarray:
    init_state = array([0.9995 * -0.9995 * (1 - 2 * start_ph)])
    sig, _ = lfilter([1.0], [1.0, -0.9995], 4 * hz / SR * sqw, zi=init_state)
    return sig


def osc(freq: Any, shape: str = "sine", seconds: float | None = None, phase: float = 0.0) -> ndarray:
    """Band-limited oscillator (sine, saw, square, tri) with PolyBLEP edges."""
    hz = asarray(freq, float)
    if hz.ndim == 0:
        hz = full(n_of(seconds or 1.0), float(hz))
    inc = clip(hz / SR, 1e-9, 0.49)
    start_ph = phase % 1.0
    ang = (start_ph + concatenate([[0.0], cumsum(inc[:-1])])) % 1.0
    if shape == "sine":
        sig = sin(2 * pi * ang)
    elif shape == "saw":
        sig = 2 * ang - 1 - _poly_blep(ang, inc)
    elif shape == "square":
        sig = _pulse_wave(ang, inc)
    else:
        sig = _leaky_tri(hz, _pulse_wave(ang, inc), start_ph)
    return sig


def _glide_voice(gen: Generator, num: int, pos: ndarray, opts: tuple) -> ndarray:
    hz_lo, n_oct, slope_db, ftype, res = opts
    hz = hz_lo * 2 ** (pos * n_oct)
    amp = (0.5 - 0.5 * cos(2 * pi * pos)) * _db_gain(slope_db * pos * n_oct)
    if ftype == "tone":
        return amp * sin(2 * pi * cumsum(hz) / SR + gen.uniform(0, 2 * pi))
    return amp * svf(gen.standard_normal(num), hz, res, "bandn") * sqrt(res)


def shepard(seconds: float, r: Generator, up: bool = True, rate: float = 0.5, f_lo: float = 40.0,
            f_hi: float = 10000.0, kind: str = "tone", q: float = 6.0, tilt: float = -1.5) -> ndarray:
    """Shepard-Risset glissando of tones or noise bands, unit RMS."""
    num, times = _grid(seconds)
    n_oct = log2(f_hi / f_lo)
    voices = max(2, int(round(n_oct)))
    direction = 1.0 if up else -1.0
    opts = (f_lo, n_oct, tilt, kind, q)
    sig = zeros(num)
    for kn in range(voices):
        sig += _glide_voice(r, num, (kn / voices + direction * rate * times / n_oct) % 1.0, opts)
    return sig / _energy(sig)


# ---------------------------------------------------------------- air

def wander(seconds: float, r: Generator, rate: float = 4.0, octaves: int = 3) -> ndarray:
    """Smooth random control signal, mean 0 and unit standard deviation."""
    num = n_of(seconds)
    span = max(16, int(1000 * num / SR) + 4)
    total = zeros(span)
    for ix in range(octaves):
        rate_hz = float(clip(rate / 3 ** ix, 0.05, 400))
        guard = int(2000 / rate_hz) + 8
        white = r.standard_normal(span + 2 * guard)
        sec_coefs = signal.butter(2, min(rate_hz / 500, 0.999), "low", output="sos")
        smooth = sosfiltfilt(sec_coefs, white)[guard:guard + span]
        total += smooth / sqrt(rate_hz) * 0.8 ** ix
    sig = interp(t_of(num) * 1000, arange(span), total)
    sig = sig - sig.mean()
    return sig / (sig.std() + 1e-12)


def _tilted_hiss(num: int, gen: Generator, mid_hz: float, lo_db: float, hi_db: float) -> ndarray:
    bins = np.fft.rfft(gen.standard_normal(num))
    hz = np.fft.rfftfreq(num, 1.0 / SR)
    pos = log2(maximum(hz, 5) / mid_hz)
    gain = -10 * log10(2 ** (-lo_db / 3.0103 * pos) + 2 ** (hi_db / 3.0103 * pos))
    src = np.fft.irfft(bins * _db_gain(gain), num)
    return src / _energy(src)


def air(seconds: float, r: Generator, center: float = 1200.0, lo_db: float = 6.0, hi_db: float = 4.5,
        flutter: float = 0.5, rate: float = 6.0, bright: float = 0.6, whistle: float = 0.0) -> ndarray:
    """Wind-like noise: a tilted hiss bed, a moving low-pass, an optional whistle, gust swells."""
    num = n_of(seconds)
    bed = _tilted_hiss(num, r, center, lo_db, hi_db)
    gust = wander(seconds, r, rate)
    if bright > 0:
        moving = clip(6 * center * 2 ** (1.2 * bright * flutter * gust), 200, 20000)
        bed = svf(bed, moving, 0.6, "low")
    if whistle > 0:
        slow = 0.6 * gust + 0.8 * wander(seconds, r, 0.4 * rate)
        pipe = svf(r.standard_normal(num), center * 2 ** (0.5 * slow), 9, "bandn")
        bed = bed / _energy(bed) + whistle * pipe / _energy(pipe)
    bed = bed * exp(1.1 * flutter * gust)
    return bed / _energy(bed)


# ---------------------------------------------------------------- designed sources

def _transient_tick(num: int, gen: Generator) -> ndarray:
    kn = min(num, n_of(0.012))
    times = t_of(kn)
    tick = butter(gen.standard_normal(kn), "high", 1800.0, 2) * exp(-times / 0.0012)
    tick += 0.6 * sin(2 * pi * 2400 * times) * exp(-times / 0.003)
    return tick / (_absmax(tick) + 1e-12)


def sub_drop(seconds: float = 1.6, r: Generator | None = None, f0: float = 120.0, f1: float = 34.0,
             drop: float = 0.25, decay: float = 0.6, harm: float = 0.3, click: float = 0.25,
             attack: float = 0.004) -> ndarray:
    """Low sine that falls from f0 to f1, with 2 soft overtones and a short tick; peaks at -1 dBFS."""
    gen = rng(0) if r is None else r
    num, times = _grid(seconds)
    track = f1 + (f0 - f1) * _decay(times, drop, 1e-3)
    ang = _TWO_PI * cumsum(track) / SR
    overtones = 0.55 * sin(2 * ang + 0.3) + 0.3 * sin(3 * ang + 0.7)
    body = sin(ang) + harm * overtones
    gate = _decay(times, decay, 1e-3)
    ramp_n = min(num, n_of(attack))
    gate[:ramp_n] *= _half_cos_ramp(ramp_n)
    body = body * gate
    if click > 0:
        tick = _transient_tick(num, gen)
        body[: tick.size] += click * tick
    return normalize(fade(body, 0.0, 0.3 * seconds), -1.0)


def _partial_table(table: Any) -> tuple[Sequence[float], Sequence[float]]:
    return BELLS[table] if isinstance(table, str) else table


def _strike_burst(num: int, gen: Generator, hz0: float) -> ndarray:
    kn = min(num, n_of(0.006))
    burst = butter(gen.standard_normal(kn), "band", (2 * hz0, min(12 * hz0, 18000.0)), 2)
    return burst * exp(-t_of(kn) / 0.0008)


def _beating_pair(gen: Generator, times: ndarray, hz0: float, ratio_amp: tuple, opts: tuple) -> ndarray | None:
    """One beating partial pair; None above 0.45 SR (no random draws then)."""
    res, amp = ratio_amp
    inharm, bri, beat_hz, tau_d, fall = opts
    ratio = res ** (1 + inharm)
    hz = hz0 * ratio
    if hz > 0.45 * SR:
        return None
    h2 = amp * ratio ** (1.2 * (bri - 0.5))
    bq = beat_hz * gen.uniform(0.4, 1.6) * (1.0 if ratio >= 1 else 0.5)
    ph1, ph2 = gen.uniform(0, 6.3), gen.uniform(0, 6.3)
    envl = h2 * _decay(times, tau_d * ratio ** -fall, 1e-4)
    return envl * (sin(2 * pi * hz * times + ph1) + 0.7 * sin(2 * pi * (hz + bq) * times + ph2)) / 1.7


def _mix_strike(sig: ndarray, gen: Generator, hz0: float, hit: float) -> None:
    burst = _strike_burst(len(sig), gen, hz0)
    sig[: len(burst)] += hit * burst * _absmax(sig) / (_absmax(burst) + 1e-12)


def bell(f0: float, seconds: float, r: Generator, partials: Any = "bar", decay: float = 1.2,
         slope: float = 0.8, bright: float = 0.5, beat: float = 1.0, stretch: float = 0.0,
         strike: float = 0.3) -> ndarray:
    """Bell made of detuned partial pairs that beat, with an optional strike noise; -1 dBFS."""
    num, times = _grid(seconds)
    opts = (stretch, bright, beat, decay, slope)
    ratios, levels = _partial_table(partials)
    sig = zeros(num)
    for pair in zip(ratios, levels):
        voice = _beating_pair(r, times, f0, pair, opts)
        if voice is None:
            continue
        sig += voice
    onset_n = min(num, n_of(0.0015))
    sig[:onset_n] *= linspace(0, 1, onset_n)
    if strike > 0:
        _mix_strike(sig, r, f0, strike)
    return normalize(fade(sig, 0.0, 0.15 * seconds), -1.0)


def fm(fc: float, ratio: float, index: float, seconds: float, decay: float = 1.0, index_decay: float = 0.3,
       attack: float = 0.002) -> ndarray:
    """Carrier phase-modulated by one sine; the modulation depth fades faster than the tone."""
    num, times = _grid(seconds)
    carrier = _TWO_PI * fc * times
    mod = sin(_TWO_PI * fc * ratio * times)
    sig = sin(carrier + index * _decay(times, index_decay, 1e-4) * mod) * _decay(times, decay, 1e-4)
    ramp_n = min(num, n_of(attack))
    sig[:ramp_n] *= _half_cos_ramp(ramp_n)
    return fade(sig, 0.0, 0.15 * seconds)


# ---------------------------------------------------------------- layering

def onset(x: ndarray, db: float = -24.0) -> float:
    """Time of the first sample within db of the peak; 0.0 for silence."""
    span = _abs_level(x)
    if len(span) == 0 or span.max() == 0:
        return 0.0
    return int(argmax(span >= span.max() * _db_gain(db))) / SR


def align(x: ndarray, t: float = 0.0, db: float = -24.0) -> ndarray:
    """Pad or crop the front so the detected onset falls at time t."""
    sig = asarray(x, float)
    target = n_of(t) if t > 0 else 0
    shift = target - int(round(onset(sig, db) * SR))
    if shift < 0:
        return sig[-shift:]
    return concatenate([zeros((shift,) + sig.shape[1:]), sig])


def pan_to_stereo(x: ndarray, pan: float = 0.0) -> ndarray:
    """Constant-power pan of mono, or balance of stereo."""
    left_g, right_g = _equal_power(pan)
    sig = asarray(x, float)
    if sig.ndim == 1:
        return column_stack([sig * left_g, sig * right_g])
    return sig * array([left_g, right_g])


def _as_track(entry: Any) -> dict:
    if isinstance(entry, dict):
        return {"x": asarray(entry["x"], float), "t": float(entry.get("t", 0.0)),
                "gain": float(entry.get("gain", 0.0)), "pan": entry.get("pan")}
    src, times, gain = entry[0], entry[1], entry[2]
    return {"x": asarray(src, float), "t": float(times), "gain": float(gain), "pan": entry[3] if len(entry) > 3 else None}


def _trim_lead(trk: dict, lead_db: float | None) -> dict:
    if lead_db is not None:
        trk["t"] -= onset(trk["x"], lead_db)
    if trk["t"] < 0:
        trk["x"] = trk["x"][n_of(-trk["t"]):]
        trk["t"] = 0.0
    return trk


def _any_stereo(trks: list[dict]) -> bool:
    return any(trk["x"].ndim == 2 or trk["pan"] is not None for trk in trks)


def _bus_length(trks: list[dict], total_s: float | None) -> int:
    return n_of(total_s) if total_s is not None else max(int(round(trk["t"] * SR)) + len(trk["x"]) for trk in trks)


def _sum_track(buf: ndarray, trk: dict, two_ch: bool) -> None:
    src = trk["x"]
    if two_ch and (src.ndim == 1 or trk["pan"] is not None):
        src = pan_to_stereo(src, trk["pan"] or 0.0)
    ix = int(round(trk["t"] * SR))
    if ix < len(buf):
        span = min(len(src), len(buf) - ix)
        buf[ix:ix + span] += _db_gain(trk["gain"]) * src[:span]


def stack(layers: Sequence[Any], length: float | None = None, onset_db: float | None = None,
          peak_db: float | None = None) -> ndarray:
    """Mix layers placed in time, with gain in dB and optional pan."""
    trks = [_trim_lead(_as_track(it), onset_db) for it in layers]
    two_ch = _any_stereo(trks)
    num = _bus_length(trks, length)
    buf = zeros((num, 2) if two_ch else num)
    for trk in trks:
        _sum_track(buf, trk, two_ch)
    return normalize(buf, peak_db) if peak_db is not None else buf


def xfade(a: ndarray, b: ndarray, seconds: float = 0.02, power: bool = True) -> ndarray:
    """Join a and b, overlapping the last part of a with the first part of b."""
    head, tail = asarray(a, float), asarray(b, float)
    k = min(n_of(seconds), len(head), len(tail))
    ramp = linspace(0, 1, k)
    if power:
        quarter = ramp * pi / 2
        gone, come = cos(quarter), sin(quarter)
    else:
        gone, come = 1 - ramp, ramp
    if head.ndim == 2:
        gone, come = gone[:, None], come[:, None]
    cut = len(head) - k
    return concatenate([head[:cut], head[cut:] * gone + tail[:k] * come, tail[k:]])
