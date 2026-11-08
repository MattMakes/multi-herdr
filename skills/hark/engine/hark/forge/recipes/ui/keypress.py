"""Recipe keypress: one press of a mechanical keyboard key, built from the physics of the key.

The board layout (mode layout, microphone pair, room) comes from ``key`` through a
fixed per-key generator. A press varies only force, timing and noise.
"""

from __future__ import annotations

import math

import numpy as np
from scipy import signal

from hark.forge.engine import dsp
from hark.forge.recipes.ui import _layers as L

TARGET = "a single mechanical keyboard key press with a crisp click and a short thock"
TARGETS = {"typing_key": TARGET}
ALTS = ["a wooden knock", "a computer mouse click", "a typewriter key", "a cork pop"]
REFSET = "typing_key"
REFS = [f"{L.ref_root()}/keyboard/{name}.wav"
        for name in ("kb_thock_01", "kb_thock_02", "kb_thock_03", "kb_thock_04", "kb_enter_01", "kb_space_01")]

SPACE = {
    "click_tc": (3e-5, 4e-4, "log"), "click_tau": (1.5e-4, 1e-3, "log"), "click_lvl": (0.1, 2.0, "log"),
    "click_lo": (6500, 9500, "log"), "click_tilt": (-3.0, 3.0), "crack": (0.2, 2.0, "log"), "snap": (0.0, 1.0),
    "cap_f": (900, 1800, "log"), "cap_tau": (0.0015, 0.0045, "log"), "cap_noise": (0.0, 1.0),
    "cap_n": (6, 30, "int"), "cap_cl": (0.1, 1.0, "log"), "cap_fast": (0.0005, 0.003, "log"),
    "cap_res": (0.05, 1.0, "log"), "mid_f": (110, 260, "log"), "mid_tau": (0.003, 0.016, "log"),
    "mid_lvl": (0.01, 0.2, "log"), "thock_f": (55, 90, "log"), "thock_tau": (0.045, 0.1, "log"),
    "thock_lvl": (0.05, 0.8, "log"), "thock_rise": (0.001, 0.025, "log"), "case_f": (70, 220, "log"),
    "case_lvl": (0.0, 0.8), "case_dec": (0.3, 1.0), "up_delay": (0.07, 0.125), "up_lvl": (0.08, 0.35, "log"),
    "up_pitch": (0.9, 1.5), "tick_lvl": (0.0, 1.0), "tick_dt": (0.004, 0.025, "log"),
    "pre_len": (0.004, 0.025, "log"), "pre_lvl": (0.001, 0.06, "log"), "ping_f": (7500, 11000, "log"),
    "ping_tau": (0.004, 0.03, "log"), "ping_lvl": (0.04, 0.3), "room_wet": (0.0, 0.4), "rt60": (0.05, 0.4, "log"),
    "room_size": (0.0, 1.0), "room_damp": (0.2, 0.95), "room_model": ["sparse", "diffuse"],
    "floor_db": (-78.0, -55.0), "rumble": (0.0, 0.7), "clip": (0.12, 0.4), "hpf": (36, 46, "log"),
    "key": (0, 999, "int"), "top_lvl": (0.0, 2.5), "top_lo": (1800, 4500, "log"), "top_tau": (0.0006, 0.004, "log"),
    "jacket_lvl": (0.0, 0.9), "jacket_dt": (0.003, 0.012, "log"), "pre_gap": (0.0005, 0.006, "log"),
    "width": (0.0, 0.6),
}

V1_DEFAULTS = {
    "click_tc": 7.62977e-05, "click_tau": 0.000400034, "click_lvl": 0.195527, "click_lo": 7323.18,
    "click_tilt": -1.25571, "crack": 0.395643, "snap": 0.0, "cap_f": 1489.18, "cap_tau": 0.00399449,
    "cap_noise": 1.0, "cap_n": 7, "cap_cl": 0.357156, "cap_fast": 0.000995191, "cap_res": 0.749554,
    "mid_f": 205.793, "mid_tau": 0.00918634, "mid_lvl": 0.0902506, "thock_f": 86.8567, "thock_tau": 0.0549253,
    "thock_lvl": 0.226067, "thock_rise": 0.00834742, "case_f": 70.0, "case_lvl": 0.581134, "case_dec": 0.736866,
    "up_delay": 0.124575, "up_lvl": 0.0969741, "up_pitch": 1.04402, "tick_lvl": 0.581597, "tick_dt": 0.0111911,
    "pre_len": 0.0128195, "pre_lvl": 0.0207414, "ping_f": 9083.43, "ping_tau": 0.00578368, "ping_lvl": 0.122274,
    "room_wet": 0.261726, "rt60": 0.113685, "room_size": 0.212985, "room_damp": 0.95, "room_model": "sparse",
    "floor_db": -55.0, "rumble": 0.0177183, "clip": 0.38353, "hpf": 44.5337, "key": 999,
}

DEFAULTS = dict(V1_DEFAULTS, click_lvl=0.6, thock_tau=0.05, clip=0.24, top_lvl=0.8, top_lo=2500.0, top_tau=0.0015,
                jacket_lvl=0.0, jacket_dt=0.008, pre_gap=0.002, pre_len=0.006, width=0.2)

N_CONTACT_S = 0.12


# ------------------------------------------------------------ building blocks

def key_rng(key: int, part: int) -> np.random.Generator:
    """The fixed per-key generator of one layout part."""
    return np.random.default_rng(int(key) * 7919 + part * 104729 + 17)


def ring(freqs, taus, amps, n: int, r: np.random.Generator) -> np.ndarray:
    """K1: a sum of decaying sines, each computed over its first 9 time constants."""
    y = np.zeros(n)
    for f, d, a in zip(freqs, taus, amps):
        if a == 0 or f >= 0.47 * L.SR:
            continue
        m = min(n, int(9 * d * L.SR) + 2)
        t = L.t_of(m)
        y[:m] += a * np.exp(-t / d) * np.sin(2 * np.pi * f * t + r.uniform(0, 2 * np.pi))
    return y


def cloud(f_lo: float, f_hi: float, count: int, tau_lo: float, tau_hi: float, n: int, kr: np.random.Generator,
          r: np.random.Generator, tilt: float = -3.0, detune: float = 0.002) -> np.ndarray:
    """K2: a mode cloud whose layout comes from ``kr``; detune and phases come from ``r``."""
    count = int(count)
    f = np.sort(np.exp(kr.uniform(math.log(f_lo), math.log(f_hi), count)))
    amps = 10 ** (tilt * np.log2(f / f_lo) / 20) * kr.uniform(0.3, 1.0, count)
    f = f * (1 + detune * r.standard_normal(count))
    u = np.clip(np.log(f / f_lo) / math.log(f_hi / f_lo), 0, 1)
    taus = np.exp(math.log(tau_lo) + u * (math.log(tau_hi) - math.log(tau_lo)))
    return ring(f, taus, amps, n, r)


def hiss(lo: float, hi: float, tau: float, n: int, r: np.random.Generator, order: int = 2) -> np.ndarray:
    """K3: decaying band noise, zero-padded to n."""
    m = min(n, int(9 * tau * L.SR) + 2)
    w = dsp.butter(r.standard_normal(m), "band", (lo, min(hi, 21600.0)), order) * np.exp(-L.t_of(m) / tau)
    return L.fit(np.asarray(w, dtype=float), n)


def pulse(tc: float) -> np.ndarray:
    """K4: a half-sine contact pulse of unit area."""
    k = max(1, int(round(tc * L.SR)))
    p = np.sin(np.pi * (np.arange(k) + 0.5) / k)
    return p / p.sum()


def drive(ir: np.ndarray, tc: float) -> np.ndarray:
    """K4b: the response driven by a contact pulse, cut to the response length."""
    return signal.fftconvolve(ir, pulse(tc))[: len(ir)]


def energy(x: np.ndarray, ms: float = 5.0) -> np.ndarray:
    """K6: divide by the RMS of the first ``ms`` milliseconds."""
    k = max(8, L.n_of(ms / 1000))
    return x / (L.rms(x[:k]) + 1e-12)


def crack(r: np.random.Generator, n: int, tau: float, tc: float) -> np.ndarray:
    """K7: a broadband crack driven by the contact pulse."""
    m = min(n, max(L.n_of(9 * tau), 8))
    w = dsp.butter(r.standard_normal(m) * np.exp(-L.t_of(m) / tau), "high", 1200, 1)
    return drive(L.fit(np.asarray(w, dtype=float), n), tc)


def contact(p: dict, r: np.random.Generator, n: int, part: int, pitch: float = 1.0, bright: float = 1.0):
    """K8: a single plastic impact (part 0 bottom-out, part 1 top-out)."""
    kr = key_rng(p["key"], part)
    tc = p["click_tc"] * math.exp(0.15 * r.standard_normal())
    hf = drive(L.unit(cloud(p["click_lo"], 18000, 40, 1.6 * p["click_tau"], 0.6 * p["click_tau"], n, kr, r,
                            tilt=p["click_tilt"])), tc)
    cr = L.unit(crack(r, n, 0.4 * p["click_tau"], 0.5 * tc))
    cap = _cap(p, r, n, kr, pitch, tc)
    mid = _case_body(p, r, n, kr, pitch)
    k = max(3, int(kr.uniform(0.25, 0.6) * 1e-3 * L.SR))
    snap = np.zeros(n)
    snap[: min(k, n)] = -np.hanning(k)[: min(k, n)]
    x = bright * p["click_lvl"] * energy(L.unit(hf) + p["crack"] * cr) + energy(cap) + p["mid_lvl"] * energy(mid)
    if p.get("top_lvl", 0) > 0:
        top = drive(hiss(p["top_lo"], 12000, p["top_tau"], n, r, 2), tc)
        x = x + bright * p["top_lvl"] * energy(top)
    return L.unit(L.unit(x) + p["snap"] * snap)


def _cap(p: dict, r: np.random.Generator, n: int, kr: np.random.Generator, pitch: float, tc: float) -> np.ndarray:
    cf = p["cap_f"] * pitch
    tau = p["cap_tau"]
    modes = ring([cf], [1.2 * tau], [1.0], n, r)
    modes = modes + p["cap_cl"] * L.unit(cloud(1.08 * cf, 2.3 * cf, p["cap_n"], 0.9 * tau, 0.45 * tau, n, kr, r))
    band = hiss(0.75 * cf, 2.4 * cf, 0.8 * tau, n, r)
    cap = (1 - p["cap_noise"]) * energy(modes) + p["cap_noise"] * energy(band)
    cap = cap * (p["cap_res"] + (1 - p["cap_res"]) * np.exp(-L.t_of(n) / p["cap_fast"]))
    return drive(cap, max(tc, 0.00012))


def _case_body(p: dict, r: np.random.Generator, n: int, kr: np.random.Generator, pitch: float) -> np.ndarray:
    mf = p["mid_f"] * pitch
    ratios = (1.0, kr.uniform(1.35, 1.6), kr.uniform(1.9, 2.4))
    tau = p["mid_tau"]
    return ring([mf * q for q in ratios], [tau, 0.7 * tau, 0.5 * tau], [1.0, 0.5, 0.3], n, r)


def ping(p: dict, r: np.random.Generator, n: int, pitch_k: float = 1.0) -> np.ndarray:
    """K9: the spring ping."""
    f = p["ping_f"] * pitch_k
    tau = p["ping_tau"]
    return L.unit(ring([f, 1.013 * f, 1.62 * f], [tau, 0.8 * tau, 0.4 * tau], [1.0, 0.5, 0.1], n, r))


def low_wave(p: dict, r: np.random.Generator, n: int) -> np.ndarray:
    """K10: the board and desk wave ("thock") driven by the finger push."""
    n = max(1, n)
    f = p["thock_f"] * (1 + 0.01 * r.standard_normal())
    t = L.t_of(n)
    tau = p["thock_tau"]
    ir = np.exp(-t / tau) * np.sin(2 * np.pi * f * t)
    ir = ir + p["case_lvl"] * np.exp(-t / (tau * p["case_dec"])) * np.sin(2 * np.pi * p["case_f"] * t + 0.5)
    k = max(2, L.n_of(p["thock_rise"]))
    force = 0.5 - 0.5 * np.cos(2 * np.pi * np.arange(k) / k)
    y = signal.fftconvolve(ir, force / force.sum())[:n]
    return L.unit(dsp.butter(y, "high", 0.75 * min(f, p["case_f"]), 2))


def friction(p: dict, r: np.random.Generator, n: int) -> np.ndarray:
    """K11: the friction swell of the key travel before the hit."""
    m = L.n_of(p["pre_len"])
    w = dsp.butter(r.standard_normal(m), "band", (700, 9000), 2)
    hits = r.uniform(size=m) < 0.004
    grit = np.where(hits, 4 * r.standard_normal(m), 0.0)
    grit = dsp.butter(grit, "high", 2000, 1)
    u = np.linspace(0, 1, m)
    return L.fit(L.unit((w + grit) * u ** 2), n)


def diffuse_room(x: np.ndarray, kr: np.random.Generator, p: dict) -> np.ndarray:
    """K12: a diffuse-field room of a small furnished space."""
    rt60 = p["rt60"]
    m = L.n_of(min(0.6, 1.1 * rt60 + 0.03))
    w = kr.standard_normal(m)
    t = L.t_of(m)
    ramp = 0.0015 + 0.008 * p["room_size"]
    onset = np.where(t > 0.0008, 1 - np.exp(-t / ramp), 0.0)
    bands = ((dsp.butter(w, "low", 700, 2), 1.0), (dsp.butter(w, "band", (700, 4000), 2), 0.75),
             (dsp.butter(w, "high", 4000, 2), 0.25))
    ir = np.zeros(m)
    for b, k in bands:
        rt_b = max(0.02, rt60 * (1 - p["room_damp"] * (1 - k)))
        ir += b * np.exp(-6.91 * t / rt_b)
    ir = ir * onset
    d = int(kr.uniform(0.8e-3, 2.5e-3) * L.SR)
    if d < m:
        ir[d] += 0.35 * math.sqrt(float(np.sum(ir ** 2)))
    ir = 0.6 * dsp.butter(ir, "low", 9000, 1) + 0.4 * ir
    wet = signal.fftconvolve(x, ir)[: len(x)] / (math.sqrt(float(np.sum(ir ** 2))) + 1e-12)
    return x + p["room_wet"] * wet


def spaced_pair(x: np.ndarray, p: dict) -> np.ndarray:
    """K13: a spaced microphone pair, each channel in its own small room."""
    kr = key_rng(p["key"], 9)
    chans = []
    for c in range(2):
        m = int(0.007 * L.SR)
        ir = np.zeros(m)
        ir[int(kr.uniform(0, 0.25e-3) * L.SR)] = 1.0
        refl = np.zeros(m)
        for _ in range(6):
            i = int(kr.uniform(0.3e-3, 6.5e-3) * L.SR)
            refl[min(i, m - 1)] += kr.uniform(0.03, 0.12) * kr.choice([-1.0, 1.0])
        ir = ir + dsp.butter(refl, "low", 12000, 1)
        y = signal.fftconvolve(x, ir)[: len(x)]
        chans.append(_mic_room(y, key_rng(p["key"], 10 + c), p))
    return np.column_stack(chans)


def _mic_room(y: np.ndarray, kr: np.random.Generator, p: dict) -> np.ndarray:
    if p["room_model"] == "diffuse":
        return diffuse_room(y, kr, p)
    if p["room_model"] != "sparse":
        raise ValueError(f"unknown room_model {p['room_model']!r}")
    out = dsp.room(y, kr, rt60=p["rt60"], size=p["room_size"], damp=p["room_damp"], wet=p["room_wet"])
    return np.asarray(out, dtype=float)[: len(y)]


def noise_floor(n: int, r: np.random.Generator, rumble: float, low_edge: float = 25.0) -> np.ndarray:
    """K14: the session noise floor (hiss plus a low rumble), unit standard deviation."""
    s = n / L.SR
    h = L.fit(dsp.noise(s, r, "pink") + dsp.noise(s, r, "white"), n)
    h = dsp.butter(h, "low", r.uniform(17000, 20000), 4)
    rum = dsp.butter(L.fit(dsp.noise(s, r, "brown"), n), "band", (low_edge, 120), 2)
    y = (1 - rumble) * h / (np.std(h) + 1e-12) + 2 * rumble * rum / (np.std(rum) + 1e-12)
    return y / (np.std(y) + 1e-12)


# --------------------------------------------------------------------- render

def _v1_merged(p: dict) -> dict:
    """V1 defaults under p. The v2 keys stay absent unless p has them (a v1 set renders the v1 sound)."""
    return dict(V1_DEFAULTS, **(p or {}))


def _pre_time(p: dict, r: np.random.Generator, v2: bool) -> float:
    pre = p["pre_len"] + (p["pre_gap"] * math.exp(0.2 * r.standard_normal()) if v2 else r.uniform(0.003, 0.02))
    if v2 and p["jacket_lvl"] > 0.005:
        pre += p["jacket_dt"]
    return pre


def _press(x: np.ndarray, p: dict, r: np.random.Generator, pre: float, nh: int, v2: bool) -> None:
    n = len(x)
    force = math.exp(0.1 * r.standard_normal())
    L.add_at(x, friction(p, r, n), L.n_of(max(0.0, pre - p["pre_len"])), p["pre_lvl"])
    if v2 and p["jacket_lvl"] > 0.005:
        jk = drive(L.unit(cloud(2500, 15000, 24, 0.0012, 0.0004, nh, key_rng(p["key"], 6), r, tilt=0.0)), 4e-5)
        t_j = max(0.0, pre - p["jacket_dt"] * (1 + 0.06 * r.standard_normal()))
        L.add_at(x, jk, L.n_of(t_j), p["jacket_lvl"])
    i = L.n_of(pre)
    L.add_at(x, contact(p, r, nh, 0, bright=force), i)
    L.add_at(x, ping(p, r, nh), i, p["ping_lvl"])
    L.add_at(x, low_wave(p, r, n - i), i, p["thock_lvl"] * force)


def _release(x: np.ndarray, p: dict, r: np.random.Generator, pre: float, nh: int) -> None:
    n = len(x)
    tu = pre + p["up_delay"] * (1 + 0.04 * r.standard_normal())
    if p["tick_lvl"] > 0.005:
        t1 = tu - p["tick_dt"] * (1 + 0.08 * r.standard_normal())
        kr = key_rng(p["key"], 5)
        for off, gain in ((0.0, 1.0), (r.uniform(0.003, 0.009), r.uniform(0.4, 1.0))):
            tk = drive(L.unit(cloud(3000, 14000, 14, 0.0008, 0.0003, nh, kr, r)), 5e-5)
            L.add_at(x, tk, L.n_of(max(0.0, min(t1 + off, tu - 0.001))), gain * p["tick_lvl"] * p["up_lvl"])
    iu = L.n_of(tu)
    L.add_at(x, contact(p, r, nh, 1, pitch=p["up_pitch"], bright=1.2), iu, p["up_lvl"])
    L.add_at(x, ping(p, r, nh, pitch_k=1 + 0.01 * r.standard_normal()), iu, 0.7 * p["ping_lvl"] * p["up_lvl"])
    L.add_at(x, low_wave(p, r, max(1, n - iu)), iu, 0.3 * p["thock_lvl"] * p["up_lvl"])


def _session(st: np.ndarray, p: dict, r: np.random.Generator) -> np.ndarray:
    if "width" in p:
        mid = st.mean(axis=1)
        side = (st[:, 0] - st[:, 1]) / 2
        st = np.column_stack([mid + p["width"] * side, mid - p["width"] * side])
    st = dsp.butter(st, "high", 22, 2)
    pk = L.peak(st)
    edge = p["hpf"] if "hpf" in p else 25.0
    for c in range(2):
        st[:, c] += noise_floor(len(st), r, p["rumble"], low_edge=edge) * pk * 10 ** (p["floor_db"] / 20)
    if "hpf" in p:
        st = dsp.butter(st, "high", p["hpf"], 8)
    return st


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One key press: stereo float64 at 48 kHz, about 0.13-0.45 s; the level is not normalised."""
    p = _v1_merged(p)
    v2 = "top_lvl" in p
    pre = _pre_time(p, rng, v2)
    dur = pre + p["clip"] * (1 + 0.05 * rng.standard_normal())
    n = L.n_of(dur + 0.05)
    nh = L.n_of(N_CONTACT_S)
    x = np.zeros(n)
    _press(x, p, rng, pre, nh, v2)
    _release(x, p, rng, pre, nh)
    st = _session(spaced_pair(x, p), p, rng)
    st = st[: L.n_of(dur)]
    st = dsp.fade(st, 0.002, rng.uniform(0.025, 0.05))
    return np.asarray(L.headroom(st), dtype=np.float64)


# ---------------------------------------------------------------- v2 checks

def key_stats(x: np.ndarray) -> dict:
    """Onset (2-12 kHz), top share in dB and time to -40 dB of a key render."""
    m = x.mean(axis=1) if x.ndim == 2 else np.asarray(x, dtype=float)
    hb = dsp.butter(m, "band", (2000, 12000), 4)
    e1 = _frames_db(hb, 48)
    ip = int(np.argmax(e1))
    within = np.nonzero(e1[: ip + 1] >= e1[ip] - 20)[0]
    pw = np.abs(np.fft.rfft(m)) ** 2
    f = np.fft.rfftfreq(len(m), 1 / L.SR)
    top = float(pw[(f >= 2000) & (f < 12000)].sum() / (pw.sum() + 1e-30))
    e5 = _frames_db(m, 240)
    j = int(np.argmax(e5))
    below = np.nonzero(e5[j:] <= e5[j] - 40)[0]
    t40 = float(below[0] * 5) if len(below) else float((len(e5) - j) * 5)
    return {"onset_ms": float(ip - within[0]), "top_db": round(10 * math.log10(top + 1e-14), 1), "t40_ms": t40}


def _frames_db(x: np.ndarray, hop: int) -> np.ndarray:
    k = max(1, len(x) // hop)
    fr = np.square(x[: k * hop]).reshape(k, hop).mean(axis=1)
    return 10 * np.log10(fr + 1e-14)


def key_penalty(x: np.ndarray) -> float:
    """The v2 key extra term, 0 to -1.5: a bright enough top and a short enough tail."""
    s = key_stats(x)
    return -0.75 * (min(1.0, max(0.0, -5 - s["top_db"]) / 6) + min(1.0, max(0.0, s["t40_ms"] - 200) / 100))


def extra_for(cat: str, weight: float = 1.0):
    """The critic extra function of a category, or None (only ``typing_key`` has one)."""
    if cat != "typing_key":
        return None
    return lambda x, row=None: weight * key_penalty(x)
