"""Mechanical keyboard key: one press and release, modelled part by part.

Every key number owns a fixed layout (its mode clouds, the two microphone
positions and their rooms). A take only changes the finger force, the timing
and the noise, so two takes of one key sound like the same physical key.
"""

from __future__ import annotations

import math

import numpy as np
from scipy import signal

from hark.forge.engine import dsp
from hark.forge.recipes.ui import _layers as kit

TARGET = "a single mechanical keyboard key press with a crisp click and a short thock"
TARGETS = {"typing_key": TARGET}
ALTS = ["a wooden knock", "a computer mouse click", "a typewriter key", "a cork pop"]
REFSET = "typing_key"
_REF_NAMES = "kb_thock_01 kb_thock_02 kb_thock_03 kb_thock_04 kb_enter_01 kb_space_01".split()
REFS = [kit.ref_root() + "/keyboard/" + stem + ".wav" for stem in _REF_NAMES]

SPACE = kit.space_table("""
    click_tc 3e-5 4e-4 log
    click_tau 1.5e-4 1e-3 log
    click_lvl 0.1 2.0 log
    click_lo 6500 9500 log
    click_tilt -3.0 3.0
    crack 0.2 2.0 log
    snap 0.0 1.0
    cap_f 900 1800 log
    cap_tau 0.0015 0.0045 log
    cap_noise 0.0 1.0
    cap_n 6 30 int
    cap_cl 0.1 1.0 log
    cap_fast 0.0005 0.003 log
    cap_res 0.05 1.0 log
    mid_f 110 260 log
    mid_tau 0.003 0.016 log
    mid_lvl 0.01 0.2 log
    thock_f 55 90 log
    thock_tau 0.045 0.1 log
    thock_lvl 0.05 0.8 log
    thock_rise 0.001 0.025 log
    case_f 70 220 log
    case_lvl 0.0 0.8
    case_dec 0.3 1.0
    up_delay 0.07 0.125
    up_lvl 0.08 0.35 log
    up_pitch 0.9 1.5
    tick_lvl 0.0 1.0
    tick_dt 0.004 0.025 log
    pre_len 0.004 0.025 log
    pre_lvl 0.001 0.06 log
    ping_f 7500 11000 log
    ping_tau 0.004 0.03 log
    ping_lvl 0.04 0.3
    room_wet 0.0 0.4
    rt60 0.05 0.4 log
    room_size 0.0 1.0
    room_damp 0.2 0.95
    room_model | sparse diffuse
    floor_db -78.0 -55.0
    rumble 0.0 0.7
    clip 0.12 0.4
    hpf 36 46 log
    key 0 999 int
    top_lvl 0.0 2.5
    top_lo 1800 4500 log
    top_tau 0.0006 0.004 log
    jacket_lvl 0.0 0.9
    jacket_dt 0.003 0.012 log
    pre_gap 0.0005 0.006 log
    width 0.0 0.6
""")

# The first tuned set. A parameter set without ``top_lvl`` renders this older sound.
V1_DEFAULTS = kit.value_table("""
    click_tc 7.62977e-05  click_tau 0.000400034  click_lvl 0.195527  click_lo 7323.18
    click_tilt -1.25571  crack 0.395643  snap 0.0  cap_f 1489.18  cap_tau 0.00399449
    cap_noise 1.0  cap_n 7  cap_cl 0.357156  cap_fast 0.000995191  cap_res 0.749554
    mid_f 205.793  mid_tau 0.00918634  mid_lvl 0.0902506  thock_f 86.8567  thock_tau 0.0549253
    thock_lvl 0.226067  thock_rise 0.00834742  case_f 70.0  case_lvl 0.581134  case_dec 0.736866
    up_delay 0.124575  up_lvl 0.0969741  up_pitch 1.04402  tick_lvl 0.581597  tick_dt 0.0111911
    pre_len 0.0128195  pre_lvl 0.0207414  ping_f 9083.43  ping_tau 0.00578368  ping_lvl 0.122274
    room_wet 0.261726  rt60 0.113685  room_size 0.212985  room_damp 0.95  room_model sparse
    floor_db -55.0  rumble 0.0177183  clip 0.38353  hpf 44.5337  key 999
""")

DEFAULTS = {**V1_DEFAULTS, **kit.value_table("""
    click_lvl 0.6  thock_tau 0.05  clip 0.24  top_lvl 0.8  top_lo 2500.0  top_tau 0.0015
    jacket_lvl 0.0  jacket_dt 0.008  pre_gap 0.002  pre_len 0.006  width 0.2
""")}

_HIT_WINDOW_S = 0.12


def key_rng(key: int, part: int) -> np.random.Generator:
    """Generator that fixes one layout part of one key. It is the same in every take."""
    return np.random.default_rng(int(key) * 7919 + part * 104729 + 17)


# --------------------------------------------------------------- signal parts

def ring(freqs, taus, amps, n: int, r: np.random.Generator) -> np.ndarray:
    """Add up decaying sines. Each one is only computed for 9 of its time constants."""
    acc = np.zeros(n)
    nyq_edge = 0.47 * kit.SR
    for freq, tau, gain in zip(freqs, taus, amps):
        if gain == 0 or freq >= nyq_edge:
            continue
        span = min(n, int(9 * tau * kit.SR) + 2)
        tt = kit.t_of(span)
        acc[:span] += gain * kit.decay(tt, tau) * kit.osc(freq, tt, r.uniform(0, kit.TWO_PI))
    return acc


def cloud(f_lo: float, f_hi: float, count: int, tau_lo: float, tau_hi: float, n: int, kr: np.random.Generator,
          r: np.random.Generator, tilt: float = -3.0, detune: float = 0.002) -> np.ndarray:
    """Many modes between f_lo and f_hi. ``kr`` places them; ``r`` detunes them and sets phases."""
    count = int(count)
    span = math.log(f_hi / f_lo)
    centres = np.sort(np.exp(kr.uniform(math.log(f_lo), math.log(f_hi), count)))
    gains = 10 ** (tilt * np.log2(centres / f_lo) / 20) * kr.uniform(0.3, 1.0, count)
    centres = centres * (1 + detune * r.standard_normal(count))
    pos = np.clip(np.log(centres / f_lo) / span, 0, 1)
    decays = np.exp(math.log(tau_lo) + pos * (math.log(tau_hi) - math.log(tau_lo)))
    return ring(centres, decays, gains, n, r)


def hiss(lo: float, hi: float, tau: float, n: int, r: np.random.Generator, order: int = 2) -> np.ndarray:
    """Band-limited noise with an exponential decay, padded with zeros to n samples."""
    span = min(n, int(9 * tau * kit.SR) + 2)
    burst = kit.bandpass(r.standard_normal(span), lo, min(hi, 21600.0), order)
    burst = burst * kit.decay(kit.t_of(span), tau)
    return kit.fit(np.asarray(burst, dtype=float), n)


def pulse(tc: float) -> np.ndarray:
    """Half a sine period, scaled so that its samples add up to 1."""
    width = max(1, int(round(tc * kit.SR)))
    arch = np.sin(np.pi * (np.arange(width) + 0.5) / width)
    return arch / arch.sum()


def _strike(response: np.ndarray, touch: float) -> np.ndarray:
    """Excite a response with a contact pulse and keep its original length."""
    return signal.fftconvolve(response, pulse(touch))[: len(response)]


def energy(x: np.ndarray, ms: float = 5.0) -> np.ndarray:
    """Scale x so that its opening ``ms`` milliseconds have unit RMS."""
    head = max(8, kit.n_of(ms / 1000))
    return x / (kit.rms(x[:head]) + 1e-12)


def _crack(gen: np.random.Generator, size: int, fall: float, touch: float) -> np.ndarray:
    span = min(size, max(kit.n_of(9 * fall), 8))
    burst = kit.highpass(gen.standard_normal(span) * kit.decay(kit.t_of(span), fall), 1200, 1)
    return _strike(kit.fit(np.asarray(burst, dtype=float), size), touch)


def _bright_part(cfg: dict, gen: np.random.Generator, size: int, lay: np.random.Generator, touch: float) -> np.ndarray:
    fall = cfg["click_tau"]
    modes = kit.unit(cloud(cfg["click_lo"], 18000, 40, 1.6 * fall, 0.6 * fall, size, lay, gen, tilt=cfg["click_tilt"]))
    bright = _strike(modes, touch)
    split = kit.unit(_crack(gen, size, 0.4 * fall, 0.5 * touch))
    return kit.unit(bright) + cfg["crack"] * split


def _keycap(cfg: dict, gen: np.random.Generator, size: int, lay: np.random.Generator, pitch: float,
            touch: float) -> np.ndarray:
    base = cfg["cap_f"] * pitch
    fall = cfg["cap_tau"]
    tonal = ring([base], [1.2 * fall], [1.0], size, gen)
    tonal = tonal + cfg["cap_cl"] * kit.unit(cloud(1.08 * base, 2.3 * base, cfg["cap_n"], 0.9 * fall, 0.45 * fall,
                                                 size, lay, gen))
    noisy = hiss(0.75 * base, 2.4 * base, 0.8 * fall, size, gen)
    mix = (1 - cfg["cap_noise"]) * energy(tonal) + cfg["cap_noise"] * energy(noisy)
    hold = cfg["cap_res"]
    mix = mix * (hold + (1 - hold) * kit.decay(kit.t_of(size), cfg["cap_fast"]))
    return _strike(mix, max(touch, 0.00012))


def _housing(cfg: dict, gen: np.random.Generator, size: int, lay: np.random.Generator, pitch: float) -> np.ndarray:
    root = cfg["mid_f"] * pitch
    steps = (1.0, lay.uniform(1.35, 1.6), lay.uniform(1.9, 2.4))
    fall = cfg["mid_tau"]
    return ring([root * s for s in steps], [fall, 0.7 * fall, 0.5 * fall], [1.0, 0.5, 0.3], size, gen)


def _dent(lay: np.random.Generator, size: int) -> np.ndarray:
    """A short negative bump; its width (0.25 to 0.6 ms) belongs to the key."""
    width = max(3, int(lay.uniform(0.25, 0.6) * 1e-3 * kit.SR))
    keep = min(width, size)
    out = np.zeros(size)
    out[:keep] = -np.hanning(width)[:keep]
    return out


def _plastic_hit(cfg: dict, gen: np.random.Generator, size: int, part: int, pitch: float = 1.0,
                 bright: float = 1.0) -> np.ndarray:
    """One plastic collision. Part 0 is the bottom-out, part 1 the return to the top."""
    lay = key_rng(cfg["key"], part)
    touch = cfg["click_tc"] * math.exp(0.15 * gen.standard_normal())
    hi_band = _bright_part(cfg, gen, size, lay, touch)
    cap = _keycap(cfg, gen, size, lay, pitch, touch)
    body = _housing(cfg, gen, size, lay, pitch)
    dent = _dent(lay, size)
    mix = bright * cfg["click_lvl"] * energy(hi_band) + energy(cap) + cfg["mid_lvl"] * energy(body)
    if cfg.get("top_lvl", 0) > 0:
        upper = _strike(hiss(cfg["top_lo"], 12000, cfg["top_tau"], size, gen, 2), touch)
        mix = mix + bright * cfg["top_lvl"] * energy(upper)
    return kit.unit(kit.unit(mix) + cfg["snap"] * dent)


def _spring(cfg: dict, gen: np.random.Generator, size: int, scale: float = 1.0) -> np.ndarray:
    base = cfg["ping_f"] * scale
    fall = cfg["ping_tau"]
    return kit.unit(ring([base, 1.013 * base, 1.62 * base], [fall, 0.8 * fall, 0.4 * fall], [1.0, 0.5, 0.1], size, gen))


def _thock(cfg: dict, gen: np.random.Generator, size: int) -> np.ndarray:
    """The low board and desk resonance, pushed by a smooth finger force."""
    size = max(1, size)
    freq = cfg["thock_f"] * (1 + 0.01 * gen.standard_normal())
    tt = kit.t_of(size)
    fall = cfg["thock_tau"]
    resp = kit.decay(tt, fall) * kit.osc(freq, tt)
    resp = resp + cfg["case_lvl"] * kit.decay(tt, fall * cfg["case_dec"]) * kit.osc(cfg["case_f"], tt, 0.5)
    rise = max(2, kit.n_of(cfg["thock_rise"]))
    push = 0.5 - 0.5 * np.cos(kit.TWO_PI * np.arange(rise) / rise)
    out = signal.fftconvolve(resp, push / push.sum())[:size]
    return kit.unit(kit.highpass(out, 0.75 * min(freq, cfg["case_f"])))


def _travel(cfg: dict, gen: np.random.Generator, size: int) -> np.ndarray:
    """Rubbing noise of the stem that swells while the key goes down."""
    span = kit.n_of(cfg["pre_len"])
    rub = kit.bandpass(gen.standard_normal(span), 700, 9000)
    spots = gen.uniform(size=span) < 0.004
    grains = kit.highpass(np.where(spots, 4 * gen.standard_normal(span), 0.0), 2000, 1)
    swell = np.linspace(0, 1, span) ** 2
    return kit.fit(kit.unit((rub + grains) * swell), size)


# --------------------------------------------------------------- rooms, mics

def _diffuse_ir(lay: np.random.Generator, cfg: dict) -> np.ndarray:
    """Three noise bands that decay at their own rates, under a soft onset."""
    rt60 = cfg["rt60"]
    span = kit.n_of(min(0.6, 1.1 * rt60 + 0.03))
    seed_noise = lay.standard_normal(span)
    tt = kit.t_of(span)
    split = [(kit.lowpass(seed_noise, 700), 1.0),
             (kit.bandpass(seed_noise, 700, 4000), 0.75),
             (kit.highpass(seed_noise, 4000), 0.25)]
    out = np.zeros(span)
    for band, keep in split:
        out += band * np.exp(-6.91 * tt / max(0.02, rt60 * (1 - cfg["room_damp"] * (1 - keep))))
    onset = 0.0015 + 0.008 * cfg["room_size"]
    return out * np.where(tt > 0.0008, 1 - kit.decay(tt, onset), 0.0)


def _diffuse_room(sig: np.ndarray, lay: np.random.Generator, cfg: dict) -> np.ndarray:
    ir = _diffuse_ir(lay, cfg)
    lag = int(lay.uniform(0.8e-3, 2.5e-3) * kit.SR)
    if lag < len(ir):
        ir[lag] += 0.35 * math.sqrt(float(np.sum(ir ** 2)))
    ir = 0.6 * kit.lowpass(ir, 9000, 1) + 0.4 * ir
    norm = math.sqrt(float(np.sum(ir ** 2))) + 1e-12
    return sig + cfg["room_wet"] * (signal.fftconvolve(sig, ir)[: len(sig)] / norm)


def _room_for(y: np.ndarray, lay: np.random.Generator, cfg: dict) -> np.ndarray:
    model = cfg["room_model"]
    if model == "diffuse":
        return _diffuse_room(y, lay, cfg)
    if model != "sparse":
        raise ValueError(f"unknown room_model {model!r}")
    wet = dsp.room(y, lay, rt60=cfg["rt60"], size=cfg["room_size"], damp=cfg["room_damp"], wet=cfg["room_wet"])
    return np.asarray(wet, dtype=float)[: len(y)]


def _mic_ir(lay: np.random.Generator) -> np.ndarray:
    span = int(0.007 * kit.SR)
    direct = np.zeros(span)
    direct[int(lay.uniform(0, 0.25e-3) * kit.SR)] = 1.0
    bounces = np.zeros(span)
    for _ in range(6):
        at = min(int(lay.uniform(0.3e-3, 6.5e-3) * kit.SR), span - 1)
        bounces[at] += lay.uniform(0.03, 0.12) * lay.choice([-1.0, 1.0])
    return direct + kit.lowpass(bounces, 12000, 1)


def _two_mics(sig: np.ndarray, cfg: dict) -> np.ndarray:
    """Two spaced microphones; each one hears the key through its own small room."""
    layout = key_rng(cfg["key"], 9)
    sides = []
    for side in (0, 1):
        heard = signal.fftconvolve(sig, _mic_ir(layout))[: len(sig)]
        sides.append(_room_for(heard, key_rng(cfg["key"], 10 + side), cfg))
    return np.column_stack(sides)


def _floor(size: int, gen: np.random.Generator, rumble: float, low_edge: float = 25.0) -> np.ndarray:
    """Background hiss mixed with a low rumble, at unit standard deviation."""
    secs = size / kit.SR
    air = kit.fit(dsp.noise(secs, gen, "pink") + dsp.noise(secs, gen, "white"), size)
    air = kit.lowpass(air, gen.uniform(17000, 20000), 4)
    low = kit.bandpass(kit.fit(dsp.noise(secs, gen, "brown"), size), low_edge, 120)
    mix = (1 - rumble) * air / (np.std(air) + 1e-12) + 2 * rumble * low / (np.std(low) + 1e-12)
    return mix / (np.std(mix) + 1e-12)


# --------------------------------------------------------------------- render

def _lead_in(cfg: dict, gen: np.random.Generator, newer: bool) -> float:
    """Seconds of key travel before the bottom-out."""
    gap = cfg["pre_gap"] * math.exp(0.2 * gen.standard_normal()) if newer else gen.uniform(0.003, 0.02)
    lead = cfg["pre_len"] + gap
    if newer and cfg["jacket_lvl"] > 0.005:
        lead += cfg["jacket_dt"]
    return lead


def _down(buf: np.ndarray, cfg: dict, gen: np.random.Generator, lead: float, hit_n: int, newer: bool) -> None:
    total = len(buf)
    force = math.exp(0.1 * gen.standard_normal())
    kit.add_at(buf, _travel(cfg, gen, total), kit.n_of(max(0.0, lead - cfg["pre_len"])), cfg["pre_lvl"])
    if newer and cfg["jacket_lvl"] > 0.005:
        rattle = cloud(2500, 15000, 24, 0.0012, 0.0004, hit_n, key_rng(cfg["key"], 6), gen, tilt=0.0)
        rattle = _strike(kit.unit(rattle), 4e-5)
        when = max(0.0, lead - cfg["jacket_dt"] * (1 + 0.06 * gen.standard_normal()))
        kit.add_at(buf, rattle, kit.n_of(when), cfg["jacket_lvl"])
    at = kit.n_of(lead)
    kit.add_at(buf, _plastic_hit(cfg, gen, hit_n, 0, bright=force), at)
    kit.add_at(buf, _spring(cfg, gen, hit_n), at, cfg["ping_lvl"])
    kit.add_at(buf, _thock(cfg, gen, total - at), at, cfg["thock_lvl"] * force)


def _pre_release_ticks(buf: np.ndarray, cfg: dict, gen: np.random.Generator, t_up: float, hit_n: int) -> None:
    """Two small ticks just before the key returns; the second follows by 3 to 9 ms."""
    first = t_up - cfg["tick_dt"] * (1 + 0.08 * gen.standard_normal())
    layout = key_rng(cfg["key"], 5)
    for delay, gain in ((0.0, 1.0), (gen.uniform(0.003, 0.009), gen.uniform(0.4, 1.0))):
        tick = _strike(kit.unit(cloud(3000, 14000, 14, 0.0008, 0.0003, hit_n, layout, gen)), 5e-5)
        when = max(0.0, min(first + delay, t_up - 0.001))
        kit.add_at(buf, tick, kit.n_of(when), gain * cfg["tick_lvl"] * cfg["up_lvl"])


def _up(buf: np.ndarray, cfg: dict, gen: np.random.Generator, lead: float, hit_n: int) -> None:
    total = len(buf)
    up_gain = cfg["up_lvl"]
    t_up = lead + cfg["up_delay"] * (1 + 0.04 * gen.standard_normal())
    if cfg["tick_lvl"] > 0.005:
        _pre_release_ticks(buf, cfg, gen, t_up, hit_n)
    at = kit.n_of(t_up)
    kit.add_at(buf, _plastic_hit(cfg, gen, hit_n, 1, pitch=cfg["up_pitch"], bright=1.2), at, up_gain)
    kit.add_at(buf, _spring(cfg, gen, hit_n, scale=1 + 0.01 * gen.standard_normal()), at,
               0.7 * cfg["ping_lvl"] * up_gain)
    kit.add_at(buf, _thock(cfg, gen, max(1, total - at)), at, 0.3 * cfg["thock_lvl"] * up_gain)


def _capture(st: np.ndarray, cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Stereo width, the noise floor of the session and the final high-pass."""
    if "width" in cfg:
        centre = st.mean(axis=1)
        diff = (st[:, 0] - st[:, 1]) / 2
        st = np.column_stack([centre + cfg["width"] * diff, centre - cfg["width"] * diff])
    st = kit.highpass(st, 22)
    level = kit.peak(st)
    has_hpf = "hpf" in cfg
    edge = cfg["hpf"] if has_hpf else 25.0
    for ch in (0, 1):
        st[:, ch] += _floor(len(st), gen, cfg["rumble"], low_edge=edge) * level * 10 ** (cfg["floor_db"] / 20)
    return kit.highpass(st, cfg["hpf"], 8) if has_hpf else st


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one press. Stereo float64 at 48 kHz, 0.13 to 0.45 s long; the level is left as is."""
    p = {**V1_DEFAULTS, **(p or {})}
    newer = "top_lvl" in p
    lead = _lead_in(p, rng, newer)
    length = lead + p["clip"] * (1 + 0.05 * rng.standard_normal())
    hit_n = kit.n_of(_HIT_WINDOW_S)
    buf = np.zeros(kit.n_of(length + 0.05))
    _down(buf, p, rng, lead, hit_n, newer)
    _up(buf, p, rng, lead, hit_n)
    out = _capture(_two_mics(buf, p), p, rng)[: kit.n_of(length)]
    out = dsp.fade(out, 0.002, rng.uniform(0.025, 0.05))
    return np.asarray(out, dtype=np.float64)


# ---------------------------------------------------------- critic helpers

def _level_frames(sig: np.ndarray, hop: int) -> np.ndarray:
    count = max(1, len(sig) // hop)
    power = np.square(sig[: count * hop]).reshape(count, hop).mean(axis=1)
    return 10 * np.log10(power + 1e-14)


def key_stats(x: np.ndarray) -> dict:
    """Measure a render: attack time in the 2-12 kHz band, its power share in dB, and the -40 dB time."""
    mono = x.mean(axis=1) if x.ndim == 2 else np.asarray(x, dtype=float)

    band = _level_frames(kit.bandpass(mono, 2000, 12000, 4), 48)
    top = int(np.argmax(band))
    onset = float(top - np.nonzero(band[: top + 1] >= band[top] - 20)[0][0])

    spec = np.abs(np.fft.rfft(mono)) ** 2
    hz = np.fft.rfftfreq(len(mono), 1 / kit.SR)
    share = float(spec[(hz >= 2000) & (hz < 12000)].sum() / (spec.sum() + 1e-30))
    share_db = round(10 * math.log10(share + 1e-14), 1)

    coarse = _level_frames(mono, 240)
    loud = int(np.argmax(coarse))
    quiet = np.nonzero(coarse[loud:] <= coarse[loud] - 40)[0]
    tail = float(quiet[0] * 5) if len(quiet) else float((len(coarse) - loud) * 5)
    return {"onset_ms": onset, "top_db": share_db, "t40_ms": tail}


def key_penalty(x: np.ndarray) -> float:
    """Score from 0 down to -1.5: it asks for a bright top and a tail under 200 ms."""
    st = key_stats(x)
    dull = min(1.0, max(0.0, -5 - st["top_db"]) / 6)
    long_tail = min(1.0, max(0.0, st["t40_ms"] - 200) / 100)
    return -0.75 * (dull + long_tail)


def extra_for(cat: str, weight: float = 1.0):
    """Critic extra for ``typing_key``; no other category gets one."""
    if cat == "typing_key":
        return lambda x, row=None: weight * key_penalty(x)
    return None
