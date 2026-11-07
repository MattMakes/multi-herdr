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

import master as mst  # noqa: E402

SR = 48000
BUSES = ("sfx", "amb", "music")
FADE_IN, FADE_OUT, SLICE_FADE_OUT = 0.002, 0.012, 0.02
RB_VERSION = "rb1"


class DesignError(Exception):
    pass


def ascii_print(*parts, file=None):
    text = " ".join(str(p) for p in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


def rpath(root: str, p: str) -> str:
    return p if os.path.isabs(p) else os.path.normpath(os.path.join(root, p))


def fwd(p: str) -> str:
    return p.replace("\\", "/")


def h(s: str) -> float:
    """Deterministic variation in [0, 1] from a text key."""
    return zlib.crc32(s.encode("utf-8")) / 4294967295.0


# ---------------------------------------------------------------- decoding

_MEM: dict[str, np.ndarray] = {}


def _channels(path: str) -> int:
    r = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "a:0", "-show_entries",
                        "stream=channels", "-of", "csv=p=0", path], capture_output=True, text=True)
    try:
        return int(r.stdout.strip().splitlines()[0])
    except (ValueError, IndexError):
        return 2


def _cache_key(path: str, extra: str = "") -> str:
    st = os.stat(path)
    return hashlib.sha1(f"{os.path.abspath(path)}|{st.st_size}|{st.st_mtime}{extra}".encode()).hexdigest()[:16]


def _cached(key: str, cache: str | None, make):
    if key in _MEM:
        return _MEM[key]
    f = os.path.join(cache, key + ".npy") if cache else None
    if f and os.path.isfile(f):
        x = np.load(f)
    else:
        x = make()
        if f:
            os.makedirs(cache, exist_ok=True)
            tmp = f"{f}.{os.getpid()}.tmp.npy"
            np.save(tmp, x)
            os.replace(tmp, f)
    _MEM[key] = x
    return x


def _ffmpeg_f32(cmd: list[str], what: str) -> np.ndarray:
    r = subprocess.run(cmd, capture_output=True)
    if r.returncode != 0:
        raise DesignError(f"{what}: {r.stderr.decode('utf-8', 'replace')[:300]}")
    return np.frombuffer(r.stdout, dtype=np.float32)


def decode(path: str, cache: str | None = None) -> np.ndarray:
    """Decode to float32 (n, 2) at 48 kHz. A mono source is duplicated at unity gain."""
    def make():
        mono = _channels(path) == 1
        raw = _ffmpeg_f32(["ffmpeg", "-v", "error", "-nostdin", "-i", path, "-vn", "-f", "f32le",
                           "-acodec", "pcm_f32le", "-ac", "1" if mono else "2", "-ar", str(SR), "-"],
                          f"decode failed for {path}")
        return np.repeat(raw[:, None], 2, axis=1) if mono else raw.reshape(-1, 2).copy()
    return _cached(_cache_key(path), cache, make)


def stretched(path: str, ratio: float, cache: str | None = None) -> np.ndarray:
    def make():
        af = (f"rubberband=tempo={ratio:.6f}:transients=mixed:detector=compound:phase=laminar:"
              "window=standard:smoothing=off:formant=shifted:pitchq=quality:channels=together")
        try:
            raw = _ffmpeg_f32(["ffmpeg", "-v", "error", "-nostdin", "-i", path, "-vn", "-af", af, "-f", "f32le",
                               "-acodec", "pcm_f32le", "-ac", "2", "-ar", str(SR), "-"], "x")
        except DesignError as e:
            raise DesignError(f"rubberband failed for {path}: {str(e)[3:]} (use stretch: none, or an ffmpeg "
                              "with the rubberband filter)") from None
        return raw.reshape(-1, 2).copy()
    return _cached(_cache_key(path, f"|{ratio:.6f}|{RB_VERSION}"), cache, make)


# ---------------------------------------------------------------- DSP

def rbj(kind: str, f0: float, gain_db: float, q: float = 0.707):
    A = 10 ** (gain_db / 40)
    w0 = 2 * math.pi * f0 / SR
    c, al = math.cos(w0), math.sin(w0) / (2 * q)
    if kind == "peak":
        b = [1 + al * A, -2 * c, 1 - al * A]
        a = [1 + al / A, -2 * c, 1 - al / A]
    else:
        s = 2 * math.sqrt(A) * al
        if kind == "low":
            b = [A * ((A + 1) - (A - 1) * c + s), 2 * A * ((A - 1) - (A + 1) * c), A * ((A + 1) - (A - 1) * c - s)]
            a = [(A + 1) + (A - 1) * c + s, -2 * ((A - 1) + (A + 1) * c), (A + 1) + (A - 1) * c - s]
        else:
            b = [A * ((A + 1) + (A - 1) * c + s), -2 * A * ((A - 1) + (A + 1) * c), A * ((A + 1) + (A - 1) * c - s)]
            a = [(A + 1) - (A - 1) * c + s, 2 * ((A - 1) - (A + 1) * c), (A + 1) - (A - 1) * c - s]
    b, a = np.array(b) / a[0], np.array(a) / a[0]
    return b, a


def apply_eq(x: np.ndarray, eq: dict | None) -> np.ndarray:
    if not eq or not len(x):
        return x
    y = x.astype(np.float64)
    if eq.get("hp"):
        y = signal.sosfilt(signal.butter(int(eq.get("hp_order", 2)), eq["hp"], "highpass", fs=SR, output="sos"), y, axis=0)
    if eq.get("lp"):
        y = signal.sosfilt(signal.butter(int(eq.get("lp_order", 2)), eq["lp"], "lowpass", fs=SR, output="sos"), y, axis=0)
    for f0, q, g in eq.get("peq", []) or []:
        b, a = rbj("peak", f0, g, q)
        y = signal.lfilter(b, a, y, axis=0)
    for key, kind in (("lowshelf", "low"), ("highshelf", "high")):
        if eq.get(key):
            b, a = rbj(kind, eq[key][0], eq[key][1])
            y = signal.lfilter(b, a, y, axis=0)
    return y.astype(np.float32)


def resample_pitch(x: np.ndarray, semis: float) -> np.ndarray:
    if abs(semis) < 1e-4 or not len(x):
        return x
    import soxr
    r = 2 ** (semis / 12)
    return soxr.resample(x.astype(np.float64), SR * r, SR, quality="VHQ").astype(np.float32)


def glide(x: np.ndarray, s0: float, s1: float) -> np.ndarray:
    import soxr
    r0, r1 = 2 ** (s0 / 12), 2 ** (s1 / 12)
    up = soxr.resample(x.astype(np.float64), SR, SR * 4, quality="VHQ")
    if max(r0, r1) > 1:
        sos = signal.butter(8, 0.95 * (SR / 2) / max(r0, r1, 1), "lowpass", fs=SR * 4, output="sos")
        up = signal.sosfiltfilt(sos, up, axis=0)
    L = max(2, int(len(x) / ((r0 + r1) / 2)))
    rates = np.linspace(r0, r1, L)
    pos = np.concatenate([[0.0], np.cumsum(rates[:-1])]) * 4
    pos = np.clip(pos, 0, len(up) - 1)
    i0 = np.floor(pos).astype(int)
    i1 = np.minimum(i0 + 1, len(up) - 1)
    fr = (pos - i0)[:, None]
    return (up[i0] * (1 - fr) + up[i1] * fr).astype(np.float32)


def curve(points, n: int, offset: float = 0.0) -> np.ndarray:
    """Piecewise-linear dB curve sampled at n samples (time = i/SR + offset), as linear gain."""
    pts = sorted((float(t), float(v)) for t, v in points)
    t = np.arange(n) / SR + offset
    return (10 ** (np.interp(t, [p[0] for p in pts], [p[1] for p in pts]) / 20)).astype(np.float32)


def fades(x: np.ndarray, fi: float, fo: float) -> np.ndarray:
    x = x.copy()
    n = len(x)
    a, b = min(n, int(fi * SR)), min(n, int(fo * SR))
    if a > 0:
        x[:a] *= mst.raised_cosine(a)[:, None].astype(np.float32)
    if b > 0:
        x[n - b:] *= mst.raised_cosine(b)[::-1][:, None].astype(np.float32)
    return x


def shape(x: np.ndarray, o: dict, fade_out: float = FADE_OUT, with_fades: bool = True) -> np.ndarray:
    """The per-sample chain: mono, EQ, pitch, glide, env, norm_peak_db, fades, gain."""
    if "reverse" in o:
        raise DesignError("reverse is refused: use a recording that already has that shape")
    x = x.astype(np.float32)
    if o.get("mono"):
        m = x.mean(axis=1, keepdims=True)
        x = np.repeat(m, 2, axis=1)
    x = apply_eq(x, o.get("eq"))
    x = resample_pitch(x, float(o.get("pitch", 0) or 0))
    if o.get("glide"):
        x = glide(x, *o["glide"])
    if o.get("env"):
        x = x * curve(o["env"], len(x))[:, None]
    if o.get("norm_peak_db") is not None and len(x):
        pk = float(np.max(np.abs(x))) or 1.0
        x = x * np.float32(10 ** (o["norm_peak_db"] / 20) / pk)
    if with_fades:
        x = fades(x, float(o.get("fade_in", FADE_IN)), float(o.get("fade_out", fade_out)))
    g = float(o.get("gain_db", 0) or 0)
    return x * np.float32(10 ** (g / 20)) if g else x


def align_index(x: np.ndarray, mode: str) -> int:
    if mode == "start" or not len(x):
        return 0
    m = np.abs(x).max(axis=1)
    if mode == "end":
        idx = np.nonzero(m > 0.003 * m.max())[0]
        return int(idx[-1]) if len(idx) else len(m) - 1
    blk = 48 if mode == "attack" else 240
    nb = len(m) // blk
    if nb < 1:
        return 0
    bm = m[: nb * blk].reshape(nb, blk).max(axis=1)
    if mode == "attack":
        return int(np.argmax(bm >= 0.1 * bm.max())) * blk
    if mode == "peak":
        return int(np.argmax(bm)) * blk + blk // 2
    raise DesignError(f"unknown align {mode!r}")


def pan_gains(p: float) -> tuple[float, float]:
    p = max(-1.0, min(1.0, float(p)))
    if p == 0:
        return 1.0, 1.0
    th = (p + 1) * math.pi / 4
    return math.sqrt(2) * math.cos(th), math.sqrt(2) * math.sin(th)


def onsets(mono: np.ndarray, delta: float, min_gap: float) -> np.ndarray:
    import librosa
    return librosa.onset.onset_detect(y=mono.astype(np.float32), sr=SR, hop_length=128, backtrack=True,
                                      delta=delta, wait=int(min_gap * SR / 128), units="samples")


def cut_slices(src: np.ndarray, o: dict, fname: str, with_fades: bool = True) -> list[tuple[int, np.ndarray]]:
    """Slice a recording of many hits. Returns [(source start sample, processed slice)]."""
    a = int(float(o.get("start", 0)) * SR)
    b = int(float(o["end"]) * SR) if o.get("end") is not None else len(src)
    win = src[a:b]
    mono = win.mean(axis=1)
    on = onsets(mono, float(o.get("delta", 0.07)), float(o.get("min_gap", 0.06)))
    w30 = int(0.03 * SR)
    pk = np.array([np.abs(mono[s:s + w30]).max() if s < len(mono) else 0.0 for s in on])
    if len(on):
        on = on[pk >= pk.max() * 10 ** (float(o.get("gate_db", -18)) / 20)]
    pre, ln = int(float(o.get("pre", 0.004)) * SR), int(float(o.get("len", 0.16)) * SR)
    out = []
    for k, s in enumerate(on):
        st, en = max(0, s - pre), min(len(win), s + ln)
        if o.get("stop_before_next", True) and k + 1 < len(on):
            en = min(en, on[k + 1] - pre)
        if en - st < int(0.02 * SR):
            continue
        out.append((a + st, shape(win[st:en], o, SLICE_FADE_OUT, with_fades)))
    if o.get("pick") is not None:
        out = [out[i] for i in o["pick"] if 0 <= i < len(out)]
    if o.get("max") is not None:
        out = out[: int(o["max"])]
    if not out:
        raise DesignError(f"slices from {fname}: no onset found (widen the window, lower delta or gate_db)")
    return out


# ---------------------------------------------------------------- design

def load_json(path: str):
    with open(path) as fh:
        return json.load(fh)


def load_design(path: str, root: str, only: str | None = None) -> dict:
    ap = rpath(root, path)
    if not os.path.isfile(ap):
        raise DesignError(f"design not found: {os.path.abspath(ap)}")
    top = load_json(ap)
    d = {k: v for k, v in top.items() if k not in ("include",)}
    for sec in ("samples", "slices", "pools"):
        d[sec] = dict(top.get(sec) or {})
    for sec in ("rules", "place", "ambience"):
        d[sec] = list(top.get(sec) or [])
    d["_owner"] = {sec: {n: os.path.basename(ap) for n in d[sec]} for sec in ("samples", "slices", "pools")}
    d["_fragments"] = []

    def merge(frag_path: str, filtered: bool):
        if filtered and only is not None and only not in frag_path:
            return
        fp = rpath(root, frag_path)
        if not os.path.isfile(fp):
            raise DesignError(f"include not found: {os.path.abspath(fp)}")
        fr = load_json(fp)
        d["_fragments"].append(fp)
        name = os.path.basename(fp)
        for sec in ("samples", "slices", "pools"):
            for n, v in (fr.get(sec) or {}).items():
                if n.startswith("_"):
                    continue
                if n in d[sec]:
                    raise DesignError(f"fragment {name}: {sec} name {n!r} is already defined "
                                      f"(by {d['_owner'][sec][n]}); nothing written")
                d[sec][n] = v
                d["_owner"][sec][n] = name
        for sec in ("rules", "place", "ambience"):
            d[sec] = list(fr.get(sec) or []) + d[sec]
        if fr.get("music") and not d.get("music"):
            d["music"] = fr["music"]
        for inc in fr.get("include") or []:
            merge(inc, False)

    for inc in top.get("include") or []:
        merge(inc, True)
    for sec in ("samples", "slices", "pools"):
        d[sec] = {k: v for k, v in d[sec].items() if not k.startswith("_")}
    return d


def load_cues(path: str | None) -> dict:
    c = load_json(path)
    if isinstance(c, list):
        c = c[0] if c else {}
    c.setdefault("fps", 60)
    c.setdefault("events", [])
    return c


def cue_path(args_cues, design, root):
    return rpath(root, args_cues or design.get("cues") or "out/cues.json")


def recordings(design: dict) -> list[tuple[str, str, str]]:
    items = [("sample", n, s["file"]) for n, s in design["samples"].items()]
    items += [("slices", n, s["file"]) for n, s in design["slices"].items()]
    items += [("ambience", a.get("name", str(i)), a["file"]) for i, a in enumerate(design["ambience"])]
    if design.get("music"):
        items.append(("music", "music", design["music"]["file"]))
    return items


class Library:
    """Processed samples, slices and pools, with a pitched-copy cache."""

    def __init__(self, design: dict, root: str, cache: str | None, with_fades: bool = True):
        self.design, self.root, self.cache = design, root, cache
        self.items: dict[str, np.ndarray] = {}
        self.src: dict[str, dict] = {}
        self.files: dict[str, str] = {}
        self.groups: dict[str, list[str]] = {}
        self._pitched: dict[tuple[str, float], np.ndarray] = {}
        for n, o in design["samples"].items():
            raw = decode(rpath(root, o["file"]), cache)
            a = int(float(o.get("start", 0)) * SR)
            b = int(float(o["end"]) * SR) if o.get("end") is not None else len(raw)
            self.items[n] = shape(raw[a:b], o, FADE_OUT, with_fades)
            self.src[n] = {"file": o["file"], "start": float(o.get("start", 0)),
                           "end": o.get("end"), "pitch": o.get("pitch", 0)}
            self.files[n] = o["file"]
        for g, o in design["slices"].items():
            raw = decode(rpath(root, o["file"]), cache)
            names = []
            for k, (st, x) in enumerate(cut_slices(raw, o, o["file"], with_fades)):
                n = f"{g}#{k}"
                self.items[n] = x
                self.src[n] = {"file": o["file"], "start": st / SR, "pitch": o.get("pitch", 0)}
                self.files[n] = o["file"]
                names.append(n)
            self.groups[g] = names
        self.pools: dict[str, list[str]] = {}
        for p, lst in design["pools"].items():
            out = []
            for s in lst:
                if s.endswith("#*"):
                    g = s[:-2]
                    if g not in self.groups:
                        raise DesignError(f"pool {p}: unknown slice group {g}")
                    out += self.groups[g]
                elif s in self.items:
                    out.append(s)
                else:
                    raise DesignError(f"pool {p}: unknown sample {s}" + (" (a slice group needs #*)" if s in self.groups else ""))
            if not out:
                raise DesignError(f"pool {p} is empty")
            self.pools[p] = out

    def get(self, name: str, semis: float = 0.0) -> np.ndarray:
        if name not in self.items:
            raise DesignError(f"unknown sample {name}")
        if abs(semis) < 0.001:
            return self.items[name]
        key = (name, round(semis, 2))
        if key not in self._pitched:
            self._pitched[key] = resample_pitch(self.items[name], semis)
        return self._pitched[key]


def rule_key(rule: dict, index: int) -> str:
    return str(rule.get("name", index))


def matches(rule: dict, ev: dict, fps: float) -> bool:
    m = rule.get("match") or {}
    if "kind" in m and not re.fullmatch(m["kind"], str(ev.get("kind", ""))):
        return False
    if "id" in m and not re.search(m["id"], str(ev.get("id") or "")):
        return False
    t = float(ev["f"]) / fps
    if "from" in m and t < float(m["from"]) - 1e-6:
        return False
    if "to" in m and t >= float(m["to"]) - 1e-6:
        return False
    if "variant" in m and ev.get("variant") != m["variant"]:
        return False
    return True


def plan_rules(design: dict, lib: Library, events: list, fps: float) -> list[dict]:
    """Every rule placement, before rendering: deterministic per rule and event."""
    out = []
    for ri, rule in enumerate(design["rules"]):
        K = rule_key(rule, ri)
        if rule.get("pool") not in lib.pools:
            raise DesignError(f"rule {K}: unknown pool {rule.get('pool')}")
        pool = lib.pools[rule["pool"]]
        n = len(pool)
        M = [e for e in events if matches(rule, e, fps)]
        counter, last = int(rule.get("cycle_start", 0)), None
        mode = rule.get("mode", "cycle")
        for i, e in enumerate(M):
            eid, f = str(e.get("id")), e["f"]
            if mode == "fixed":
                item = pool[0]
            elif mode == "hash":
                item = pool[int(math.floor(h(f"{eid}|{f}|{K}") * n)) % n]
            else:
                item = pool[counter % n]
                if n > 1 and item == last:
                    counter += 1
                    item = pool[counter % n]
                counter += 1
            last = item
            lo, hi = (rule.get("pitch") or [0, 0])
            semis = lo + (hi - lo) * h(f"p|{eid}|{f}|{K}") if hi != lo else lo
            frac = i / max(1, len(M) - 1)
            if rule.get("pitch_ramp"):
                a, b = rule["pitch_ramp"]
                semis += a + (b - a) * frac
            g = float(rule.get("gain_db", 0))
            if rule.get("gain_ramp"):
                a, b = rule["gain_ramp"]
                g += a + (b - a) * frac
            g += float(rule.get("weight_db", 0)) * math.log2(max(float(e.get("weight", 1)), 0.001) / float(rule.get("weight_ref", 1.0)))
            g += float(rule.get("jitter_db", 0)) * (2 * h(f"g|{eid}|{f}|{K}") - 1)
            pan = rule.get("pan", "cue")
            pan = float(e.get("pan", 0) or 0) * float(rule.get("pan_scale", 1.0)) if pan == "cue" else float(pan)
            own = float(design["samples"].get(item, design["slices"].get(item.split("#")[0], {})).get("pitch", 0) or 0)
            out.append({"rule": K, "rule_index": ri, "event": e, "sample": item, "semis": semis, "own_pitch": own,
                        "gain_db": g, "pan": pan, "align": rule.get("align", "attack"),
                        "offset": float(rule.get("offset", 0)), "bus": rule.get("bus", "sfx"),
                        "no_cue": bool(rule.get("no_cue")), "t": float(f) / fps + float(rule.get("offset", 0))})
    return out


class Mixer:
    def __init__(self, n: int):
        self.n = n
        self.bus = {b: np.zeros((n, 2), np.float32) for b in BUSES}
        self.trace: list[dict] = []

    def add(self, bus: str, x: np.ndarray, start: int, gain: float, pan: float, meta: dict):
        if bus not in self.bus:
            raise DesignError(f"unknown bus {bus}")
        self.trace.append({"bus": bus, "start": int(start), "n": int(len(x)), "gain": float(gain), "pan": float(pan), **meta})
        a, b = max(0, start), min(self.n, start + len(x))
        if a >= b:
            return
        gl, gr = pan_gains(pan)
        seg = x[a - start:b - start]
        self.bus[bus][a:b, 0] += seg[:, 0] * np.float32(gain * gl)
        self.bus[bus][a:b, 1] += seg[:, 1] * np.float32(gain * gr)


def ambience_bed(src: np.ndarray, a: dict) -> tuple[np.ndarray, bool]:
    s0 = int(float(a.get("src_start", 0)) * SR)
    s1 = int(float(a["src_end"]) * SR) if a.get("src_end") is not None else len(src)
    win = src[s0:s1]
    need = int((float(a["until"]) - float(a["at"])) * SR)
    looped = False
    if len(win) < need:
        xf = int(1.5 * SR)
        if len(win) <= xf:
            raise DesignError(f"ambience {a.get('name', a['file'])}: the clean window ({len(win) / SR:.2f} s) is "
                              "shorter than the 1.5 s loop crossfade; widen src_start..src_end")
        fo = np.sqrt(mst.raised_cosine(xf)[::-1])[:, None].astype(np.float32)
        fi = np.sqrt(mst.raised_cosine(xf))[:, None].astype(np.float32)
        out = win.copy()
        while len(out) < need:
            mix = out[-xf:] * fo + win[:xf] * fi
            out = np.concatenate([out[:-xf], mix, win[xf:]])
        win, looped = out, True
    return win[:need], looped


def render(design: dict, root: str, cues: dict, cache: str | None) -> dict:
    fps = float(cues.get("fps", 60))
    dur = design.get("duration")
    if dur is None:
        if cues.get("total") is None:
            raise DesignError("no duration: set duration in the design, or total in the cue file")
        dur = float(cues["total"]) / fps
    dur = float(dur)
    missing = [(k, n, p) for k, n, p in recordings(design) if not os.path.isfile(rpath(root, p))]
    if missing:
        lines = "\n".join(f"  {k} {n}: {fwd(os.path.relpath(rpath(root, p), root))}" for k, n, p in missing)
        raise DesignError(f"{len(missing)} recording(s) missing; nothing written:\n{lines}")
    lib = Library(design, root, cache)
    events = sorted(cues.get("events", []), key=lambda e: float(e["f"]))
    N = int(round(dur * SR))
    mx = Mixer(N)
    placements, sounded, sounded_keys, used = [], [], set(), set()
    for p in plan_rules(design, lib, events, fps):
        total = p["semis"]
        x = lib.get(p["sample"], total)
        start = int(round(p["t"] * SR)) - align_index(x, p["align"])
        mx.add(p["bus"], x, start, 10 ** (p["gain_db"] / 20), p["pan"],
               {"sample": p["sample"], "rule": p["rule"], "kind": p["event"].get("kind")})
        e = p["event"]
        placements.append({"t": round(p["t"], 6), "event": e.get("id"), "kind": e.get("kind"), "rule": p["rule"],
                           "sample": p["sample"], "gain_db": round(p["gain_db"], 4), "pitch": round(total, 4),
                           "pitch_total": round(total + p["own_pitch"], 4), "pan": round(p["pan"], 4),
                           "align": p["align"]})
        used.add(lib.files[p["sample"]])
        if p["bus"] == "sfx" and not p["no_cue"]:
            k = (e["f"], e.get("kind"), e.get("id"))
            if k not in sounded_keys:
                sounded_keys.add(k)
                sounded.append(e)
    for pl in design["place"]:
        x = lib.get(pl["sample"], float(pl.get("pitch", 0)))
        t = float(pl["at"]) + float(pl.get("offset", 0))
        start = int(round(t * SR)) - align_index(x, pl.get("align", "attack"))
        g = float(pl.get("gain_db", 0))
        mx.add(pl.get("bus", "sfx"), x, start, 10 ** (g / 20), float(pl.get("pan", 0)), {"sample": pl["sample"], "kind": "place"})
        placements.append({"t": round(t, 6), "event": None, "kind": "place", "sample": pl["sample"],
                           "gain_db": g, "bus": pl.get("bus", "sfx")})
        used.add(lib.files[pl["sample"]])
    amb_rep = []
    for i, a in enumerate(design["ambience"]):
        bed, looped = ambience_bed(decode(rpath(root, a["file"]), cache), a)
        if a.get("width") is not None:
            mid, side = (bed[:, 0] + bed[:, 1]) / 2, (bed[:, 0] - bed[:, 1]) / 2 * float(a["width"])
            bed = np.stack([mid + side, mid - side], 1).astype(np.float32)
        bed = apply_eq(bed, a.get("eq"))
        fi, fo = float(a.get("fade_in", 1.0)), float(a.get("fade_out", 1.0))
        bed = fades(bed, fi, fo)
        if a.get("auto"):
            bed = bed * curve([[t - float(a["at"]), v] for t, v in a["auto"]], len(bed))[:, None]
        g = float(a.get("gain_db", 0))
        mx.add("amb", bed, int(round(float(a["at"]) * SR)), 10 ** (g / 20), 0.0, {"name": a.get("name"), "kind": "ambience"})
        body = bed[int(fi * SR): max(int(fi * SR), len(bed) - int(fo * SR))]
        rms = float(np.sqrt(np.mean(body.astype(np.float64) ** 2))) if len(body) else 0.0
        amb_rep.append({"name": a.get("name"), "file": a["file"], "at": a["at"], "until": a["until"],
                        "looped": looped, "body_rms_dbfs": round(mst.db(rms) + g, 2)})
        used.add(a["file"])
    seg_rep = []
    mu = design.get("music")
    if mu:
        ratio = 1.0 if mu.get("stretch") == "none" else float(mu.get("target_bpm", 120)) / float(mu["src_bpm"])
        path = rpath(root, mu["file"])
        src = stretched(path, ratio, cache) if abs(ratio - 1) >= 1e-6 else decode(path, cache)
        k = 1.0 / ratio
        for s in mu["segments"]:
            fi, fo = float(s.get("fade_in", 0.008)), float(s.get("fade_out", 0.06))
            pre = int(fi * SR)
            a = int(round(float(s["src_from"]) * k * SR)) - pre
            if a < 0:
                pre += a
                a = 0
            b = int(round(float(s["src_to"]) * k * SR)) + int(fo * SR)
            x = fades(src[a:b], fi if s.get("fade_in_at_start", True) else 0.0, fo)
            if s.get("tail_fade"):
                m = min(len(x), int(float(s["tail_fade"]) * SR))
                x[len(x) - m:] *= mst.raised_cosine(m)[::-1][:, None].astype(np.float32)
            start = int(round(float(s["at"]) * SR)) - pre
            mx.add("music", x, start, 10 ** (float(s.get("gain_db", 0)) / 20), 0.0, {"label": s.get("label"), "kind": "music"})
            seg_rep.append({**s, "stretched_from": a / SR, "film_start": start / SR})
        mb = apply_eq(mx.bus["music"], mu.get("eq"))
        mx.bus["music"] = mb * np.float32(10 ** (float(mu.get("gain_db", 0)) / 20))
        used.add(mu["file"])
    for name, cfg in (design.get("buses") or {}).items():
        if name.startswith("_"):
            continue
        if name not in BUSES:
            raise DesignError(f"buses: unknown bus {name!r} (use sfx, amb or music)")
        x = apply_eq(mx.bus[name], cfg.get("eq"))
        if cfg.get("auto"):
            x = x * curve(cfg["auto"], N)[:, None]
        mx.bus[name] = (x * np.float32(10 ** (float(cfg.get("gain_db", 0)) / 20))).astype(np.float32)
    unsounded = [{"t": float(e["f"]) / fps, "kind": e.get("kind"), "id": e.get("id")}
                 for e in events if (e["f"], e.get("kind"), e.get("id")) not in sounded_keys]
    return {"mixer": mx, "lib": lib, "duration": dur, "fps": fps, "events": events,
            "report": {"placements": placements, "unsounded_events": unsounded, "ambience": amb_rep,
                       "music_segments": seg_rep, "files_used": sorted(fwd(p) for p in used)},
            "sounded": sounded}


def bus_curve(design: dict, bus: str, start: int, n: int) -> np.ndarray:
    """The linear gain of a bus's auto ride and gain_db over samples [start, start + n)."""
    cfg = (design.get("buses") or {}).get(bus) or {}
    g = np.full(n, 10 ** (float(cfg.get("gain_db", 0)) / 20), np.float32)
    if cfg.get("auto"):
        g = g * curve(cfg["auto"], n, start / SR)
    return g


# ---------------------------------------------------------------- outputs

def safe(name: str, keep: str = r"A-Za-z0-9._#-") -> str:
    return re.sub(f"[^{keep}]+", "_", name)


def write_wav(path: str, x: np.ndarray, subtype: str = "FLOAT"):
    import soundfile as sf
    os.makedirs(os.path.dirname(os.path.abspath(path)) or ".", exist_ok=True)
    sf.write(path, x, SR, subtype=subtype)


def dump(design: dict, root: str, cache: str | None, folder: str) -> int:
    lib = Library(design, root, cache)
    os.makedirs(folder, exist_ok=True)
    rows = []
    for n, x in lib.items.items():
        write_wav(os.path.join(folder, safe(n).replace("#", "-") + ".wav"), x, "PCM_24")
        m = np.abs(x).max(axis=1) if len(x) else np.zeros(1)
        pk = float(m.max()) or 1e-12
        rms = float(np.sqrt(np.mean(x.astype(np.float64) ** 2))) if len(x) else 0.0
        src = dict(lib.src[n])
        if "#" in n:
            src.pop("end", None)
        row = {"name": n, "file": lib.files[n], "src": src, "dur": round(len(x) / SR, 4),
               "peak_dbfs": round(mst.db(pk), 2), "rms_dbfs": round(mst.db(rms), 2),
               "attack_ms": round(align_index(x, "attack") / SR * 1000, 2),
               "peak_ms": round(align_index(x, "peak") / SR * 1000, 2),
               "head_db": round(mst.db(float(m[:96].max())) - mst.db(pk), 2),
               "tail_db": round(mst.db(float(m[-960:].max())) - mst.db(pk), 2)}
        rows.append(row)
        ascii_print(f"{n}: {row['dur']:.3f} s  peak {row['peak_dbfs']} dBFS  attack {row['attack_ms']} ms  "
                    f"head {row['head_db']} dB  tail {row['tail_db']} dB")
    for p, items in lib.pools.items():
        rows.append({"pool": p, "size": len(items), "items": items})
        ascii_print(f"pool {p}: {len(items)} item(s)")
    with open(os.path.join(folder, "_samples.json"), "w") as fh:
        json.dump(rows, fh, indent=1)
    return 0


def peak_rms(x: np.ndarray) -> dict:
    pk = float(np.max(np.abs(x))) if len(x) else 0.0
    rms = float(np.sqrt(np.mean(x.astype(np.float64) ** 2))) if len(x) else 0.0
    return {"peak_dbfs": round(mst.db(pk), 2), "rms_dbfs": round(mst.db(rms), 2)}


def run(args) -> int:
    root = os.path.abspath(args.root)
    cache = rpath(root, args.cache) if args.cache else os.path.join(root, "out/audio/cache")
    design = load_design(args.design, root, args.only)
    if args.dump:
        return dump(design, root, cache, rpath(root, args.dump))
    cpath = cue_path(args.cues, design, root)
    if os.path.isfile(cpath):
        cues = load_cues(cpath)
    elif design["rules"]:
        raise DesignError(f"cue file not found: {cpath} (pass --cues)")
    else:
        cues = {"fps": 60, "events": []}
    solo = args.only is not None
    no_master = args.no_master or solo
    tag = safe(args.only, "A-Za-z0-9._-") if solo else None
    out = rpath(root, args.out or (f"out/audio-review/solo/{tag}.wav" if solo else "out/audio/soundtrack.wav"))
    base = os.path.splitext(out)[0]
    stems = rpath(root, args.stems or (base + "_stems" if solo else "out/audio/stems"))
    report_p = rpath(root, args.report or (base + "_report.json" if solo else "out/qa/mix_report.json"))
    cues_out = rpath(root, args.cues_out or (base + "_cues.json" if solo else "out/qa/cues_sounded.json"))
    r = render(design, root, cues, cache)
    mx = r["mixer"]
    os.makedirs(stems, exist_ok=True)
    buses = {}
    for b in BUSES:
        write_wav(os.path.join(stems, f"{b}.wav"), mx.bus[b])
        buses[b] = peak_rms(mx.bus[b])
    total = mx.bus["sfx"] + mx.bus["amb"] + mx.bus["music"]
    pre_path = out if no_master else os.path.join(os.path.dirname(out), "premaster.wav")
    if no_master or np.any(total):
        write_wav(pre_path, total)
    rep = r["report"]
    rep["buses"] = buses
    rep["premaster_peak_dbfs"] = peak_rms(total)["peak_dbfs"]
    keep = ("f", "kind", "weight", "id", "pan", "dur", "apexFrac")
    sounded = []
    for e in r["sounded"]:
        o = {k: e[k] for k in keep if k in e}
        if e.get("kind") in ("riser", "swell"):
            o["ends"] = True
        sounded.append(o)
    tot = cues.get("total", int(r["duration"] * r["fps"]))
    for path, obj in ((cues_out, {"fps": r["fps"], "total": tot, "events": sounded}), (report_p, rep)):
        os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
        with open(path, "w") as fh:
            json.dump(obj, fh, indent=1)
    if args.trace:
        tp = rpath(root, args.trace)
        os.makedirs(os.path.dirname(tp) or ".", exist_ok=True)
        with open(tp, "w") as fh:
            json.dump(mx.trace, fh, indent=1)
    ascii_print(f"{pre_path}: peak {rep['premaster_peak_dbfs']} dBFS, {len(rep['placements'])} placement(s), "
                f"{len(rep['unsounded_events'])} unsounded event(s)")
    for b in BUSES:
        ascii_print(f"  {b}: peak {buses[b]['peak_dbfs']} dBFS  rms {buses[b]['rms_dbfs']} dBFS")
    if no_master:
        return 0
    if not np.any(total):
        ascii_print("the mix is all silent: nothing is mastered" +
                    (f" ({out} is from an earlier render)" if os.path.exists(out) else ""))
        return 1
    m = design.get("master") or {}
    script = args.master_script or os.path.join(os.path.dirname(os.path.abspath(__file__)), "master.py")
    cmd = [sys.executable, "-I", script, pre_path, out, "--lufs", str(m.get("lufs", -14)),
           "--ceiling", str(m.get("ceiling", 0.77)), "--json", os.path.join(os.path.dirname(report_p), "master.json")]
    if m.get("no_glue", True):
        cmd.append("--no-glue")
    if m.get("no_comp"):
        cmd.append("--no-comp")
    if m.get("mono_hz") is not None:
        cmd += ["--mono-hz", str(m["mono_hz"])]
    res = subprocess.run(cmd, capture_output=True, text=True)
    ascii_print((res.stdout + res.stderr)[-1200:].rstrip())
    if res.returncode != 0:
        ascii_print("master gate FAILED")
        return 1
    return 0


def self_test() -> int:
    import tempfile
    import soundfile as sf
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        fx.write(os.path.join(d, "b.wav"), fx.burst(0.1, 900, 40, 1, 0.8, 0.0))
        fx.put_json(os.path.join(d, "c.json"), {"fps": 60, "events": [{"f": 60, "kind": "hit", "id": "a:h:0"}]})
        fx.put_json(os.path.join(d, "d.json"), {"duration": 2.0, "samples": {"x.b": {"file": "b.wav"}},
                                                "pools": {"x.b": ["x.b"]},
                                                "rules": [{"name": "x.r", "pool": "x.b", "align": "attack"}]})
        base = ["--root", d, "--design", "d.json", "--cues", "c.json", "--no-master", "--out", "o.wav", "--cache", "cache"]
        ck("M1 renders", main(base) == 0)
        sfx = sf.read(os.path.join(d, "out/audio/stems/sfx.wav"), always_2d=True)[0]
        m = np.abs(sfx).max(axis=1)
        first = int(np.argmax(m > 0.1 * m.max()))
        ck("M1 attack within 1 ms of sample 48000", abs(first - 48000) <= 48)
        amb = sf.read(os.path.join(d, "out/audio/stems/amb.wav"), always_2d=True)[0]
        mus = sf.read(os.path.join(d, "out/audio/stems/music.wav"), always_2d=True)[0]
        out = sf.read(os.path.join(d, "o.wav"), always_2d=True)[0]
        ck("M1 amb and music stems are zero", not np.any(amb) and not np.any(mus))
        ck("M1 stems sum to the output", np.array_equal((sfx + amb + mus).astype(np.float32), out.astype(np.float32)))
        rep = load_json(os.path.join(d, "out/qa/mix_report.json"))
        ck("M1 report: 1 placement at t 1.0, 0 unsounded",
           len(rep["placements"]) == 1 and rep["placements"][0]["t"] == 1.0 and not rep["unsounded_events"])
        first_bytes = open(os.path.join(d, "o.wav"), "rb").read()
        _MEM.clear()
        main(base)
        ck("M2 a second render is bit-identical", open(os.path.join(d, "o.wav"), "rb").read() == first_bytes)
        # M5, M6: cycle order and pitch ramp
        fx.write(os.path.join(d, "b2.wav"), fx.burst(0.1, 500, 40, 2))
        fx.write(os.path.join(d, "b3.wav"), fx.burst(0.1, 700, 40, 3))
        ev = [{"f": 10 * k + 10, "kind": "pop", "id": f"a:p:{k}"} for k in range(7)]
        fx.put_json(os.path.join(d, "c2.json"), {"fps": 60, "events": ev})
        fx.put_json(os.path.join(d, "d2.json"), {
            "duration": 3.0, "samples": {"s0": {"file": "b.wav"}, "s1": {"file": "b2.wav"}, "s2": {"file": "b3.wav"}},
            "pools": {"p": ["s0", "s1", "s2"]},
            "rules": [{"name": "cyc", "pool": "p"},
                      {"name": "ramp", "pool": "p", "match": {"id": "a:p:[0-4]$"}, "pitch": [1, 2], "pitch_ramp": [0, 1],
                       "bus": "amb"}]})
        args2 = ["--root", d, "--design", "d2.json", "--cues", "c2.json", "--no-master", "--out", "o2.wav", "--cache", "cache"]
        main(args2)
        pl = load_json(os.path.join(d, "out/qa/mix_report.json"))["placements"]
        cyc = [p["sample"] for p in pl if p["rule"] == "cyc"]
        ck("M5 cycle order", cyc == ["s0", "s1", "s2", "s0", "s1", "s2", "s0"])
        ramp = [p["pitch"] for p in pl if p["rule"] == "ramp"]
        parts = [p - (k / 4) for k, p in enumerate(ramp)]
        ck("M6 pitches in [1, 3] with the ramp part 0..1", len(ramp) == 5 and all(1 <= p <= 3 for p in ramp)
           and all(1 <= q <= 2 for q in parts))
        main(args2)
        ck("M6 the same pitches again", ramp == [p["pitch"] for p in load_json(os.path.join(d, "out/qa/mix_report.json"))["placements"] if p["rule"] == "ramp"])
        # M7: pitch 12 halves the length and doubles the frequency
        t = np.arange(SR) / SR
        fx.write(os.path.join(d, "sine.wav"), np.stack([0.5 * np.sin(2 * np.pi * 440 * t)] * 2, 1))
        raw = decode(os.path.join(d, "sine.wav"))
        y = shape(raw, {"pitch": 12})
        spec = np.abs(np.fft.rfft(y[:, 0]))
        f = np.argmax(spec) * SR / len(y)
        ck("M7 pitch 12: half length, double frequency", abs(len(y) - SR / 2) <= 1 and abs(f - 880) < 5)
        # M8: a 10 s bed from a 4 s window loops smoothly
        src = fx.noise_bed(4.0, 0.1).astype(np.float32)
        bed, looped = ambience_bed(src, {"at": 0, "until": 10, "file": "x"})
        step = np.abs(np.diff(src, axis=0)).max()
        ck("M8 looped 10 s bed without a step", looped and len(bed) == 10 * SR and np.abs(np.diff(bed, axis=0)).max() <= step * 1.5)
        try:
            ambience_bed(src[: SR], {"at": 0, "until": 10, "file": "x", "name": "am.short"})
            ck("short ambience window is rejected", False)
        except DesignError as e:
            ck("short ambience window is rejected by name", "am.short" in str(e))
        # M9: music stretch none, a click at 2.0 s lands on 5.0 s
        clk = np.zeros((SR * 4, 2))
        clk[2 * SR] = 0.9
        clk[3 * SR // 2: 2 * SR] = 0.05
        fx.write(os.path.join(d, "m.wav"), clk, subtype="FLOAT")
        fx.put_json(os.path.join(d, "d3.json"), {"duration": 8.0, "music": {"file": "m.wav", "src_bpm": 120, "stretch": "none",
                    "segments": [{"src_from": 2.0, "src_to": 3.0, "at": 5.0}]}})
        main(["--root", d, "--design", "d3.json", "--no-master", "--out", "o3.wav", "--cache", "cache"])
        mus = sf.read(os.path.join(d, "out/audio/stems/music.wav"), always_2d=True)[0][:, 0]
        ck("M9 click at 5.0 s +-1 sample", abs(int(np.argmax(np.abs(mus))) - 5 * SR) <= 1)
        pre = mus[5 * SR - 384: 5 * SR]
        ck("M9 pre-roll fade before the click", abs(pre[0]) < 1e-6 and abs(pre[-1]) > 0.04)
        # M3, M4 and the full project with mastering
        p = fx.project(os.path.join(d, "film"))
        r = p["root"]
        ck("full mix with master passes the gate", main(["--root", r, "--trace", "out/qa/adds.json"]) == 0)
        full = load_json(os.path.join(r, "out/qa/mix_report.json"))
        ck("full mix sounds every cue", not full["unsounded_events"])
        main(["--root", r, "--only", "ui", "--out", "out/audio-review/ui/solo.wav"])
        solo = load_json(os.path.join(r, "out/audio-review/ui/solo_report.json"))
        key = lambda q: (q["rule"], q["sample"], q["gain_db"], q["pitch"], q["pan"], q["t"])
        ck("M3 solo placements equal the full mix ones",
           sorted(map(key, solo["placements"])) == sorted(key(q) for q in full["placements"] if q.get("rule", "").startswith("ui.")))
        fx.put_json(os.path.join(r, "audio/design/dup.json"), {"samples": {"ui.tap": {"file": "audio/assets/ui/fx-1_tap.wav"}}})
        top = load_json(p["top"])
        top["include"].append("audio/design/dup.json")
        fx.put_json(os.path.join(r, "audio/dup_top.json"), top)
        try:
            load_design("audio/dup_top.json", r)
            ck("M4 duplicate name is an error", False)
        except DesignError as e:
            ck("M4 duplicate name names the fragment and the name", "dup.json" in str(e) and "ui.tap" in str(e))
    return ck.code()


def parse(argv):
    p = argparse.ArgumentParser(prog="mix.py", description="Render a JSON sound design to stems and a master.")
    p.add_argument("--root", default=".")
    p.add_argument("--design", default="audio/sounddesign.json")
    p.add_argument("--cues")
    p.add_argument("--out")
    p.add_argument("--stems")
    p.add_argument("--no-master", action="store_true")
    p.add_argument("--only")
    p.add_argument("--dump")
    p.add_argument("--report")
    p.add_argument("--cues-out")
    p.add_argument("--trace")
    p.add_argument("--cache")
    p.add_argument("--master-script")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def main(argv=None) -> int:
    args = parse(sys.argv[1:] if argv is None else argv)
    if args.self_test:
        return self_test()
    try:
        return run(args)
    except DesignError as e:
        ascii_print(f"mix.py: error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
