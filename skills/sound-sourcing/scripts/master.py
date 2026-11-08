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
SILENT_LUFS = float("-inf")

# BS.1770 K-weighting at 48 kHz: the high shelf, then the RLB high-pass.
_SHELF_BA = (np.array([1.53512485958697, -2.69169618940638, 1.19839281085285]),
       np.array([1.0, -1.69065929318241, 0.73248077421585]))
_RLB_BA = (np.array([1.0, -2.0, 1.0]), np.array([1.0, -1.99004745483398, 0.99007225036621]))


def db(sig: float, floor: float = -200.0) -> float:
    return 20.0 * math.log10(sig) if sig > 0 else floor


def as_stereo(sig: np.ndarray) -> np.ndarray:
    sig = np.asarray(sig, dtype=np.float64)
    if sig.ndim == 1:
        sig = sig[:, None]
    if sig.shape[1] == 1:
        sig = np.repeat(sig, 2, axis=1)
    return sig[:, :2]


# ---------------------------------------------------------------- measurement

def kweight(sig: np.ndarray) -> np.ndarray:
    shaped = signal.lfilter(_SHELF_BA[0], _SHELF_BA[1], sig, axis=0)
    return signal.lfilter(_RLB_BA[0], _RLB_BA[1], shaped, axis=0)


def _per_block_energy(kw_sig: np.ndarray, win_s: float, hop_s: float) -> tuple[np.ndarray, np.ndarray]:
    """Per-block summed channel mean square of a K-weighted signal, and block centres."""
    total = len(kw_sig)
    span, step = int(round(win_s * SR)), int(round(hop_s * SR))
    if total < span:
        return np.array([0.0]), np.array([total / 2 / SR])
    running = np.vstack([np.zeros((1, kw_sig.shape[1])), np.cumsum(kw_sig * kw_sig, axis=0)])
    heads = np.arange(0, total - span + 1, step)
    energy = ((running[heads + span] - running[heads]) / span).sum(axis=1)
    return energy, (heads + span / 2) / SR


def _energy_to_lufs(energy: np.ndarray) -> np.ndarray:
    with np.errstate(divide="ignore"):
        level = -0.691 + 10.0 * np.log10(energy)
    return np.where(energy > 0, level, -70.0)


def block_loudness(sig: np.ndarray, win: float, hop: float, kw: np.ndarray | None = None):
    kw_sig = kweight(as_stereo(sig)) if kw is None else kw
    energy, centres = _per_block_energy(kw_sig, win, hop)
    return _energy_to_lufs(energy), centres


def _louder_than(energy: np.ndarray, floor_lu: float) -> np.ndarray:
    """The block powers whose loudness is above threshold."""
    with np.errstate(divide="ignore"):
        level = -0.691 + 10.0 * np.log10(energy)
    return energy[level > floor_lu]


def _pooled_lufs(energy: np.ndarray) -> float:
    return -0.691 + 10.0 * math.log10(energy.mean())


def integrated(sig: np.ndarray, kw: np.ndarray | None = None) -> float:
    """Gated integrated loudness (absolute -70 LUFS, relative -10 LU)."""
    kw_sig = kweight(as_stereo(sig)) if kw is None else kw
    if len(kw_sig) < int(0.4 * SR):
        return SILENT_LUFS
    energy, _ = _per_block_energy(kw_sig, 0.4, 0.1)
    kept = _louder_than(energy, -70.0)
    if len(kept):
        kept = _louder_than(kept, _pooled_lufs(kept) - 10.0)
    return _pooled_lufs(kept) if len(kept) else SILENT_LUFS


def lra(sig: np.ndarray, kw: np.ndarray | None = None) -> float:
    short, _ = block_loudness(sig, 3.0, 0.1, kw)
    short = short[short > -70.0]
    if len(short) < 2:
        return 0.0
    cut = 10.0 * math.log10(np.mean(10.0 ** (short / 10.0))) - 20.0
    short = short[short > cut]
    if len(short) < 2:
        return 0.0
    return float(np.percentile(short, 95) - np.percentile(short, 10))


def true_peak(sig: np.ndarray) -> float:
    over4 = signal.resample_poly(as_stereo(sig), 4, 1, axis=0)
    return db(float(np.max(np.abs(over4))) if len(over4) else 0.0)


def measure(sig: np.ndarray) -> dict:
    sig = as_stereo(sig)
    weighted = kweight(sig)
    momentary, mom_t = block_loudness(sig, 0.4, 0.1, weighted)
    loudest = int(np.argmax(momentary))
    last50 = sig[-int(0.05 * SR):]
    return {
        "lufs": integrated(sig, weighted),
        "true_peak_db": true_peak(sig),
        "sample_peak_db": db(float(np.max(np.abs(sig))) if len(sig) else 0.0),
        "lra": lra(sig, weighted),
        "momentary_max": float(momentary[loudest]),
        "momentary_max_t": float(mom_t[loudest]),
        "tail_50ms_db": db(float(np.sqrt(np.mean(last50 ** 2))) if len(last50) else 0.0),
        "last_480_zero": bool(len(sig) >= 480 and not np.any(sig[-480:])),
        "seconds": len(sig) / SR,
    }


# ---------------------------------------------------------------- processing

def raised_cosine(n: int) -> np.ndarray:
    if n <= 1:
        return np.ones(max(n, 0))
    return 0.5 - 0.5 * np.cos(np.pi * np.arange(n) / (n - 1))


def _bw_filter(sig, poles, corner, mode_):
    sections = signal.butter(poles, corner, btype=mode_, fs=SR, output="sos")
    return signal.sosfilt(sections, sig, axis=0)


def mono_below(sig: np.ndarray, corner: float) -> np.ndarray:
    if corner <= 0:
        return sig
    centre_ch = (sig[:, 0] + sig[:, 1]) / 2
    side_ch = _bw_filter((sig[:, 0] - sig[:, 1]) / 2, 2, corner, "highpass")
    return np.stack([centre_ch + side_ch, centre_ch - side_ch], axis=1)


def _per_block(vals: np.ndarray, width: int, reducer) -> np.ndarray:
    extra = (-len(vals)) % width
    if extra:
        vals = np.concatenate([vals, np.full(extra, vals[-1])])
    return reducer(vals.reshape(-1, width), axis=1)


def _spread_blocks(per_block: np.ndarray, width: int, count: int) -> np.ndarray:
    mids = np.arange(len(per_block)) * width + (width - 1) / 2
    return np.interp(np.arange(count), mids, per_block)


def compressor(sig: np.ndarray, thr=-14.0, ratio=2.0, att=0.015, rel=0.12, det=0.005, blk=48):
    det_sig = np.mean(sig * sig, axis=1)
    det_sig = _bw_filter(det_sig, 1, 1.0 / (2 * math.pi * det), "lowpass")
    det_sig = _per_block(np.maximum(det_sig, 1e-20), blk, np.max)
    with np.errstate(divide="ignore"):
        excess = np.maximum(0.0, 10.0 * np.log10(np.maximum(det_sig, 1e-20)) - thr)
    want = -excess * (1 - 1 / ratio)
    k_att, k_rel = math.exp(-blk / (att * SR)), math.exp(-blk / (rel * SR))
    level, traj = 0.0, np.empty_like(want)
    for idx, goal in enumerate(want):
        k = k_att if goal < level else k_rel
        level = goal + (level - goal) * k
        traj[idx] = level
    return sig * (10.0 ** (_spread_blocks(traj, blk, len(sig)) / 20.0))[:, None]


def glue(sig: np.ndarray, amount=1.15, sheen=0.25) -> np.ndarray:
    shaped = np.tanh(0.9 * amount * sig) / math.tanh(amount)
    return shaped + sheen * _bw_filter(shaped, 1, 2500, "highpass")


def limiter_gain(sig: np.ndarray, ceiling=0.77, look=0.004, rel=0.08, blk=16) -> np.ndarray:
    """Per-sample gain of the 4x oversampled lookahead true-peak limiter."""
    count = len(sig)
    over4 = np.abs(signal.resample_poly(sig, 4, 1, axis=0)).max(axis=1)[: 4 * count]
    if len(over4) < 4 * count:
        over4 = np.concatenate([over4, np.zeros(4 * count - len(over4))])
    per_sample = over4.reshape(count, 4).max(axis=1)
    look_n = max(16, int(look * SR))
    lookahead = maximum_filter1d(per_sample, size=look_n, origin=-(look_n // 2), mode="nearest")
    allowed = np.minimum(1.0, ceiling / np.maximum(lookahead, 1e-12))
    gains = _per_block(allowed, blk, np.min)
    taps = max(3, look_n // blk)
    kernel = signal.windows.hann(taps + 2)[1:-1]
    kernel /= kernel.sum()
    ext = np.pad(gains, (taps // 2, taps - 1 - taps // 2), mode="edge")
    gains = np.minimum(gains, np.convolve(ext, kernel, mode="valid"))
    decay = math.exp(-blk / (rel * SR))
    smooth, held = np.empty_like(gains), 1.0
    for idx, val in enumerate(gains):
        held = val if val < held else val + (held - val) * decay
        smooth[idx] = held
    smooth = minimum_filter1d(smooth, size=3, mode="nearest")
    return np.minimum(_spread_blocks(smooth, blk, count), allowed)


def sidechain_gain(trigger: np.ndarray, count: int, depth=0.55, tau=0.14, blk=48) -> np.ndarray:
    level = np.abs(as_stereo(trigger)).max(axis=1)
    level = np.concatenate([level, np.zeros(max(0, count - len(level)))])[:count]
    blocks = _per_block(level, blk, np.max)
    audible = blocks[blocks > 0]
    blocks = blocks / (np.percentile(audible, 99.9) if len(audible) else 1.0)
    decay, follower, held = math.exp(-blk / (tau * SR)), np.empty_like(blocks), 0.0
    for idx, val in enumerate(blocks):
        held = val if val > held else held * decay
        follower[idx] = held
    return _spread_blocks(1.0 - depth * np.clip(follower, 0, 1), blk, count)


def duck_times(count: int, marks, depth=0.55, tau=0.14) -> np.ndarray:
    """Time-list ducker: each (t, weight) pulls the gain down; overlaps keep the lowest gain."""
    gains = np.ones(count)
    span = int(0.6 * SR)
    bump = np.exp(-np.arange(span) / SR / tau)
    ramp = int(0.003 * SR)
    bump[:ramp] *= raised_cosine(ramp)
    for when, weight in marks:
        first = int(round(when * SR))
        from_, to_ = max(0, first), min(count, first + span)
        if from_ < to_:
            gains[from_:to_] = np.minimum(gains[from_:to_], 1.0 - depth * weight * bump[from_ - first:to_ - first])
    return gains


CHAIN_DEFAULTS = {"lufs": -14.0, "ceiling": 0.77, "mono_hz": 120.0, "comp": True, "use_glue": False,
          "fade": 0.0, "tail_zero": 480, "end_fade": 0.06}


def _prepare(sig: np.ndarray, conf: dict) -> np.ndarray:
    """High-pass, mono bass, optional compressor, glue and end fade."""
    sig = mono_below(_bw_filter(as_stereo(sig), 2, 28, "highpass"), conf["mono_hz"])
    if conf["comp"]:
        sig = compressor(sig)
    if conf["use_glue"]:
        sig = glue(sig)
    if conf["fade"] > 0:
        count = len(sig)
        times = np.arange(count) / SR
        sig = sig * (np.clip((count / SR - 0.03 - times) / conf["fade"], 0, 1) ** 2)[:, None]
    return sig


def _loudness_pass(sig: np.ndarray, target: float, top: float) -> tuple[np.ndarray, float]:
    """Gain to the target and limit, up to 8 rounds. Returns (x, max limiter gain reduction in dB)."""
    worst_gr = 0.0
    for rnd in range(8):
        now = integrated(sig)
        if math.isfinite(now):
            sig = sig * 10.0 ** ((target - now) / 20.0)
        gains = limiter_gain(sig, top)
        worst_gr = max(worst_gr, -db(float(gains.min())))
        sig = sig * gains[:, None]
        now = integrated(sig)
        if rnd >= 2 and (not math.isfinite(now) or abs(now - target) <= 0.05):
            break
    return sig, worst_gr


def _trim_ends(sig: np.ndarray, conf: dict) -> np.ndarray:
    count = len(sig)
    head = min(count, int(0.003 * SR))
    sig[:head] *= raised_cosine(head)[:, None]
    tail = min(count, int(max(conf["end_fade"], 0.01) * SR))
    sig[count - tail:] *= raised_cosine(tail)[::-1][:, None]
    if conf["tail_zero"] > 0:
        sig[max(0, count - conf["tail_zero"]):] = 0.0
    return sig


def finish(sig, **opts):
    """The full chain. opts override FINISH. Returns (master, max limiter gain reduction in dB)."""
    conf = dict(CHAIN_DEFAULTS, **opts)
    sig, worst_gr = _loudness_pass(_prepare(sig, conf), conf["lufs"], conf["ceiling"])
    return _trim_ends(sig, conf), worst_gr


def fail_list(res: dict, args) -> list[str]:
    """Gate failures against args.lufs, args.tol and args.max_tp."""
    on_target = math.isfinite(res["lufs"]) and abs(res["lufs"] - args.lufs) <= args.tol
    tests = [(on_target, f"loudness {res['lufs']:.2f} LUFS is outside {args.lufs:.1f} +-{args.tol} LU"),
              (res["true_peak_db"] <= args.max_tp,
               f"true peak {res['true_peak_db']:.2f} dBTP is above {args.max_tp} dBTP"),
              (res["last_480_zero"], "the last 480 samples are not digital zero")]
    return [why for passed, why in tests if not passed]


def load_audio(src: str) -> np.ndarray:
    import soundfile as snd
    sig, fs_in = snd.read(src, dtype="float64", always_2d=True)
    sig = as_stereo(sig)
    if fs_in != SR:
        from fractions import Fraction
        ratio = Fraction(SR, fs_in).limit_denominator(1000)
        sig = signal.resample_poly(sig, ratio.numerator, ratio.denominator, axis=0)
    return sig


def _json_safe(val):
    if isinstance(val, float) and not math.isfinite(val):
        return None
    return val


def _render_master(args, sig: np.ndarray) -> dict:
    """Master x to args.output and measure the written file."""
    if not args.output:
        raise ValueError("OUTPUT is required unless --measure")
    if args.sidechain:
        sig = sig * sidechain_gain(load_audio(args.sidechain), len(sig), args.sc_depth, args.sc_tau)[:, None]
    shaped, reduction = finish(sig, lufs=args.lufs, ceiling=args.ceiling, mono_hz=args.mono_hz, comp=not args.no_comp,
                   use_glue=not args.no_glue, fade=args.fade, tail_zero=args.tail_zero)
    import soundfile as snd
    snd.write(args.output, shaped.astype(np.float32), SR, subtype="PCM_24")
    res = measure(snd.read(args.output, dtype="float64", always_2d=True)[0])
    res["limiter_max_gr_db"] = reduction
    res["output"] = args.output
    return res


def _show(res: dict) -> None:
    verdict = "PASS" if res["pass"] else "FAIL"
    fmt = lambda val: "n.a." if val is None else f"{val:.2f}"
    print(f"{verdict}  {fmt(res['lufs'])} LUFS  TP {fmt(res['true_peak_db'])} dBTP  LRA {fmt(res['lra'])} LU  "
          f"loudest {fmt(res['momentary_max'])} LUFS-M at {res['momentary_max_t']:.2f}s")
    for problem in res["fails"]:
        print("  " + problem)


def _emit_json(target: str | None, res: dict) -> None:
    if target == "-":
        print(json.dumps(res, indent=1))
    elif target:
        with open(target, "w") as handle:
            json.dump(res, handle, indent=1)


def execute(args) -> int:
    sig = load_audio(args.input)
    res = measure(sig) if args.measure else _render_master(args, sig)
    res["fails"] = fail_list(res, args)
    res["pass"] = not res["fails"]
    res = {key_: _json_safe(val) for key_, val in res.items()}
    _show(res)
    _emit_json(args.json, res)
    return 0 if res["pass"] else 1


def self_test() -> int:
    import os
    import tempfile
    import soundfile as sf
    gen = np.random.default_rng(7)
    all_ok = True

    def expect(label, holds):
        nonlocal all_ok
        all_ok &= bool(holds)
        print(("ok   " if holds else "FAIL ") + label)

    # 1 kHz sine at -20 dBFS peak per channel reads close to the BS.1770 reference.
    times = np.arange(SR * 5) / SR
    tone = 10 ** (-20 / 20) * np.sin(2 * np.pi * 1000 * times)
    expect("1 kHz stereo sine meter", abs(integrated(np.stack([tone, tone], 1)) + 20.0) < 0.1)
    with tempfile.TemporaryDirectory() as tmp:
        hiss = gen.standard_normal((SR * 10, 2))
        hiss = signal.lfilter([0.05], [1, -0.95], hiss, axis=0)  # darker than white
        hiss *= 10 ** ((-30 - integrated(hiss)) / 20)
        wav_in, wav_out = os.path.join(tmp, "in.wav"), os.path.join(tmp, "out.wav")
        sf.write(wav_in, hiss.astype(np.float32), SR, subtype="FLOAT")
        opts = cli_args([wav_in, wav_out, "--json", os.path.join(tmp, "m.json")])
        rc = execute(opts)
        shaped = sf.read(wav_out, always_2d=True)[0]
        res = measure(shaped)
        expect("M10 exit 0", rc == 0)
        expect("M10 loudness -14 +-0.5", abs(res["lufs"] + 14) <= 0.5)
        expect("M10 true peak <= -1.5 dBTP", res["true_peak_db"] <= -1.5)
        expect("M10 last 480 zero, first sample 0", res["last_480_zero"] and not np.any(shaped[0]))
        sf.write(wav_in, hiss.astype(np.float32), SR, subtype="FLOAT")
        expect("M11 --measure on an unmastered tail fails", execute(cli_args([wav_in, "--measure"])) == 1)
    return 0 if all_ok else 1


def cli_args(argv):
    ap = argparse.ArgumentParser(prog="master.py", description="Master a premix and measure it.")
    ap.add_argument("input", nargs="?")
    ap.add_argument("output", nargs="?")
    ap.add_argument("--measure", action="store_true")
    ap.add_argument("--lufs", type=float, default=-14.0)
    ap.add_argument("--ceiling", type=float, default=0.77)
    ap.add_argument("--tol", type=float, default=0.5)
    ap.add_argument("--max-tp", type=float, default=-1.5)
    ap.add_argument("--sidechain")
    ap.add_argument("--sc-depth", type=float, default=0.55)
    ap.add_argument("--sc-tau", type=float, default=0.14)
    ap.add_argument("--mono-hz", type=float, default=120.0)
    ap.add_argument("--no-comp", action="store_true")
    ap.add_argument("--no-glue", action="store_true")
    ap.add_argument("--fade", type=float, default=0.0)
    ap.add_argument("--tail-zero", type=int, default=480)
    ap.add_argument("--json")
    ap.add_argument("--self-test", action="store_true")
    return ap.parse_args(argv)


def _guarded(args) -> int:
    """run(args) with every error as 1 line and exit 1."""
    try:
        if not args.input:
            raise ValueError("INPUT is required")
        return execute(args)
    except Exception as err:  # one line, exit 1
        print(f"error: {err}", file=sys.stderr)
        return 1


def main(argv=None) -> int:
    args = cli_args(sys.argv[1:] if argv is None else argv)
    return self_test() if args.self_test else _guarded(args)


if __name__ == "__main__":
    sys.exit(main())
