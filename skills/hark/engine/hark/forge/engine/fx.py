"""Effects for forge recipes: reverb, dynamics, EQ match, motion, glitch, grains.

Randomness comes only from an explicit generator. Mono input stays mono unless
the function says it returns stereo.
"""

from __future__ import annotations

import threading
from collections import OrderedDict
from typing import Any

import numpy as np
from scipy import ndimage, signal

from hark.forge.engine import dsp
from hark.forge.engine.args import bind, with_signature
from hark.forge.engine.dsp import SR, n_of, t_of

IR_CACHE_MB = 64

_IR_STORE: OrderedDict = OrderedDict()
_STORE_GUARD = threading.Lock()
_STORE_COUNTS = {"hits": 0, "misses": 0}


# ---------------------------------------------------------------- helpers

def _level_rms(sig: np.ndarray) -> float:
    return float(np.sqrt(np.mean(np.square(sig)))) + 1e-12


def _downmix(sig: np.ndarray) -> np.ndarray:
    sig = np.asarray(sig, dtype=float)
    return sig.mean(axis=1) if sig.ndim == 2 else sig


def _schroeder_ap(sig: np.ndarray, dly: int, gn: float) -> np.ndarray:
    """Schroeder all-pass of delay d and gain g (exact LTI form)."""
    bv = np.zeros(dly + 1)
    av = np.zeros(dly + 1)
    bv[0], bv[dly] = -gn, 1.0
    av[0], av[dly] = 1.0, -gn
    return signal.lfilter(bv, av, sig, axis=0)


def _shelf_coefs(kind_s: str, gain_v: float, corner: float) -> tuple[float, float, float]:
    """First-order shelf as (b0, b1, a1) for y = b0 x + b1 x1 - a1 y1."""
    cnt = np.tan(np.pi * float(np.clip(corner, 20, 0.45 * SR)) / SR)
    sv = np.sqrt(gain_v)
    if kind_s == "high":
        c_b0, c_b1, tap_a, tap_b = sv + cnt, cnt - sv, 1 / sv + cnt, cnt - 1 / sv
    else:
        c_b0, c_b1, tap_a, tap_b = 1 + cnt * sv, cnt * sv - 1, 1 + cnt / sv, cnt / sv - 1
    return c_b0 / tap_a, c_b1 / tap_a, tap_b / tap_a


def _cubic_read(sig: np.ndarray, where: np.ndarray) -> np.ndarray:
    """Cubic Hermite read at fractional positions; zero outside x."""
    sig = np.asarray(sig, dtype=float)
    padn = np.zeros((2,) + sig.shape[1:])
    xpad = np.concatenate([padn, sig, padn, padn])
    ix = np.floor(where).astype(int)
    frac = where - ix
    good = (ix >= -1) & (ix <= len(sig))
    jx = np.clip(ix, -1, len(sig)) + 2
    mono_in, xa, xb, xc = xpad[jx - 1], xpad[jx], xpad[jx + 1], xpad[jx + 2]
    if sig.ndim == 2:
        frac = frac[:, None]
        good = good[:, None]
    cf1 = 0.5 * (xb - mono_in)
    cf2 = mono_in - 2.5 * xa + 2 * xb - 0.5 * xc
    cf3 = 0.5 * (xc - mono_in) + 1.5 * (xa - xb)
    return np.where(good, ((cf3 * frac + cf2) * frac + cf1) * frac + xa, 0.0)


def _smoothing_k(msec: float) -> float:
    return float(np.exp(-1.0 / max(1.0, msec * 1e-3 * SR)))


def _env_follower(sig: np.ndarray, att_k: float, rel_k: float) -> np.ndarray:
    """Peak follower with separate attack and release coefficients."""
    acc = np.empty(len(sig))
    ev = 0.0
    for ix, vec in enumerate(sig.tolist()):
        av = att_k if vec > ev else rel_k
        ev = av * ev + (1 - av) * vec
        acc[ix] = ev
    return acc


def _soft_edges(sig: np.ndarray, cnt: int) -> np.ndarray:
    sig = np.array(sig, dtype=float, copy=True)
    cnt = int(min(cnt, len(sig) // 2))
    if cnt > 1:
        wt = 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, cnt))
        wt = wt if sig.ndim == 1 else wt[:, None]
        sig[:cnt] *= wt
        sig[len(sig) - cnt:] *= wt[::-1]
    return sig


def _prime_list(upto: int) -> np.ndarray:
    is_prime = np.ones(upto + 1, dtype=bool)
    is_prime[:2] = False
    for cfg in range(2, int(upto ** 0.5) + 1):
        if is_prime[cfg]:
            is_prime[cfg * cfg::cfg] = False
    return np.nonzero(is_prime)[0]


# ---------------------------------------------------------------- IR cache

def ir_cache_stats() -> dict:
    """Hits, misses, entries and size in MB of the reverb IR cache."""
    with _STORE_GUARD:
        mbytes = sum(vec[0].nbytes for vec in _IR_STORE.values()) / 2 ** 20
        return {"hits": _STORE_COUNTS["hits"], "misses": _STORE_COUNTS["misses"], "entries": len(_IR_STORE), "mb": round(mbytes, 1)}


def ir_cache_clear() -> None:
    """Empty the IR cache and zero its counters."""
    with _STORE_GUARD:
        _IR_STORE.clear()
        _STORE_COUNTS["hits"] = _STORE_COUNTS["misses"] = 0


def _store_fetch(cache_key: Any) -> Any:
    with _STORE_GUARD:
        if cache_key in _IR_STORE:
            _IR_STORE.move_to_end(cache_key)
            _STORE_COUNTS["hits"] += 1
            return _IR_STORE[cache_key]
        _STORE_COUNTS["misses"] += 1
        return None


def _store_save(cache_key: Any, resp: np.ndarray, st_v: Any) -> None:
    with _STORE_GUARD:
        _IR_STORE[cache_key] = (resp.copy(), st_v)
        cap_b = IR_CACHE_MB * 2 ** 20
        while len(_IR_STORE) > 1 and sum(vec[0].nbytes for vec in _IR_STORE.values()) > cap_b:
            _IR_STORE.popitem(last=False)


def _snap_log(vec: Any, stp: float) -> Any:
    if vec is None or vec <= 0:
        return vec
    return float(2 ** (round(np.log2(vec) / stp) * stp))


def _snap_lin(vec: float, stp: float = 0.05) -> float:
    return round(round(vec / stp) * stp, 4)


def ir_grid(rt60: float = 1.6, size: float = 0.6, predelay: float = 0.01, damp: float = 0.5, bass: float = 1.2,
            mod: float = 1.0, diffuse: float = 0.7, width: float = 1.0, tail: float | None = None,
            lowcut: float = 0.0) -> dict:
    """Snap room settings to a grid so a search keeps hitting the IR cache."""
    acc = {"rt60": _snap_log(rt60, 1 / 24), "tail": _snap_log(tail, 1 / 24), "lowcut": _snap_log(lowcut, 1 / 12),
           "predelay": round(round(predelay / 0.0005) * 0.0005, 5)}
    for cnt, vec in (("size", size), ("damp", damp), ("bass", bass), ("mod", mod), ("diffuse", diffuse),
                 ("width", width)):
        acc[cnt] = _snap_lin(vec)
    return acc


# ---------------------------------------------------------------- early reflections

def _reflection_taps(gen: np.random.Generator, mm: int, t_top: float, n_taps: int, spread_w: float, chan: int) -> np.ndarray:
    tsec = np.sort(np.exp(gen.uniform(np.log(0.003), np.log(t_top), n_taps)))
    if chan == 1:
        tsec = tsec * (1 + spread_w * gen.uniform(-0.12, 0.12, n_taps))
    sgn = gen.choice([-1.0, 1.0], n_taps)
    resp = np.zeros(mm)
    for tm, sv in zip(tsec, sgn):
        resp[min(mm - 1, n_of(tm))] += sv * 0.003 / tm
    return resp


def _tilt_dark(resp: np.ndarray, damping: float) -> np.ndarray:
    mm = len(resp)
    edge_set = np.linspace(0, mm, 4).astype(int)
    acc = np.zeros(mm)
    for jx in range(3):
        piece = np.zeros(mm)
        piece[edge_set[jx]:edge_set[jx + 1]] = resp[edge_set[jx]:edge_set[jx + 1]]
        acc += dsp.butter(piece, "low", 14000 - damping * 4000 * (jx + 1), 1)
    return acc / (np.sqrt(np.sum(acc * acc)) + 1e-12)


def early(x: np.ndarray, r: np.random.Generator, size: float = 0.5, taps: int = 18, damp: float = 0.5,
          width: float = 1.0) -> np.ndarray:
    """Stereo early reflections of the mono mix of x."""
    mono_in = _downmix(x)
    t_top = 0.006 + 0.05 * size
    mm = n_of(t_top) + 2
    n_ch = [_tilt_dark(_reflection_taps(r, mm, t_top, taps, width, chan), damp) for chan in range(2)]
    return np.column_stack([signal.fftconvolve(mono_in, ch) for ch in n_ch])


# ---------------------------------------------------------------- FDN reverb

def _line_lengths(gen: np.random.Generator, room_size: float, n_lines: int) -> np.ndarray:
    lo_c = (0.006 + 0.024 * room_size) * SR
    goals = np.geomspace(lo_c, 2.7 * lo_c, n_lines) * np.exp(gen.uniform(-0.06, 0.06, n_lines))
    pool_v = list(_prime_list(int(3.2 * lo_c) + 50))
    acc = []
    for goal_i in goals:
        cfg = min(pool_v, key=lambda qv: abs(qv - goal_i))
        pool_v.remove(cfg)
        acc.append(cfg)
    return gen.permutation(np.array(acc, dtype=float))


def _io_weights(gen: np.random.Generator, n_lines: int) -> tuple[np.ndarray, np.ndarray]:
    bv = gen.choice([-1.0, 1.0], n_lines) / np.sqrt(n_lines)
    out_mix = gen.choice([-1.0, 1.0], (n_lines, 2))
    if abs(out_mix[:, 0] @ out_mix[:, 1]) >= n_lines:
        out_mix[n_lines // 2:, 1] *= -1
    out_mix[:, 1] -= out_mix[:, 0] * (out_mix[:, 0] @ out_mix[:, 1]) / n_lines
    return bv, out_mix / np.linalg.norm(out_mix, axis=0)


def _fwht(vec: np.ndarray) -> np.ndarray:
    vec = vec.copy()
    n_lines = vec.shape[-1]
    hh_ = 1
    while hh_ < n_lines:
        for ix in range(0, n_lines, 2 * hh_):
            cfg = vec[..., ix:ix + hh_].copy()
            qv = vec[..., ix + hh_:ix + 2 * hh_].copy()
            vec[..., ix:ix + hh_] = cfg + qv
            vec[..., ix + hh_:ix + 2 * hh_] = cfg - qv
        hh_ *= 2
    return vec / np.sqrt(n_lines)


class _DelayNet:
    """Feedback delay network state for one input channel."""

    def __init__(self, lens: np.ndarray, mod_depth: np.ndarray, lfo_hz: np.ndarray, ph: np.ndarray,
                 lo_c: tuple, hi_c: tuple, bv: np.ndarray, out_mix: np.ndarray):
        self.dl, self.depth, self.rate, self.phase = lens, mod_depth, lfo_hz, ph
        self.lo, self.hi, self.b, self.cm = lo_c, hi_c, bv, out_mix
        n_lines = len(lens)
        self.size = 1 << int(np.ceil(np.log2(int(lens.max() + mod_depth.max() + 8))))
        self.buf = np.zeros((n_lines, self.size))
        self.ya = np.zeros(n_lines)
        self.zl = np.zeros(n_lines)
        self.zh = np.zeros(n_lines)
        self.block = max(1, int(lens.min() - mod_depth.max()) - 3)

    def _tap_lines(self, at0: int, cnt: int) -> np.ndarray:
        ix = np.arange(at0, at0 + cnt)[:, None]
        dly = self.dl + self.depth * np.sin(2 * np.pi * self.rate * ix / SR + self.phase)
        mm = np.floor(dly - 0.5).astype(int)
        frac_c = (1 - (dly - mm)) / (1 + (dly - mm))
        row_ix = np.arange(len(self.dl))[None, :]
        tap_a = self.buf[row_ix, (ix - mm) % self.size]
        tap_b = self.buf[row_ix, (ix - mm - 1) % self.size]
        res = np.empty_like(tap_a)
        ap_state = self.ya
        for jx in range(cnt):
            ap_state = frac_c[jx] * tap_a[jx] + tap_b[jx] - frac_c[jx] * ap_state
            res[jx] = ap_state
        self.ya = ap_state
        return res

    def _damp_lines(self, res: np.ndarray) -> np.ndarray:
        acc = np.empty_like(res)
        for cnt in range(res.shape[1]):
            lo_b0, lo_b1, lo_a1 = self.lo[0][cnt], self.lo[1][cnt], self.lo[2]
            hi_b0, hi_b1, hi_a1 = self.hi
            tm, z_lo = signal.lfilter([lo_b0, lo_b1], [1.0, lo_a1], res[:, cnt], zi=[self.zl[cnt]])
            uu, z_hi = signal.lfilter([hi_b0, hi_b1], [1.0, hi_a1], tm, zi=[self.zh[cnt]])
            self.zl[cnt], self.zh[cnt] = z_lo[0], z_hi[0]
            acc[:, cnt] = uu
        return acc

    def run(self, x: np.ndarray, n: int) -> np.ndarray:
        acc = np.zeros((n, 2))
        at0 = 0
        while at0 < n:
            cnt = min(self.block, n - at0)
            vec = self._damp_lines(self._tap_lines(at0, cnt))
            acc[at0:at0 + cnt] = vec @ self.cm
            mixd = _fwht(vec)
            feed = np.zeros(cnt)
            chunk = x[at0:at0 + cnt]
            feed[: len(chunk)] = chunk
            ids = np.arange(at0, at0 + cnt) % self.size
            self.buf[:, ids] = (mixd + feed[:, None] * self.b[None, :]).T
            at0 += cnt
        return acc


def _loop_gain(lens: np.ndarray, t_dec: float) -> np.ndarray:
    return 10 ** (-3 * lens / (max(t_dec, 0.02) * SR))


def _damping_shelves(lens: np.ndarray, t60: float, t60_lo: float, t60_hi: float, lo_hz: float, hi_hz: float):
    g_mid, g_lo, g_hi = _loop_gain(lens, t60), _loop_gain(lens, t60_lo), _loop_gain(lens, t60_hi)
    lo_set = [_shelf_coefs("low", glo_i / gmid_i, lo_hz) for glo_i, gmid_i in zip(g_lo, g_mid)]
    lo_b0 = np.array([sv[0] for sv in lo_set]) * g_mid
    lo_b1 = np.array([sv[1] for sv in lo_set]) * g_mid
    lo_a1 = lo_set[0][2] if len(set(round(sv[2], 15) for sv in lo_set)) == 1 else None
    hi_set = [_shelf_coefs("high", ghi_i / gmid_i, hi_hz) for ghi_i, gmid_i in zip(g_hi, g_mid)]
    return g_mid, lo_set, hi_set, lo_b0, lo_b1, lo_a1


class _RoomPlan:
    """Derived settings of one IR build."""

    def __init__(self, cfg: dict):
        self.p = cfg
        self.rt_lo = cfg["rt60"] * cfg["bass"]
        self.rt_hi = cfg["rt60"] * max(0.05, 1 - 0.85 * cfg["damp"])
        self.tail = cfg["tail"] if cfg["tail"] is not None else 1.2 * max(cfg["rt60"], self.rt_lo)
        self.pd = n_of(cfg["predelay"]) if cfg["predelay"] > 0 else 0
        self.n = max(n_of(self.tail), 64)


def _decay_curve(len_k: float, lo_sh: tuple, g_mid_k: float, hi_sh: tuple) -> tuple[np.ndarray, np.ndarray]:
    freq = np.geomspace(20.0, 0.999 * SR / 2, 64)
    wt = 2 * np.pi * freq / SR
    _, resp_lo = signal.freqz([lo_sh[0] * g_mid_k, lo_sh[1] * g_mid_k], [1.0, lo_sh[2]], worN=wt)
    _, resp_hi = signal.freqz([hi_sh[0], hi_sh[1]], [1.0, hi_sh[2]], worN=wt)
    hh_ = np.minimum(np.abs(resp_lo * resp_hi), 0.999999)
    return freq, -3 * len_k / (SR * np.log10(hh_))


def _spectral_decay(sig: np.ndarray, total_n: int, fall_db: Any) -> np.ndarray:
    f_ax, tvec, zz = signal.stft(sig, fs=SR, nperseg=1024, noverlap=768, boundary="even")
    zz = zz * fall_db(f_ax[:, None], tvec[None, :])
    _, res = signal.istft(zz, fs=SR, nperseg=1024, noverlap=768, boundary=True)
    acc = np.zeros(total_n)
    acc[: min(total_n, len(res))] = res[:total_n]
    return acc


def _noise_tail(gen: np.random.Generator, total_n: int, freq: np.ndarray, t_dec: np.ndarray, spread_w: float) -> np.ndarray:
    def decay_v(fq: np.ndarray, tvec: np.ndarray) -> np.ndarray:
        return np.exp(-6.91 * tvec / np.interp(fq, freq, t_dec))

    mm = _spectral_decay(gen.standard_normal(total_n + 1024), total_n, decay_v)
    sdev = dsp.butter(_spectral_decay(gen.standard_normal(total_n + 1024), total_n, decay_v), "high", 200.0, 2)
    lr_v = np.column_stack([mm + spread_w * sdev, mm - spread_w * sdev]) / np.sqrt(1 + 0.9 * spread_w ** 2)
    return lr_v * np.sqrt(13.8 / (np.median(t_dec) * SR))


def _air_loss(resp: np.ndarray, total_n: int, air_v: float) -> np.ndarray:
    def decay_v(fq: np.ndarray, tvec: np.ndarray) -> np.ndarray:
        lfo_hz = air_v * 0.17 * 343 * (fq / 10000) ** 2
        return 10 ** (-lfo_hz * np.maximum(tvec - 512 / SR, 0) / 20)

    return np.column_stack([_spectral_decay(resp[:, chan], total_n, decay_v) for chan in range(resp.shape[1])])


def _diffusion_kernel(total_n: int, room_size: float, diff_v: float, chan: int) -> np.ndarray:
    sig = np.zeros(total_n)
    sig[0] = 1.0
    if diff_v > 0:
        for jx, msec in enumerate([1.1, 1.7, 2.9, 4.3]):
            sig = _schroeder_ap(sig, int(msec * (0.6 + 0.8 * room_size) * 1e-3 * SR) + 3 * chan + jx, min(0.8, diff_v))
    return sig


def _make_net(gen: np.random.Generator, cfg: dict, lens: np.ndarray, shelf_set: tuple) -> "_DelayNet":
    """The feedback delay network of one input; draws taps, then depth, rate and phase from r."""
    n_lines = len(lens)
    lo_set, hi_set, lo_b0, lo_b1 = shelf_set[1], shelf_set[2], shelf_set[3], shelf_set[4]
    bv, out_mix = _io_weights(gen, n_lines)
    mod_depth = cfg["mod"] * (3 + 9 * gen.uniform(0, 1, n_lines)) * (0.5 + 0.5 * cfg["size"])
    lfo_hz = gen.uniform(0.15, 1.1, n_lines)
    ph = gen.uniform(0, 2 * np.pi, n_lines)
    return _DelayNet(lens, mod_depth, lfo_hz, ph, (lo_b0, lo_b1, lo_set[0][2]), hi_set[0], bv, out_mix)


def _mix_reflections(res: np.ndarray, gen: np.random.Generator, cfg: dict, total_n: int) -> np.ndarray:
    if not (cfg["early_db"] is not None and cfg["early_db"] > -90):
        return res
    ev = 10 ** (cfg["early_db"] / 10)
    impulse = np.array([1.0])
    refl = early(impulse, gen, cfg["size"], damp=cfg["damp"], width=cfg["width"])[:total_n]
    res = res / np.sqrt(1 + ev)
    res[: len(refl)] += refl * np.sqrt(ev / (1 + ev))
    return res


def _side_scale(res: np.ndarray, wt: float) -> np.ndarray:
    mid_s, sidesig = (res[:, 0] + res[:, 1]) / 2, (res[:, 0] - res[:, 1]) / 2
    return np.column_stack([mid_s + wt * sidesig, mid_s - wt * sidesig])


def _net_tail(gen: np.random.Generator, plan: _RoomPlan, lens: np.ndarray, shelf_set: tuple, chan: int) -> np.ndarray:
    cfg, total_n = plan.p, plan.n
    g_mid = shelf_set[0]
    sig = _diffusion_kernel(total_n, cfg["size"], cfg["diffuse"], chan)
    net = _make_net(gen, cfg, lens, shelf_set)
    res = net.run(sig, total_n) / np.sqrt(np.mean(g_mid ** 2 / (1 - g_mid ** 2)) / len(lens))
    if cfg["late"] == "noise":
        res = _crossed_tail(gen, plan, res, lens, shelf_set)
    res = _mix_reflections(res, gen, cfg, total_n)
    return _side_scale(res, cfg["width"])


def _crossed_tail(gen: np.random.Generator, plan: _RoomPlan, res: np.ndarray, lens: np.ndarray, shelf_set: tuple) -> np.ndarray:
    g_mid, lo_set, hi_set = shelf_set[0], shelf_set[1], shelf_set[2]
    cnt = int(np.argmin(np.abs(lens - np.median(lens))))
    freq, t_dec = _decay_curve(lens[cnt], lo_set[cnt], g_mid[cnt], hi_set[cnt])
    late_s = _noise_tail(gen, plan.n, freq, t_dec, 1.0)
    txs = 0.06 + 0.1 * plan.p["size"]
    uu = np.clip((t_of(plan.n) - 0.5 * txs) / txs, 0, 1)[:, None]
    return res * np.cos(uu * np.pi / 2) + late_s * np.sin(uu * np.pi / 2)


def _synth_ir(gen: np.random.Generator, cfg: dict) -> np.ndarray:
    assert cfg["lines"] in (4, 8, 16, 32), "lines must be 4, 8, 16 or 32"
    plan = _RoomPlan(cfg)
    lens = _line_lengths(gen, cfg["size"], cfg["lines"])
    shelf_set = _damping_shelves(lens, cfg["rt60"], plan.rt_lo, plan.rt_hi, cfg["lf_hz"], cfg["hf_hz"])
    resp = np.stack([_net_tail(gen, plan, lens, shelf_set, chan) for chan in range(cfg["inputs"])], axis=1)
    resp = np.stack([_polish_ir(resp[:, chan, :], plan, cfg) for chan in range(cfg["inputs"])], axis=1)
    return np.concatenate([np.zeros((plan.pd,) + resp.shape[1:]), resp])


def _polish_ir(res: np.ndarray, plan: _RoomPlan, cfg: dict) -> np.ndarray:
    if cfg["air"] > 0:
        res = _air_loss(res, plan.n, cfg["air"])
    if cfg["lowcut"] > 0:
        res = dsp.butter(res, "high", cfg["lowcut"], 2)
    cnt = min(plan.n, n_of(min(0.1, 0.2 * plan.tail)))
    res = res.copy()
    res[plan.n - cnt:] *= ((1 - np.linspace(0, 1, cnt)) ** 2)[:, None]
    return res


_IR_FIELDS = ("rt60", "size", "predelay", "damp", "bass", "mod", "diffuse", "early_db", "width", "late", "lines",
            "tail", "inputs", "hf_hz", "lf_hz", "air", "lowcut")


def _typed_ir_settings(av: dict) -> dict:
    """The settings of reverb_ir's arguments, typed, in _IR_KEYS order."""
    return dict(rt60=float(av["rt60"]), size=float(av["size"]), predelay=float(av["predelay"]), damp=float(av["damp"]),
                bass=float(av["bass"]), mod=float(av["mod"]), diffuse=float(av["diffuse"]),
                early_db=None if av["early_db"] is None else float(av["early_db"]), width=float(av["width"]),
                late=str(av["late"]), lines=int(av["lines"]), tail=None if av["tail"] is None else float(av["tail"]),
                inputs=int(av["inputs"]), hf_hz=float(av["hf_hz"]), lf_hz=float(av["lf_hz"]), air=float(av["air"]),
                lowcut=float(av["lowcut"]))


IR_DEFAULTS: dict[str, Any] = dict(
    rt60=1.6, size=0.6, predelay=0.01, damp=0.5, bass=1.2, mod=1.0, diffuse=0.7, early_db=-6.0, width=1.0,
    late="noise", lines=16, tail=None, inputs=1, hf_hz=4000.0, lf_hz=250.0, air=1.0, lowcut=0.0, seed=None,
    cache=True)


def reverb_ir(r: np.random.Generator, *args: Any, **opts: Any) -> np.ndarray:
    """Reverb impulse response of shape (n, inputs, 2), cached per settings and generator state.

    Settings and defaults are IR_DEFAULTS, in signature order.
    """
    av = bind("reverb_ir", IR_DEFAULTS, args, opts)
    cfg = _typed_ir_settings(av)
    key_tuple = tuple(cfg[cnt] for cnt in _IR_FIELDS)
    if av["seed"] is not None:
        resp = _ir_by_seed(int(av["seed"]), cfg, key_tuple)
    elif av["cache"] and r is not None and hasattr(r, "bit_generator"):
        resp = _ir_by_state(r, cfg, key_tuple)
    else:
        resp = _synth_ir(r, cfg)
    return resp


with_signature(reverb_ir, ("r",), IR_DEFAULTS)


def _ir_by_seed(seed_v: int, cfg: dict, key_tuple: tuple) -> np.ndarray:
    cache_key = ("seed", seed_v, key_tuple)
    found = _store_fetch(cache_key)
    if found is not None:
        return found[0].copy()
    resp = _synth_ir(dsp.rng(seed_v), cfg)
    _store_save(cache_key, resp, None)
    return resp


def _ir_by_state(gen: np.random.Generator, cfg: dict, key_tuple: tuple) -> np.ndarray:
    cache_key = ("state", repr(gen.bit_generator.state), key_tuple)
    found = _store_fetch(cache_key)
    if found is not None:
        gen.bit_generator.state = found[1]
        return found[0].copy()
    resp = _synth_ir(gen, cfg)
    _store_save(cache_key, resp, gen.bit_generator.state)
    return resp


_SPACE_FIELDS = ("rt60", "size", "predelay", "damp", "bass", "mod", "diffuse", "width", "tail", "lowcut")


def _ir_for_reverb(av: dict, n_in: int) -> np.ndarray:
    space = {cnt: av[cnt] for cnt in _SPACE_FIELDS}
    if av["grid"]:
        space = ir_grid(**space)
    return reverb_ir(av["r"], early_db=av["early_db"], late=av["late"], lines=av["lines"], inputs=n_in, air=av["air"],
                     seed=av["ir_seed"], **space)


def _blend_wet(sig: np.ndarray, cols: np.ndarray, wet_sig: np.ndarray, wet_v: float, dry_v: float, two_ch: bool) -> np.ndarray:
    if not two_ch:
        acc = wet_v * wet_sig.mean(axis=1) * np.sqrt(2)
        acc[: len(sig)] += dry_v * cols.mean(axis=1)
        return acc
    acc = wet_sig * wet_v
    acc[: len(sig)] += dry_v * (cols if cols.shape[1] == 2 else np.column_stack([cols[:, 0], cols[:, 0]]))
    return acc


REVERB_DEFAULTS: dict[str, Any] = dict(
    rt60=1.6, size=0.6, predelay=0.01, damp=0.5, bass=1.2, mod=1.0, diffuse=0.7, early_db=-6.0, width=1.0, wet=0.3,
    dry=1.0, late="noise", lines=16, tail=None, stereo=None, air=1.0, lowcut=0.0, ir_seed=None, grid=False)


def reverb(x: np.ndarray, r: np.random.Generator, *args: Any, **opts: Any) -> np.ndarray:
    """FDN reverb; wet is the level of the unit-energy tail relative to the dry sound.

    Settings and defaults are REVERB_DEFAULTS, in signature order.
    """
    av = {"r": r, **bind("reverb", REVERB_DEFAULTS, args, opts)}
    x = np.asarray(x, dtype=float)
    cols = x[:, None] if x.ndim == 1 else x
    resp = _ir_for_reverb(av, cols.shape[1])
    wet_sig = sum(np.column_stack([signal.fftconvolve(cols[:, chan], resp[:, chan, oc]) for oc in range(2)])
                 for chan in range(cols.shape[1]))
    return _blend_wet(x, cols, wet_sig, av["wet"], av["dry"], x.ndim == 2 if av["stereo"] is None else av["stereo"])


with_signature(reverb, ("x", "r"), REVERB_DEFAULTS)


# ---------------------------------------------------------------- dynamics

def _apply_shelf(sig: np.ndarray, kind_s: str, db_v: float, corner: float) -> np.ndarray:
    c_b0, c_b1, tap_b = _shelf_coefs(kind_s, 10 ** (db_v / 20), corner)
    return signal.lfilter([c_b0, c_b1], [1.0, tap_b], sig, axis=0)


def _shape_tape(uu: np.ndarray, gn: float, offset: float, over: int) -> np.ndarray:
    uu = _apply_shelf(uu, "high", 4.0, 3000.0 / over)
    bv2 = 0.12 + offset
    uu = np.tanh(gn * uu + bv2) - np.tanh(bv2)
    return _apply_shelf(uu, "high", -4.0, 3000.0 / over)


def _shape_clip(uu: np.ndarray, gn: float, offset: float, over: int) -> np.ndarray:
    return np.clip(gn * uu + offset, -1, 1) - np.clip(offset, -1, 1)


def _shape_fold(uu: np.ndarray, gn: float, offset: float, over: int) -> np.ndarray:
    return np.sin(np.pi / 2 * (gn * uu + offset)) - np.sin(np.pi / 2 * offset)


def _shape_tanh(uu: np.ndarray, gn: float, offset: float, over: int) -> np.ndarray:
    return np.tanh(gn * uu + offset) - np.tanh(offset)


_SHAPERS = {"tape": _shape_tape, "hard": _shape_clip, "fold": _shape_fold}


def _shape(uu: np.ndarray, kind_m: str, gn: float, offset: float, over: int) -> np.ndarray:
    return _SHAPERS.get(kind_m, _shape_tanh)(uu, gn, offset, over)


def _relevel(res: np.ndarray, sig: np.ndarray, lvl_mode: str | None) -> np.ndarray:
    if lvl_mode == "rms":
        return res * (_level_rms(sig) - 1e-12) / (_level_rms(res) - 1e-12 + 1e-30)
    if lvl_mode == "peak":
        return res * np.max(np.abs(sig)) / (np.max(np.abs(res)) + 1e-12)
    return res


def saturate(x: np.ndarray, drive_db: float = 6.0, mode: str = "tanh", mix: float = 1.0, bias: float = 0.0,
             os: int | None = None, level: str | None = "rms") -> np.ndarray:
    """Oversampled waveshaper (tanh, tape, hard, fold)."""
    x = np.asarray(x, dtype=float)
    gn = 10 ** (drive_db / 20)
    over = os if os is not None else (8 if mode in ("hard", "fold") else 4)
    window = ("kaiser", 9.0)
    uu = signal.resample_poly(x, over, 1, axis=0, window=window) if over > 1 else x
    res = _shape(uu, mode, gn, bias, over)
    res = signal.resample_poly(res, 1, over, axis=0, window=window)[: len(x)] if over > 1 else res
    if mode == "tape":
        res = dsp.butter(res, "low", 17000.0, 1)
    if bias != 0 or mode == "tape":
        res = dsp.butter(res, "high", 8.0, 1)
    res = _relevel(res, x, level)
    return mix * res + (1 - mix) * x


def _lagged_rise(env_f: np.ndarray, att_k: float) -> np.ndarray:
    acc = np.empty(len(env_f))
    sv = 0.0
    for ix, vec in enumerate(env_f.tolist()):
        sv = min(att_k * sv + (1 - att_k) * vec, vec)
        acc[ix] = sv
    return acc


def _ratio_db(av: np.ndarray, bv: np.ndarray, scl: float) -> np.ndarray:
    return np.clip(20 * np.log10((av + 1e-9) / (bv + 1e-9)) / scl, 0, 1)


def _attack_part(mm: np.ndarray, sv: float, att_ms: float) -> np.ndarray:
    env_f = _env_follower(mm, _smoothing_k(0.1 * sv), _smoothing_k(15 * sv))
    ess = _lagged_rise(env_f, _smoothing_k(att_ms * sv))
    return _ratio_db(env_f, ess, 6.0)


def _sustain_part(mm: np.ndarray, sv: float, av: np.ndarray, bottom_db: float) -> tuple[np.ndarray, np.ndarray]:
    elen = _env_follower(mm, _smoothing_k(0.3 * sv), _smoothing_k(350 * sv))
    eq_v = _env_follower(mm, _smoothing_k(0.3 * sv), _smoothing_k(25 * sv))
    dly = _ratio_db(elen, eq_v, 9.0) * (1 - av)
    gate_v = np.clip((20 * np.log10(elen / (mm.max() + 1e-12) + 1e-12) - bottom_db) / 12, 0, 1)
    return dly, gate_v


def _gain_smooth(gain_v: np.ndarray) -> np.ndarray:
    lo_c = gain_v.min() if len(gain_v) else 0.0
    return _env_follower(gain_v - lo_c, _smoothing_k(0.2), _smoothing_k(5)) + lo_c


def transient(x: np.ndarray, attack_db: float = 3.0, sustain_db: float = 0.0, attack_ms: float = 8.0,
              speed: float = 1.0, floor_db: float = -60.0) -> np.ndarray:
    """Level-independent attack and sustain shaper, stereo linked."""
    x = np.asarray(x, dtype=float)
    mm = np.abs(x).max(axis=1) if x.ndim == 2 else np.abs(x)
    av = _attack_part(mm, speed, attack_ms)
    dly, gate_v = _sustain_part(mm, speed, av, floor_db)
    gn = 10 ** (_gain_smooth(attack_db * av + sustain_db * dly * gate_v) / 20)
    return x * (gn[:, None] if x.ndim == 2 else gn)


def _release_hold(gn: np.ndarray, coef_v: float) -> np.ndarray:
    acc = np.empty(len(gn))
    sv = 1.0
    for ix, vec in enumerate(gn.tolist()):
        sv = 1 - (1 - sv) * coef_v
        if vec < sv:
            sv = vec
        acc[ix] = sv
    return acc


def _peak_track(sig: np.ndarray, tp: bool) -> np.ndarray:
    peak_v = np.abs(sig).max(axis=1) if sig.ndim == 2 else np.abs(sig)
    if tp and len(sig) > 1:
        upper = np.abs(signal.resample_poly(sig, 4, 1, axis=0))
        upper = upper.max(axis=1) if upper.ndim == 2 else upper
        peak_v = np.maximum(peak_v, upper[: 4 * len(sig)].reshape(len(sig), 4).max(axis=1))
    return peak_v


def _limiter_gain(needed: np.ndarray, wt: int, rel_s: float) -> np.ndarray:
    gn = ndimage.minimum_filter1d(needed, wt, mode="nearest")
    gn = _release_hold(gn, _smoothing_k(rel_s * 1000))
    return ndimage.uniform_filter1d(gn, wt, mode="nearest")


def limit(x: np.ndarray, ceiling_db: float = -1.0, lookahead: float = 0.0025, release: float = 0.08,
          true_peak: bool = True) -> np.ndarray:
    """Offline look-ahead peak limiter with a true-peak detector."""
    x = np.asarray(x, dtype=float)
    chan = 10 ** (ceiling_db / 20)
    needed = np.minimum(1.0, chan / np.maximum(_peak_track(x, true_peak), 1e-12))
    gn = _limiter_gain(needed, 2 * max(1, n_of(lookahead)) + 1, release)
    res = x * (gn[:, None] if x.ndim == 2 else gn)
    return np.clip(res, -chan, chan)


def _linkwitz(sig: np.ndarray, kind_s: str, freq: float) -> np.ndarray:
    return dsp.butter(dsp.butter(sig, kind_s, freq, 2), kind_s, freq, 2)


def split(x: np.ndarray, freqs: Any) -> list[np.ndarray]:
    """Linkwitz-Riley 4th-order band split, bands low to high."""
    band_list: list[np.ndarray] = []
    remain = np.asarray(x, dtype=float)
    for freq in sorted(freqs):
        band_list = [_linkwitz(bv, "low", freq) + _linkwitz(bv, "high", freq) for bv in band_list]
        band_list.append(_linkwitz(remain, "low", freq))
        remain = _linkwitz(remain, "high", freq)
    return band_list + [remain]


# ---------------------------------------------------------------- EQ match

def _profile_of_array(av: np.ndarray) -> np.ndarray:
    from hark.forge.engine import tone

    return av if av.ndim == 1 and len(av) == len(tone.CEN) else tone.profile(av)


def _goal_profile(goal: Any) -> np.ndarray:
    from hark.forge.engine import tone

    if isinstance(goal, tone.ToneRef):
        prof_v = np.asarray(goal.med, dtype=float)
    elif isinstance(goal, (list, tuple)) and goal:
        prof_v = np.asarray(tone.ToneRef(list(goal)).med, dtype=float)
    else:
        prof_v = _profile_of_array(np.asarray(goal, dtype=float))
    return prof_v


def match_curve(x: np.ndarray, target: Any, amount: float = 1.0, max_db: float = 12.0, smooth: int = 1) -> list:
    """28 band gains in dB that move x toward the target profile."""
    from hark.forge.engine import tone

    cfg = tone.profile(x)
    dly = _goal_profile(target) - cfg
    wt = 10 ** (cfg / 10)
    dly = dly - np.sum(wt * dly) / np.sum(wt)
    dly = np.where(cfg < -50, np.minimum(dly, 6.0), dly)
    dly = np.clip(dly, -max_db, max_db)
    for _ in range(int(smooth)):
        ev = np.concatenate([[dly[0]], dly, [dly[-1]]])
        dly = 0.25 * ev[:-2] + 0.5 * ev[1:-1] + 0.25 * ev[2:]
    return [round(float(vec), 2) for vec in amount * dly]


def _minphase_fir(mag_h: np.ndarray, n_fft: int, n_taps: int) -> np.ndarray:
    chan = np.fft.ifft(np.log(np.maximum(mag_h, 1e-12))).real
    freq = np.zeros(n_fft)
    freq[0] = chan[0]
    freq[1:n_fft // 2] = 2 * chan[1:n_fft // 2]
    freq[n_fft // 2] = chan[n_fft // 2]
    hh_ = np.fft.ifft(np.exp(np.fft.fft(freq))).real[:n_taps]
    window = np.ones(n_taps)
    window[n_taps // 2:] = np.hanning(2 * (n_taps - n_taps // 2))[n_taps - n_taps // 2:]
    return hh_ * window


def eq_bands(x: np.ndarray, gains_db: Any, phase: str = "min", taps: int = 4096) -> np.ndarray:
    """FIR EQ from 28 band gains; minimum phase by default."""
    from hark.forge.engine import tone

    x = np.asarray(x, dtype=float)
    n_fft = 4 * taps
    freq = np.fft.fftfreq(n_fft, 1.0 / SR)
    gn = np.interp(np.log2(np.maximum(np.abs(freq), 1)), np.log2(tone.CEN), np.asarray(gains_db, float))
    mag_v = 10 ** (gn / 20)
    if phase == "linear":
        hh_ = np.roll(np.fft.ifft(mag_v).real, taps // 2)[:taps] * np.hanning(taps)
        lag_n = taps // 2
    else:
        hh_ = _minphase_fir(mag_v, n_fft, taps)
        lag_n = 0
    kernel = hh_ if x.ndim == 1 else hh_[:, None]
    res = signal.fftconvolve(x, kernel, axes=0) if x.ndim == 2 else signal.fftconvolve(x, hh_)
    return res[lag_n:lag_n + len(x)]


def eq_match(x: np.ndarray, target: Any, amount: float = 1.0, max_db: float = 12.0, smooth: int = 1,
             phase: str = "min", iters: int = 2) -> np.ndarray:
    """Iterative 1/3-octave EQ match to a target, RMS kept."""
    x = np.asarray(x, dtype=float)
    acc_db = np.zeros(28)
    res = x
    for _ in range(max(1, int(iters))):
        acc_db = np.clip(acc_db + np.array(match_curve(res, target, 1.0, max_db, smooth)), -max_db, max_db)
        res = eq_bands(x, acc_db, phase)
    if amount != 1:
        res = eq_bands(x, amount * acc_db, phase)
    return res * _level_rms(x) / _level_rms(res)


# ---------------------------------------------------------------- motion

def _emit_time(tm: np.ndarray, t_zero: float, vec: float, range_m: float, chan: float) -> np.ndarray:
    bv = tm - t_zero
    return t_zero + (chan * chan * bv - np.sqrt(vec * vec * chan * chan * bv * bv + range_m * range_m * (chan * chan - vec * vec))) / (chan * chan - vec * vec)


def _flyby_times(span_s: float, vec: float, range_m: float, t_zero: float, chan: float) -> tuple[np.ndarray, np.ndarray]:
    """Emission times and distances of the heard samples of a straight pass."""
    def range_at(t_emit: Any) -> Any:
        return np.hypot(vec * (t_emit - t_zero), range_m)

    t_begin, t_end = range_at(0.0) / chan, span_s + range_at(span_s) / chan
    tm = t_begin + np.arange(int((t_end - t_begin) * SR)) / SR
    t_emit = _emit_time(tm, t_zero, vec, range_m, chan)
    return t_emit, range_at(t_emit)


def _distance_lp(res: np.ndarray, rng_m: np.ndarray, range_m: float, air_v: float) -> np.ndarray:
    att_k = air_v * 0.17 * np.maximum(rng_m - range_m, 0)
    corner = np.clip(10000 / np.maximum(10 ** (att_k / 10) - 1, 1e-9) ** 0.25, 200, 0.45 * SR)
    return dsp.svf(res, corner, 0.707, "low")


def _flyby_pan(res: np.ndarray, sidesig: np.ndarray, spread_w: float) -> np.ndarray:
    sv = np.clip(sidesig, -1, 1) * spread_w
    ang = (sv + 1) * np.pi / 4
    return np.column_stack([res * np.cos(ang), res * np.sin(ang)]) * np.sqrt(2)


def doppler(x: np.ndarray, speed: float = 40.0, dist: float = 4.0, t_close: float | None = None, air: float = 1.0,
            width: float = 0.8, stereo: bool = True, c: float = 343.0, floor_db: float = -30.0) -> np.ndarray:
    """A source passing the listener on a straight line."""
    mono_in = _downmix(x)
    span_s = len(mono_in) / SR
    t_zero = span_s / 2 if t_close is None else t_close
    vec = float(np.clip(speed, 1e-3, 0.95 * c))
    t_emit, rng_m = _flyby_times(span_s, vec, dist, t_zero, c)
    res = _cubic_read(mono_in, t_emit * SR) * np.maximum(dist / rng_m, 10 ** (floor_db / 20))
    res = _distance_lp(res, rng_m, dist, air)
    if not stereo:
        return res
    return _flyby_pan(res, vec * (t_emit - t_zero) / rng_m, width)


def decorrelate(x: np.ndarray, amount: float = 1.0, cut: float = 250.0) -> np.ndarray:
    """Mono to stereo with an all-pass side signal above cut Hz."""
    mono_in = _downmix(x)
    sidesig = mono_in
    for dly, gn in ((149, 0.6), (263, -0.55), (397, 0.5)):
        sidesig = _schroeder_ap(sidesig, dly, gn)
    sidesig = dsp.butter(sidesig, "high", cut, 2)
    sidesig = sidesig * 0.7 * _level_rms(mono_in) / _level_rms(sidesig)
    nrm = np.sqrt(1 + 0.49 * amount ** 2)
    return np.column_stack([mono_in + amount * sidesig, mono_in - amount * sidesig]) / nrm


def pan(x: np.ndarray, pos: Any) -> np.ndarray:
    """Constant-power pan along a control curve."""
    x = np.asarray(x, dtype=float)
    cfg = np.clip(dsp.curve(pos, len(x)), -1, 1)
    ang = (cfg + 1) * np.pi / 4
    g_lo, grain_n = np.cos(ang) * np.sqrt(2), np.sin(ang) * np.sqrt(2)
    if x.ndim == 1:
        return np.column_stack([x * g_lo, x * grain_n])
    return np.column_stack([x[:, 0] * g_lo, x[:, 1] * grain_n])


def width(x: np.ndarray, w: Any) -> np.ndarray:
    """Mid/side width: 0 mono, 1 unchanged, 2 wider."""
    x = np.asarray(x, dtype=float)
    if x.ndim == 1:
        x = decorrelate(x, 1.0)
    cnt = np.clip(dsp.curve(w, len(x)), 0, 2)
    mid_s = (x[:, 0] + x[:, 1]) / 2
    sidesig = (x[:, 0] - x[:, 1]) / 2 * cnt
    return np.column_stack([mid_s + sidesig, mid_s - sidesig])


def sweep_filter(x: np.ndarray, f0: float, f1: float, q: float = 4.0, curve_: str = "exp", power: float = 1.0,
                 mode: str = "bandn") -> np.ndarray:
    """State-variable filter that sweeps from f0 to f1."""
    x = np.asarray(x, dtype=float)
    trk = dsp.sweep(f0, f1, len(x) / SR, curve_, power)
    trk = np.concatenate([trk, np.full(max(0, len(x) - len(trk)), trk[-1])])[: len(x)]
    return dsp.svf(x, trk, q, mode)


def _resample_track(sig: np.ndarray, trk_pos: np.ndarray) -> np.ndarray:
    where = np.concatenate([[0.0], np.cumsum(trk_pos[:-1])])
    where = where[where < len(sig) - 1]
    return _cubic_read(sig, where)


def varispeed(x: np.ndarray, speed: Any) -> np.ndarray:
    """Read x at a speed curve: 2 is an octave up, 0 is stopped."""
    x = np.asarray(x, dtype=float)
    if np.isscalar(speed):
        total_n = int(len(x) / max(float(speed), 1e-3))
    elif isinstance(speed, (list, tuple)) and speed and isinstance(speed[0], (list, tuple)):
        total_n = n_of(max(cfg[0] for cfg in speed))
    else:
        total_n = len(speed)
    return _resample_track(x, np.maximum(dsp.curve(speed, total_n), 0))


def tape_stop(x: np.ndarray, start: float | None = None, seconds: float = 0.6, curve_: float = 1.6) -> np.ndarray:
    """Tape slowing to a stop."""
    x = np.asarray(x, dtype=float)
    semis = max(0.0, len(x) / SR - 0.6 * seconds) if start is None else start
    trk_pos = np.concatenate([np.ones(n_of(semis)), (1 - np.linspace(0, 1, n_of(seconds))) ** curve_])
    res = _resample_track(x, trk_pos)
    cnt = min(len(res), n_of(0.005))
    if cnt:
        wt = np.linspace(1, 0, cnt)
        res[len(res) - cnt:] *= wt if res.ndim == 1 else wt[:, None]
    return res


# ---------------------------------------------------------------- glitch and grains

def crush(x: np.ndarray, bits: int | None = 8, rate: float | None = None, mix: float = 1.0) -> np.ndarray:
    """Sample-rate reduction and bit crush, no dither."""
    x = np.asarray(x, dtype=float)
    res = x
    if rate is not None:
        ix = np.arange(len(x))
        ids = np.minimum(np.floor(np.floor(ix * rate / SR) * SR / rate).astype(int), len(x) - 1)
        res = res[ids]
    if bits is not None:
        qv = 2 ** (bits - 1)
        res = np.round(res * qv) / qv
    return mix * res + (1 - mix) * x


def _repeat_copy(seg_len: np.ndarray, cnt: int, st_step: float, gate_v: float, n_fade: int, fall_db: float):
    if st_step:
        lfo_hz = 2 ** (cnt * st_step / 12)
        chan = _cubic_read(seg_len, np.arange(0, len(seg_len), lfo_hz)[: len(seg_len)])
    else:
        chan = seg_len.copy()
    gn = max(2, int(len(seg_len) * gate_v))
    chan[gn:] = 0
    chan[:gn] = _soft_edges(chan[:gn], n_fade)
    return chan * 10 ** (cnt * fall_db / 20)


def stutter(x: np.ndarray, start: float, length: float, repeats: int = 4, gate: float = 0.85,
            decay_db: float = 0.0, pitch_step: float = 0.0, fade: float = 0.002) -> np.ndarray:
    """Repeat one slice of x, gated, with optional pitch steps and decay."""
    x = np.asarray(x, dtype=float)
    at0 = n_of(start) if start > 0 else 0
    seg_len = x[at0:at0 + n_of(length)]
    cps = [_repeat_copy(seg_len, cnt, pitch_step, gate, n_of(fade), decay_db) for cnt in range(int(repeats))]
    return np.concatenate([x[:at0]] + cps + [x[at0 + len(seg_len):]])


def _up_octave(seg_len: np.ndarray, cnt: int) -> np.ndarray:
    hh_ = _soft_edges(seg_len[::2], cnt)
    acc = np.concatenate([hh_, hh_])[: len(seg_len)]
    if len(acc) < len(seg_len):
        acc = np.concatenate([acc, np.zeros((len(seg_len) - len(acc),) + seg_len.shape[1:])])
    return acc


def _g_reverse(seg_len: np.ndarray, cnt: int) -> np.ndarray:
    return seg_len[::-1].copy()


def _g_drop(seg_len: np.ndarray, cnt: int) -> np.ndarray:
    return np.zeros_like(seg_len)


def _g_crush(seg_len: np.ndarray, cnt: int) -> np.ndarray:
    return crush(seg_len, 6, 6000)


def _g_keep(seg_len: np.ndarray, cnt: int) -> np.ndarray:
    return seg_len.copy()


# Edit by the number of cumulative thresholds at or under u; 0 (repeat) falls back to reverse with no previous slice.
_GLITCH_OPS = (_g_reverse, _g_reverse, _g_drop, _up_octave, _g_crush, _g_keep)


def _g_slice(seg_len: np.ndarray, uu: float, thr: np.ndarray, last: np.ndarray | None, cnt: int) -> np.ndarray:
    ix = int(np.searchsorted(thr, uu, side="right"))
    if ix == 0 and last is not None and len(last) == len(seg_len):
        return last.copy()
    return _GLITCH_OPS[ix](seg_len, cnt)


def glitch(x: np.ndarray, r: np.random.Generator, grid: float = 0.03, repeat: float = 0.25, reverse: float = 0.1,
           drop: float = 0.08, octave: float = 0.08, crush_: float = 0.08, fade: float = 0.0015) -> np.ndarray:
    """Random slice edits on a time grid: repeat, reverse, drop, octave up, crush."""
    x = np.asarray(x, dtype=float)
    len_n, cnt = max(16, n_of(grid)), n_of(fade)
    thr = np.cumsum([repeat, reverse, drop, octave, crush_])
    acc = np.zeros_like(x)
    last = None
    for ix in range(0, len(x), len_n):
        seg_len = x[ix:ix + len_n]
        sv = _soft_edges(_g_slice(seg_len, r.uniform(), thr, last, cnt), cnt)
        acc[ix:ix + len(sv)] = sv
        last = sv
    return acc


def _position_curve(where: Any, total_n: int) -> np.ndarray:
    if isinstance(where, tuple) and len(where) == 2 and all(np.isscalar(cfg) for cfg in where):
        return np.linspace(where[0], where[1], total_n)
    return dsp.curve(where, total_n)


def _grain_scatter(acc: np.ndarray, sv: np.ndarray, gen: np.random.Generator, pcs: np.ndarray, times_v: tuple,
             knob_d: tuple) -> None:
    """Add grains at a jittered period until seconds."""
    secs, per, tj, g_lo = times_v
    window = np.hanning(g_lo)
    total_n = len(acc)
    tm = 0.0
    while tm < secs:
        tvec = tm + tj * per * gen.uniform(-0.5, 0.5)
        ix = n_of(tvec) if tvec > 0 else 0
        if ix < total_n:
            _place_grain(acc, sv, gen, ix, pcs, g_lo, window, knob_d)
        tm += per


def granular(src: np.ndarray, seconds: float, r: np.random.Generator, grain: float = 0.06, density: float = 40.0,
             pos: Any = (0.0, 1.0), pos_jitter: float = 0.02, pitch: float = 0.0, pitch_jitter: float = 0.0,
             time_jitter: float = 0.5, spread: float = 0.6, reverse: float = 0.0) -> np.ndarray:
    """Stereo granular cloud read from a source."""
    knob_d = (pitch, pitch_jitter, pos_jitter, reverse, spread)
    sv = _downmix(src)
    total_n = n_of(seconds)
    acc = np.zeros((total_n, 2))
    pcs = _position_curve(pos, total_n)
    per = 1.0 / max(density, 1e-3)
    _grain_scatter(acc, sv, r, pcs, (seconds, per, time_jitter, n_of(grain)), knob_d)
    return acc / np.sqrt(max(1.0, density * grain * 0.375))


def _add_panned(acc: np.ndarray, gn: np.ndarray, ix: int, ang: float) -> None:
    mm = min(len(gn), len(acc) - ix)
    acc[ix:ix + mm, 0] += gn[:mm] * np.cos(ang) * np.sqrt(2)
    acc[ix:ix + mm, 1] += gn[:mm] * np.sin(ang) * np.sqrt(2)


def _place_grain(acc: np.ndarray, sv: np.ndarray, gen: np.random.Generator, ix: int, pcs: np.ndarray, g_lo: int,
               window: np.ndarray, knob_d: tuple) -> None:
    pitch_v, pjit, pj, rev_p, pan_spread = knob_d
    lfo_hz = 2 ** ((pitch_v + pjit * gen.standard_normal()) / 12)
    cfg0 = (pcs[min(ix, len(acc) - 1)] + pj * gen.standard_normal()) * (len(sv) - g_lo * lfo_hz)
    start_s = np.clip(cfg0, 0, max(0.0, len(sv) - g_lo * lfo_hz - 2))
    gn = _cubic_read(sv, start_s + np.arange(g_lo) * lfo_hz) * window
    if gen.uniform() < rev_p:
        gn = gn[::-1]
    _add_panned(acc, gn, ix, (np.clip(pan_spread * gen.uniform(-1, 1), -1, 1) + 1) * np.pi / 4)


def _swell_lead(wet_v: np.ndarray, cnt: int, secs: float) -> np.ndarray:
    sw_ = wet_v[:cnt][::-1].copy()
    kfac = min(cnt, n_of(0.35 * secs))
    sw_[:kfac] *= (np.linspace(0, 1, kfac) ** 2)[:, None]
    return sw_


def _hit_tail(hmag: np.ndarray, gen: np.random.Generator, space: tuple, room_amt: float) -> np.ndarray:
    t60, room_size, damping = space
    if room_amt > 0:
        return reverb(hmag, gen, 0.6 * t60, room_size, predelay=0.0, damp=damping, wet=room_amt, dry=1.0, stereo=True)
    return dsp.stereo_of(hmag)


def reverse_swell(hit: np.ndarray, r: np.random.Generator, seconds: float = 1.0, rt60: float = 2.5,
                  size: float = 0.8, damp: float = 0.4, swell_db: float = -6.0, gap: float = 0.0,
                  hit_room: float = 0.15) -> np.ndarray:
    """A reversed reverb swell that leads into the hit."""
    hmag = _downmix(hit)
    wet_v = reverb(hmag, r, rt60, size, predelay=0.0, damp=damp, wet=1.0, dry=0.0, stereo=True,
                 tail=max(seconds, rt60) + 0.1)
    cnt = n_of(seconds)
    sw_ = _swell_lead(wet_v, cnt, seconds)
    post = _hit_tail(hmag, r, (rt60, size, damp), hit_room)
    sw_ = sw_ * np.max(np.abs(post)) * 10 ** (swell_db / 20) / (np.max(np.abs(sw_)) + 1e-12)
    gn = n_of(gap) if gap > 0 else 0
    acc = np.zeros((cnt + gn + len(post), 2))
    acc[: len(sw_)] += sw_
    acc[cnt + gn:] += post
    return acc
