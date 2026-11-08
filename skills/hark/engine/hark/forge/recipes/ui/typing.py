"""Digital typing ticks: one high-passed key, plus a typed run built from many keys.

The module itself is the single-key recipe. ``RUN`` is a second recipe object for
the typed run. Both share one seed file and differ in their critic target text.
"""

from __future__ import annotations

import math
import types

import numpy as np

from hark.forge.engine import dsp
from hark.forge.recipes.ui import _layers as kit

TARGET = "a short crisp high-passed digital typing tick for a motion graphics title"
RUN_TARGET = "a quick run of crisp high-passed digital typing ticks for an animated title"
TARGETS = {"pro_typing_key": TARGET, "pro_typing_run": RUN_TARGET}
ALTS = ["a mouse click", "a drop of water", "a hi-hat", "a spray can hiss"]
RUN_ALTS = ["rain on a window", "a hi-hat pattern", "a clock ticking", "popcorn popping"]
REFSET = "pro_typing_key"

SPACE = kit.space_table("""
    ridge 6500 13500 log
    ridge_db 0.0 12.0
    ridge_q 0.5 2.5 log
    lowcut 2500 5000 log
    hicut 12000 20000 log
    att 0.0015 0.005 log
    att_curve 0.6 3.0 log
    tau1 0.0012 0.006 log
    tail_db -40.0 -12.0
    tau2 0.008 0.05 log
    touch_db -30.0 -4.0
    touch_dt 0.0015 0.008 log
    rattle_db -40.0 -18.0
    tink_db -40.0 2.0
    rel_db -40.0 -14.0
    rel_dt 0.035 0.075 log
    length 0.05 0.1
    gate 0.004 0.02 log
    key 0 999 int
    grain 0.0 1.0
    grain_rate 300 4000 log
    tink_glide -12.0 12.0
""")

DEFAULTS = kit.value_table("""
    ridge 9600.0  ridge_db 6.0  ridge_q 1.0  lowcut 3400.0  hicut 16000.0  att 0.003
    att_curve 1.2  tau1 0.0025  tail_db -24.0  tau2 0.016  touch_db -12.0  touch_dt 0.003
    rattle_db -30.0  tink_db -32.0  rel_db -40.0  rel_dt 0.05  length 0.075  gate 0.008
    key 7  grain 0.0  grain_rate 1500.0  tink_glide 0.0
""")

RUN_SPACE = {**SPACE, **kit.space_table("""
    rate 8.5 14.0
    jit 0.12 0.35
    lvl_jit 1.0 4.5
    accent 0.0 0.35
    gap 0.0 0.2
    var 0.01 0.08
    dur 1.1 1.9
    swell_db -60.0 -26.0
    swell_len 0.2 0.5
""")}
RUN_DEFAULTS = {**DEFAULTS, **kit.value_table("""
    rate 11.5  jit 0.22  lvl_jit 2.5  accent 0.15  gap 0.08  var 0.04  dur 1.5
    swell_db -42.0  swell_len 0.3
""")}

# A layer whose level sits at or under this many dB is left out.
_SILENT_DB = -39.5
# The typed run moves between this many key layouts.
_LAYOUT_COUNT = 5
_TOP_RIDGE_HZ = 16000.0


def _gain(level_db: float) -> float:
    return 10 ** (level_db / 20)


def key_rng(key: int, part: int = 0) -> np.random.Generator:
    """Generator tied to one key number; the recipe only uses part 0."""
    return np.random.default_rng(int(key) * 7919 + part * 104729 + 31)


# ---------------------------------------------------------------- key parts

def _ridge_filter(sig: np.ndarray, cfg: dict, ridge: float | None = None) -> np.ndarray:
    """Steep high-pass, a peaking boost at the ridge, a soft top roll-off and a hard cut at 18.5 kHz."""
    centre = cfg["ridge"] if ridge is None else ridge
    out = kit.highpass(sig, cfg["lowcut"], 6)
    out = dsp.peq(out, centre, cfg["ridge_db"], cfg["ridge_q"])
    out = kit.lowpass(out, cfg["hicut"])
    return np.asarray(kit.lowpass(out, 18500, 8), dtype=float)


def burst_env(n: int, att: float, curve: float, tau1: float, tail: float, tau2: float) -> np.ndarray:
    """Envelope that rises along a power curve, drops fast, then fades in a quiet long tail."""
    size = max(0, n)
    rise = max(2, kit.n_of(att))
    head = min(rise, size)
    shape = np.empty(size)
    shape[:head] = (np.arange(head) / rise) ** curve
    since = (np.arange(head, size) - (head - 1)) / kit.SR
    shape[head:] = (1 - tail) * kit.decay(since, tau1) + tail * kit.decay(since, tau2)
    return shape


def _sparse_grain(size: int, gen: np.random.Generator, rate: float) -> np.ndarray:
    """One random signed impulse per block of 1/rate s, scaled to unit RMS."""
    step = max(1, int(kit.SR / rate))
    block_starts = np.arange(0, size, step)
    count = len(block_starts)
    where = block_starts + gen.integers(0, step, count)
    signs = np.where(gen.uniform(size=count) < 0.5, -1.0, 1.0)
    sizes = gen.uniform(0.3, 1.0, count)
    out = np.zeros(size)
    inside = where < size
    out[where[inside]] = (signs * sizes)[inside]
    return out / (kit.rms(out) + 1e-12)


def _add_rattle(shape: np.ndarray, cfg: dict, gen: np.random.Generator, lay: np.random.Generator,
                hit_at: int) -> None:
    """Add 1 to 3 small late rebounds to the envelope."""
    size = len(shape)
    for _ in range(int(lay.integers(1, 4))):
        when = hit_at / kit.SR + lay.uniform(0.012, 0.045) * (1 + 0.1 * gen.standard_normal())
        level = cfg["rattle_db"] + lay.uniform(-4.0, 2.0)
        fall = lay.uniform(0.001, 0.003)
        start = kit.n_of(when)
        if start >= size - 8:
            continue
        since = kit.t_of(size - start)
        shape[start:] += _gain(level) * kit.decay(since, fall) * np.minimum(1.0, since / 0.0004)


def _partial_phase(tt: np.ndarray, freq: float, glide: float, gen: np.random.Generator) -> np.ndarray:
    """Phase of one tink partial. With a glide it bends by that many semitones over about 5 ms."""
    base = freq * (1 + 0.003 * gen.standard_normal())
    if glide == 0:
        return kit.TWO_PI * base * tt
    track = np.minimum(base * 2 ** ((glide / 12) * (1 - kit.decay(tt, 0.005))), 17000.0)
    return kit.TWO_PI * np.cumsum(track) / kit.SR


def _add_tink(out: np.ndarray, cfg: dict, gen: np.random.Generator, lay: np.random.Generator, hit_at: int,
              ridge_k: float) -> None:
    """Add three small metallic partials after the hit, levelled against the opening 10 ms."""
    freqs = np.sort(lay.uniform(5000, 12500, 3)) * ridge_k
    falls = lay.uniform(0.002, 0.006, 3)
    span = len(out) - hit_at
    if span <= 0:
        return
    tt = kit.t_of(span)
    ring = np.zeros(span)
    for freq, level, fall in zip(freqs, (1.0, 0.6, 0.4), falls):
        phase = _partial_phase(tt, freq, cfg["tink_glide"], gen)
        ring += level * kit.decay(tt, fall) * np.sin(phase + gen.uniform(0, 6.28))
    fade_in = min(24, span)
    ring[:fade_in] *= np.arange(fade_in) / 24
    win = kit.n_of(0.01)
    out[hit_at:] += _gain(cfg["tink_db"]) * ring * kit.rms(out[:win]) / (kit.rms(ring[:win]) + 1e-12)


def _add_lift(out: np.ndarray, cfg: dict, gen: np.random.Generator, t_lift: float, ridge: float,
              tail: float) -> None:
    """Add the softer, brighter tick of the key coming back up."""
    start = kit.n_of(t_lift)
    span = len(out) - start
    if span <= 0:
        return
    shape = burst_env(span, 0.0008, 1.0, 0.6 * cfg["tau1"], 0.5 * tail, 0.4 * cfg["tau2"])
    noise = _ridge_filter(gen.standard_normal(span) * shape, cfg, ridge=min(1.15 * ridge, _TOP_RIDGE_HZ))
    out[start:] += _gain(cfg["rel_db"]) * kit.unit(noise) * kit.peak(out)


def _cut(sig: np.ndarray, gate: float, end: int) -> np.ndarray:
    """Cut at ``end`` with a raised-cosine fade over the last ``gate`` seconds."""
    fade = max(1, min(len(sig) - 2, kit.n_of(gate)))
    end = min(len(sig), end)
    out = np.array(sig[:end], dtype=float)
    first = max(0, end - fade)
    width = end - first
    out[first:] *= 0.5 + 0.5 * np.cos(np.pi * np.arange(1, width + 1) / width)
    return out


def _timing(cfg: dict, gen: np.random.Generator) -> tuple[float | None, int, int]:
    """Return the lift time (None when the lift is off), the buffer size and the cut sample."""
    length = cfg["length"] * (1 + 0.06 * gen.standard_normal())
    has_lift = cfg["rel_db"] > _SILENT_DB
    t_lift = cfg["touch_dt"] + cfg["rel_dt"] * (1 + 0.08 * gen.standard_normal())
    if has_lift:
        size = kit.n_of(max(length, t_lift + 0.025))
        return t_lift, size, size
    size = kit.n_of(max(length, 0.0))
    return None, size, kit.n_of(length)


def _source(cfg: dict, gen: np.random.Generator, size: int) -> np.ndarray:
    """White noise, blended with sparse grain when ``grain`` is above 0."""
    noise = gen.standard_normal(size)
    mix = cfg["grain"]
    if mix > 0:
        noise = (1 - mix) * noise + mix * _sparse_grain(size, gen, cfg["grain_rate"])
    return noise


def _contact_shape(cfg: dict, gen: np.random.Generator, size: int) -> tuple[np.ndarray, int, float]:
    """Return the envelope of finger touch and key hit, the hit sample and the tail level."""
    hit_at = min(size, kit.n_of(cfg["touch_dt"] * (1 + 0.1 * gen.standard_normal())))
    tail = _gain(cfg["tail_db"] + 1.5 * gen.standard_normal())
    att, curve, tau1, tau2 = cfg["att"], cfg["att_curve"], cfg["tau1"], cfg["tau2"]
    hit = np.zeros(size)
    hit[hit_at:] = burst_env(size - hit_at, 0.6 * att, curve, tau1, tail, tau2)
    touch = _gain(cfg["touch_db"]) * burst_env(size, att, curve, 0.7 * tau1, 0.3 * tail, 0.5 * tau2)
    return np.maximum(hit, touch), hit_at, tail


def _tick(cfg: dict, gen: np.random.Generator, force: float = 1.0, ridge_k: float = 1.0) -> np.ndarray:
    """A mono key tick: touch and hit, then the optional rattle, tink and lift, then the cut."""
    lay = key_rng(cfg["key"])
    t_lift, size, end = _timing(cfg, gen)
    ridge = cfg["ridge"] * ridge_k * (1 + 0.015 * gen.standard_normal())
    noise = _source(cfg, gen, size)
    shape, hit_at, tail = _contact_shape(cfg, gen, size)
    if cfg["rattle_db"] > _SILENT_DB:
        _add_rattle(shape, cfg, gen, lay, hit_at)
    out = _ridge_filter(noise * shape, cfg, ridge=min(ridge, _TOP_RIDGE_HZ))
    if cfg["tink_db"] > _SILENT_DB:
        _add_tink(out, cfg, gen, lay, hit_at, ridge_k)
    if t_lift is not None:
        _add_lift(out, cfg, gen, t_lift, ridge, tail)
    return force * kit.unit(_cut(out, cfg["gate"], end))


def _both_channels(mono: np.ndarray) -> np.ndarray:
    return np.column_stack([mono, mono]).astype(np.float64)


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one key: 2 equal channels, float64 at 48 kHz, 50 to 100 ms, peak near the force."""
    cfg = kit.merged(DEFAULTS, p)
    force = math.exp(0.08 * rng.standard_normal())
    return _both_channels(dsp.fade(_tick(cfg, rng, force), 0.0, 0.0015))


# ---------------------------------------------------------------- typed run

def key_times(p: dict, r: np.random.Generator) -> list[float]:
    """Key start times with jitter and word gaps, from 0 up to dur - 0.07 s."""
    typical = 1 / p["rate"]
    lo, hi = 0.62 * typical, 1.75 * typical
    stop = p["dur"] - 0.07
    now, starts = 0.0, []
    while now < stop:
        starts.append(now)
        step = typical * math.exp(p["jit"] * r.standard_normal())
        if r.uniform() < p["gap"]:
            step *= r.uniform(1.4, 1.7)
        now += min(max(step, lo), hi)
    return starts


def _pick_layout(r: np.random.Generator, prev: int) -> int:
    """Pick a layout index at random, never the same one twice in a row."""
    pick = int(r.integers(0, _LAYOUT_COUNT))
    if pick != prev:
        return pick
    return (pick + 1 + int(r.integers(0, _LAYOUT_COUNT - 1))) % _LAYOUT_COUNT


def _lay_down_keys(out: np.ndarray, cfg: dict, gen: np.random.Generator, starts: list[float]) -> None:
    last, drift = -1, 0.0
    spread = cfg["var"]
    for when in starts:
        layout = _pick_layout(gen, last)
        last = layout
        drift = 0.6 * drift + cfg["lvl_jit"] * gen.standard_normal()
        boost = gen.uniform(3, 5) if gen.uniform() < cfg["accent"] else 0.0
        level = drift + boost
        ridge_k = math.exp(spread * (layout - 2) / 2 + 0.3 * spread * gen.standard_normal())
        tick = _tick(dict(cfg, key=int(cfg["key"]) + 101 * layout), gen, 1.0, ridge_k)
        kit.add_at(out, tick, kit.n_of(when) if when > 0 else 0, _gain(min(level, 6.0)))


def _add_swell(out: np.ndarray, cfg: dict, gen: np.random.Generator, last: float) -> None:
    """Add a dark noise swell that grows under the closing keys."""
    span = min(len(out), kit.n_of(min(cfg["swell_len"], last)))
    rumble = kit.bandpass(dsp.noise(span / kit.SR, gen, "brown")[:span], 90, 900)
    rumble = kit.unit(rumble * np.linspace(0, 1, span) ** 2.5)
    out[len(out) - span:] += _gain(cfg["swell_db"]) * kit.peak(out) * rumble


def render_run(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render a typed run: 2 equal channels, float64 at 48 kHz, close to ``dur`` seconds."""
    cfg = kit.merged(RUN_DEFAULTS, p)
    starts = key_times(cfg, rng)
    size = kit.n_of(starts[-1] + cfg["length"] + 0.06)
    out = np.zeros(size)
    _lay_down_keys(out, cfg, rng, starts)
    if cfg["swell_db"] > -59:
        _add_swell(out, cfg, rng, starts[-1])
    hiss = kit.bandpass(rng.standard_normal(size), cfg["lowcut"], 16000)
    out = out + hiss / (np.std(hiss) + 1e-12) * kit.peak(out) * 10 ** (-68 / 20)
    return _both_channels(dsp.fade(out, 0.002, 0.004))


# ------------------------------------------------------------ critic checks

def _to_mono(x: np.ndarray) -> np.ndarray:
    arr = np.asarray(x, dtype=float)
    return arr.mean(axis=1) if arr.ndim == 2 else arr


def env_db(x: np.ndarray) -> np.ndarray:
    """Level of each 1 ms frame in dB, with the loudest frame at 0."""
    mono = _to_mono(x)
    mono = kit.fit(mono, max(48, len(mono)))
    frames = len(mono) // 48
    power = np.square(mono[: frames * 48]).reshape(frames, 48).mean(axis=1)
    level = 10 * np.log10(power + 1e-14)
    return level - level.max()


def _ms_to(mono: np.ndarray, floor_db: float) -> float | None:
    """Milliseconds from the peak of a 10 ms running mean until the level reaches ``floor_db``."""
    smooth = np.convolve(np.square(mono), np.ones(480) / 480, mode="same")[::48]
    level = 10 * np.log10(smooth + 1e-14)
    level = level - level.max()
    top = int(np.argmax(level))
    hits = np.nonzero(level[top:] <= floor_db)[0]
    return float(hits[0]) if len(hits) else None


def _low_share_db(mono: np.ndarray) -> float:
    """Share of the power under 2 kHz, in dB with 1 decimal."""
    spec = np.abs(np.fft.rfft(mono * np.hanning(len(mono)))) ** 2
    hz = np.fft.rfftfreq(len(mono), 1 / kit.SR)
    share = spec[hz < 2000].sum() / (spec.sum() + 1e-30)
    return round(10 * math.log10(share + 1e-14), 1)


def key_stats(x: np.ndarray) -> dict:
    """Measure one key: attack_ms, t20_ms, t40_ms, lo2k_db and length_s."""
    mono = _to_mono(x)
    level = env_db(mono)
    top = int(np.argmax(level))
    first = int(np.nonzero(level[: top + 1] >= -20)[0][0])
    return {"attack_ms": float(top - first), "t20_ms": _ms_to(mono, -20), "t40_ms": _ms_to(mono, -40),
            "lo2k_db": _low_share_db(mono), "length_s": round(len(mono) / kit.SR, 3)}


def outside(v: float | None, lo: float, hi: float, scale: float) -> float:
    """How far v lies outside [lo, hi], in units of scale. A missing v counts as 1."""
    if v is None:
        return 1.0
    return max(0.0, lo - v, v - hi) / scale


def key_penalty(x: np.ndarray) -> float:
    """Score from 0 down to -3 for attack, decay, missing lows and length of one key."""
    st = key_stats(x)
    misses = [outside(st["attack_ms"], 1.5, 5.5, 3), outside(st["t20_ms"], 10, 19, 6),
              outside(st["t40_ms"], 42, 72, 25), max(0.0, st["lo2k_db"] + 30) / 10,
              outside(st["length_s"], 0.05, 0.1, 0.03)]
    return -min(3.0, 0.75 * sum(min(1.0, m) for m in misses))


def run_stats(x: np.ndarray) -> dict:
    """Measure a typed run: keys, med_ms, min_ms, max_ms and lo2k_db."""
    mono = _to_mono(x)
    level = env_db(mono)
    rising = np.nonzero((level[1:] > -32) & (level[:-1] <= -32))[0] + 1
    onsets: list[int] = []
    for at in rising:
        if onsets and at - onsets[-1] < 45:
            continue
        onsets.append(int(at))
    gaps = np.diff(onsets) if len(onsets) >= 2 else np.array([0])
    return {"keys": len(onsets), "med_ms": float(np.median(gaps)), "min_ms": float(gaps.min()),
            "max_ms": float(gaps.max()), "lo2k_db": _low_share_db(mono)}


def run_penalty(x: np.ndarray) -> float:
    """Score from 0 down to -3 for key spacing, missing lows and fewer than 10 keys."""
    st = run_stats(x)
    score = min(1.0, outside(st["med_ms"], 72, 100, 20)) + min(1.0, max(0.0, st["lo2k_db"] + 28) / 10)
    if st["keys"] < 10:
        score += 0.5
    return -min(3.0, 0.75 * score)


_PENALTIES = {"pro_typing_key": key_penalty, "pro_typing_run": run_penalty}


def extra_for(cat: str, weight: float = 1.0):
    """Critic extra for ``pro_typing_key`` or ``pro_typing_run``; None for other categories."""
    judge = _PENALTIES.get(cat)
    if judge is None:
        return None
    return lambda x, row=None: weight * judge(x)


RUN = types.SimpleNamespace(
    MODULE=__name__, TARGET=RUN_TARGET, TARGETS={"pro_typing_run": RUN_TARGET}, ALTS=RUN_ALTS,
    REFSET="pro_typing_run", SPACE=RUN_SPACE, DEFAULTS=RUN_DEFAULTS, render=render_run, extra_for=extra_for,
)
