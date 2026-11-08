#!/usr/bin/env python3
"""Render a JSON sound design to stems, a premaster and a master.

Every output sample comes from a recording named in the design. The code only
trims, shapes, places, pans, equalises, resamples, sums and masters.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import numpy as np  # noqa: E402
from scipy import signal  # noqa: E402

import master as mastering  # noqa: E402

SR = 48000
BUSES = ("sfx", "amb", "music")
EDGE_RISE, EDGE_FALL, CHOP_FALL = 0.002, 0.012, 0.02
STRETCH_TAG = "rb1"


class DesignError(Exception):
    pass


def ascii_print(*pieces, file=None):
    line_out = " ".join(str(piece) for piece in pieces)
    print(line_out.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def rpath(base_dir: str, target: str) -> str:
    return target if os.path.isabs(target) else os.path.normpath(os.path.join(base_dir, target))


def posix_path(target: str) -> str:
    return target.replace("\\", "/")


def unit_hash(text: str) -> float:
    """Deterministic variation in [0, 1] from a text key."""
    return zlib.crc32(text.encode("utf-8")) / 4294967295.0


# ---------------------------------------------------------------- decoding

_RAM_CACHE: dict[str, np.ndarray] = {}


def _probe_channel_count(media: str) -> int:
    proc = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                        "stream=channels", "-of", "csv=p=0", media], capture_output=True, text=True)
    try:
        return int(proc.stdout.strip().splitlines()[0])
    except (ValueError, IndexError):
        return 2


def _fingerprint(media: str, salt: str = "") -> str:
    info = os.stat(media)
    return hashlib.sha1(f"{os.path.abspath(media)}|{info.st_size}|{info.st_mtime}{salt}".encode()).hexdigest()[:16]


def _memo(fp: str, store: str | None, build):
    if fp in _RAM_CACHE:
        return _RAM_CACHE[fp]
    npy_path = os.path.join(store, fp + ".npy") if store else None
    if npy_path and os.path.isfile(npy_path):
        arr = np.load(npy_path)
    else:
        arr = build()
        if npy_path:
            os.makedirs(store, exist_ok=True)
            staging = f"{npy_path}.{os.getpid()}.tmp.npy"
            np.save(staging, arr)
            os.replace(staging, npy_path)
    _RAM_CACHE[fp] = arr
    return arr


def _pcm_from_ffmpeg(argv: list[str], label: str) -> np.ndarray:
    proc = subprocess.run(argv, capture_output=True)
    if proc.returncode != 0:
        raise DesignError(f"{label}: {proc.stderr.decode('utf-8', 'replace')[:300]}")
    return np.frombuffer(proc.stdout, dtype=np.float32)


def decode(media: str, store: str | None = None) -> np.ndarray:
    """Decode to float32 (n, 2) at 48 kHz. A mono source is duplicated at unity gain."""
    def build():
        one_ch = _probe_channel_count(media) == 1
        pcm = _pcm_from_ffmpeg(["ffmpeg", "-v", "error", "-nostdin", "-i", media, "-vn", "-f", "f32le",
                           "-acodec", "pcm_f32le", "-ac", "1" if one_ch else "2", "-ar", str(SR), "-"],
                          f"decode failed for {media}")
        return np.repeat(pcm[:, None], 2, axis=1) if one_ch else pcm.reshape(-1, 2).copy()
    return _memo(_fingerprint(media), store, build)


def stretched(media: str, tempo: float, store: str | None = None) -> np.ndarray:
    def build():
        chain = (f"rubberband=tempo={tempo:.6f}:transients=mixed:detector=compound:phase=laminar:"
              "window=standard:smoothing=off:formant=shifted:pitchq=quality:channels=together")
        try:
            pcm = _pcm_from_ffmpeg(["ffmpeg", "-v", "error", "-nostdin", "-i", media, "-vn", "-af", chain, "-f", "f32le",
                               "-acodec", "pcm_f32le", "-ac", "2", "-ar", str(SR), "-"], "x")
        except DesignError as err:
            raise DesignError(f"rubberband failed for {media}: {str(err)[3:]} (use stretch: none, or an ffmpeg "
                              "with the rubberband filter)") from None
        return pcm.reshape(-1, 2).copy()
    return _memo(_fingerprint(media, f"|{tempo:.6f}|{STRETCH_TAG}"), store, build)


# ---------------------------------------------------------------- DSP

def biquad_coeffs(kind: str, centre: float, gain_db: float, width_q: float = 0.707):
    amp = 10 ** (gain_db / 40)
    omega = 2 * math.pi * centre / SR
    cosw, alpha = math.cos(omega), math.sin(omega) / (2 * width_q)
    if kind == "peak":
        num = [1 + alpha * amp, -2 * cosw, 1 - alpha * amp]
        den = [1 + alpha / amp, -2 * cosw, 1 - alpha / amp]
    else:
        shelf_k = 2 * math.sqrt(amp) * alpha
        if kind == "low":
            num = [amp * ((amp + 1) - (amp - 1) * cosw + shelf_k), 2 * amp * ((amp - 1) - (amp + 1) * cosw), amp * ((amp + 1) - (amp - 1) * cosw - shelf_k)]
            den = [(amp + 1) + (amp - 1) * cosw + shelf_k, -2 * ((amp - 1) + (amp + 1) * cosw), (amp + 1) + (amp - 1) * cosw - shelf_k]
        else:
            num = [amp * ((amp + 1) + (amp - 1) * cosw + shelf_k), -2 * amp * ((amp - 1) + (amp + 1) * cosw), amp * ((amp + 1) + (amp - 1) * cosw - shelf_k)]
            den = [(amp + 1) - (amp - 1) * cosw + shelf_k, 2 * ((amp - 1) - (amp + 1) * cosw), (amp + 1) - (amp - 1) * cosw - shelf_k]
    num, den = np.array(num) / den[0], np.array(den) / den[0]
    return num, den


def equalise(sig: np.ndarray, bands: dict | None) -> np.ndarray:
    if not bands or not len(sig):
        return sig
    wet = sig.astype(np.float64)
    if bands.get("hp"):
        wet = signal.sosfilt(signal.butter(int(bands.get("hp_order", 2)), bands["hp"], "highpass", fs=SR, output="sos"), wet, axis=0)
    if bands.get("lp"):
        wet = signal.sosfilt(signal.butter(int(bands.get("lp_order", 2)), bands["lp"], "lowpass", fs=SR, output="sos"), wet, axis=0)
    for centre, width_q, boost in bands.get("peq", []) or []:
        num, den = biquad_coeffs("peak", centre, boost, width_q)
        wet = signal.lfilter(num, den, wet, axis=0)
    for slot, kind in (("lowshelf", "low"), ("highshelf", "high")):
        if bands.get(slot):
            num, den = biquad_coeffs(kind, bands[slot][0], bands[slot][1])
            wet = signal.lfilter(num, den, wet, axis=0)
    return wet.astype(np.float32)


def repitch(sig: np.ndarray, shift: float) -> np.ndarray:
    if abs(shift) < 1e-4 or not len(sig):
        return sig
    import soxr
    ratio_ = 2 ** (shift / 12)
    return soxr.resample(sig.astype(np.float64), SR * ratio_, SR, quality="VHQ").astype(np.float32)


def pitch_slide(sig: np.ndarray, semi_a: float, semi_b: float) -> np.ndarray:
    import soxr
    rate_a, rate_b = 2 ** (semi_a / 12), 2 ** (semi_b / 12)
    over = soxr.resample(sig.astype(np.float64), SR, SR * 4, quality="VHQ")
    if max(rate_a, rate_b) > 1:
        lp = signal.butter(8, 0.95 * (SR / 2) / max(rate_a, rate_b, 1), "lowpass", fs=SR * 4, output="sos")
        over = signal.sosfiltfilt(lp, over, axis=0)
    count = max(2, int(len(sig) / ((rate_a + rate_b) / 2)))
    steps = np.linspace(rate_a, rate_b, count)
    idx_f = np.concatenate([[0.0], np.cumsum(steps[:-1])]) * 4
    idx_f = np.clip(idx_f, 0, len(over) - 1)
    lo_i = np.floor(idx_f).astype(int)
    hi_i = np.minimum(lo_i + 1, len(over) - 1)
    frac = (idx_f - lo_i)[:, None]
    return (over[lo_i] * (1 - frac) + over[hi_i] * frac).astype(np.float32)


def db_ramp(knots, count: int, t0: float = 0.0) -> np.ndarray:
    """Piecewise-linear dB curve sampled at n samples (time = i/SR + offset), as linear gain."""
    pairs = sorted((float(times), float(v)) for times, v in knots)
    times = np.arange(count) / SR + t0
    return (10 ** (np.interp(times, [kp[0] for kp in pairs], [kp[1] for kp in pairs]) / 20)).astype(np.float32)


def edge_fades(sig: np.ndarray, rise_s: float, fall_s: float) -> np.ndarray:
    sig = sig.copy()
    total = len(sig)
    n_rise, n_fall = min(total, int(rise_s * SR)), min(total, int(fall_s * SR))
    if n_rise > 0:
        sig[:n_rise] *= mastering.raised_cosine(n_rise)[:, None].astype(np.float32)
    if n_fall > 0:
        sig[total - n_fall:] *= mastering.raised_cosine(n_fall)[::-1][:, None].astype(np.float32)
    return sig


def _colour(sig: np.ndarray, opts: dict) -> np.ndarray:
    """Mono, EQ, pitch and glide."""
    if opts.get("mono"):
        sig = np.repeat(sig.mean(axis=1, keepdims=True), 2, axis=1)
    sig = repitch(equalise(sig, opts.get("eq")), float(opts.get("pitch", 0) or 0))
    return pitch_slide(sig, *opts["glide"]) if opts.get("glide") else sig


def _contour(sig: np.ndarray, opts: dict) -> np.ndarray:
    """env and norm_peak_db."""
    if opts.get("env"):
        sig = sig * db_ramp(opts["env"], len(sig))[:, None]
    if opts.get("norm_peak_db") is not None and len(sig):
        peak = float(np.max(np.abs(sig))) or 1.0
        sig = sig * np.float32(10 ** (opts["norm_peak_db"] / 20) / peak)
    return sig


def shape(sig: np.ndarray, opts: dict, tail_s: float = EDGE_FALL, with_fades: bool = True) -> np.ndarray:
    """The per-sample chain: mono, EQ, pitch, glide, env, norm_peak_db, fades, gain."""
    if "reverse" in opts:
        raise DesignError("reverse is refused: use a recording that already has that shape")
    sig = _contour(_colour(sig.astype(np.float32), opts), opts)
    if with_fades:
        sig = edge_fades(sig, float(opts.get("fade_in", EDGE_RISE)), float(opts.get("fade_out", tail_s)))
    level = float(opts.get("gain_db", 0) or 0)
    return sig * np.float32(10 ** (level / 20)) if level else sig


# Block alignment modes: (block size, index of the block maximum array).
_ANCHOR_MODES = {"attack": (48, lambda bm, blk: int(np.argmax(bm >= 0.1 * bm.max())) * blk),
          "peak": (240, lambda bm, blk: int(np.argmax(bm)) * blk + blk // 2)}


def _last_audible(env: np.ndarray) -> int:
    loud = np.nonzero(env > 0.003 * env.max())[0]
    return int(loud[-1]) if len(loud) else len(env) - 1


def _blockwise_anchor(env: np.ndarray, mode: str) -> int:
    width, locate = _ANCHOR_MODES.get(mode, (240, None))
    count = len(env) // width
    if count < 1:
        return 0
    if locate is None:
        raise DesignError(f"unknown align {mode!r}")
    return locate(env[: count * width].reshape(count, width).max(axis=1), width)


def align_index(sig: np.ndarray, mode: str) -> int:
    if mode == "start" or not len(sig):
        return 0
    env = np.abs(sig).max(axis=1)
    return _last_audible(env) if mode == "end" else _blockwise_anchor(env, mode)


def pan_gains(pos: float) -> tuple[float, float]:
    pos = max(-1.0, min(1.0, float(pos)))
    if pos == 0:
        return 1.0, 1.0
    angle = (pos + 1) * math.pi / 4
    return math.sqrt(2) * math.cos(angle), math.sqrt(2) * math.sin(angle)


def detect_hits(down: np.ndarray, delta: float, spacing: float) -> np.ndarray:
    import librosa
    return librosa.onset.onset_detect(y=down.astype(np.float32), sr=SR, hop_length=128, backtrack=True,
                                      delta=delta, wait=int(spacing * SR / 128), units="samples")


def _crop(whole: np.ndarray, opts: dict) -> tuple[int, np.ndarray]:
    first = int(float(opts.get("start", 0)) * SR)
    last = int(float(opts["end"]) * SR) if opts.get("end") is not None else len(whole)
    return first, whole[first:last]


def _loud_hits(down: np.ndarray, opts: dict) -> np.ndarray:
    hits = detect_hits(down, float(opts.get("delta", 0.07)), float(opts.get("min_gap", 0.06)))
    probe = int(0.03 * SR)
    peaks = np.array([np.abs(down[pos:pos + probe]).max() if pos < len(down) else 0.0 for pos in hits])
    if len(hits):
        hits = hits[peaks >= peaks.max() * 10 ** (float(opts.get("gate_db", -18)) / 20)]
    return hits


def _hit_spans(hits: np.ndarray, opts: dict, total: int) -> list[tuple[int, int]]:
    """(start, end) in the window of each slice that is at least 20 ms long."""
    lead, span = int(float(opts.get("pre", 0.004)) * SR), int(float(opts.get("len", 0.16)) * SR)
    clip_next = opts.get("stop_before_next", True)
    spans = []
    for idx, hit in enumerate(hits):
        lo, hi = max(0, hit - lead), min(total, hit + span)
        if clip_next and idx + 1 < len(hits):
            hi = min(hi, hits[idx + 1] - lead)
        if hi - lo >= int(0.02 * SR):
            spans.append((lo, hi))
    return spans


def _select(chosen: list, opts: dict) -> list:
    if opts.get("pick") is not None:
        chosen = [chosen[idx] for idx in opts["pick"] if 0 <= idx < len(chosen)]
    return chosen[: int(opts["max"])] if opts.get("max") is not None else chosen


def cut_slices(whole: np.ndarray, opts: dict, fname: str, with_fades: bool = True) -> list[tuple[int, np.ndarray]]:
    """Slice a recording of many hits. Returns [(source start sample, processed slice)]."""
    first, win = _crop(whole, opts)
    bounds = _hit_spans(_loud_hits(win.mean(axis=1), opts), opts, len(win))
    chops = _select([(first + state, shape(win[state:en], opts, CHOP_FALL, with_fades)) for state, en in bounds], opts)
    if not chops:
        raise DesignError(f"slices from {fname}: no onset found (widen the window, lower delta or gate_db)")
    return chops


# ---------------------------------------------------------------- design

def load_json(path: str):
    with open(path) as fh:
        return json.load(fh)


_KEYED_SECTIONS = ("samples", "slices", "pools")
_SEQ_SECTIONS = ("rules", "place", "ambience")


def _absorb_keyed(merged: dict, frag: dict, origin: str) -> None:
    for section in _KEYED_SECTIONS:
        for label, val in (frag.get(section) or {}).items():
            if label.startswith("_"):
                continue
            if label in merged[section]:
                raise DesignError(f"fragment {origin}: {section} name {label!r} is already defined "
                                  f"(by {merged['_owner'][section][label]}); nothing written")
            merged[section][label] = val
            merged["_owner"][section][label] = origin


def _absorb_fragment(merged: dict, base_dir: str, rel: str) -> None:
    """Merge 1 fragment and its own includes into d. Fragment lists go in front."""
    abs_path = rpath(base_dir, rel)
    if not os.path.isfile(abs_path):
        raise DesignError(f"include not found: {os.path.abspath(abs_path)}")
    frag = load_json(abs_path)
    merged["_fragments"].append(abs_path)
    _absorb_keyed(merged, frag, os.path.basename(abs_path))
    for section in _SEQ_SECTIONS:
        merged[section] = list(frag.get(section) or []) + merged[section]
    if frag.get("music") and not merged.get("music"):
        merged["music"] = frag["music"]
    for child in frag.get("include") or []:
        _absorb_fragment(merged, base_dir, child)


def _seed_design(head: dict, abs_path: str) -> dict:
    merged = {key_: val for key_, val in head.items() if key_ not in ("include",)}
    merged.update({section: dict(head.get(section) or {}) for section in _KEYED_SECTIONS})
    merged.update({section: list(head.get(section) or []) for section in _SEQ_SECTIONS})
    merged["_owner"] = {section: {label: os.path.basename(abs_path) for label in merged[section]} for section in _KEYED_SECTIONS}
    merged["_fragments"] = []
    return merged


def load_design(path: str, base_dir: str, only: str | None = None) -> dict:
    abs_path = rpath(base_dir, path)
    if not os.path.isfile(abs_path):
        raise DesignError(f"design not found: {os.path.abspath(abs_path)}")
    head = load_json(abs_path)
    merged = _seed_design(head, abs_path)
    for child in head.get("include") or []:
        if only is None or only in child:
            _absorb_fragment(merged, base_dir, child)
    for section in _KEYED_SECTIONS:
        merged[section] = {key_: val for key_, val in merged[section].items() if not key_.startswith("_")}
    return merged


def load_cues(src_path: str | None) -> dict:
    doc = load_json(src_path)
    if isinstance(doc, list):
        doc = doc[0] if doc else {}
    doc.setdefault("fps", 60)
    doc.setdefault("events", [])
    return doc


def cue_path(given, spec, base_dir):
    return rpath(base_dir, given or spec.get("cues") or "out/cues.json")


def source_files(spec: dict) -> list[tuple[str, str, str]]:
    items = [("sample", label, entry["file"]) for label, entry in spec["samples"].items()]
    items += [("slices", label, entry["file"]) for label, entry in spec["slices"].items()]
    items += [("ambience", amb.get("name", str(idx)), amb["file"]) for idx, amb in enumerate(spec["ambience"])]
    if spec.get("music"):
        items.append(("music", "music", spec["music"]["file"]))
    return items


class Library:
    """Processed samples, slices and pools, with a pitched-copy cache."""

    def __init__(self, spec: dict, base_dir: str, store: str | None, with_fades: bool = True):
        self.design, self.root, self.cache = spec, base_dir, store
        self.items: dict[str, np.ndarray] = {}
        self.origin: dict[str, dict] = {}
        self.source_of: dict[str, str] = {}
        self.chop_sets: dict[str, list[str]] = {}
        self._repitched: dict[tuple[str, float], np.ndarray] = {}
        for label, opts in spec["samples"].items():
            self._load_take(label, opts, with_fades)
        for group, opts in spec["slices"].items():
            self._load_chops(group, opts, with_fades)
        self.pools: dict[str, list[str]] = {pool_id: self._build_pool(pool_id, members) for pool_id, members in spec["pools"].items()}

    def _load_take(self, label: str, opts: dict, with_fades: bool) -> None:
        decoded = decode(rpath(self.root, opts["file"]), self.cache)
        _, clip = _crop(decoded, opts)
        self.items[label] = shape(clip, opts, EDGE_FALL, with_fades)
        self.origin[label] = {"file": opts["file"], "start": float(opts.get("start", 0)),
                       "end": opts.get("end"), "pitch": opts.get("pitch", 0)}
        self.source_of[label] = opts["file"]

    def _load_chops(self, group: str, opts: dict, with_fades: bool) -> None:
        decoded = decode(rpath(self.root, opts["file"]), self.cache)
        labels = []
        for idx, (pos, sig) in enumerate(cut_slices(decoded, opts, opts["file"], with_fades)):
            label = f"{group}#{idx}"
            self.items[label] = sig
            self.origin[label] = {"file": opts["file"], "start": pos / SR, "pitch": opts.get("pitch", 0)}
            self.source_of[label] = opts["file"]
            labels.append(label)
        self.chop_sets[group] = labels

    def _chop_group(self, pool_id: str, group: str) -> list[str]:
        if group not in self.chop_sets:
            raise DesignError(f"pool {pool_id}: unknown slice group {group}")
        return self.chop_sets[group]

    def _expand_entry(self, pool_id: str, entry: str) -> list[str]:
        if entry.endswith("#*"):
            return self._chop_group(pool_id, entry[:-2])
        if entry not in self.items:
            raise DesignError(f"pool {pool_id}: unknown sample {entry}" + (" (a slice group needs #*)" if entry in self.chop_sets else ""))
        return [entry]

    def _build_pool(self, pool_id: str, members: list) -> list[str]:
        flat = [label for entry in members for label in self._expand_entry(pool_id, entry)]
        if not flat:
            raise DesignError(f"pool {pool_id} is empty")
        return flat

    def get(self, label: str, semis: float = 0.0) -> np.ndarray:
        if label not in self.items:
            raise DesignError(f"unknown sample {label}")
        if abs(semis) < 0.001:
            return self.items[label]
        memo_key = (label, round(semis, 2))
        if memo_key not in self._repitched:
            self._repitched[memo_key] = repitch(self.items[label], semis)
        return self._repitched[memo_key]


def rule_key(rl: dict, index: int) -> str:
    return str(rl.get("name", index))


def rule_hits(rl: dict, ev: dict, rate: float) -> bool:
    crit = rl.get("match") or {}
    when = lambda: float(ev["f"]) / rate
    checks = [("kind", lambda want: re.fullmatch(want, str(ev.get("kind", "")))),
             ("id", lambda want: re.search(want, str(ev.get("id") or ""))),
             ("from", lambda want: when() >= float(want) - 1e-6),
             ("to", lambda want: when() < float(want) - 1e-6),
             ("variant", lambda want: ev.get("variant") == want)]
    return all(check(crit[field]) for field, check in checks if field in crit)


def _round_robin(takes: list, want: int, cursor: int) -> list:
    """count picks in order, never the same take twice in a row."""
    size, prev_take, picked = len(takes), None, []
    for _ in range(want):
        take = takes[cursor % size]
        if size > 1 and take == prev_take:
            cursor += 1
            take = takes[cursor % size]
        cursor += 1
        prev_take = take
        picked.append(take)
    return picked


def _choose_takes(rl: dict, takes: list, hits: list, tag: str) -> list:
    size, how = len(takes), rl.get("mode", "cycle")
    if how == "fixed":
        return [takes[0]] * len(hits)
    if how == "hash":
        return [takes[int(math.floor(unit_hash(f"{ev.get('id')}|{ev['f']}|{tag}") * size)) % size] for ev in hits]
    return _round_robin(takes, len(hits), int(rl.get("cycle_start", 0)))


def _rule_pitch(rl: dict, seed: str, frac: float) -> float:
    span_lo, span_hi = (rl.get("pitch") or [0, 0])
    semis = span_lo + (span_hi - span_lo) * unit_hash(f"p|{seed}") if span_hi != span_lo else span_lo
    if rl.get("pitch_ramp"):
        ramp_lo, ramp_hi = rl["pitch_ramp"]
        semis += ramp_lo + (ramp_hi - ramp_lo) * frac
    return semis


def _rule_level(rl: dict, ev: dict, seed: str, frac: float) -> float:
    level = float(rl.get("gain_db", 0))
    if rl.get("gain_ramp"):
        ramp_lo, ramp_hi = rl["gain_ramp"]
        level += ramp_lo + (ramp_hi - ramp_lo) * frac
    level += float(rl.get("weight_db", 0)) * math.log2(max(float(ev.get("weight", 1)), 0.001) / float(rl.get("weight_ref", 1.0)))
    level += float(rl.get("jitter_db", 0)) * (2 * unit_hash(f"g|{seed}") - 1)
    return level


def _rule_pan(rl: dict, ev: dict) -> float:
    how = rl.get("pan", "cue")
    return float(ev.get("pan", 0) or 0) * float(rl.get("pan_scale", 1.0)) if how == "cue" else float(how)


def _base_pitch(spec: dict, take: str) -> float:
    return float(spec["samples"].get(take, spec["slices"].get(take.split("#")[0], {})).get("pitch", 0) or 0)


def _schedule_one(spec: dict, bank, rl: dict, order: int, cue_list: list, rate: float) -> list[dict]:
    tag = rule_key(rl, order)
    if rl.get("pool") not in bank.pools:
        raise DesignError(f"rule {tag}: unknown pool {rl.get('pool')}")
    hits = [ev for ev in cue_list if rule_hits(rl, ev, rate)]
    span = max(1, len(hits) - 1)
    rows = []
    for idx, (ev, take) in enumerate(zip(hits, _choose_takes(rl, bank.pools[rl["pool"]], hits, tag))):
        seed, frame = f"{ev.get('id')}|{ev['f']}|{tag}", ev["f"]
        rows.append({"rule": tag, "rule_index": order, "event": ev, "sample": take, "semis": _rule_pitch(rl, seed, idx / span),
                    "own_pitch": _base_pitch(spec, take), "gain_db": _rule_level(rl, ev, seed, idx / span),
                    "pan": _rule_pan(rl, ev), "align": rl.get("align", "attack"),
                    "offset": float(rl.get("offset", 0)), "bus": rl.get("bus", "sfx"),
                    "no_cue": bool(rl.get("no_cue")), "t": float(frame) / rate + float(rl.get("offset", 0))})
    return rows


def schedule_rules(spec: dict, bank: Library, cue_list: list, rate: float) -> list[dict]:
    """Every rule placement, before rendering: deterministic per rule and event."""
    rows = []
    for order, rl in enumerate(spec["rules"]):
        rows += _schedule_one(spec, bank, rl, order, cue_list, rate)
    return rows


class BusBoard:
    def __init__(self, length: int):
        self.length = length
        self.lanes = {lane: np.zeros((length, 2), np.float32) for lane in BUSES}
        self.trace: list[dict] = []

    def add(self, lanes: str, sig: np.ndarray, at: int, gain: float, pan: float, meta: dict):
        if lanes not in self.lanes:
            raise DesignError(f"unknown bus {lanes}")
        self.trace.append({"bus": lanes, "start": int(at), "n": int(len(sig)), "gain": float(gain), "pan": float(pan), **meta})
        lo, lane = max(0, at), min(self.length, at + len(sig))
        if lo >= lane:
            return
        left_g, right_g = pan_gains(pan)
        chunk = sig[lo - at:lane - at]
        self.lanes[lanes][lo:lane, 0] += chunk[:, 0] * np.float32(gain * left_g)
        self.lanes[lanes][lo:lane, 1] += chunk[:, 1] * np.float32(gain * right_g)


def loop_bed(whole: np.ndarray, amb: dict) -> tuple[np.ndarray, bool]:
    s0 = int(float(amb.get("src_start", 0)) * SR)
    s1 = int(float(amb["src_end"]) * SR) if amb.get("src_end") is not None else len(whole)
    clip = whole[s0:s1]
    want = int((float(amb["until"]) - float(amb["at"])) * SR)
    looped = False
    if len(clip) < want:
        overlap = int(1.5 * SR)
        if len(clip) <= overlap:
            raise DesignError(f"ambience {amb.get('name', amb['file'])}: the clean window ({len(clip) / SR:.2f} s) is "
                              "shorter than the 1.5 s loop crossfade; widen src_start..src_end")
        fall_s = np.sqrt(mastering.raised_cosine(overlap)[::-1])[:, None].astype(np.float32)
        rise_s = np.sqrt(mastering.raised_cosine(overlap))[:, None].astype(np.float32)
        grown = clip.copy()
        while len(grown) < want:
            blend = grown[-overlap:] * fall_s + clip[:overlap] * rise_s
            grown = np.concatenate([grown[:-overlap], blend, clip[overlap:]])
        clip, looped = grown, True
    return clip[:want], looped


def _film_length(spec: dict, cue_doc: dict, rate: float) -> float:
    length_s = spec.get("duration")
    if length_s is None and cue_doc.get("total") is None:
        raise DesignError("no duration: set duration in the design, or total in the cue file")
    return float(length_s) if length_s is not None else float(float(cue_doc["total"]) / rate)


def _require_sources(spec: dict, base_dir: str) -> None:
    absent = [(kind_, label, rel) for kind_, label, rel in source_files(spec) if not os.path.isfile(rpath(base_dir, rel))]
    if absent:
        listing = "\n".join(f"  {kind_} {label}: {posix_path(os.path.relpath(rpath(base_dir, rel), base_dir))}" for kind_, label, rel in absent)
        raise DesignError(f"{len(absent)} recording(s) missing; nothing written:\n{listing}")


def _note_heard(state: dict, ev: dict) -> None:
    ident = (ev["f"], ev.get("kind"), ev.get("id"))
    if ident not in state["sounded_keys"]:
        state["sounded_keys"].add(ident)
        state["sounded"].append(ev)


def _drop_rule_hit(state: dict, row: dict) -> None:
    """Mix 1 planned rule placement and record it."""
    bank, semis_all, ev = state["lib"], row["semis"], row["event"]
    sig = bank.get(row["sample"], semis_all)
    at = int(round(row["t"] * SR)) - align_index(sig, row["align"])
    state["mx"].add(row["bus"], sig, at, 10 ** (row["gain_db"] / 20), row["pan"],
                 {"sample": row["sample"], "rule": row["rule"], "kind": ev.get("kind")})
    state["placements"].append({"t": round(row["t"], 6), "event": ev.get("id"), "kind": ev.get("kind"), "rule": row["rule"],
                             "sample": row["sample"], "gain_db": round(row["gain_db"], 4), "pitch": round(semis_all, 4),
                             "pitch_total": round(semis_all + row["own_pitch"], 4), "pan": round(row["pan"], 4),
                             "align": row["align"]})
    state["used"].add(bank.source_of[row["sample"]])
    if row["bus"] == "sfx" and not row["no_cue"]:
        _note_heard(state, ev)


def _drop_manual(state: dict, row: dict) -> None:
    """Mix 1 hand placement (design place list) and record it."""
    sig = state["lib"].get(row["sample"], float(row.get("pitch", 0)))
    when = float(row["at"]) + float(row.get("offset", 0))
    at = int(round(when * SR)) - align_index(sig, row.get("align", "attack"))
    level = float(row.get("gain_db", 0))
    state["mx"].add(row.get("bus", "sfx"), sig, at, 10 ** (level / 20), float(row.get("pan", 0)), {"sample": row["sample"], "kind": "place"})
    state["placements"].append({"t": round(when, 6), "event": None, "kind": "place", "sample": row["sample"],
                             "gain_db": level, "bus": row.get("bus", "sfx")})
    state["used"].add(state["lib"].source_of[row["sample"]])


def _tailor_bed(bed: np.ndarray, amb: dict) -> np.ndarray:
    """Width, EQ, fades and the auto ride of an ambience bed."""
    if amb.get("width") is not None:
        mid, side = (bed[:, 0] + bed[:, 1]) / 2, (bed[:, 0] - bed[:, 1]) / 2 * float(amb["width"])
        bed = np.stack([mid + side, mid - side], 1).astype(np.float32)
    bed = edge_fades(equalise(bed, amb.get("eq")), float(amb.get("fade_in", 1.0)), float(amb.get("fade_out", 1.0)))
    if amb.get("auto"):
        bed = bed * db_ramp([[when - float(amb["at"]), level] for when, level in amb["auto"]], len(bed))[:, None]
    return bed


def _lay_bed(state: dict, amb: dict) -> dict:
    """Mix 1 ambience bed. Returns its report row."""
    bed, looped = loop_bed(decode(rpath(state["root"], amb["file"]), state["cache"]), amb)
    bed = _tailor_bed(bed, amb)
    rise_s, fall_s = float(amb.get("fade_in", 1.0)), float(amb.get("fade_out", 1.0))
    level = float(amb.get("gain_db", 0))
    state["mx"].add("amb", bed, int(round(float(amb["at"]) * SR)), 10 ** (level / 20), 0.0, {"name": amb.get("name"), "kind": "ambience"})
    body = bed[int(rise_s * SR): max(int(rise_s * SR), len(bed) - int(fall_s * SR))]
    rms = float(np.sqrt(np.mean(body.astype(np.float64) ** 2))) if len(body) else 0.0
    state["used"].add(amb["file"])
    return {"name": amb.get("name"), "file": amb["file"], "at": amb["at"], "until": amb["until"],
            "looped": looped, "body_rms_dbfs": round(mastering.db(rms) + level, 2)}


def _lay_cut(state: dict, whole: np.ndarray, seg: dict, scale: float) -> dict:
    """Mix 1 music segment from the (stretched) source. Returns its report row."""
    rise_s, fall_s = float(seg.get("fade_in", 0.008)), float(seg.get("fade_out", 0.06))
    lead = int(rise_s * SR)
    src_lo = int(round(float(seg["src_from"]) * scale * SR)) - lead
    lead, src_lo = (lead + src_lo, 0) if src_lo < 0 else (lead, src_lo)
    src_hi = int(round(float(seg["src_to"]) * scale * SR)) + int(fall_s * SR)
    sig = edge_fades(whole[src_lo:src_hi], rise_s if seg.get("fade_in_at_start", True) else 0.0, fall_s)
    if seg.get("tail_fade"):
        tail_n = min(len(sig), int(float(seg["tail_fade"]) * SR))
        sig[len(sig) - tail_n:] *= mastering.raised_cosine(tail_n)[::-1][:, None].astype(np.float32)
    at = int(round(float(seg["at"]) * SR)) - lead
    state["mx"].add("music", sig, at, 10 ** (float(seg.get("gain_db", 0)) / 20), 0.0, {"label": seg.get("label"), "kind": "music"})
    return {**seg, "stretched_from": src_lo / SR, "film_start": at / SR}


def _lay_music(state: dict, track: dict) -> list[dict]:
    tempo = 1.0 if track.get("stretch") == "none" else float(track.get("target_bpm", 120)) / float(track["src_bpm"])
    media = rpath(state["root"], track["file"])
    whole = stretched(media, tempo, state["cache"]) if abs(tempo - 1) >= 1e-6 else decode(media, state["cache"])
    cuts = [_lay_cut(state, whole, s, 1.0 / tempo) for s in track["segments"]]
    music_lane = equalise(state["mx"].lanes["music"], track.get("eq"))
    state["mx"].lanes["music"] = music_lane * np.float32(10 ** (float(track.get("gain_db", 0)) / 20))
    state["used"].add(track["file"])
    return cuts


def _bus_automation(sig: np.ndarray, conf: dict, count: int) -> np.ndarray:
    sig = equalise(sig, conf.get("eq"))
    if conf.get("auto"):
        sig = sig * db_ramp(conf["auto"], count)[:, None]
    return (sig * np.float32(10 ** (float(conf.get("gain_db", 0)) / 20))).astype(np.float32)


def _apply_bus_settings(spec: dict, board, count: int) -> None:
    for lane_id, conf in (spec.get("buses") or {}).items():
        if lane_id.startswith("_"):
            continue
        if lane_id not in BUSES:
            raise DesignError(f"buses: unknown bus {lane_id!r} (use sfx, amb or music)")
        board.lanes[lane_id] = _bus_automation(board.lanes[lane_id], conf, count)


def render(spec: dict, base_dir: str, cues: dict, store: str | None) -> dict:
    fps = float(cues.get("fps", 60))
    length_s = _film_length(spec, cues, fps)
    _require_sources(spec, base_dir)
    bank = Library(spec, base_dir, store)
    cue_list = sorted(cues.get("events", []), key=lambda ev: float(ev["f"]))
    total_n = int(round(length_s * SR))
    state = {"root": base_dir, "cache": store, "lib": bank, "mx": BusBoard(total_n), "placements": [], "sounded": [],
          "sounded_keys": set(), "used": set()}
    for plan_row in schedule_rules(spec, bank, cue_list, fps):
        _drop_rule_hit(state, plan_row)
    for manual in spec["place"]:
        _drop_manual(state, manual)
    amb_rep = [_lay_bed(state, amb) for amb in spec["ambience"]]
    seg_rep = _lay_music(state, spec["music"]) if spec.get("music") else []
    _apply_bus_settings(spec, state["mx"], total_n)
    unsounded = [{"t": float(ev["f"]) / fps, "kind": ev.get("kind"), "id": ev.get("id")}
                 for ev in cue_list if (ev["f"], ev.get("kind"), ev.get("id")) not in state["sounded_keys"]]
    return {"mixer": state["mx"], "lib": bank, "duration": length_s, "fps": fps, "events": cue_list,
            "report": {"placements": state["placements"], "unsounded_events": unsounded, "ambience": amb_rep,
                       "music_segments": seg_rep, "files_used": sorted(posix_path(plan_row) for plan_row in state["used"])},
            "sounded": state["sounded"]}


def bus_curve(spec: dict, lanes: str, offset0: int, count: int) -> np.ndarray:
    """The linear gain of a bus's auto ride and gain_db over samples [start, start + n)."""
    conf = (spec.get("buses") or {}).get(lanes) or {}
    gain_arr = np.full(count, 10 ** (float(conf.get("gain_db", 0)) / 20), np.float32)
    if conf.get("auto"):
        gain_arr = gain_arr * db_ramp(conf["auto"], count, offset0 / SR)
    return gain_arr


# ---------------------------------------------------------------- outputs

def safe(text: str, allowed: str = r"A-Za-z0-9._#-") -> str:
    return re.sub(f"[^{allowed}]+", "_", text)


def save_wav(dest: str, sig: np.ndarray, subtype: str = "FLOAT"):
    import soundfile as snd
    os.makedirs(os.path.dirname(os.path.abspath(dest)) or ".", exist_ok=True)
    snd.write(dest, sig, SR, subtype=subtype)


def export_takes(spec: dict, base_dir: str, store: str | None, dest_dir: str) -> int:
    bank = Library(spec, base_dir, store)
    os.makedirs(dest_dir, exist_ok=True)
    entries = []
    for label, sig in bank.items.items():
        save_wav(os.path.join(dest_dir, safe(label).replace("#", "-") + ".wav"), sig, "PCM_24")
        env = np.abs(sig).max(axis=1) if len(sig) else np.zeros(1)
        peak = float(env.max()) or 1e-12
        level = float(np.sqrt(np.mean(sig.astype(np.float64) ** 2))) if len(sig) else 0.0
        origin = dict(bank.origin[label])
        if "#" in label:
            origin.pop("end", None)
        entry = {"name": label, "file": bank.source_of[label], "src": origin, "dur": round(len(sig) / SR, 4),
               "peak_dbfs": round(mastering.db(peak), 2), "rms_dbfs": round(mastering.db(level), 2),
               "attack_ms": round(align_index(sig, "attack") / SR * 1000, 2),
               "peak_ms": round(align_index(sig, "peak") / SR * 1000, 2),
               "head_db": round(mastering.db(float(env[:96].max())) - mastering.db(peak), 2),
               "tail_db": round(mastering.db(float(env[-960:].max())) - mastering.db(peak), 2)}
        entries.append(entry)
        ascii_print(f"{label}: {entry['dur']:.3f} s  peak {entry['peak_dbfs']} dBFS  attack {entry['attack_ms']} ms  "
                    f"head {entry['head_db']} dB  tail {entry['tail_db']} dB")
    for pool_id, members in bank.pools.items():
        entries.append({"pool": pool_id, "size": len(members), "items": members})
        ascii_print(f"pool {pool_id}: {len(members)} item(s)")
    with open(os.path.join(dest_dir, "_samples.json"), "w") as handle:
        json.dump(entries, handle, indent=1)
    return 0


def level_stats(sig: np.ndarray) -> dict:
    peak = float(np.max(np.abs(sig))) if len(sig) else 0.0
    level = float(np.sqrt(np.mean(sig.astype(np.float64) ** 2))) if len(sig) else 0.0
    return {"peak_dbfs": round(mastering.db(peak), 2), "rms_dbfs": round(mastering.db(level), 2)}


def _resolve_cues(args, spec: dict, base_dir: str) -> dict:
    where = cue_path(args.cues, spec, base_dir)
    if os.path.isfile(where):
        return load_cues(where)
    if spec["rules"]:
        raise DesignError(f"cue file not found: {where} (pass --cues)")
    return {"fps": 60, "events": []}


def _target_paths(args, base_dir: str) -> dict:
    """out, stems, report, cues_out and no_master; a solo render (--only) writes beside out."""
    alone = args.only is not None
    if alone:
        slug = safe(args.only, "A-Za-z0-9._-")
        wav_out = rpath(base_dir, args.out or f"out/audio-review/solo/{slug}.wav")
        stem_base = os.path.splitext(wav_out)[0]
        fallbacks = (stem_base + "_stems", stem_base + "_report.json", stem_base + "_cues.json")
    else:
        wav_out = rpath(base_dir, args.out or "out/audio/soundtrack.wav")
        fallbacks = ("out/audio/stems", "out/qa/mix_report.json", "out/qa/cues_sounded.json")
    explicit = (args.stems, args.report, args.cues_out)
    stems_dir, report_p, cues_p = (rpath(base_dir, chosen or dflt) for chosen, dflt in zip(explicit, fallbacks))
    return {"out": wav_out, "stems": stems_dir, "report": report_p, "cues_out": cues_p, "no_master": args.no_master or alone}


def _save_stems(board, folder: str) -> dict:
    os.makedirs(folder, exist_ok=True)
    stats = {}
    for lane in BUSES:
        save_wav(os.path.join(folder, f"{lane}.wav"), board.lanes[lane])
        stats[lane] = level_stats(board.lanes[lane])
    return stats


def _heard_list(sounded: list) -> list[dict]:
    fields = ("f", "kind", "weight", "id", "pan", "dur", "apexFrac")
    rows = []
    for ev in sounded:
        row = {fld: ev[fld] for fld in fields if fld in ev}
        if ev.get("kind") in ("riser", "swell"):
            row["ends"] = True
        rows.append(row)
    return rows


def _save_json(dest: str, payload) -> None:
    os.makedirs(os.path.dirname(dest) or ".", exist_ok=True)
    with open(dest, "w") as handle:
        json.dump(payload, handle, indent=1)


def _echo_summary(premaster: str, summary: dict, stats: dict) -> None:
    ascii_print(f"{premaster}: peak {summary['premaster_peak_dbfs']} dBFS, {len(summary['placements'])} placement(s), "
                f"{len(summary['unsounded_events'])} unsounded event(s)")
    for lane in BUSES:
        ascii_print(f"  {lane}: peak {stats[lane]['peak_dbfs']} dBFS  rms {stats[lane]['rms_dbfs']} dBFS")


def _mastering_args(conf: dict) -> list[str]:
    extra = ["--no-glue"] if conf.get("no_glue", True) else []
    extra += ["--no-comp"] if conf.get("no_comp") else []
    return extra + (["--mono-hz", str(conf["mono_hz"])] if conf.get("mono_hz") is not None else [])


def _run_mastering(args, spec: dict, premaster: str, where: dict) -> int:
    conf = spec.get("master") or {}
    runner = args.master_script or os.path.join(os.path.dirname(os.path.abspath(__file__)), "master.py")
    argv = [sys.executable, "-I", runner, premaster, where["out"], "--lufs", str(conf.get("lufs", -14)),
           "--ceiling", str(conf.get("ceiling", 0.77)),
           "--json", os.path.join(os.path.dirname(where["report"]), "master.json")] + _mastering_args(conf)
    proc2 = subprocess.run(argv, capture_output=True, text=True)
    ascii_print((proc2.stdout + proc2.stderr)[-1200:].rstrip())
    if proc2.returncode != 0:
        ascii_print("master gate FAILED")
        return 1
    return 0


def _wrap_up(args, spec: dict, where: dict, premaster: str, summed: np.ndarray) -> int:
    if where["no_master"]:
        return 0
    if not np.any(summed):
        dest = where["out"]
        ascii_print("the mix is all silent: nothing is mastered" +
                    (f" ({dest} is from an earlier render)" if os.path.exists(dest) else ""))
        return 1
    return _run_mastering(args, spec, premaster, where)


def _save_reports(args, base_dir: str, where: dict, res: dict, cue_doc: dict) -> None:
    frames = cue_doc.get("total", int(res["duration"] * res["fps"]))
    _save_json(where["cues_out"], {"fps": res["fps"], "total": frames, "events": _heard_list(res["sounded"])})
    _save_json(where["report"], res["report"])
    if args.trace:
        _save_json(rpath(base_dir, args.trace), res["mixer"].trace)


def execute(args) -> int:
    base_dir = os.path.abspath(args.root)
    store = rpath(base_dir, args.cache) if args.cache else os.path.join(base_dir, "out/audio/cache")
    spec = load_design(args.design, base_dir, args.only)
    if args.dump:
        return export_takes(spec, base_dir, store, rpath(base_dir, args.dump))
    cue_doc = _resolve_cues(args, spec, base_dir)
    where = _target_paths(args, base_dir)
    res = render(spec, base_dir, cue_doc, store)
    board = res["mixer"]
    stats = _save_stems(board, where["stems"])
    summed = board.lanes["sfx"] + board.lanes["amb"] + board.lanes["music"]
    premaster = where["out"] if where["no_master"] else os.path.join(os.path.dirname(where["out"]), "premaster.wav")
    if where["no_master"] or np.any(summed):
        save_wav(premaster, summed)
    summary = res["report"]
    summary["buses"] = stats
    summary["premaster_peak_dbfs"] = level_stats(summed)["peak_dbfs"]
    _save_reports(args, base_dir, where, res, cue_doc)
    _echo_summary(premaster, summary, stats)
    return _wrap_up(args, spec, where, premaster, summed)


def _chk_one_hit(ck, fx, sf, tmp: str) -> None:
    """M1, M2: 1 hit renders on its cue, bit-identical twice."""
    fx.write(os.path.join(tmp, "b.wav"), fx.burst(0.1, 900, 40, 1, 0.8, 0.0))
    fx.put_json(os.path.join(tmp, "c.json"), {"fps": 60, "events": [{"f": 60, "kind": "hit", "id": "a:h:0"}]})
    fx.put_json(os.path.join(tmp, "d.json"), {"duration": 2.0, "samples": {"x.b": {"file": "b.wav"}},
                                            "pools": {"x.b": ["x.b"]},
                                            "rules": [{"name": "x.r", "pool": "x.b", "align": "attack"}]})
    argv0 = ["--root", tmp, "--design", "d.json", "--cues", "c.json", "--no-master", "--out", "o.wav", "--cache", "cache"]
    ck("M1 renders", main(argv0) == 0)
    sfx = sf.read(os.path.join(tmp, "out/audio/stems/sfx.wav"), always_2d=True)[0]
    env = np.abs(sfx).max(axis=1)
    onset = int(np.argmax(env > 0.1 * env.max()))
    ck("M1 attack within 1 ms of sample 48000", abs(onset - 48000) <= 48)
    amb = sf.read(os.path.join(tmp, "out/audio/stems/amb.wav"), always_2d=True)[0]
    mus_l = sf.read(os.path.join(tmp, "out/audio/stems/music.wav"), always_2d=True)[0]
    outp = sf.read(os.path.join(tmp, "o.wav"), always_2d=True)[0]
    ck("M1 amb and music stems are zero", not np.any(amb) and not np.any(mus_l))
    ck("M1 stems sum to the output", np.array_equal((sfx + amb + mus_l).astype(np.float32), outp.astype(np.float32)))
    summary = load_json(os.path.join(tmp, "out/qa/mix_report.json"))
    ck("M1 report: 1 placement at t 1.0, 0 unsounded",
       len(summary["placements"]) == 1 and summary["placements"][0]["t"] == 1.0 and not summary["unsounded_events"])
    bytes0 = open(os.path.join(tmp, "o.wav"), "rb").read()
    _RAM_CACHE.clear()
    main(argv0)
    ck("M2 a second render is bit-identical", open(os.path.join(tmp, "o.wav"), "rb").read() == bytes0)


def _chk_rotation(ck, fx, sf, tmp: str) -> None:
    """M5, M6: cycle order and pitch ramp."""
    fx.write(os.path.join(tmp, "b2.wav"), fx.burst(0.1, 500, 40, 2))
    fx.write(os.path.join(tmp, "b3.wav"), fx.burst(0.1, 700, 40, 3))
    hits = [{"f": 10 * idx + 10, "kind": "pop", "id": f"a:p:{idx}"} for idx in range(7)]
    fx.put_json(os.path.join(tmp, "c2.json"), {"fps": 60, "events": hits})
    fx.put_json(os.path.join(tmp, "d2.json"), {
        "duration": 3.0, "samples": {"s0": {"file": "b.wav"}, "s1": {"file": "b2.wav"}, "s2": {"file": "b3.wav"}},
        "pools": {"p": ["s0", "s1", "s2"]},
        "rules": [{"name": "cyc", "pool": "p"},
                  {"name": "ramp", "pool": "p", "match": {"id": "a:p:[0-4]$"}, "pitch": [1, 2], "pitch_ramp": [0, 1],
                   "bus": "amb"}]})
    argv2 = ["--root", tmp, "--design", "d2.json", "--cues", "c2.json", "--no-master", "--out", "o2.wav", "--cache", "cache"]
    main(argv2)
    order = _report_column(tmp, "cyc", "sample")
    ck("M5 cycle order", order == ["s0", "s1", "s2", "s0", "s1", "s2", "s0"])
    ramp_vals = _report_column(tmp, "ramp", "pitch")
    residue = [pv - (idx / 4) for idx, pv in enumerate(ramp_vals)]
    ck("M6 pitches in [1, 3] with the ramp part 0..1", len(ramp_vals) == 5 and all(1 <= pv <= 3 for pv in ramp_vals)
       and all(1 <= qv <= 2 for qv in residue))
    main(argv2)
    ck("M6 the same pitches again", ramp_vals == _report_column(tmp, "ramp", "pitch"))


def _report_column(tmp: str, rl: str, col: str) -> list:
    """field of every placement of rule in the last mix report."""
    rows = load_json(os.path.join(tmp, "out/qa/mix_report.json"))["placements"]
    return [p[col] for p in rows if p["rule"] == rl]


def _chk_repitch_and_loop(ck, fx, sf, tmp: str) -> None:
    """M7, M8: resample pitch and the ambience loop."""
    times = np.arange(SR) / SR
    fx.write(os.path.join(tmp, "sine.wav"), np.stack([0.5 * np.sin(2 * np.pi * 440 * times)] * 2, 1))
    pcm = decode(os.path.join(tmp, "sine.wav"))
    wet = shape(pcm, {"pitch": 12})
    fft_mag = np.abs(np.fft.rfft(wet[:, 0]))
    peak_hz = np.argmax(fft_mag) * SR / len(wet)
    ck("M7 pitch 12: half length, double frequency", abs(len(wet) - SR / 2) <= 1 and abs(peak_hz - 880) < 5)
    # M8: a 10 s bed from a 4 s window loops smoothly
    whole = fx.noise_bed(4.0, 0.1).astype(np.float32)
    looped_bed, looped = loop_bed(whole, {"at": 0, "until": 10, "file": "x"})
    max_step = np.abs(np.diff(whole, axis=0)).max()
    ck("M8 looped 10 s bed without a step", looped and len(looped_bed) == 10 * SR and np.abs(np.diff(looped_bed, axis=0)).max() <= max_step * 1.5)
    try:
        loop_bed(whole[: SR], {"at": 0, "until": 10, "file": "x", "name": "am.short"})
        ck("short ambience window is rejected", False)
    except DesignError as ev:
        ck("short ambience window is rejected by name", "am.short" in str(ev))


def _chk_music_cut(ck, fx, sf, tmp: str) -> None:
    """M9: music stretch none, a click at 2.0 s lands on 5.0 s."""
    click = np.zeros((SR * 4, 2))
    click[2 * SR] = 0.9
    click[3 * SR // 2: 2 * SR] = 0.05
    fx.write(os.path.join(tmp, "m.wav"), click, subtype="FLOAT")
    fx.put_json(os.path.join(tmp, "d3.json"), {"duration": 8.0, "music": {"file": "m.wav", "src_bpm": 120, "stretch": "none",
                "segments": [{"src_from": 2.0, "src_to": 3.0, "at": 5.0}]}})
    main(["--root", tmp, "--design", "d3.json", "--no-master", "--out", "o3.wav", "--cache", "cache"])
    mus_l = sf.read(os.path.join(tmp, "out/audio/stems/music.wav"), always_2d=True)[0][:, 0]
    ck("M9 click at 5.0 s +-1 sample", abs(int(np.argmax(np.abs(mus_l))) - 5 * SR) <= 1)
    lead = mus_l[5 * SR - 384: 5 * SR]
    ck("M9 pre-roll fade before the click", abs(lead[0]) < 1e-6 and abs(lead[-1]) > 0.04)


def _chk_whole_film(ck, fx, sf, tmp: str) -> None:
    """M3, M4 and the full project with mastering."""
    pv = fx.project(os.path.join(tmp, "film"))
    proj_root = pv["root"]
    ck("full mix with master passes the gate", main(["--root", proj_root, "--trace", "out/qa/adds.json"]) == 0)
    whole = load_json(os.path.join(proj_root, "out/qa/mix_report.json"))
    ck("full mix sounds every cue", not whole["unsounded_events"])
    main(["--root", proj_root, "--only", "ui", "--out", "out/audio-review/ui/solo.wav"])
    alone = load_json(os.path.join(proj_root, "out/audio-review/ui/solo_report.json"))
    cmp_key = lambda qv: (qv["rule"], qv["sample"], qv["gain_db"], qv["pitch"], qv["pan"], qv["t"])
    ck("M3 solo placements equal the full mix ones",
       sorted(map(cmp_key, alone["placements"])) == sorted(cmp_key(qv) for qv in whole["placements"] if qv.get("rule", "").startswith("ui.")))
    fx.put_json(os.path.join(proj_root, "audio/design/dup.json"), {"samples": {"ui.tap": {"file": "audio/assets/ui/fx-1_tap.wav"}}})
    head = load_json(pv["top"])
    head["include"].append("audio/design/dup.json")
    fx.put_json(os.path.join(proj_root, "audio/dup_top.json"), head)
    try:
        load_design("audio/dup_top.json", proj_root)
        ck("M4 duplicate name is an error", False)
    except DesignError as ev:
        ck("M4 duplicate name names the fragment and the name", "dup.json" in str(ev) and "ui.tap" in str(ev))


def self_test() -> int:
    import tempfile
    import soundfile as sf
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as tmp:
        for case in (_chk_one_hit, _chk_rotation, _chk_repitch_and_loop, _chk_music_cut, _chk_whole_film):
            case(ck, fx, sf, tmp)
        _tmpl_checks(ck, os.path.join(tmp, "tpl"))
    return ck.code()


def _tmpl_scaffold(tmp: str) -> None:
    """A new project from the templates, with the placeholder file names."""
    import shutil
    import fixtures as fx
    tdir = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "templates")
    os.makedirs(os.path.join(tmp, "audio", "design"))
    os.makedirs(os.path.join(tmp, "out"))
    shutil.copy(os.path.join(tdir, "cues.json"), os.path.join(tmp, "out", "cues.json"))
    shutil.copy(os.path.join(tdir, "design_fragment.json"), os.path.join(tmp, "audio", "design", "demo.json"))
    head = load_json(os.path.join(tdir, "sounddesign.json"))
    head["include"] = ["audio/design/demo.json"]
    fx.put_json(os.path.join(tmp, "audio", "sounddesign.json"), head)


def _stand_in_audio(fx, kind_: str, rel: str, idx: int) -> np.ndarray:
    """A generated stand-in for 1 placeholder recording."""
    gens = {"music": lambda: fx.click_track(120, 20), "ambience": lambda: fx.noise_bed(45.0, 0.02, 7),
              "slices": lambda: fx.hits_track(16)}
    if kind_ in gens:
        return gens[kind_]()
    if "cymbal" in rel or "air" in rel:
        return fx.noise_bed(2.5, 0.1, 11)
    return fx.burst(0.6, 300 + 50 * idx, 12, idx + 1, 0.7, 0.12)


def _tmpl_mix(ck, tmp: str, spec: dict) -> tuple[dict, dict]:
    """W2: swap each placeholder for a fixture and render. Returns (made files, mix report)."""
    import contextlib
    import io
    import fixtures as fx
    written = {}
    for kind_, _, rel in source_files(spec):
        if rel not in written:
            written[rel] = fx.write(rpath(tmp, rel), _stand_in_audio(fx, kind_, rel, len(written)))
    with contextlib.redirect_stdout(io.StringIO()):
        rc = main(["--root", tmp, "--trace", "out/qa/adds.json"])
    summary = load_json(os.path.join(tmp, "out", "qa", "mix_report.json"))
    heard = {qv.get("rule") for qv in summary["placements"]}
    ck("W2 the swapped template renders and passes the master gate", rc == 0)
    ck("W2 every template rule has placements, no unsounded cue",
       {rr["name"] for rr in spec["rules"]} <= heard and not summary["unsounded_events"])
    return written, summary


def _tmpl_rights(ck, tmp: str, written: dict, summary: dict) -> None:
    """W5: the rights audit and the credits on the swapped template."""
    import contextlib
    import io
    import fixtures as fx
    import rights_audit
    import credits
    recs = [{"id": f"fx-{idx}", "category": rel.split("/")[2], "file": rel, "title": os.path.basename(rel),
                "author": "test", "page_url": f"https://example.org/{idx}", "rights": "CC0"}
               for idx, rel in enumerate(sorted(written))]
    fx.put_json(os.path.join(tmp, "audio", "assets", "manifests", "all.json"), recs)
    fx.put_json(os.path.join(tmp, "audio", "assets", "manifests", "rights.json"),
                [{"id": ev["id"], "file_check": {"path": ev["file"]}, "rights_seen": "CC0 read on the page",
                  "rights_confirmed": True, "audit_tool": rights_audit.TOOL} for ev in recs])
    with contextlib.redirect_stdout(io.StringIO()):
        rc_audit = rights_audit.main(["--root", tmp, "--report", "out/qa/mix_report.json", "--decode", "none"])
        rc_credits = credits.main(["--root", tmp, "--design", "audio/sounddesign.json",
                           "--rights", "audio/assets/manifests/rights.json"])
    text = open(os.path.join(tmp, "CREDITS.md")).read()
    ck("W5 the rights audit and the credits exit 0",  rc_audit == 0 and rc_credits == 0)
    ck("W5 CREDITS.md names each used file once",
       all(text.count(f'"{os.path.basename(rel)}"') == 1 for rel in summary["files_used"]))


def _tmpl_checks(ck, tmp: str):
    """W1, W2, W5: the templates in a new project, then with each placeholder swapped for a fixture."""
    import contextlib
    import io
    _tmpl_scaffold(tmp)
    errbuf = io.StringIO()
    with contextlib.redirect_stderr(errbuf):
        rc = main(["--root", tmp])
    spec = load_design("audio/sounddesign.json", tmp)
    pairs = [(kind_, label) for kind_, label, _ in source_files(spec)]
    ck("W1 the template stops and lists every placeholder by kind and name",
       rc != 0 and all(f"{kind_} {label}:" in errbuf.getvalue() for kind_, label in pairs) and len(pairs) >= 12)
    written, summary = _tmpl_mix(ck, tmp, spec)
    _tmpl_rights(ck, tmp, written, summary)


def cli_args(tokens):
    ap = argparse.ArgumentParser(prog="mix.py", description="Render a JSON sound design to stems and a master.")
    ap.add_argument("--root", default=".")
    ap.add_argument("--design", default="audio/sounddesign.json")
    ap.add_argument("--cues")
    ap.add_argument("--out")
    ap.add_argument("--stems")
    ap.add_argument("--no-master", action="store_true")
    ap.add_argument("--only")
    ap.add_argument("--dump")
    ap.add_argument("--report")
    ap.add_argument("--cues-out")
    ap.add_argument("--trace")
    ap.add_argument("--cache")
    ap.add_argument("--master-script")
    ap.add_argument("--self-test", action="store_true")
    return ap.parse_args(tokens)


def main(argv=None) -> int:
    args = cli_args(sys.argv[1:] if argv is None else argv)
    if args.self_test:
        return self_test()
    try:
        return execute(args)
    except DesignError as ev:
        ascii_print(f"mix.py: error: {ev}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
