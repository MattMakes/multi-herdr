"""Pops: a physical bubble pop (version 1) and designed pops for motion graphics (version 2).

Version 2 takes any parameter set that holds ``style`` (one of chirp, blip,
bubble). Sets without it go to the bubble model of version 1. ``V2`` is the
recipe object of version 2.
"""

from __future__ import annotations

import math
import types

import numpy as np

from hark.forge.engine import dsp, fx
from hark.forge.recipes.ui import _layers as kit

TARGET = "a small bubble pop"
V2_TARGET = "a short tonal pop"
TARGETS = {"pop": TARGET, "pro_pop": V2_TARGET}
ALTS = ["a cork pop", "a water drop", "a finger snap", "a mouse click"]
V2_ALTS = ["a mouse click", "a laser zap", "a synth beep", "a finger snap"]
REFSET = "pop"
REFS = [f"{kit.ref_root()}/ui/ui_pop_{i:02d}.wav" for i in range(1, 5)]


# ------------------------------------------------------------ the bubble model

SPACE = kit.space_table("""
    f0 400 3500 log
    q 4 40 log
    rise -0.1 1.4
    rise_tau 0.002 0.08 log
    att 0.0001 0.03 log
    h2 0.0 0.3
    wobble 0.0 0.05
    breath_amp 0.0 1.0
    breath_tau 0.001 0.03 log
    f2_ratio 1.6 4.5
    f2_amp 0.0 0.8
    f2_q 3 25 log
    f2_follow 0.5 1.0
    sub_amp 0.0 0.6
    sub_ratio 1.2 2.6
    sub_delay 0.002 0.03 log
    click_amp 0.0 1.5
    click_w 5e-05 0.0006 log
    tear 0.0001 0.003 log
    jerks 1 6 int
    click_lp 2000 9000 log
    shred_amp 0.0 0.5
    shred_tau 0.0002 0.003 log
    shred_hp 500 5000 log
    hiss_amp 0.0 0.6
    hiss_f 1500 6000 log
    hiss_q 0.5 3.0
    hiss_tau 0.001 0.03 log
    puff_amp 0.0 0.35
    puff_f 120 400 log
    ring_delay 0.0 0.003
    rt60 0.05 0.45 log
    wet 0.0 0.45
    damp 0.3 0.95
    size 0.05 0.8
    air_db -12.0 2.0
    floor_db -86.0 -64.0
    swell_amp 0.0 0.6
    swell_t 0.0005 0.012 log
    pre 0.004 0.035
    hp 40 160 log
    lp 4000 14000 log
    clip 0.14 0.38
    cut_fade 0.004 0.03 log
    cut_db -56.0 -42.0
""")

DEFAULTS = kit.value_table("""
    f0 440.159  q 22.1207  rise 0.991892  rise_tau 0.0159722  att 0.000357098  h2 0.00847939
    wobble 0.0112906  breath_amp 0.0  breath_tau 0.00848874  f2_ratio 3.82514  f2_amp 0.8  f2_q 25.0
    f2_follow 0.948223  sub_amp 0.00328062  sub_ratio 2.08913  sub_delay 0.0111736  click_amp 0.685048
    click_w 7.72837e-05  tear 0.000171419  jerks 1  click_lp 4881.31  shred_amp 0.200779
    shred_tau 0.00153019  shred_hp 5000.0  hiss_amp 0.0659931  hiss_f 2418.88  hiss_q 2.86992
    hiss_tau 0.00113073  puff_amp 0.0  puff_f 121.063  ring_delay 0.00105542  rt60 0.249181  wet 0.291252
    damp 0.490701  size 0.05  air_db -0.476997  floor_db -64.4367  swell_amp 0.160452  swell_t 0.00252584
    pre 0.0269276  hp 92.6866  lp 10181.4  clip 0.274425  cut_fade 0.0153059
""")

V1_BOXES = {
    "main": kit.space_table("""
        swell_amp 0.15 0.6
        swell_t 0.002 0.012 log
        click_amp 0 0.7
    """),
    "bloop": kit.space_table("""
        f0 350 900 log
        rise 0.7 1.4
        rise_tau 0.025 0.08 log
        att 0.006 0.04 log
        q 18 40 log
        click_amp 0 0.35
        shred_amp 0 0.2
        ring_delay 0 0.025
    """),
    "small": kit.space_table("""
        swell_amp 0.15 0.6
        swell_t 0.002 0.012 log
        f0 1400 3500 log
        rise 0 1.0
        click_amp 0 0.8
        shred_amp 0 0.45
    """),
    "lip": kit.space_table("""
        f0 450 1300 log
        rise -0.1 0.35
        rise_tau 0.005 0.05 log
        q 4 14 log
        breath_amp 0.2 1.0
        att 0.0003 0.004 log
        click_amp 0 0.6
        sub_amp 0 0.2
    """),
}

V1_N_S = 0.35


def v1_family_space(fam: str) -> dict:
    """Version 1 space, with the ranges of one named family laid over it."""
    if fam not in V1_BOXES:
        raise KeyError(f"unknown pop version 1 family {fam!r}")
    return kit.narrow_space(SPACE, V1_BOXES[fam])


def rupture(w: float, n: int) -> np.ndarray:
    """Film rupture: a short positive bump followed by a broader negative bump that cancels its area."""
    gain, band = max(2, kit.n_of(w)), max(3, kit.n_of(2.5 * w))
    pos = 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, gain))
    neg = 0.5 - 0.5 * np.cos(np.linspace(0, 2 * np.pi, band))
    neg = neg * -pos.sum() / neg.sum()
    return kit.fit(np.concatenate([pos, neg]), n)


def tear_crackle(w: float, tear: float, jerks: int, n: int, r: np.random.Generator) -> np.ndarray:
    """Several rupture pulses scattered over the time the film takes to tear."""
    times = np.sort(np.concatenate([[0.0], r.uniform(0, tear, int(jerks) - 1)]))
    wave = np.zeros(n)
    pulse = rupture(w, n)
    for span, tt in enumerate(times):
        amp = 1.0
        if span >= 1:
            amp = r.uniform(0.25, 0.8) * 0.8 ** span * (-1.0 if r.uniform() < 0.25 else 1.0)
        kit.add_at(wave, pulse, kit.n_of(tt) if tt > 0 else 0, amp)
    return wave


def pitch_drift(depth: float, size: int, gen: np.random.Generator) -> np.ndarray:
    """Low-passed random pitch drift whose RMS is ``depth``."""
    if depth <= 0:
        return np.zeros(size)
    noise = dsp.butter(gen.standard_normal(size), "low", 60, 2)
    return noise * depth / (kit.rms(noise) + 1e-12)


def cavity_pitch(f0: float, rise: float, rise_tau: float, size: int, wob: np.ndarray) -> np.ndarray:
    """Pitch of the air cavity, which climbs as the bubble collapses."""
    tt = kit.t_of(size)
    return f0 * (1 + rise * (1 - np.exp(-tt / rise_tau))) * (1 + wob)


def cavity_ring(f0: float, q: float, rise: float, rise_tau: float, att: float, h2: float, size: int,
                 gen: np.random.Generator, wob: np.ndarray) -> np.ndarray:
    """Ringing of the cavity along its climbing pitch, plus a faint octave."""
    tt = kit.t_of(size)
    phase = np.cumsum(2 * np.pi * cavity_pitch(f0, rise, rise_tau, size, wob) / kit.SR) + gen.uniform(-0.3, 0.3)
    shape = (1 - np.exp(-tt / att)) * np.exp(-tt / (q / (np.pi * f0)))
    return shape * np.sin(phase) + h2 * shape ** 2 * np.sin(2 * phase + 0.5)


def _band_noise(gen: np.random.Generator, size: int, fc, q: float) -> np.ndarray:
    return np.asarray(dsp.svf(gen.standard_normal(size), fc, q, "band"), dtype=float)


def _cavity(cfg: dict, gen: np.random.Generator, size: int) -> np.ndarray:
    wob = pitch_drift(cfg["wobble"], size, gen)
    body = cavity_ring(cfg["f0"], cfg["q"], cfg["rise"], cfg["rise_tau"], cfg["att"], cfg["h2"], size, gen, wob)
    body += cfg["f2_amp"] * cavity_ring(cfg["f0"] * cfg["f2_ratio"], cfg["f2_q"], cfg["rise"] * cfg["f2_follow"],
                                       cfg["rise_tau"], cfg["att"], 0.0, size, gen, wob)
    if cfg["breath_amp"] > 0.01:
        body += cfg["breath_amp"] * _cavity_breath(cfg, gen, size, wob)
    if cfg["sub_amp"] > 0.01:
        sub = cavity_ring(cfg["f0"] * cfg["sub_ratio"], 0.7 * cfg["q"], 0.4 * cfg["rise"], 0.6 * cfg["rise_tau"],
                           max(cfg["att"], 0.0002), cfg["h2"], size, gen, np.zeros(size))
        kit.add_at(body, sub, kit.n_of(cfg["sub_delay"]), cfg["sub_amp"])
    return body


def _cavity_breath(cfg: dict, gen: np.random.Generator, size: int, wob: np.ndarray) -> np.ndarray:
    """Breathy band noise centred on the ring pitch, scaled to peak 1."""
    tt = kit.t_of(size)
    fc = cavity_pitch(cfg["f0"], cfg["rise"], cfg["rise_tau"], size, wob)
    br = _band_noise(gen, size, fc, max(2.0, cfg["q"] / 2))
    br *= (1 - np.exp(-tt / max(cfg["att"], 0.0003))) * np.exp(-tt / cfg["breath_tau"])
    return kit.unit(br)


def _burst_event(cfg: dict, gen: np.random.Generator, size: int) -> np.ndarray:
    tt = kit.t_of(size)
    sig = cfg["click_amp"] * _film_tick(cfg, gen, size)
    shred = dsp.butter(gen.standard_normal(size), "high", cfg["shred_hp"], 2) * np.exp(-tt / cfg["shred_tau"])
    sig = sig + 0.5 * cfg["shred_amp"] * shred
    kit.add_at(sig, _cavity(cfg, gen, size), kit.n_of(cfg["ring_delay"]) if cfg["ring_delay"] > 0 else 0)
    hiss = _band_noise(gen, size, cfg["hiss_f"], cfg["hiss_q"]) * (1 - np.exp(-tt / 0.0003)) * np.exp(-tt / cfg["hiss_tau"])
    return sig + cfg["hiss_amp"] * kit.unit(hiss) + cfg["puff_amp"] * _low_puff(cfg, tt)


def _film_tick(cfg: dict, gen: np.random.Generator, size: int) -> np.ndarray:
    """Tear crackle of the film; a ``click_lp`` under 19 kHz low-passes it."""
    tick = tear_crackle(cfg["click_w"], cfg["tear"], cfg["jerks"], size, gen)
    if cfg["click_lp"] < 19000:
        tick = dsp.butter(tick, "low", cfg["click_lp"], 2)
    return tick


def _low_puff(cfg: dict, tt: np.ndarray) -> np.ndarray:
    """Very short, strongly damped low sine at ``puff_f``."""
    return np.sin(2 * np.pi * cfg["puff_f"] * tt) * np.exp(-2.5 * cfg["puff_f"] * tt) * (1 - np.exp(-tt / 0.0004))


def _air_leak(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    stats = kit.n_of(cfg["swell_t"])
    noise = gen.standard_normal(stats)
    leak = _band_of(noise, cfg["f0"], 3) + 0.6 * _band_of(noise, cfg["f0"] * cfg["f2_ratio"], 2)
    leak = leak + 0.9 * dsp.butter(noise, "high", 3000, 2)
    leak = leak * np.exp(5 * (np.arange(stats) / stats - 1))
    return cfg["swell_amp"] * kit.unit(leak)


def _band_of(noise: np.ndarray, fc: float, q: float) -> np.ndarray:
    return np.asarray(dsp.svf(noise, fc, q, "band"), dtype=float)


def _end_sample(sig: np.ndarray, cfg: dict, end: int, head: int) -> int:
    """End sample: where the level first falls ``cut_db`` under its loudest 5 ms frame, plus the fade."""
    mono = kit.fit(sig.mean(axis=1) if sig.ndim == 2 else sig, max(240, len(sig)))
    span = len(mono) // 240
    e = 20 * np.log10(np.sqrt(np.square(mono[: span * 240]).reshape(span, 240).mean(axis=1)) + 1e-12)
    i = int(np.argmax(e))
    quiet = np.nonzero(e[i:] < e[i] + cfg["cut_db"])[0]
    j = int(quiet[0]) if len(quiet) else span - i
    died = (i + j) * 240 + kit.n_of(cfg["cut_fade"])
    return min(len(sig), max(end, died), head + kit.n_of(0.42))


def _placed_event(cfg: dict, rng: np.random.Generator) -> tuple[np.ndarray, int]:
    """Return the event in its room (with lead silence and optional leak) and the leak length."""
    sig = _burst_event(cfg, rng, kit.n_of(V1_N_S))
    lead = 0
    if cfg["swell_amp"] > 0.01:
        leak = _air_leak(cfg, rng)
        lead = len(leak)
        sig = np.concatenate([leak, sig])
    sig = np.concatenate([np.zeros(kit.n_of(cfg["pre"])), sig])
    sig = dsp.room(sig, rng, rt60=cfg["rt60"], size=cfg["size"], damp=cfg["damp"], wet=cfg["wet"])
    return np.asarray(sig, dtype=float), lead


def _old_finish(sig: np.ndarray, cfg: dict, rng: np.random.Generator) -> np.ndarray:
    """Finish of the oldest sets, which have no ``clip``: mic, tail trim and a short fade."""
    sig = dsp.mic(sig, rng, air_db=cfg["air_db"], floor_db=cfg["floor_db"])
    sig = dsp.trim_tail(np.asarray(sig, dtype=float), -62.0, 0.02)
    return np.asarray(dsp.fade(sig, 0.001, 0.02), dtype=np.float64)


def render_v1(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render version 1, the bubble pop: mono float64 at 48 kHz, about 0.2 to 0.45 s after the edit."""
    p = kit.merged(DEFAULTS, p)
    sig, lead = _placed_event(p, rng)
    if "clip" not in p:
        return _old_finish(sig, p, rng)
    sig = dsp.butter(dsp.butter(sig, "high", p["hp"], 4), "low", p["lp"], 2)
    sig = np.asarray(dsp.mic(sig, rng, air_db=p["air_db"], floor_db=p["floor_db"]), dtype=float)
    head = kit.n_of(p["pre"]) + lead
    end = head + kit.n_of(p["clip"])
    if "cut_db" in p:
        end = _end_sample(sig, p, end, head)
    return np.asarray(dsp.fade(sig[:end], 0.001, p["cut_fade"]), dtype=np.float64)


# ------------------------------------------------------------ designed pops

V2_SPACE = kit.space_table("""
    style | chirp blip bubble
    f_hi 3000 11500 log
    f_lo 90 900 log
    sweep 0.025 0.14
    ease 0.5 2.0
    steps 0.0 1.0
    step_ms 4.0 13.0
    duty 0.45 1.0
    pre_db -40.0 0.0
    pre_hz 2500 11000 log
    pre_t 0.008 0.04
    peak_at 0.1 0.8
    start_db -12.0 0.0
    tail_db -20.0 -3.0
    tail_hold 0.0 0.035
    tail_tau 0.006 0.05 log
    harm 0.0 0.7
    odd 0.0 1.0
    f_blip 700 2200 log
    box 0.003 0.07 log
    b_att 0.0004 0.004 log
    b_rel 0.002 0.014 log
    b_drop 0.0 5.0
    b_tau 0.006 0.3 log
    f0 280 1400 log
    rise 0.1 1.6
    rise_tau 0.003 0.04 log
    q_tau 0.01 0.09 log
    att 0.0004 0.004 log
    h2 0.0 0.3
    daughter 0.0 0.6
    d_ratio 1.3 2.4
    d_delay 0.004 0.016
    tick_db -40.0 6.0
    tick_hz 1800 9000 log
    tick_ms 1 12 log
    breath 0.0 0.5
    room 0.0 0.25
    floor_db -110.0 -62.0
    hp 60 400 log
    lp 4500 18000 log
    drive 0.0 8.0
    length 0.03 0.4
    fade 0.002 0.03 log
    width 0.0 0.4
    gdepth -1.0 1.0
    c_breath -60.0 -6.0
    c_bq 1.5 8 log
    land_drop 0.0 7.0
    s_att 0.0 0.5
    pre_ring 0.0 8.0
    c_in 0.0 1.0
    c_air -60.0 -10.0
    c_nat 0.0 1.0
    c_ptau 0.002 0.03 log
    c_tau 0.006 0.05 log
    c_h2 0.0 0.3
    c_puff -60.0 -6.0
    c_puff_hz 800 6000 log
    c_puff_ms 0.6 6 log
    c_thump -60.0 -6.0
    c_room 0.0 0.3
    c_pre -60.0 -3.0
    c_pre_hz 3000 11000 log
    c_pre_t 0.01 0.04
    n6 0.0 1.0
    n_f0 150 12000 log
    n_f1 100 6000 log
    n_ptau 0.002 0.06 log
    n_sweep 0.0 0.14
    n_ease 0.4 2.0
    n_att 0.0003 0.04 log
    n_tau 0.003 0.06 log
    n_h2 0.0 0.4
    n_h3 0.0 0.2
    n_m2 0.0 0.6
    n_m2r 1.3 4.5
    n_m2d 1.0 4.0
    n_breath -60.0 -8.0
    n_dbl -60.0 0.0
    n_dgap 0.02 0.12
    n_dratio 0.6 2.0
    n_dtau 0.4 1.6
""")

V2_DEFAULTS = kit.value_table("""
    style chirp  f_hi 9800.0  f_lo 360.0  sweep 0.055  ease 1.0  steps 1.0  step_ms 9.0  duty 0.62
    pre_db -2.0  pre_hz 9800.0  pre_t 0.032  peak_at 0.3  start_db -2.0  tail_db -11.0  tail_hold 0.006
    tail_tau 0.009  harm 0.35  odd 0.6  f_blip 1220.0  box 0.005  b_att 0.0008  b_rel 0.003  b_drop 0.0
    b_tau 0.3  f0 520.0  rise 0.9  rise_tau 0.012  q_tau 0.035  att 0.0008  h2 0.08  daughter 0.15
    d_ratio 1.8  d_delay 0.008  tick_db -30.0  tick_hz 3500.0  tick_ms 1.5  breath 0.12  room 0.06
    floor_db -110.0  hp 150.0  lp 16000.0  drive 1.5  length 0.16  fade 0.004  width 0.15  gdepth -1.0
    c_breath -60.0  c_bq 3.0  land_drop 0.0  s_att 0.0  pre_ring 0.0  c_in 0.0  c_air -60.0  c_nat 0.0
    c_ptau 0.008  c_tau 0.018  c_h2 0.1  c_puff -60.0  c_puff_hz 2500.0  c_puff_ms 2.0  c_thump -60.0
    c_room 0.0  c_pre -60.0  c_pre_hz 9000.0  c_pre_t 0.025  n6 0.0  n_f0 1500.0  n_f1 600.0  n_ptau 0.01
    n_sweep 0.0  n_ease 1.0  n_att 0.001  n_tau 0.015  n_h2 0.1  n_h3 0.0  n_m2 0.0  n_m2r 2.3  n_m2d 2.0
    n_breath -60.0  n_dbl -60.0  n_dgap 0.05  n_dratio 1.33  n_dtau 1.0
""")

FINE = {
    "chirp": ["ease", "duty", "start_db", "tail_tau", "harm", "odd", "tick_db", "tick_hz", "hp", "lp", "drive",
              "fade", "peak_at", "c_breath", "land_drop"],
    "blip": ["b_att", "b_rel", "b_tau", "harm", "odd", "tick_db", "tick_hz", "hp", "lp", "drive", "fade"],
    "bubble": ["rise_tau", "att", "h2", "d_ratio", "d_delay", "tick_db", "tick_hz", "tick_ms", "breath", "room",
               "hp", "lp", "drive", "fade", "width", "floor_db"],
}


def family_space(params: dict, style: str | None = None) -> dict:
    """Version 2 space in which only the fine keys of the style stay free; the rest are pinned."""
    style = style or params.get("style") or "chirp"
    if style not in FINE:
        raise ValueError(f"unknown pop style {style!r}")
    full = _full_member(params)
    return {span: (spec if span in FINE[style] else [full[span]]) for span, spec in V2_SPACE.items()}


def _full_member(params: dict) -> dict:
    """Lay the member values over the version 2 defaults."""
    return dict(V2_DEFAULTS, **params)


def _top_fade(hz, span: float):
    return np.clip((18000 - span * np.asarray(hz)) / 3000, 0, 1)


def harmonics(ph: np.ndarray, f: np.ndarray, harm: float, odd: float) -> np.ndarray:
    """Sine plus 2nd and 3rd harmonics; each harmonic fades away as it nears 18 kHz."""
    wave = np.sin(ph)
    for span, gain in ((2, harm * (1 - odd)), (3, harm * odd)):
        if gain > 0:
            wave = wave + gain * _top_fade(f, span) * np.sin(span * ph)
    return wave


def click(n: int, db: float, hz: float, r: np.random.Generator, ms: float = 1.5) -> np.ndarray:
    """Short band-noise click at sample 0. With ``ms`` over 1.5 it becomes the bright 1-4 kHz snap."""
    if ms <= 1.5:
        span = min(n, kit.n_of(0.003))
        noise = dsp.butter(r.standard_normal(span), "band", (hz / 1.5, min(1.5 * hz, 20000.0)), 2)
        noise = noise * np.exp(-kit.t_of(span) / (ms / 1000 / 2.5))
    else:
        span = min(n, kit.n_of(0.004))
        tt = kit.t_of(span)
        noise = dsp.butter(r.standard_normal(span), "band", (hz / 2, min(1.7 * hz, 20000.0)), 4)
        noise = noise * (1 - np.exp(-tt / 0.0003)) * np.exp(-tt / (ms / 1000)) ** 2
        noise = dsp.butter(noise, "low", min(4000.0, 2 * hz), 4)
    out = np.zeros(n)
    out[:span] = 10 ** (db / 20) * kit.unit(np.asarray(noise, dtype=float))
    return out


def _add_puff(wave: np.ndarray, cfg: dict, gen: np.random.Generator, rise_tau: float) -> None:
    if cfg["c_puff"] <= -59:
        return
    ms = cfg["c_puff_ms"] / 1000
    hz = cfg["c_puff_hz"]
    span = min(len(wave), kit.n_of(max(0.004, 5 * ms)))
    tt = kit.t_of(span)
    noise = dsp.butter(gen.standard_normal(span), "band", (hz / 2.2, min(2.2 * hz, 16000.0)), 2)
    noise = noise * (1 - np.exp(-tt / rise_tau)) * np.exp(-tt / ms)
    wave[:span] += 10 ** (cfg["c_puff"] / 20) * kit.unit(noise)


def _add_thump(wave: np.ndarray, cfg: dict, fl: float) -> None:
    if cfg["c_thump"] <= -59:
        return
    span = min(len(wave), kit.n_of(1.5 / fl))
    tt = kit.t_of(span)
    th = np.sin(2 * np.pi * fl * tt) * np.exp(-2 * fl * tt) * (1 - np.exp(-tt / 0.0006))
    wave[:span] += 10 ** (cfg["c_thump"] / 20) * kit.unit(th)


def _soft_edges(wave: np.ndarray, cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Fade both edges, then add the optional soft pre-tick and the small room."""
    gain = min(len(wave), kit.n_of(0.0003))
    wave[:gain] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, gain))
    band = min(len(wave), kit.n_of(0.004))
    wave[len(wave) - band:] *= np.cos(np.linspace(0, np.pi / 2, band)) ** 2
    if cfg["c_pre"] > -59:
        wave = np.concatenate([10 ** (cfg["c_pre"] / 20) * _soft_tick(cfg), wave])
    if cfg["c_room"] > 0.005:
        wave = fx.reverb(wave, gen, rt60=0.1 + 0.4 * cfg["c_room"], size=0.12, predelay=0.003, damp=0.6, wet=cfg["c_room"],
                      stereo=True, width=0.25)
        wave = dsp.trim_tail(np.asarray(wave, dtype=float), -56.0, 0.012)
    return np.asarray(wave, dtype=float)


def _soft_tick(cfg: dict) -> np.ndarray:
    """Soft tick before the pop: a Hann-shaped sine at ``c_pre_hz`` that fades out after 1.25 ms."""
    stats = kit.n_of(cfg["c_pre_t"])
    tt = kit.t_of(stats)
    pre = np.sin(2 * np.pi * cfg["c_pre_hz"] * tt) * np.exp(-np.maximum(tt - 0.00125, 0) / 0.002)
    kp = min(stats, kit.n_of(0.0025))
    pre[:kp] *= np.hanning(kp)
    return pre


def soft_chirp(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Round sine whose pitch drops quickly and then settles."""
    size = kit.n_of(min(0.4, max(0.03, 7 * cfg["c_tau"] + 0.006)))
    tt = kit.t_of(size)
    hz = cfg["f_lo"] + (cfg["f_hi"] - cfg["f_lo"]) * np.exp(-tt / cfg["c_ptau"])
    phase = np.cumsum(2 * np.pi * hz / kit.SR) + gen.uniform(0, 2 * np.pi)
    shape = (1 - np.exp(-tt / 0.0004)) * np.exp(-tt / cfg["c_tau"])
    h2 = cfg["c_h2"] * _top_fade(hz, 2) * np.exp(-2 * tt / cfg["c_tau"]) * np.sin(2 * phase + 0.6)
    wave = (np.sin(phase) + h2) * shape
    _add_puff(wave, cfg, gen, 0.0008)
    _add_thump(wave, cfg, max(80.0, 0.35 * cfg["f_lo"]))
    return _soft_edges(wave, cfg, gen)


def _travel_pitch(cfg: dict, size: int, f0: float, f1: float) -> np.ndarray:
    tt = kit.t_of(size)
    if cfg["n_sweep"] > 0:
        gl = (1 - np.clip(tt / cfg["n_sweep"], 0, 1)) ** cfg["n_ease"]
    else:
        gl = np.exp(-tt / cfg["n_ptau"])
    return np.exp(math.log(f1) + (math.log(f0) - math.log(f1)) * gl)


def _travel_overtones(wave: np.ndarray, cfg: dict, gen: np.random.Generator, hz: np.ndarray, phase: np.ndarray,
                      dec: np.ndarray) -> np.ndarray:
    """Add harmonics 2 and 3 and an inharmonic mode at ``n_m2r`` times the pitch."""
    if cfg["n_h2"] > 0:
        wave = wave + cfg["n_h2"] * _top_fade(hz, 2) * dec * np.sin(2 * phase + 0.5)
    if cfg["n_h3"] > 0:
        wave = wave + cfg["n_h3"] * _top_fade(hz, 3) * dec ** 2 * np.sin(3 * phase + 1.1)
    if cfg["n_m2"] > 0:
        ph2 = np.cumsum(2 * np.pi * hz * cfg["n_m2r"] / kit.SR) + gen.uniform(0, 2 * np.pi)
        wave = wave + cfg["n_m2"] * _top_fade(hz, cfg["n_m2r"]) * dec ** cfg["n_m2d"] * np.sin(ph2)
    return wave


def _travel_breath(cfg: dict, gen: np.random.Generator, hz: np.ndarray, shape: np.ndarray, span: int) -> np.ndarray:
    """Band noise that follows the pitch, ``n_breath`` dB down, shaped like the body."""
    band = _band_noise(gen, len(shape), np.minimum(hz, 16000.0), cfg["c_bq"])
    band = band * math.sqrt(0.5) / (kit.rms(band[:span]) + 1e-12)
    return 10 ** (cfg["n_breath"] / 20) * band * shape


def travel_pop(cfg: dict, gen: np.random.Generator, size: int, f0: float, f1: float, tau: float,
               att: float) -> np.ndarray:
    """One round body; its log pitch moves from f0 to f1 while it decays."""
    tt = kit.t_of(size)
    hz = _travel_pitch(cfg, size, f0, f1)
    phase = np.cumsum(2 * np.pi * hz / kit.SR) + gen.uniform(0, 2 * np.pi)
    env_r = 0.5 - 0.5 * np.cos(np.pi * np.clip(tt / att, 0, 1))
    dec = np.exp(-np.maximum(tt - att, 0) / tau)
    wave = _travel_overtones(np.sin(phase), cfg, gen, hz, phase, dec)
    shape = env_r * dec
    wave = wave * shape
    if cfg["n_breath"] > -59:
        wave = wave + _travel_breath(cfg, gen, hz, shape, min(size, max(16, kit.n_of(att + tau))))
    return wave


def soft_pop(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Moving-pitch pop, optionally followed by a second, retuned pop."""
    double = cfg["n_dbl"] > -59
    gap = cfg["n_dgap"] if double else 0.0
    dur = cfg["n_att"] + 6.5 * cfg["n_tau"] * (max(1.0, cfg["n_dtau"]) if double else 1.0) + gap + 0.006
    size = kit.n_of(min(0.4, max(0.03, dur)))
    wave = travel_pop(cfg, gen, size, cfg["n_f0"], cfg["n_f1"], cfg["n_tau"], cfg["n_att"])
    if double:
        span = kit.n_of(gap)
        if span < size:
            wave[span:] += 10 ** (cfg["n_dbl"] / 20) * travel_pop(cfg, gen, size - span, cfg["n_f0"] * cfg["n_dratio"],
                                                         cfg["n_f1"] * cfg["n_dratio"], cfg["n_tau"] * cfg["n_dtau"],
                                                         cfg["n_att"])
    _add_puff(wave, cfg, gen, 0.0006)
    _add_thump(wave, cfg, max(70.0, 0.35 * min(cfg["n_f0"], cfg["n_f1"])))
    return _soft_edges(wave, cfg, gen)


def _held_steps(cfg: dict, since: np.ndarray, logp: np.ndarray, log_at) -> tuple[np.ndarray, np.ndarray]:
    """Hold the log pitch inside each step block and build the gate of the blocks."""
    B = cfg["step_ms"] / 1000
    sp = np.maximum(since, 0)
    held = log_at(np.clip(np.floor(sp / B) * B / cfg["sweep"], 0, 1))
    logp = np.where(since < cfg["sweep"], (1 - cfg["steps"]) * logp + cfg["steps"] * held, logp)
    raw = _block_gate(cfg, since, sp, B)
    depth = cfg["steps"] if cfg["gdepth"] < 0 else cfg["gdepth"]
    lvl = np.where(since >= 0, 1 - depth * (1 - (0.5 - 0.5 * np.cos(np.pi * raw))), 1.0)
    return logp, lvl


def _block_gate(cfg: dict, since: np.ndarray, sp: np.ndarray, B: float) -> np.ndarray:
    """Gate from 0 to 1: a trapezoid per block by duty cycle, then a 0.7 ms ramp after the landing."""
    t_land = math.ceil(cfg["sweep"] / B - 1e-9) * B
    sig = np.mod(sp, B) / B
    e_f = 0.0007 / B
    e_r = max(e_f, cfg["s_att"] * cfg["duty"])
    return np.where(since < t_land, np.clip(np.minimum(sig / e_r, (cfg["duty"] - sig) / e_f), 0, 1),
                    np.clip((since - t_land) / 0.0007, 0, 1))


def _level_curve(cfg: dict, since: np.ndarray, prog: np.ndarray) -> np.ndarray:
    pa = cfg["peak_at"]
    db = np.where(prog < pa, cfg["start_db"] * (1 - prog / max(pa, 1e-3)), cfg["tail_db"] * (prog - pa) / max(1 - pa, 1e-3))
    db = db - 8.686 * np.maximum(since - cfg["sweep"] - cfg["tail_hold"], 0) / cfg["tail_tau"]
    return 10 ** (db / 20)


def _add_air(wave: np.ndarray, cfg: dict, gen: np.random.Generator, shape: np.ndarray) -> np.ndarray:
    an = dsp.butter(gen.standard_normal(len(wave)), "band", (2500, 14000), 2)
    an = an * math.sqrt(0.5) / (kit.rms(an) + 1e-12)
    span = math.exp(-1 / (kit.SR * 2.5 * cfg["tail_tau"]))
    ea = np.empty(len(shape))
    last = 0.0
    for i, v in enumerate(shape):
        last = max(v, last * span)
        ea[i] = last
    return wave + 10 ** (cfg["c_air"] / 20) * an * ea


def _add_pre_blip(wave: np.ndarray, cfg: dict, t0: float) -> None:
    lvl = 10 ** (cfg["pre_db"] / 20)
    span = min(len(wave), kit.n_of(0.003))
    tt = kit.t_of(len(wave))
    wave[:span] += lvl * np.hanning(span) * np.sin(2 * np.pi * cfg["pre_hz"] * tt[:span])
    if cfg["pre_ring"] > 0.05:
        kk = min(len(wave), kit.n_of(t0 + 0.002))
        tt = tt[:kk]
        ring = np.sin(2 * np.pi * cfg["pre_hz"] * tt) * np.exp(-tt / (cfg["pre_ring"] / 1000))
        wave[:kk] += 0.5 * lvl * ring * np.clip(tt / 0.0015, 0, 1)


def _eased_pitch(cfg: dict, since: np.ndarray, prog: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Eased log pitch with the optional steps, the block gate and the drop after the landing."""
    lo, hi = math.log(cfg["f_lo"]), math.log(cfg["f_hi"])

    def log_at(v):
        return hi + (lo - hi) * v ** cfg["ease"]

    logp, gate = log_at(prog), np.ones(len(since))
    if cfg["steps"] > 0:
        logp, gate = _held_steps(cfg, since, logp, log_at)
    if cfg["land_drop"] > 0:
        logp = logp - math.log(2) * cfg["land_drop"] / 12 * (1 - np.exp(-np.maximum(since - cfg["sweep"], 0) / 0.02))
    return np.exp(logp), gate


def _gated_level(cfg: dict, since: np.ndarray, prog: np.ndarray, gate: np.ndarray, i0: int) -> np.ndarray:
    """Level curve times the gate: zero before the start, then a 0.8 ms raised-cosine fade-in."""
    shape = np.where(since < 0, 0.0, _level_curve(cfg, since, prog) * gate)
    gain = max(0, min(kit.n_of(0.0008), len(since) - i0))
    shape[i0:i0 + gain] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, gain))
    return shape


def step_chirp(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Designed chirp, stepped or smooth. The ``n6`` and ``c_nat`` switches pick the natural pops."""
    if cfg["n6"] > 0.5:
        return soft_pop(cfg, gen)
    if cfg["c_nat"] > 0.5:
        return soft_chirp(cfg, gen)
    t0 = cfg["pre_t"] if cfg["pre_db"] > -39 else 0.0
    size = kit.n_of(t0 + cfg["sweep"] + cfg["tail_hold"] + 5.5 * cfg["tail_tau"] + cfg["fade"] + 0.002)
    since = kit.t_of(size) - t0
    prog = np.clip(since / cfg["sweep"], 0, 1)
    hz, gate = _eased_pitch(cfg, since, prog)
    wave = harmonics(np.cumsum(2 * np.pi * hz / kit.SR) + gen.uniform(0, 2 * np.pi), hz, cfg["harm"], cfg["odd"])
    i0 = kit.n_of(t0) if t0 > 0 else 0
    shape = _gated_level(cfg, since, prog, gate, i0)
    return _chirp_extras(wave * shape, cfg, gen, hz, shape, t0, i0)


def _chirp_extras(wave: np.ndarray, cfg: dict, gen: np.random.Generator, hz: np.ndarray, shape: np.ndarray, t0: float,
                  i0: int) -> np.ndarray:
    """Add the breath, air, pre-blip and onset click, then fade in when ``c_in`` asks."""
    size = len(wave)
    if cfg["c_breath"] > -59:
        band = _band_noise(gen, size, np.minimum(hz, 16000.0), cfg["c_bq"])
        wave = wave + 10 ** (cfg["c_breath"] / 20) * band * math.sqrt(0.5) / (kit.rms(band) + 1e-12) * shape
    if cfg["c_air"] > -59:
        wave = _add_air(wave, cfg, gen, shape)
    if cfg["pre_db"] > -39:
        _add_pre_blip(wave, cfg, t0)
    if cfg["tick_db"] > -39:
        wave = wave + np.roll(click(size, cfg["tick_db"], cfg["tick_hz"], gen, cfg["tick_ms"]), i0)
    if cfg["c_in"] > 0.01:
        span = min(size, kit.n_of(cfg["c_in"] / 1000))
        wave[:span] *= 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, span))
    return wave


def _blip_shape(wave: np.ndarray, cfg: dict, tt: np.ndarray) -> np.ndarray:
    """Shape the blip: cosine rise, hold until ``box``, cosine release, all under a ``b_tau`` decay."""
    gain = np.clip(tt / cfg["b_att"], 0, 1)
    rl = np.clip((tt - cfg["box"]) / cfg["b_rel"], 0, 1)
    return wave * (0.5 - 0.5 * np.cos(np.pi * gain)) * (0.5 + 0.5 * np.cos(np.pi * rl)) * np.exp(-tt / cfg["b_tau"])


def tone_blip(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Gated tone burst with harmonics; its pitch can fall by ``b_drop`` semitones."""
    size = kit.n_of(cfg["box"] + cfg["b_rel"] + cfg["fade"] + 0.004)
    tt = kit.t_of(size)
    hz = cfg["f_blip"] * 2 ** (-(cfg["b_drop"] / 12) * np.clip(tt / max(cfg["box"], 1e-3), 0, 1))
    wave = harmonics(np.cumsum(2 * np.pi * hz / kit.SR) + gen.uniform(0, 2 * np.pi), hz, cfg["harm"], cfg["odd"])
    wave = _blip_shape(wave, cfg, tt)
    if cfg["tick_db"] > -39:
        wave = wave + click(size, cfg["tick_db"], cfg["tick_hz"], gen, cfg["tick_ms"])
    return wave


def _second_bubble(cfg: dict, tt: np.ndarray) -> np.ndarray:
    """Ring of a smaller second bubble: higher by ``d_ratio``, with less rise and a shorter decay."""
    dhz = cfg["f0"] * cfg["d_ratio"] * (1 + 0.4 * cfg["rise"] * (1 - np.exp(-tt / (0.6 * cfg["rise_tau"]))))
    dshape = (1 - np.exp(-tt / max(cfg["att"], 0.0003))) * np.exp(-tt / (0.6 * cfg["q_tau"]))
    return cfg["daughter"] * dshape * np.sin(np.cumsum(2 * np.pi * dhz / kit.SR))


def _rising_ring(cfg: dict, gen: np.random.Generator, tt: np.ndarray, trk: np.ndarray) -> np.ndarray:
    """Rising sine along ``trk`` plus a quieter octave that dies twice as fast."""
    phase = np.cumsum(2 * np.pi * trk / kit.SR) + gen.uniform(-0.3, 0.3)
    shape = (1 - np.exp(-tt / cfg["att"])) * np.exp(-tt / cfg["q_tau"])
    return shape * np.sin(phase) + cfg["h2"] * shape ** 2 * np.sin(2 * phase + 0.4)


def clean_bubble(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """Clean rising sine with a tick, an optional second bubble and breath that follows the pitch."""
    size = kit.n_of(max(cfg["length"], 0.06) + 0.05)
    tt = kit.t_of(size)
    trk = cfg["f0"] * (1 + cfg["rise"] * (1 - np.exp(-tt / cfg["rise_tau"])))
    wave = _rising_ring(cfg, gen, tt, trk)
    if cfg["daughter"] > 0.01:
        kit.add_at(wave, _second_bubble(cfg, tt), kit.n_of(cfg["d_delay"]))
    if cfg["breath"] > 0.01:
        band = _band_noise(gen, size, trk, 4) * (1 - np.exp(-tt / 0.0006)) * np.exp(-tt / 0.006)
        wave = wave + cfg["breath"] * kit.unit(band)
    if cfg["tick_db"] > -39:
        wave = wave + click(size, cfg["tick_db"], cfg["tick_hz"], gen, cfg["tick_ms"])
    return wave


def _bubble_space(wave: np.ndarray, cfg: dict, gen: np.random.Generator) -> np.ndarray:
    if cfg["room"] > 0.005:
        wave = fx.reverb(wave, gen, rt60=0.12 + 0.5 * cfg["room"], size=0.15, predelay=0.004, damp=0.6, wet=cfg["room"],
                      stereo=True, width=0.3 + cfg["width"])
    else:
        wave = np.column_stack([wave, wave])
    if cfg["floor_db"] > -100:
        wave = dsp.mic(np.asarray(wave, dtype=float), gen, floor_db=cfg["floor_db"])
    return np.asarray(wave, dtype=float)[: kit.n_of(cfg["length"])]


STYLES = {"chirp": step_chirp, "blip": tone_blip, "bubble": clean_bubble}


def render_v2(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render version 2. Chirp and blip come out mono (chirp is stereo with a room); bubble is stereo."""
    p = dict(V2_DEFAULTS, **(p or {}))
    if p["style"] not in STYLES:
        raise ValueError(f"unknown pop style {p['style']!r}")
    wave = _tone_stage(np.asarray(STYLES[p["style"]](p, rng), dtype=float), p)
    if p["style"] == "bubble":
        wave = _bubble_space(wave, p, rng)
    elif p["style"] == "blip":
        wave = wave[: min(len(wave), max(kit.n_of(p["length"]), kit.n_of(p["box"] + p["b_rel"] + 0.002)))]
    return _fade_end(wave, p["fade"]).astype(np.float64)


def _tone_stage(wave: np.ndarray, cfg: dict) -> np.ndarray:
    """Tone stage: optional parallel tanh drive, a steep high-pass and an optional low-pass."""
    if cfg["drive"] > 0.2:
        wave = np.asarray(fx.saturate(wave, drive_db=cfg["drive"], mode="tanh", mix=0.5, level="rms"), dtype=float)
    wave = dsp.butter(wave, "high", cfg["hp"], 4)
    if cfg["lp"] < 17500:
        wave = dsp.butter(wave, "low", cfg["lp"], 2)
    return np.asarray(wave, dtype=float)


def _fade_end(wave: np.ndarray, fade: float) -> np.ndarray:
    """Return a copy with a squared-cosine fade over the closing ``fade`` seconds."""
    span = min(len(wave), kit.n_of(fade))
    noise = np.cos(np.linspace(0, np.pi / 2, span)) ** 2
    wave = np.array(wave, dtype=float)
    wave[len(wave) - span:] *= noise if wave.ndim == 1 else noise[:, None]
    return wave


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """Render one pop: version 2 when ``p`` names a ``style``, else version 1."""
    return render_v2(p, rng) if "style" in (p or {}) else render_v1(p, rng)


# ------------------------------------------------------------ member tables

def _with_defaults(base: dict, table: dict[str, dict]) -> list[tuple[str, dict]]:
    return [(name, {**V2_DEFAULTS, **base, **edits}) for name, edits in table.items()]


ROUND6_BASE = kit.value_table("""
    n6 1  drive 0  steps 0  pre_db -40  tick_db -40  harm 0  c_breath -60  hp 120  lp 16000  fade 0.004
""")
ROUND6 = {
    "chirp": kit.value_table("""
        n_f0 9800  n_f1 350  n_ptau 0.03  n_att 0.012  n_tau 0.018  n_h2 0.05  n_breath -12  c_bq 2.0
        c_puff -22  c_puff_hz 7000  c_puff_ms 0.6  c_pre -10  c_pre_hz 9500  c_pre_t 0.026  c_room 0.08
    """),
    "boop": kit.value_table("""
        n_f0 1200  n_f1 210  n_ptau 0.014  n_att 0.0008  n_tau 0.016  n_h2 0.05  c_puff -12  c_puff_hz 4500
        c_puff_ms 0.6  hp 90  c_room 0.06
    """),
    "bloop": kit.value_table("""
        n_f0 260  n_f1 540  n_ptau 0.022  n_att 0.004  n_tau 0.016  n_h2 0.15  n_breath -24  c_bq 3.0
        c_room 0.1  hp 100
    """),
    "double": kit.value_table("""
        n_f0 1700  n_f1 950  n_ptau 0.005  n_att 0.0008  n_tau 0.008  n_h2 0.1  n_m2 0.2  n_m2r 2.76
        n_m2d 2.5  n_dbl -4  n_dgap 0.07  n_dratio 0.75  n_dtau 1.2  c_puff -16  c_puff_hz 3500
        c_puff_ms 0.6  c_room 0.08
    """),
    "tink": kit.value_table("""
        n_f0 2300  n_f1 1900  n_ptau 0.004  n_att 0.0005  n_tau 0.012  n_h2 0.03  n_m2 0.45  n_m2r 2.76
        n_m2d 1.5  c_puff -20  c_puff_hz 6000  c_puff_ms 0.5  c_room 0.08
    """),
    "thup": kit.value_table("""
        n_f0 200  n_f1 300  n_ptau 0.01  n_att 0.0015  n_tau 0.012  n_h2 0.1  c_puff -2  c_puff_hz 900
        c_puff_ms 2.5  c_thump -6  c_room 0.08  hp 80
    """),
}

# The kit pops keep their numbered names (Q-POP-1).
BUBBLES = {
    "pop_01": kit.value_table("""
        f0 520  rise 0.9  q_tau 0.014  att 0.002  room 0.15  length 0.2  tick_db 0  tick_ms 6  tick_hz 1700
    """),
    "pop_02": kit.value_table("""
        f0 1100  rise 0.55  rise_tau 0.006  q_tau 0.009  tick_db -6  tick_ms 4  tick_hz 1700  daughter 0.3
        room 0.1  length 0.15
    """),
    "pop_03": kit.value_table("""
        f0 330  rise 1.3  rise_tau 0.03  q_tau 0.03  att 0.005  tick_db 0  tick_ms 7  tick_hz 1700
        daughter 0  room 0.08  length 0.25
    """),
    "pop_04": kit.value_table("""
        f0 720  rise 0.25  q_tau 0.008  breath 0.45  tick_db -2  tick_ms 5  tick_hz 1700  daughter 0
        room 0.12  length 0.16
    """),
    "pop_05": kit.value_table("""
        f0 450  rise 0.15  q_tau 0.01  tick_db 2  tick_ms 6  tick_hz 1700  room 0.22  length 0.22
    """),
}

ROUND5_BASE = kit.value_table("""
    c_nat 1  drive 0  steps 0  pre_db -40  tick_db -40  harm 0  c_breath -60  hp 150  lp 15000  fade 0.004
""")
ROUND5 = {
    "chirp": kit.value_table("""
        f_hi 6500  f_lo 420  c_ptau 0.009  c_tau 0.02  c_h2 0.08  c_puff -16  c_puff_hz 3500  c_puff_ms 1.2
        c_room 0.08  c_pre -10  c_pre_hz 9500  c_pre_t 0.028
    """),
    "mouth": kit.value_table("""
        f_hi 1500  f_lo 500  c_ptau 0.005  c_tau 0.014  c_h2 0.12  c_puff -9  c_puff_hz 1800  c_puff_ms 1.5
        c_thump -12  c_room 0.12
    """),
    "blip": kit.value_table("""
        f_hi 1350  f_lo 1150  c_ptau 0.006  c_tau 0.009  c_h2 0.15  c_puff -22  c_puff_hz 4000
        c_puff_ms 0.8  c_room 0.06
    """),
    "drop": kit.value_table("""
        f_hi 1150  f_lo 230  c_ptau 0.016  c_tau 0.018  c_h2 0.1  c_puff -16  c_puff_hz 2500  c_room 0.1
        hp 120
    """),
    "plip": kit.value_table("""
        f_hi 2000  f_lo 1350  c_ptau 0.007  c_tau 0.015  c_h2 0.06  c_puff -14  c_puff_hz 5000
        c_puff_ms 1.0  c_room 0.2
    """),
    "droplet": kit.value_table("""
        f_hi 3200  f_lo 850  c_ptau 0.004  c_tau 0.011  c_h2 0.05  c_puff -12  c_puff_hz 6000  c_puff_ms 0.8
        c_room 0.1
    """),
}

ROUND3 = {
    "m0": kit.value_table("""
        f_hi 9800  f_lo 360  sweep 0.055  ease 0.74  steps 0.6  gdepth 0.5  step_ms 7  duty 0.72  s_att 0.3
        pre_db -3  pre_ring 5  peak_at 0.55  start_db -2.2  tail_db -22  tail_hold 0.002  tail_tau 0.0075
        harm 0.22  odd 0.2  tick_db -18  tick_hz 4000  hp 110  lp 14000  drive 2.5  fade 0.008  c_breath -15
        c_bq 3.0  land_drop 3  c_in 0.3
    """),
    "m1": kit.value_table("""
        f_hi 5200  f_lo 300  sweep 0.045  ease 1.4  steps 0  pre_db -40  peak_at 0.03  start_db -0.5
        tail_db -22  tail_hold 0.002  tail_tau 0.006  harm 0.05  odd 0  tick_db -34  tick_hz 7000  hp 260
        lp 11000  drive 1.0  fade 0.004  c_breath -13  c_bq 2.5  land_drop 2  c_in 0.3
    """),
    "m2": kit.value_table("""
        style blip  f_blip 1220  box 0.005  harm 0.5102  odd 0.06914  step_ms 7  b_att 0.0018  b_rel 0.00202
        tick_db -32.96618  tick_hz 2533.60892  hp 81.01064  lp 4820.49678  drive 5.31339  length 0.03
        fade 0.02436
    """),
    "m3": kit.value_table("""
        f_hi 1150  f_lo 140  sweep 0.06  ease 1.16  steps 0  pre_db -40  peak_at 0.1  start_db -4
        tail_db -12  tail_hold 0.003  tail_tau 0.0119  harm 0.11  odd 0.7  tick_db -22  tick_hz 6000  hp 300
        lp 13800  drive 3.0  fade 0.02  c_breath -18  c_bq 3.0  land_drop 2  c_in 0.3
    """),
    "m4": kit.value_table("""
        style blip  f_blip 1600  box 0.045  b_drop 4  harm 0.3305  odd 0  step_ms 7  b_att 0.00143
        b_rel 0.002  tick_db -7.33112  tick_hz 5329.17327  hp 78.94849  lp 13005.45302  drive 0.61736
        length 0.07  fade 0.02784
    """),
    "m5": kit.value_table("""
        f_hi 3200  f_lo 700  sweep 0.03  ease 0.8  steps 0  pre_db -40  peak_at 0.45  start_db -6
        tail_db -18  tail_hold 0.002  tail_tau 0.009  harm 0.08  odd 0  tick_db -24  tick_hz 6500  hp 200
        lp 12000  drive 1.5  fade 0.006  c_breath -17  c_bq 4.0  land_drop 1.5  c_in 0.3
    """),
}
ROUND4_EDITS = {"m0": {}, "m1": {"harm": 0.08, "odd": 0, "c_breath": -10, "c_bq": 2.0, "c_air": -20}, "m2": {},
                "m3": {}, "m4": {}, "m5": {}}

FAMILIES = {
    "pro_pop": _with_defaults(ROUND6_BASE, ROUND6),
    "pop": _with_defaults({"style": "bubble"}, BUBBLES),
}
ROUND5_FAMILY = _with_defaults(ROUND5_BASE, ROUND5)
ROUND4_FAMILY = [(name, {**V2_DEFAULTS, **ROUND3[name], **ROUND4_EDITS[name]}) for name in ROUND3]


# ------------------------------------------------------------ critic penalty

V2_TARGETS = {
    "pro_pop": [("length_s", 0.025, 0.16, 0.05), ("t40_ms", 4, 70, 25), ("low_db", -200, -10, 6),
                ("corr", 0.95, 1.0, 0.1)],
    "pop": [("length_s", 0.12, 0.42, 0.1), ("t40_ms", 25, 160, 40), ("low_db", -200, -18, 6)],
}


def _beyond(v, lo: float, hi: float, scale: float) -> float:
    if v is None:
        return 1.0
    return (lo - v) / scale if v < lo else (v - hi) / scale if v > hi else 0.0


def _lowest_share(shares) -> float:
    if isinstance(shares, dict):
        shares = list(shares.values())
    return float(shares[0])


def pro_penalty(x: np.ndarray, cat: str) -> float:
    """Profile penalty of a category, from the notify measurement. A category without targets scores 0."""
    targets = V2_TARGETS.get(cat)
    if not targets:
        return 0.0
    from hark.forge.recipes.risers import notify  # same layer; imported only when a penalty is asked for

    stats = dict(notify.measure(x))
    stats["low_db"] = 10 * math.log10(10 ** (_lowest_share(stats["shares"]) / 10) + 1e-12)
    return float(sum(min(2.0, _beyond(stats.get(span), lo, hi, since)) for span, lo, hi, since in targets))


def extra_for(cat: str, weight: float = 1.0):
    """Critic extra: the penalty times minus ``weight``."""
    return lambda sig, row=None: -weight * pro_penalty(sig, cat)


V2 = types.SimpleNamespace(
    MODULE=__name__, TARGET=V2_TARGET, TARGETS={"pro_pop": V2_TARGET, "pop": TARGET}, ALTS=V2_ALTS,
    REFSET="pro_pop", SPACE=V2_SPACE, DEFAULTS=V2_DEFAULTS, render=render_v2, FAMILIES=FAMILIES,
    family_space=family_space, extra_for=extra_for, pro_penalty=pro_penalty,
)
