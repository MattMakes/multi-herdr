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
from hark.forge.engine.dsp import SR, n_of, t_of

IR_CACHE_MB = 64

_CACHE: OrderedDict = OrderedDict()
_LOCK = threading.Lock()
_STATS = {"hits": 0, "misses": 0}


# ---------------------------------------------------------------- helpers

def _rms(x: np.ndarray) -> float:
    return float(np.sqrt(np.mean(np.square(x)))) + 1e-12


def _mono(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=float)
    return x.mean(axis=1) if x.ndim == 2 else x


def _allpass(x: np.ndarray, d: int, g: float) -> np.ndarray:
    """Schroeder all-pass of delay d and gain g (exact LTI form)."""
    b = np.zeros(d + 1)
    a = np.zeros(d + 1)
    b[0], b[d] = -g, 1.0
    a[0], a[d] = 1.0, -g
    return signal.lfilter(b, a, x, axis=0)


def _shelf(kind: str, gain: float, fc: float) -> tuple[float, float, float]:
    """First-order shelf as (b0, b1, a1) for y = b0 x + b1 x1 - a1 y1."""
    k = np.tan(np.pi * float(np.clip(fc, 20, 0.45 * SR)) / SR)
    s = np.sqrt(gain)
    if kind == "high":
        b0, b1, a0, a1 = s + k, k - s, 1 / s + k, k - 1 / s
    else:
        b0, b1, a0, a1 = 1 + k * s, k * s - 1, 1 + k / s, k / s - 1
    return b0 / a0, b1 / a0, a1 / a0


def _hermite(x: np.ndarray, pos: np.ndarray) -> np.ndarray:
    """Cubic Hermite read at fractional positions; zero outside x."""
    x = np.asarray(x, dtype=float)
    pad = np.zeros((2,) + x.shape[1:])
    xp = np.concatenate([pad, x, pad, pad])
    i = np.floor(pos).astype(int)
    fr = pos - i
    ok = (i >= -1) & (i <= len(x))
    j = np.clip(i, -1, len(x)) + 2
    xm, x0, x1, x2 = xp[j - 1], xp[j], xp[j + 1], xp[j + 2]
    if x.ndim == 2:
        fr = fr[:, None]
        ok = ok[:, None]
    c1 = 0.5 * (x1 - xm)
    c2 = xm - 2.5 * x0 + 2 * x1 - 0.5 * x2
    c3 = 0.5 * (x2 - xm) + 1.5 * (x0 - x1)
    return np.where(ok, ((c3 * fr + c2) * fr + c1) * fr + x0, 0.0)


def _coef(ms: float) -> float:
    return float(np.exp(-1.0 / max(1.0, ms * 1e-3 * SR)))


def _follow(x: np.ndarray, att: float, rel: float) -> np.ndarray:
    """Peak follower with separate attack and release coefficients."""
    out = np.empty(len(x))
    e = 0.0
    for i, v in enumerate(x.tolist()):
        a = att if v > e else rel
        e = a * e + (1 - a) * v
        out[i] = e
    return out


def _edge_fade(x: np.ndarray, k: int) -> np.ndarray:
    x = np.array(x, dtype=float, copy=True)
    k = int(min(k, len(x) // 2))
    if k > 1:
        w = 0.5 - 0.5 * np.cos(np.linspace(0, np.pi, k))
        w = w if x.ndim == 1 else w[:, None]
        x[:k] *= w
        x[len(x) - k:] *= w[::-1]
    return x


def _primes(limit: int) -> np.ndarray:
    sieve = np.ones(limit + 1, dtype=bool)
    sieve[:2] = False
    for p in range(2, int(limit ** 0.5) + 1):
        if sieve[p]:
            sieve[p * p::p] = False
    return np.nonzero(sieve)[0]


# ---------------------------------------------------------------- IR cache

def ir_cache_stats() -> dict:
    """Hits, misses, entries and size in MB of the reverb IR cache."""
    with _LOCK:
        mb = sum(v[0].nbytes for v in _CACHE.values()) / 2 ** 20
        return {"hits": _STATS["hits"], "misses": _STATS["misses"], "entries": len(_CACHE), "mb": round(mb, 1)}


def ir_cache_clear() -> None:
    """Empty the IR cache and zero its counters."""
    with _LOCK:
        _CACHE.clear()
        _STATS["hits"] = _STATS["misses"] = 0


def _cache_get(key: Any) -> Any:
    with _LOCK:
        if key in _CACHE:
            _CACHE.move_to_end(key)
            _STATS["hits"] += 1
            return _CACHE[key]
        _STATS["misses"] += 1
        return None


def _cache_put(key: Any, ir: np.ndarray, state: Any) -> None:
    with _LOCK:
        _CACHE[key] = (ir.copy(), state)
        budget = IR_CACHE_MB * 2 ** 20
        while len(_CACHE) > 1 and sum(v[0].nbytes for v in _CACHE.values()) > budget:
            _CACHE.popitem(last=False)


def _log_snap(v: Any, step: float) -> Any:
    if v is None or v <= 0:
        return v
    return float(2 ** (round(np.log2(v) / step) * step))


def _lin_snap(v: float, step: float = 0.05) -> float:
    return round(round(v / step) * step, 4)


def ir_grid(rt60: float = 1.6, size: float = 0.6, predelay: float = 0.01, damp: float = 0.5, bass: float = 1.2,
            mod: float = 1.0, diffuse: float = 0.7, width: float = 1.0, tail: float | None = None,
            lowcut: float = 0.0) -> dict:
    """Snap room settings to a grid so a search keeps hitting the IR cache."""
    out = {"rt60": _log_snap(rt60, 1 / 24), "tail": _log_snap(tail, 1 / 24), "lowcut": _log_snap(lowcut, 1 / 12),
           "predelay": round(round(predelay / 0.0005) * 0.0005, 5)}
    for k, v in (("size", size), ("damp", damp), ("bass", bass), ("mod", mod), ("diffuse", diffuse),
                 ("width", width)):
        out[k] = _lin_snap(v)
    return out


# ---------------------------------------------------------------- early reflections

def _early_channel(r: np.random.Generator, m: int, t_hi: float, taps: int, width: float, c: int) -> np.ndarray:
    ts = np.sort(np.exp(r.uniform(np.log(0.003), np.log(t_hi), taps)))
    if c == 1:
        ts = ts * (1 + width * r.uniform(-0.12, 0.12, taps))
    signs = r.choice([-1.0, 1.0], taps)
    ir = np.zeros(m)
    for t, s in zip(ts, signs):
        ir[min(m - 1, n_of(t))] += s * 0.003 / t
    return ir


def _darken(ir: np.ndarray, damp: float) -> np.ndarray:
    m = len(ir)
    edges = np.linspace(0, m, 4).astype(int)
    out = np.zeros(m)
    for j in range(3):
        part = np.zeros(m)
        part[edges[j]:edges[j + 1]] = ir[edges[j]:edges[j + 1]]
        out += dsp.butter(part, "low", 14000 - damp * 4000 * (j + 1), 1)
    return out / (np.sqrt(np.sum(out * out)) + 1e-12)


def early(x: np.ndarray, r: np.random.Generator, size: float = 0.5, taps: int = 18, damp: float = 0.5,
          width: float = 1.0) -> np.ndarray:
    """Stereo early reflections of the mono mix of x."""
    xm = _mono(x)
    t_hi = 0.006 + 0.05 * size
    m = n_of(t_hi) + 2
    chans = [_darken(_early_channel(r, m, t_hi, taps, width, c), damp) for c in range(2)]
    return np.column_stack([signal.fftconvolve(xm, ch) for ch in chans])


# ---------------------------------------------------------------- FDN reverb

def _delays(r: np.random.Generator, size: float, nl: int) -> np.ndarray:
    lo = (0.006 + 0.024 * size) * SR
    targets = np.geomspace(lo, 2.7 * lo, nl) * np.exp(r.uniform(-0.06, 0.06, nl))
    pool = list(_primes(int(3.2 * lo) + 50))
    out = []
    for tg in targets:
        p = min(pool, key=lambda q: abs(q - tg))
        pool.remove(p)
        out.append(p)
    return r.permutation(np.array(out, dtype=float))


def _taps(r: np.random.Generator, nl: int) -> tuple[np.ndarray, np.ndarray]:
    b = r.choice([-1.0, 1.0], nl) / np.sqrt(nl)
    cm = r.choice([-1.0, 1.0], (nl, 2))
    if abs(cm[:, 0] @ cm[:, 1]) >= nl:
        cm[nl // 2:, 1] *= -1
    cm[:, 1] -= cm[:, 0] * (cm[:, 0] @ cm[:, 1]) / nl
    return b, cm / np.linalg.norm(cm, axis=0)


def _hadamard(v: np.ndarray) -> np.ndarray:
    v = v.copy()
    nl = v.shape[-1]
    h = 1
    while h < nl:
        for i in range(0, nl, 2 * h):
            p = v[..., i:i + h].copy()
            q = v[..., i + h:i + 2 * h].copy()
            v[..., i:i + h] = p + q
            v[..., i + h:i + 2 * h] = p - q
        h *= 2
    return v / np.sqrt(nl)


class _Fdn:
    """Feedback delay network state for one input channel."""

    def __init__(self, dl: np.ndarray, depth: np.ndarray, rate: np.ndarray, phase: np.ndarray,
                 lo: tuple, hi: tuple, b: np.ndarray, cm: np.ndarray):
        self.dl, self.depth, self.rate, self.phase = dl, depth, rate, phase
        self.lo, self.hi, self.b, self.cm = lo, hi, b, cm
        nl = len(dl)
        self.size = 1 << int(np.ceil(np.log2(int(dl.max() + depth.max() + 8))))
        self.buf = np.zeros((nl, self.size))
        self.ya = np.zeros(nl)
        self.zl = np.zeros(nl)
        self.zh = np.zeros(nl)
        self.block = max(1, int(dl.min() - depth.max()) - 3)

    def _read(self, i0: int, k: int) -> np.ndarray:
        i = np.arange(i0, i0 + k)[:, None]
        d = self.dl + self.depth * np.sin(2 * np.pi * self.rate * i / SR + self.phase)
        m = np.floor(d - 0.5).astype(int)
        eta = (1 - (d - m)) / (1 + (d - m))
        rows = np.arange(len(self.dl))[None, :]
        a0 = self.buf[rows, (i - m) % self.size]
        a1 = self.buf[rows, (i - m - 1) % self.size]
        y = np.empty_like(a0)
        ya = self.ya
        for j in range(k):
            ya = eta[j] * a0[j] + a1[j] - eta[j] * ya
            y[j] = ya
        self.ya = ya
        return y

    def _shelves(self, y: np.ndarray) -> np.ndarray:
        out = np.empty_like(y)
        for k in range(y.shape[1]):
            lb0, lb1, la1 = self.lo[0][k], self.lo[1][k], self.lo[2]
            hb0, hb1, ha1 = self.hi
            t, zl = signal.lfilter([lb0, lb1], [1.0, la1], y[:, k], zi=[self.zl[k]])
            u, zh = signal.lfilter([hb0, hb1], [1.0, ha1], t, zi=[self.zh[k]])
            self.zl[k], self.zh[k] = zl[0], zh[0]
            out[:, k] = u
        return out

    def run(self, x: np.ndarray, n: int) -> np.ndarray:
        out = np.zeros((n, 2))
        i0 = 0
        while i0 < n:
            k = min(self.block, n - i0)
            v = self._shelves(self._read(i0, k))
            out[i0:i0 + k] = v @ self.cm
            mixed = _hadamard(v)
            xin = np.zeros(k)
            seg = x[i0:i0 + k]
            xin[: len(seg)] = seg
            idx = np.arange(i0, i0 + k) % self.size
            self.buf[:, idx] = (mixed + xin[:, None] * self.b[None, :]).T
            i0 += k
        return out


def _gains(dl: np.ndarray, rt: float) -> np.ndarray:
    return 10 ** (-3 * dl / (max(rt, 0.02) * SR))


def _shelf_pair(dl: np.ndarray, rt60: float, rt_lo: float, rt_hi: float, lf: float, hf: float):
    gm, gl, gh = _gains(dl, rt60), _gains(dl, rt_lo), _gains(dl, rt_hi)
    lows = [_shelf("low", g_l / g_m, lf) for g_l, g_m in zip(gl, gm)]
    lb0 = np.array([s[0] for s in lows]) * gm
    lb1 = np.array([s[1] for s in lows]) * gm
    la1 = lows[0][2] if len(set(round(s[2], 15) for s in lows)) == 1 else None
    highs = [_shelf("high", g_h / g_m, hf) for g_h, g_m in zip(gh, gm)]
    return gm, lows, highs, lb0, lb1, la1


class _Room:
    """Derived settings of one IR build."""

    def __init__(self, p: dict):
        self.p = p
        self.rt_lo = p["rt60"] * p["bass"]
        self.rt_hi = p["rt60"] * max(0.05, 1 - 0.85 * p["damp"])
        self.tail = p["tail"] if p["tail"] is not None else 1.2 * max(p["rt60"], self.rt_lo)
        self.pd = n_of(p["predelay"]) if p["predelay"] > 0 else 0
        self.n = max(n_of(self.tail), 64)


def _rt_table(dlk: float, low: tuple, gmk: float, high: tuple) -> tuple[np.ndarray, np.ndarray]:
    f = np.geomspace(20.0, 0.999 * SR / 2, 64)
    w = 2 * np.pi * f / SR
    _, hl = signal.freqz([low[0] * gmk, low[1] * gmk], [1.0, low[2]], worN=w)
    _, hh = signal.freqz([high[0], high[1]], [1.0, high[2]], worN=w)
    h = np.minimum(np.abs(hl * hh), 0.999999)
    return f, -3 * dlk / (SR * np.log10(h))


def _stft_decay(x: np.ndarray, n: int, decay_db: Any) -> np.ndarray:
    ft, tt, z = signal.stft(x, fs=SR, nperseg=1024, noverlap=768, boundary="even")
    z = z * decay_db(ft[:, None], tt[None, :])
    _, y = signal.istft(z, fs=SR, nperseg=1024, noverlap=768, boundary=True)
    out = np.zeros(n)
    out[: min(n, len(y))] = y[:n]
    return out


def _late_noise(r: np.random.Generator, n: int, f: np.ndarray, rt: np.ndarray, width: float) -> np.ndarray:
    def dec(ff: np.ndarray, tt: np.ndarray) -> np.ndarray:
        return np.exp(-6.91 * tt / np.interp(ff, f, rt))

    m = _stft_decay(r.standard_normal(n + 1024), n, dec)
    sd = dsp.butter(_stft_decay(r.standard_normal(n + 1024), n, dec), "high", 200.0, 2)
    lr = np.column_stack([m + width * sd, m - width * sd]) / np.sqrt(1 + 0.9 * width ** 2)
    return lr * np.sqrt(13.8 / (np.median(rt) * SR))


def _air(ir: np.ndarray, n: int, air: float) -> np.ndarray:
    def dec(ff: np.ndarray, tt: np.ndarray) -> np.ndarray:
        rate = air * 0.17 * 343 * (ff / 10000) ** 2
        return 10 ** (-rate * np.maximum(tt - 512 / SR, 0) / 20)

    return np.column_stack([_stft_decay(ir[:, c], n, dec) for c in range(ir.shape[1])])


def _diffused(n: int, size: float, diffuse: float, c: int) -> np.ndarray:
    x = np.zeros(n)
    x[0] = 1.0
    if diffuse > 0:
        for j, ms in enumerate([1.1, 1.7, 2.9, 4.3]):
            x = _allpass(x, int(ms * (0.6 + 0.8 * size) * 1e-3 * SR) + 3 * c + j, min(0.8, diffuse))
    return x


def _one_input(r: np.random.Generator, rm: _Room, dl: np.ndarray, shelves: tuple, c: int) -> np.ndarray:
    p, n = rm.p, rm.n
    gm, lows, highs, lb0, lb1, _ = shelves
    nl = len(dl)
    x = _diffused(n, p["size"], p["diffuse"], c)
    b, cm = _taps(r, nl)
    depth = p["mod"] * (3 + 9 * r.uniform(0, 1, nl)) * (0.5 + 0.5 * p["size"])
    rate = r.uniform(0.15, 1.1, nl)
    phase = r.uniform(0, 2 * np.pi, nl)
    fdn = _Fdn(dl, depth, rate, phase, (lb0, lb1, lows[0][2]), highs[0], b, cm)
    y = fdn.run(x, n) / np.sqrt(np.mean(gm ** 2 / (1 - gm ** 2)) / nl)
    if p["late"] == "noise":
        y = _cross_late(r, rm, y, dl, shelves)
    if p["early_db"] is not None and p["early_db"] > -90:
        e = 10 ** (p["early_db"] / 10)
        imp = np.array([1.0])
        er = early(imp, r, p["size"], damp=p["damp"], width=p["width"])[:n]
        y = y / np.sqrt(1 + e)
        y[: len(er)] += er * np.sqrt(e / (1 + e))
    mid, side = (y[:, 0] + y[:, 1]) / 2, (y[:, 0] - y[:, 1]) / 2
    return np.column_stack([mid + p["width"] * side, mid - p["width"] * side])


def _cross_late(r: np.random.Generator, rm: _Room, y: np.ndarray, dl: np.ndarray, shelves: tuple) -> np.ndarray:
    gm, lows, highs = shelves[0], shelves[1], shelves[2]
    k = int(np.argmin(np.abs(dl - np.median(dl))))
    f, rt = _rt_table(dl[k], lows[k], gm[k], highs[k])
    late = _late_noise(r, rm.n, f, rt, 1.0)
    tx = 0.06 + 0.1 * rm.p["size"]
    u = np.clip((t_of(rm.n) - 0.5 * tx) / tx, 0, 1)[:, None]
    return y * np.cos(u * np.pi / 2) + late * np.sin(u * np.pi / 2)


def _build_ir(r: np.random.Generator, p: dict) -> np.ndarray:
    assert p["lines"] in (4, 8, 16, 32), "lines must be 4, 8, 16 or 32"
    rm = _Room(p)
    dl = _delays(r, p["size"], p["lines"])
    shelves = _shelf_pair(dl, p["rt60"], rm.rt_lo, rm.rt_hi, p["lf_hz"], p["hf_hz"])
    ir = np.stack([_one_input(r, rm, dl, shelves, c) for c in range(p["inputs"])], axis=1)
    ir = np.stack([_finish_ir(ir[:, c, :], rm, p) for c in range(p["inputs"])], axis=1)
    return np.concatenate([np.zeros((rm.pd,) + ir.shape[1:]), ir])


def _finish_ir(y: np.ndarray, rm: _Room, p: dict) -> np.ndarray:
    if p["air"] > 0:
        y = _air(y, rm.n, p["air"])
    if p["lowcut"] > 0:
        y = dsp.butter(y, "high", p["lowcut"], 2)
    k = min(rm.n, n_of(min(0.1, 0.2 * rm.tail)))
    y = y.copy()
    y[rm.n - k:] *= ((1 - np.linspace(0, 1, k)) ** 2)[:, None]
    return y


_IR_KEYS = ("rt60", "size", "predelay", "damp", "bass", "mod", "diffuse", "early_db", "width", "late", "lines",
            "tail", "inputs", "hf_hz", "lf_hz", "air", "lowcut")


def reverb_ir(r: np.random.Generator, rt60: float = 1.6, size: float = 0.6, predelay: float = 0.01,
              damp: float = 0.5, bass: float = 1.2, mod: float = 1.0, diffuse: float = 0.7, early_db: float = -6.0,
              width: float = 1.0, late: str = "noise", lines: int = 16, tail: float | None = None, inputs: int = 1,
              hf_hz: float = 4000.0, lf_hz: float = 250.0, air: float = 1.0, lowcut: float = 0.0,
              seed: int | None = None, cache: bool = True) -> np.ndarray:
    """Reverb impulse response of shape (n, inputs, 2), cached per settings and generator state."""
    p = dict(rt60=float(rt60), size=float(size), predelay=float(predelay), damp=float(damp), bass=float(bass),
             mod=float(mod), diffuse=float(diffuse), early_db=None if early_db is None else float(early_db),
             width=float(width), late=str(late), lines=int(lines), tail=None if tail is None else float(tail),
             inputs=int(inputs), hf_hz=float(hf_hz), lf_hz=float(lf_hz), air=float(air), lowcut=float(lowcut))
    params = tuple(p[k] for k in _IR_KEYS)
    if seed is not None:
        return _seeded_ir(int(seed), p, params)
    if cache and r is not None and hasattr(r, "bit_generator"):
        return _stateful_ir(r, p, params)
    return _build_ir(r, p)


def _seeded_ir(seed: int, p: dict, params: tuple) -> np.ndarray:
    key = ("seed", seed, params)
    hit = _cache_get(key)
    if hit is not None:
        return hit[0].copy()
    ir = _build_ir(dsp.rng(seed), p)
    _cache_put(key, ir, None)
    return ir


def _stateful_ir(r: np.random.Generator, p: dict, params: tuple) -> np.ndarray:
    key = ("state", repr(r.bit_generator.state), params)
    hit = _cache_get(key)
    if hit is not None:
        r.bit_generator.state = hit[1]
        return hit[0].copy()
    ir = _build_ir(r, p)
    _cache_put(key, ir, r.bit_generator.state)
    return ir


def reverb(x: np.ndarray, r: np.random.Generator, rt60: float = 1.6, size: float = 0.6, predelay: float = 0.01,
           damp: float = 0.5, bass: float = 1.2, mod: float = 1.0, diffuse: float = 0.7, early_db: float = -6.0,
           width: float = 1.0, wet: float = 0.3, dry: float = 1.0, late: str = "noise", lines: int = 16,
           tail: float | None = None, stereo: bool | None = None, air: float = 1.0, lowcut: float = 0.0,
           ir_seed: int | None = None, grid: bool = False) -> np.ndarray:
    """FDN reverb; wet is the level of the unit-energy tail relative to the dry sound."""
    x = np.asarray(x, dtype=float)
    xs = x[:, None] if x.ndim == 1 else x
    stereo = x.ndim == 2 if stereo is None else stereo
    room = dict(rt60=rt60, size=size, predelay=predelay, damp=damp, bass=bass, mod=mod, diffuse=diffuse,
                width=width, tail=tail, lowcut=lowcut)
    if grid:
        room = ir_grid(**room)
    ir = reverb_ir(r, early_db=early_db, late=late, lines=lines, inputs=xs.shape[1], air=air, seed=ir_seed, **room)
    wetsig = sum(np.column_stack([signal.fftconvolve(xs[:, c], ir[:, c, o]) for o in range(2)])
                 for c in range(xs.shape[1]))
    if not stereo:
        out = wet * wetsig.mean(axis=1) * np.sqrt(2)
        out[: len(x)] += dry * xs.mean(axis=1)
        return out
    out = wetsig * wet
    out[: len(x)] += dry * (xs if xs.shape[1] == 2 else np.column_stack([xs[:, 0], xs[:, 0]]))
    return out


# ---------------------------------------------------------------- dynamics

def _shelf_filter(x: np.ndarray, kind: str, db: float, fc: float) -> np.ndarray:
    b0, b1, a1 = _shelf(kind, 10 ** (db / 20), fc)
    return signal.lfilter([b0, b1], [1.0, a1], x, axis=0)


def _curve(u: np.ndarray, mode: str, g: float, bias: float, os_: int) -> np.ndarray:
    if mode == "tape":
        u = _shelf_filter(u, "high", 4.0, 3000.0 / os_)
        bb = 0.12 + bias
        u = np.tanh(g * u + bb) - np.tanh(bb)
        return _shelf_filter(u, "high", -4.0, 3000.0 / os_)
    if mode == "hard":
        return np.clip(g * u + bias, -1, 1) - np.clip(bias, -1, 1)
    if mode == "fold":
        return np.sin(np.pi / 2 * (g * u + bias)) - np.sin(np.pi / 2 * bias)
    return np.tanh(g * u + bias) - np.tanh(bias)


def _match_level(y: np.ndarray, x: np.ndarray, level: str | None) -> np.ndarray:
    if level == "rms":
        return y * (_rms(x) - 1e-12) / (_rms(y) - 1e-12 + 1e-30)
    if level == "peak":
        return y * np.max(np.abs(x)) / (np.max(np.abs(y)) + 1e-12)
    return y


def saturate(x: np.ndarray, drive_db: float = 6.0, mode: str = "tanh", mix: float = 1.0, bias: float = 0.0,
             os: int | None = None, level: str | None = "rms") -> np.ndarray:
    """Oversampled waveshaper (tanh, tape, hard, fold)."""
    x = np.asarray(x, dtype=float)
    g = 10 ** (drive_db / 20)
    os_ = os if os is not None else (8 if mode in ("hard", "fold") else 4)
    win = ("kaiser", 9.0)
    u = signal.resample_poly(x, os_, 1, axis=0, window=win) if os_ > 1 else x
    y = _curve(u, mode, g, bias, os_)
    y = signal.resample_poly(y, 1, os_, axis=0, window=win)[: len(x)] if os_ > 1 else y
    if mode == "tape":
        y = dsp.butter(y, "low", 17000.0, 1)
    if bias != 0 or mode == "tape":
        y = dsp.butter(y, "high", 8.0, 1)
    y = _match_level(y, x, level)
    return mix * y + (1 - mix) * x


def _slow_rise(ef: np.ndarray, att: float) -> np.ndarray:
    out = np.empty(len(ef))
    s = 0.0
    for i, v in enumerate(ef.tolist()):
        s = min(att * s + (1 - att) * v, v)
        out[i] = s
    return out


def _db_ratio(a: np.ndarray, b: np.ndarray, scale: float) -> np.ndarray:
    return np.clip(20 * np.log10((a + 1e-9) / (b + 1e-9)) / scale, 0, 1)


def transient(x: np.ndarray, attack_db: float = 3.0, sustain_db: float = 0.0, attack_ms: float = 8.0,
              speed: float = 1.0, floor_db: float = -60.0) -> np.ndarray:
    """Level-independent attack and sustain shaper, stereo linked."""
    x = np.asarray(x, dtype=float)
    m = np.abs(x).max(axis=1) if x.ndim == 2 else np.abs(x)
    s = speed
    ef = _follow(m, _coef(0.1 * s), _coef(15 * s))
    es = _slow_rise(ef, _coef(attack_ms * s))
    a = _db_ratio(ef, es, 6.0)
    el = _follow(m, _coef(0.3 * s), _coef(350 * s))
    eq = _follow(m, _coef(0.3 * s), _coef(25 * s))
    d = _db_ratio(el, eq, 9.0) * (1 - a)
    gate = np.clip((20 * np.log10(el / (m.max() + 1e-12) + 1e-12) - floor_db) / 12, 0, 1)
    gain = attack_db * a + sustain_db * d * gate
    lo = gain.min() if len(gain) else 0.0
    gain = _follow(gain - lo, _coef(0.2), _coef(5)) + lo
    g = 10 ** (gain / 20)
    return x * (g[:, None] if x.ndim == 2 else g)


def _release(g: np.ndarray, coef: float) -> np.ndarray:
    out = np.empty(len(g))
    s = 1.0
    for i, v in enumerate(g.tolist()):
        s = 1 - (1 - s) * coef
        if v < s:
            s = v
        out[i] = s
    return out


def limit(x: np.ndarray, ceiling_db: float = -1.0, lookahead: float = 0.0025, release: float = 0.08,
          true_peak: bool = True) -> np.ndarray:
    """Offline look-ahead peak limiter with a true-peak detector."""
    x = np.asarray(x, dtype=float)
    c = 10 ** (ceiling_db / 20)
    pk = np.abs(x).max(axis=1) if x.ndim == 2 else np.abs(x)
    if true_peak and len(x) > 1:
        up = np.abs(signal.resample_poly(x, 4, 1, axis=0))
        up = up.max(axis=1) if up.ndim == 2 else up
        pk = np.maximum(pk, up[: 4 * len(x)].reshape(len(x), 4).max(axis=1))
    need = np.minimum(1.0, c / np.maximum(pk, 1e-12))
    w = 2 * max(1, n_of(lookahead)) + 1
    g = ndimage.minimum_filter1d(need, w, mode="nearest")
    g = _release(g, _coef(release * 1000))
    g = ndimage.uniform_filter1d(g, w, mode="nearest")
    y = x * (g[:, None] if x.ndim == 2 else g)
    return np.clip(y, -c, c)


def _lr4(x: np.ndarray, kind: str, f: float) -> np.ndarray:
    return dsp.butter(dsp.butter(x, kind, f, 2), kind, f, 2)


def split(x: np.ndarray, freqs: Any) -> list[np.ndarray]:
    """Linkwitz-Riley 4th-order band split, bands low to high."""
    bands: list[np.ndarray] = []
    rest = np.asarray(x, dtype=float)
    for f in sorted(freqs):
        bands = [_lr4(b, "low", f) + _lr4(b, "high", f) for b in bands]
        bands.append(_lr4(rest, "low", f))
        rest = _lr4(rest, "high", f)
    return bands + [rest]


# ---------------------------------------------------------------- EQ match

def _target_profile(target: Any) -> np.ndarray:
    from hark.forge.engine import tone

    if isinstance(target, tone.ToneRef):
        return np.asarray(target.median, dtype=float)
    if isinstance(target, (list, tuple)) and target:
        return np.asarray(tone.ToneRef(list(target)).median, dtype=float)
    a = np.asarray(target, dtype=float)
    if a.ndim == 1 and len(a) == len(tone.CEN):
        return a
    return tone.profile(a)


def match_curve(x: np.ndarray, target: Any, amount: float = 1.0, max_db: float = 12.0, smooth: int = 1) -> list:
    """28 band gains in dB that move x toward the target profile."""
    from hark.forge.engine import tone

    p = tone.profile(x)
    d = _target_profile(target) - p
    w = 10 ** (p / 10)
    d = d - np.sum(w * d) / np.sum(w)
    d = np.where(p < -50, np.minimum(d, 6.0), d)
    d = np.clip(d, -max_db, max_db)
    for _ in range(int(smooth)):
        e = np.concatenate([[d[0]], d, [d[-1]]])
        d = 0.25 * e[:-2] + 0.5 * e[1:-1] + 0.25 * e[2:]
    return [round(float(v), 2) for v in amount * d]


def _min_phase(h_mag: np.ndarray, nf: int, taps: int) -> np.ndarray:
    c = np.fft.ifft(np.log(np.maximum(h_mag, 1e-12))).real
    f = np.zeros(nf)
    f[0] = c[0]
    f[1:nf // 2] = 2 * c[1:nf // 2]
    f[nf // 2] = c[nf // 2]
    h = np.fft.ifft(np.exp(np.fft.fft(f))).real[:taps]
    win = np.ones(taps)
    win[taps // 2:] = np.hanning(2 * (taps - taps // 2))[taps - taps // 2:]
    return h * win


def eq_bands(x: np.ndarray, gains_db: Any, phase: str = "min", taps: int = 4096) -> np.ndarray:
    """FIR EQ from 28 band gains; minimum phase by default."""
    from hark.forge.engine import tone

    x = np.asarray(x, dtype=float)
    nf = 4 * taps
    f = np.fft.fftfreq(nf, 1.0 / SR)
    g = np.interp(np.log2(np.maximum(np.abs(f), 1)), np.log2(tone.CEN), np.asarray(gains_db, float))
    mag = 10 ** (g / 20)
    if phase == "linear":
        h = np.roll(np.fft.ifft(mag).real, taps // 2)[:taps] * np.hanning(taps)
        delay = taps // 2
    else:
        h = _min_phase(mag, nf, taps)
        delay = 0
    kern = h if x.ndim == 1 else h[:, None]
    y = signal.fftconvolve(x, kern, axes=0) if x.ndim == 2 else signal.fftconvolve(x, h)
    return y[delay:delay + len(x)]


def eq_match(x: np.ndarray, target: Any, amount: float = 1.0, max_db: float = 12.0, smooth: int = 1,
             phase: str = "min", iters: int = 2) -> np.ndarray:
    """Iterative 1/3-octave EQ match to a target, RMS kept."""
    x = np.asarray(x, dtype=float)
    tot = np.zeros(28)
    y = x
    for _ in range(max(1, int(iters))):
        tot = np.clip(tot + np.array(match_curve(y, target, 1.0, max_db, smooth)), -max_db, max_db)
        y = eq_bands(x, tot, phase)
    if amount != 1:
        y = eq_bands(x, amount * tot, phase)
    return y * _rms(x) / _rms(y)


# ---------------------------------------------------------------- motion

def _emission_times(t: np.ndarray, t0: float, v: float, dist: float, c: float) -> np.ndarray:
    b = t - t0
    return t0 + (c * c * b - np.sqrt(v * v * c * c * b * b + dist * dist * (c * c - v * v))) / (c * c - v * v)


def doppler(x: np.ndarray, speed: float = 40.0, dist: float = 4.0, t_close: float | None = None, air: float = 1.0,
            width: float = 0.8, stereo: bool = True, c: float = 343.0, floor_db: float = -30.0) -> np.ndarray:
    """A source passing the listener on a straight line."""
    xm = _mono(x)
    dur = len(xm) / SR
    t0 = dur / 2 if t_close is None else t_close
    v = float(np.clip(speed, 1e-3, 0.95 * c))

    def dist_at(te: Any) -> Any:
        return np.hypot(v * (te - t0), dist)

    t_first, t_last = dist_at(0.0) / c, dur + dist_at(dur) / c
    t = t_first + np.arange(int((t_last - t_first) * SR)) / SR
    te = _emission_times(t, t0, v, dist, c)
    rr = dist_at(te)
    y = _hermite(xm, te * SR) * np.maximum(dist / rr, 10 ** (floor_db / 20))
    att = air * 0.17 * np.maximum(rr - dist, 0)
    fc = np.clip(10000 / np.maximum(10 ** (att / 10) - 1, 1e-9) ** 0.25, 200, 0.45 * SR)
    y = dsp.svf(y, fc, 0.707, "low")
    if not stereo:
        return y
    s = np.clip(v * (te - t0) / rr, -1, 1) * width
    theta = (s + 1) * np.pi / 4
    return np.column_stack([y * np.cos(theta), y * np.sin(theta)]) * np.sqrt(2)


def decorrelate(x: np.ndarray, amount: float = 1.0, cut: float = 250.0) -> np.ndarray:
    """Mono to stereo with an all-pass side signal above cut Hz."""
    xm = _mono(x)
    side = xm
    for d, g in ((149, 0.6), (263, -0.55), (397, 0.5)):
        side = _allpass(side, d, g)
    side = dsp.butter(side, "high", cut, 2)
    side = side * 0.7 * _rms(xm) / _rms(side)
    norm = np.sqrt(1 + 0.49 * amount ** 2)
    return np.column_stack([xm + amount * side, xm - amount * side]) / norm


def pan(x: np.ndarray, pos: Any) -> np.ndarray:
    """Constant-power pan along a control curve."""
    x = np.asarray(x, dtype=float)
    p = np.clip(dsp.curve(pos, len(x)), -1, 1)
    theta = (p + 1) * np.pi / 4
    gl, gr = np.cos(theta) * np.sqrt(2), np.sin(theta) * np.sqrt(2)
    if x.ndim == 1:
        return np.column_stack([x * gl, x * gr])
    return np.column_stack([x[:, 0] * gl, x[:, 1] * gr])


def width(x: np.ndarray, w: Any) -> np.ndarray:
    """Mid/side width: 0 mono, 1 unchanged, 2 wider."""
    x = np.asarray(x, dtype=float)
    if x.ndim == 1:
        x = decorrelate(x, 1.0)
    k = np.clip(dsp.curve(w, len(x)), 0, 2)
    mid = (x[:, 0] + x[:, 1]) / 2
    side = (x[:, 0] - x[:, 1]) / 2 * k
    return np.column_stack([mid + side, mid - side])


def sweep_filter(x: np.ndarray, f0: float, f1: float, q: float = 4.0, curve_: str = "exp", power: float = 1.0,
                 mode: str = "bandn") -> np.ndarray:
    """State-variable filter that sweeps from f0 to f1."""
    x = np.asarray(x, dtype=float)
    tr = dsp.sweep(f0, f1, len(x) / SR, curve_, power)
    tr = np.concatenate([tr, np.full(max(0, len(x) - len(tr)), tr[-1])])[: len(x)]
    return dsp.svf(x, tr, q, mode)


def _read_track(x: np.ndarray, track: np.ndarray) -> np.ndarray:
    pos = np.concatenate([[0.0], np.cumsum(track[:-1])])
    pos = pos[pos < len(x) - 1]
    return _hermite(x, pos)


def varispeed(x: np.ndarray, speed: Any) -> np.ndarray:
    """Read x at a speed curve: 2 is an octave up, 0 is stopped."""
    x = np.asarray(x, dtype=float)
    if np.isscalar(speed):
        n = int(len(x) / max(float(speed), 1e-3))
    elif isinstance(speed, (list, tuple)) and speed and isinstance(speed[0], (list, tuple)):
        n = n_of(max(p[0] for p in speed))
    else:
        n = len(speed)
    return _read_track(x, np.maximum(dsp.curve(speed, n), 0))


def tape_stop(x: np.ndarray, start: float | None = None, seconds: float = 0.6, curve_: float = 1.6) -> np.ndarray:
    """Tape slowing to a stop."""
    x = np.asarray(x, dtype=float)
    st = max(0.0, len(x) / SR - 0.6 * seconds) if start is None else start
    track = np.concatenate([np.ones(n_of(st)), (1 - np.linspace(0, 1, n_of(seconds))) ** curve_])
    y = _read_track(x, track)
    k = min(len(y), n_of(0.005))
    if k:
        w = np.linspace(1, 0, k)
        y[len(y) - k:] *= w if y.ndim == 1 else w[:, None]
    return y


# ---------------------------------------------------------------- glitch and grains

def crush(x: np.ndarray, bits: int | None = 8, rate: float | None = None, mix: float = 1.0) -> np.ndarray:
    """Sample-rate reduction and bit crush, no dither."""
    x = np.asarray(x, dtype=float)
    y = x
    if rate is not None:
        i = np.arange(len(x))
        idx = np.minimum(np.floor(np.floor(i * rate / SR) * SR / rate).astype(int), len(x) - 1)
        y = y[idx]
    if bits is not None:
        q = 2 ** (bits - 1)
        y = np.round(y * q) / q
    return mix * y + (1 - mix) * x


def _stutter_copy(sl: np.ndarray, k: int, pitch_step: float, gate: float, fade_n: int, decay_db: float):
    if pitch_step:
        rate = 2 ** (k * pitch_step / 12)
        c = _hermite(sl, np.arange(0, len(sl), rate)[: len(sl)])
    else:
        c = sl.copy()
    g = max(2, int(len(sl) * gate))
    c[g:] = 0
    c[:g] = _edge_fade(c[:g], fade_n)
    return c * 10 ** (k * decay_db / 20)


def stutter(x: np.ndarray, start: float, length: float, repeats: int = 4, gate: float = 0.85,
            decay_db: float = 0.0, pitch_step: float = 0.0, fade: float = 0.002) -> np.ndarray:
    """Repeat one slice of x, gated, with optional pitch steps and decay."""
    x = np.asarray(x, dtype=float)
    i0 = n_of(start) if start > 0 else 0
    sl = x[i0:i0 + n_of(length)]
    copies = [_stutter_copy(sl, k, pitch_step, gate, n_of(fade), decay_db) for k in range(int(repeats))]
    return np.concatenate([x[:i0]] + copies + [x[i0 + len(sl):]])


def _octave_up(sl: np.ndarray, k: int) -> np.ndarray:
    h = _edge_fade(sl[::2], k)
    out = np.concatenate([h, h])[: len(sl)]
    if len(out) < len(sl):
        out = np.concatenate([out, np.zeros((len(sl) - len(out),) + sl.shape[1:])])
    return out


def _glitch_slice(sl: np.ndarray, u: float, th: np.ndarray, prev: np.ndarray | None, k: int) -> np.ndarray:
    if u < th[0] and prev is not None and len(prev) == len(sl):
        return prev.copy()
    if u < th[1]:
        return sl[::-1].copy()
    if u < th[2]:
        return np.zeros_like(sl)
    if u < th[3]:
        return _octave_up(sl, k)
    if u < th[4]:
        return crush(sl, 6, 6000)
    return sl.copy()


def glitch(x: np.ndarray, r: np.random.Generator, grid: float = 0.03, repeat: float = 0.25, reverse: float = 0.1,
           drop: float = 0.08, octave: float = 0.08, crush_: float = 0.08, fade: float = 0.0015) -> np.ndarray:
    """Random slice edits on a time grid: repeat, reverse, drop, octave up, crush."""
    x = np.asarray(x, dtype=float)
    ln, k = max(16, n_of(grid)), n_of(fade)
    th = np.cumsum([repeat, reverse, drop, octave, crush_])
    out = np.zeros_like(x)
    prev = None
    for i in range(0, len(x), ln):
        sl = x[i:i + ln]
        s = _edge_fade(_glitch_slice(sl, r.uniform(), th, prev, k), k)
        out[i:i + len(s)] = s
        prev = s
    return out


def _pos_control(pos: Any, n: int) -> np.ndarray:
    if isinstance(pos, tuple) and len(pos) == 2 and all(np.isscalar(p) for p in pos):
        return np.linspace(pos[0], pos[1], n)
    return dsp.curve(pos, n)


def granular(src: np.ndarray, seconds: float, r: np.random.Generator, grain: float = 0.06, density: float = 40.0,
             pos: Any = (0.0, 1.0), pos_jitter: float = 0.02, pitch: float = 0.0, pitch_jitter: float = 0.0,
             time_jitter: float = 0.5, spread: float = 0.6, reverse: float = 0.0) -> np.ndarray:
    """Stereo granular cloud read from a source."""
    s = _mono(src)
    n = n_of(seconds)
    out = np.zeros((n, 2))
    pc = _pos_control(pos, n)
    period = 1.0 / max(density, 1e-3)
    gl = n_of(grain)
    win = np.hanning(gl)
    t = 0.0
    while t < seconds:
        tt = t + time_jitter * period * r.uniform(-0.5, 0.5)
        i = n_of(tt) if tt > 0 else 0
        if i < n:
            _add_grain(out, s, r, i, pc, gl, win, (pitch, pitch_jitter, pos_jitter, reverse, spread))
        t += period
    return out / np.sqrt(max(1.0, density * grain * 0.375))


def _add_grain(out: np.ndarray, s: np.ndarray, r: np.random.Generator, i: int, pc: np.ndarray, gl: int,
               win: np.ndarray, knobs: tuple) -> None:
    pitch, pitch_jitter, pos_jitter, reverse, spread = knobs
    rate = 2 ** ((pitch + pitch_jitter * r.standard_normal()) / 12)
    p0 = (pc[min(i, len(out) - 1)] + pos_jitter * r.standard_normal()) * (len(s) - gl * rate)
    start = np.clip(p0, 0, max(0.0, len(s) - gl * rate - 2))
    g = _hermite(s, start + np.arange(gl) * rate) * win
    if r.uniform() < reverse:
        g = g[::-1]
    theta = (np.clip(spread * r.uniform(-1, 1), -1, 1) + 1) * np.pi / 4
    m = min(gl, len(out) - i)
    out[i:i + m, 0] += g[:m] * np.cos(theta) * np.sqrt(2)
    out[i:i + m, 1] += g[:m] * np.sin(theta) * np.sqrt(2)


def reverse_swell(hit: np.ndarray, r: np.random.Generator, seconds: float = 1.0, rt60: float = 2.5,
                  size: float = 0.8, damp: float = 0.4, swell_db: float = -6.0, gap: float = 0.0,
                  hit_room: float = 0.15) -> np.ndarray:
    """A reversed reverb swell that leads into the hit."""
    hm = _mono(hit)
    wet = reverb(hm, r, rt60, size, predelay=0.0, damp=damp, wet=1.0, dry=0.0, stereo=True,
                 tail=max(seconds, rt60) + 0.1)
    k = n_of(seconds)
    sw = wet[:k][::-1].copy()
    kf = min(k, n_of(0.35 * seconds))
    sw[:kf] *= (np.linspace(0, 1, kf) ** 2)[:, None]
    if hit_room > 0:
        after = reverb(hm, r, 0.6 * rt60, size, predelay=0.0, damp=damp, wet=hit_room, dry=1.0, stereo=True)
    else:
        after = dsp.stereo_of(hm)
    sw = sw * np.max(np.abs(after)) * 10 ** (swell_db / 20) / (np.max(np.abs(sw)) + 1e-12)
    g = n_of(gap) if gap > 0 else 0
    out = np.zeros((k + g + len(after), 2))
    out[: len(sw)] += sw
    out[k + g:] += after
    return out
