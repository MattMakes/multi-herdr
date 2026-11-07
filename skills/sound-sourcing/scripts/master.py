#!/usr/bin/env python3
"""Master a premix to a loudness target and measure it (BS.1770).

Usage: master.py INPUT [OUTPUT] [--measure] [--lufs -14] [--ceiling 0.77] ...
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import json
import math
import sys

import numpy as np
from scipy import signal
from scipy.ndimage import maximum_filter1d, minimum_filter1d

SR = 48000
NEG_INF = float("-inf")

# BS.1770 K-weighting at 48 kHz: the high shelf, then the RLB high-pass.
_K1 = (np.array([1.53512485958697, -2.69169618940638, 1.19839281085285]),
       np.array([1.0, -1.69065929318241, 0.73248077421585]))
_K2 = (np.array([1.0, -2.0, 1.0]), np.array([1.0, -1.99004745483398, 0.99007225036621]))


def db(x: float, floor: float = -200.0) -> float:
    return 20.0 * math.log10(x) if x > 0 else floor


def as_stereo(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, dtype=np.float64)
    if x.ndim == 1:
        x = x[:, None]
    if x.shape[1] == 1:
        x = np.repeat(x, 2, axis=1)
    return x[:, :2]


# ---------------------------------------------------------------- measurement

def kweight(x: np.ndarray) -> np.ndarray:
    y = signal.lfilter(_K1[0], _K1[1], x, axis=0)
    return signal.lfilter(_K2[0], _K2[1], y, axis=0)


def _block_power(k: np.ndarray, win: float, hop: float) -> tuple[np.ndarray, np.ndarray]:
    """Per-block summed channel mean square of a K-weighted signal, and block centres."""
    n = len(k)
    w, h = int(round(win * SR)), int(round(hop * SR))
    if n < w:
        return np.array([0.0]), np.array([n / 2 / SR])
    csum = np.vstack([np.zeros((1, k.shape[1])), np.cumsum(k * k, axis=0)])
    starts = np.arange(0, n - w + 1, h)
    power = ((csum[starts + w] - csum[starts]) / w).sum(axis=1)
    return power, (starts + w / 2) / SR


def _to_lufs(power: np.ndarray) -> np.ndarray:
    with np.errstate(divide="ignore"):
        lv = -0.691 + 10.0 * np.log10(power)
    return np.where(power > 0, lv, -70.0)


def block_loudness(x: np.ndarray, win: float, hop: float, kw: np.ndarray | None = None):
    k = kweight(as_stereo(x)) if kw is None else kw
    p, t = _block_power(k, win, hop)
    return _to_lufs(p), t


def integrated(x: np.ndarray, kw: np.ndarray | None = None) -> float:
    """Gated integrated loudness (absolute -70 LUFS, relative -10 LU)."""
    k = kweight(as_stereo(x)) if kw is None else kw
    if len(k) < int(0.4 * SR):
        return NEG_INF
    p, _ = _block_power(k, 0.4, 0.1)
    with np.errstate(divide="ignore"):
        lv = -0.691 + 10.0 * np.log10(p)
    keep = p[lv > -70.0]
    if not len(keep):
        return NEG_INF
    rel = -0.691 + 10.0 * math.log10(keep.mean()) - 10.0
    with np.errstate(divide="ignore"):
        lk = -0.691 + 10.0 * np.log10(keep)
    keep = keep[lk > rel]
    if not len(keep):
        return NEG_INF
    return -0.691 + 10.0 * math.log10(keep.mean())


def lra(x: np.ndarray, kw: np.ndarray | None = None) -> float:
    st, _ = block_loudness(x, 3.0, 0.1, kw)
    st = st[st > -70.0]
    if len(st) < 2:
        return 0.0
    gate = 10.0 * math.log10(np.mean(10.0 ** (st / 10.0))) - 20.0
    st = st[st > gate]
    if len(st) < 2:
        return 0.0
    return float(np.percentile(st, 95) - np.percentile(st, 10))


def true_peak(x: np.ndarray) -> float:
    up = signal.resample_poly(as_stereo(x), 4, 1, axis=0)
    return db(float(np.max(np.abs(up))) if len(up) else 0.0)


def measure(x: np.ndarray) -> dict:
    x = as_stereo(x)
    kw = kweight(x)
    mom, mt = block_loudness(x, 0.4, 0.1, kw)
    i = int(np.argmax(mom))
    tail = x[-int(0.05 * SR):]
    return {
        "lufs": integrated(x, kw),
        "true_peak_db": true_peak(x),
        "sample_peak_db": db(float(np.max(np.abs(x))) if len(x) else 0.0),
        "lra": lra(x, kw),
        "momentary_max": float(mom[i]),
        "momentary_max_t": float(mt[i]),
        "tail_50ms_db": db(float(np.sqrt(np.mean(tail ** 2))) if len(tail) else 0.0),
        "last_480_zero": bool(len(x) >= 480 and not np.any(x[-480:])),
        "seconds": len(x) / SR,
    }


# ---------------------------------------------------------------- processing

def raised_cosine(n: int) -> np.ndarray:
    if n <= 1:
        return np.ones(max(n, 0))
    return 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / (n - 1))


def _butter(x, order, hz, kind):
    sos = signal.butter(order, hz, btype=kind, fs=SR, output="sos")
    return signal.sosfilt(sos, x, axis=0)


def mono_below(x: np.ndarray, hz: float) -> np.ndarray:
    if hz <= 0:
        return x
    mid = (x[:, 0] + x[:, 1]) / 2
    side = _butter((x[:, 0] - x[:, 1]) / 2, 2, hz, "highpass")
    return np.stack([mid + side, mid - side], axis=1)


def _block_reduce(v: np.ndarray, size: int, fn) -> np.ndarray:
    pad = (-len(v)) % size
    if pad:
        v = np.concatenate([v, np.full(pad, v[-1])])
    return fn(v.reshape(-1, size), axis=1)


def _to_samples(gblocks: np.ndarray, size: int, n: int) -> np.ndarray:
    centres = np.arange(len(gblocks)) * size + (size - 1) / 2
    return np.interp(np.arange(n), centres, gblocks)


def compressor(x: np.ndarray, thr=-14.0, ratio=2.0, att=0.015, rel=0.12, det=0.005, blk=48):
    d = np.mean(x * x, axis=1)
    d = _butter(d, 1, 1.0 / (2 * math.pi * det), "lowpass")
    d = _block_reduce(np.maximum(d, 1e-20), blk, np.max)
    with np.errstate(divide="ignore"):
        over = np.maximum(0.0, 10.0 * np.log10(np.maximum(d, 1e-20)) - thr)
    target = -over * (1 - 1 / ratio)
    ca, cr = math.exp(-blk / (att * SR)), math.exp(-blk / (rel * SR))
    g, out = 0.0, np.empty_like(target)
    for i, t in enumerate(target):
        c = ca if t < g else cr
        g = t + (g - t) * c
        out[i] = g
    return x * (10.0 ** (_to_samples(out, blk, len(x)) / 20.0))[:, None]


def glue(x: np.ndarray, drive=1.15, air=0.25) -> np.ndarray:
    y = np.tanh(0.9 * drive * x) / math.tanh(drive)
    return y + air * _butter(y, 1, 2500, "highpass")


def limiter_gain(x: np.ndarray, ceiling=0.77, look=0.004, rel=0.08, blk=16) -> np.ndarray:
    """Per-sample gain of the 4x oversampled lookahead true-peak limiter."""
    n = len(x)
    up = np.abs(signal.resample_poly(x, 4, 1, axis=0)).max(axis=1)[: 4 * n]
    if len(up) < 4 * n:
        up = np.concatenate([up, np.zeros(4 * n - len(up))])
    peak = up.reshape(n, 4).max(axis=1)
    L = max(16, int(look * SR))
    ahead = maximum_filter1d(peak, size=L, origin=-(L // 2), mode="nearest")
    need = np.minimum(1.0, ceiling / np.maximum(ahead, 1e-12))
    g = _block_reduce(need, blk, np.min)
    k = max(3, L // blk)
    win = signal.windows.hann(k + 2)[1:-1]
    win /= win.sum()
    padded = np.pad(g, (k // 2, k - 1 - k // 2), mode="edge")
    g = np.minimum(g, np.convolve(padded, win, mode="valid"))
    coef = math.exp(-blk / (rel * SR))
    out, cur = np.empty_like(g), 1.0
    for i, v in enumerate(g):
        cur = v if v < cur else v + (cur - v) * coef
        out[i] = cur
    out = minimum_filter1d(out, size=3, mode="nearest")
    return np.minimum(_to_samples(out, blk, n), need)


def sidechain_gain(key: np.ndarray, n: int, depth=0.55, tau=0.14, blk=48) -> np.ndarray:
    k = np.abs(as_stereo(key)).max(axis=1)
    k = np.concatenate([k, np.zeros(max(0, n - len(k)))])[:n]
    b = _block_reduce(k, blk, np.max)
    nz = b[b > 0]
    b = b / (np.percentile(nz, 99.9) if len(nz) else 1.0)
    coef, env, e = math.exp(-blk / (tau * SR)), np.empty_like(b), 0.0
    for i, v in enumerate(b):
        e = v if v > e else e * coef
        env[i] = e
    return _to_samples(1.0 - depth * np.clip(env, 0, 1), blk, n)


def duck_times(n: int, hits, depth=0.55, tau=0.14) -> np.ndarray:
    """Time-list ducker: each (t, weight) pulls the gain down; overlaps keep the lowest gain."""
    g = np.ones(n)
    m = int(0.6 * SR)
    shape = np.exp(-np.arange(m) / SR / tau)
    a = int(0.003 * SR)
    shape[:a] *= raised_cosine(a)
    for t, w in hits:
        s = int(round(t * SR))
        lo, hi = max(0, s), min(n, s + m)
        if lo < hi:
            g[lo:hi] = np.minimum(g[lo:hi], 1.0 - depth * w * shape[lo - s:hi - s])
    return g


def finish(x, lufs=-14.0, ceiling=0.77, mono_hz=120.0, comp=True, use_glue=False,
           fade=0.0, tail_zero=480, end_fade=0.06):
    """The full chain. Returns (master, max limiter gain reduction in dB)."""
    x = _butter(as_stereo(x), 2, 28, "highpass")
    x = mono_below(x, mono_hz)
    if comp:
        x = compressor(x)
    if use_glue:
        x = glue(x)
    n = len(x)
    if fade > 0:
        t = np.arange(n) / SR
        x = x * (np.clip((n / SR - 0.03 - t) / fade, 0, 1) ** 2)[:, None]
    max_gr = 0.0
    for p in range(8):
        cur = integrated(x)
        if math.isfinite(cur):
            x = x * 10.0 ** ((lufs - cur) / 20.0)
        g = limiter_gain(x, ceiling)
        max_gr = max(max_gr, -db(float(g.min())))
        x = x * g[:, None]
        cur = integrated(x)
        if p >= 2 and (not math.isfinite(cur) or abs(cur - lufs) <= 0.05):
            break
    h = min(n, int(0.003 * SR))
    x[:h] *= raised_cosine(h)[:, None]
    t = min(n, int(max(end_fade, 0.01) * SR))
    x[n - t:] *= raised_cosine(t)[::-1][:, None]
    if tail_zero > 0:
        x[max(0, n - tail_zero):] = 0.0
    return x, max_gr


def gate(rep: dict, target: float, tol: float, max_tp: float) -> list[str]:
    fails = []
    if not math.isfinite(rep["lufs"]) or abs(rep["lufs"] - target) > tol:
        fails.append(f"loudness {rep['lufs']:.2f} LUFS is outside {target:.1f} +-{tol} LU")
    if rep["true_peak_db"] > max_tp:
        fails.append(f"true peak {rep['true_peak_db']:.2f} dBTP is above {max_tp} dBTP")
    if not rep["last_480_zero"]:
        fails.append("the last 480 samples are not digital zero")
    return fails


def read_audio(path: str) -> np.ndarray:
    import soundfile as sf
    x, rate = sf.read(path, dtype="float64", always_2d=True)
    x = as_stereo(x)
    if rate != SR:
        from fractions import Fraction
        f = Fraction(SR, rate).limit_denominator(1000)
        x = signal.resample_poly(x, f.numerator, f.denominator, axis=0)
    return x


def _clean(v):
    if isinstance(v, float) and not math.isfinite(v):
        return None
    return v


def run(args) -> int:
    x = read_audio(args.input)
    if args.measure:
        rep = measure(x)
    else:
        if not args.output:
            raise ValueError("OUTPUT is required unless --measure")
        if args.sidechain:
            x = x * sidechain_gain(read_audio(args.sidechain), len(x), args.sc_depth, args.sc_tau)[:, None]
        y, gr = finish(x, args.lufs, args.ceiling, args.mono_hz, not args.no_comp, not args.no_glue,
                       args.fade, args.tail_zero)
        import soundfile as sf
        sf.write(args.output, y.astype(np.float32), SR, subtype="PCM_24")
        rep = measure(sf.read(args.output, dtype="float64", always_2d=True)[0])
        rep["limiter_max_gr_db"] = gr
        rep["output"] = args.output
    rep["fails"] = gate(rep, args.lufs, args.tol, args.max_tp)
    rep["pass"] = not rep["fails"]
    rep = {k: _clean(v) for k, v in rep.items()}
    word = "PASS" if rep["pass"] else "FAIL"
    f = lambda v: "n.a." if v is None else f"{v:.2f}"
    print(f"{word}  {f(rep['lufs'])} LUFS  TP {f(rep['true_peak_db'])} dBTP  LRA {f(rep['lra'])} LU  "
          f"loudest {f(rep['momentary_max'])} LUFS-M at {rep['momentary_max_t']:.2f}s")
    for line in rep["fails"]:
        print("  " + line)
    if args.json == "-":
        print(json.dumps(rep, indent=1))
    elif args.json:
        with open(args.json, "w") as fh:
            json.dump(rep, fh, indent=1)
    return 0 if rep["pass"] else 1


def self_test() -> int:
    import os
    import tempfile
    import soundfile as sf
    rng = np.random.default_rng(7)
    ok = True

    def check(name, cond):
        nonlocal ok
        ok &= bool(cond)
        print(("ok   " if cond else "FAIL ") + name)

    # 1 kHz sine at -20 dBFS peak per channel reads close to the BS.1770 reference.
    t = np.arange(SR * 5) / SR
    sine = 10 ** (-20 / 20) * np.sin(2 * np.pi * 1000 * t)
    check("1 kHz stereo sine meter", abs(integrated(np.stack([sine, sine], 1)) + 20.0) < 0.1)
    with tempfile.TemporaryDirectory() as d:
        noise = rng.standard_normal((SR * 10, 2))
        noise = signal.lfilter([0.05], [1, -0.95], noise, axis=0)  # darker than white
        noise *= 10 ** ((-30 - integrated(noise)) / 20)
        src, out = os.path.join(d, "in.wav"), os.path.join(d, "out.wav")
        sf.write(src, noise.astype(np.float32), SR, subtype="FLOAT")
        ns = parse([src, out, "--json", os.path.join(d, "m.json")])
        code = run(ns)
        y = sf.read(out, always_2d=True)[0]
        rep = measure(y)
        check("M10 exit 0", code == 0)
        check("M10 loudness -14 +-0.5", abs(rep["lufs"] + 14) <= 0.5)
        check("M10 true peak <= -1.5 dBTP", rep["true_peak_db"] <= -1.5)
        check("M10 last 480 zero, first sample 0", rep["last_480_zero"] and not np.any(y[0]))
        sf.write(src, noise.astype(np.float32), SR, subtype="FLOAT")
        check("M11 --measure on an unmastered tail fails", run(parse([src, "--measure"])) == 1)
    return 0 if ok else 1


def parse(argv):
    p = argparse.ArgumentParser(prog="master.py", description="Master a premix and measure it.")
    p.add_argument("input", nargs="?")
    p.add_argument("output", nargs="?")
    p.add_argument("--measure", action="store_true")
    p.add_argument("--lufs", type=float, default=-14.0)
    p.add_argument("--ceiling", type=float, default=0.77)
    p.add_argument("--tol", type=float, default=0.5)
    p.add_argument("--max-tp", type=float, default=-1.5)
    p.add_argument("--sidechain")
    p.add_argument("--sc-depth", type=float, default=0.55)
    p.add_argument("--sc-tau", type=float, default=0.14)
    p.add_argument("--mono-hz", type=float, default=120.0)
    p.add_argument("--no-comp", action="store_true")
    p.add_argument("--no-glue", action="store_true")
    p.add_argument("--fade", type=float, default=0.0)
    p.add_argument("--tail-zero", type=int, default=480)
    p.add_argument("--json")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def main(argv=None) -> int:
    args = parse(sys.argv[1:] if argv is None else argv)
    if args.self_test:
        return self_test()
    if not args.input:
        print("error: INPUT is required", file=sys.stderr)
        return 1
    try:
        return run(args)
    except Exception as e:  # one line, exit 1
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
