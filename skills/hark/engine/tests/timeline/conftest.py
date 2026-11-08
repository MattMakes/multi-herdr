"""Fixtures of the timeline tests, plus stand-ins for modules of other hark units that may not be built yet.

``install()`` puts a stand-in into ``sys.modules`` only when the real module cannot be found. The stand-ins follow
the signatures of docs/specs/hark/ARCHITECTURE.md and do just enough for the timeline tests. Integration runs the
same tests against the real modules. Model stubs (CLAP, AST, vocabulary) replace the real models through
``monkeypatch`` in the ``stub_models`` fixture.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import math
import os
import subprocess
import sys
import types
from pathlib import Path

import numpy as np

SR = 48000


def _absent(name: str) -> bool:
    try:
        return importlib.util.find_spec(name) is None
    except (ModuleNotFoundError, ValueError):
        return True


def _module(name: str, attrs: dict, package: bool = False) -> types.ModuleType:
    mod = types.ModuleType(name)
    mod.__dict__.update(attrs)
    mod.__standin__ = True
    if package:
        mod.__path__ = []
    sys.modules[name] = mod
    parent, _, child = name.rpartition(".")
    if parent in sys.modules:
        setattr(sys.modules[parent], child, mod)
    return mod


# ---------------------------------------------------------------- hark.core.audio

class AudioError(RuntimeError):
    pass


AUDIO_EXTS = frozenset({".wav", ".flac", ".ogg", ".oga", ".opus", ".mp3", ".m4a", ".aac", ".aif", ".aiff", ".wma"})
VIDEO_EXTS = frozenset({".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi"})


def file_id(path: str) -> str:
    st = os.stat(path)
    with open(path, "rb") as fh:
        head = hashlib.sha1(fh.read(65536)).hexdigest()[:12]
    return hashlib.sha1(f"{os.path.abspath(path)}|{st.st_size}|{st.st_mtime_ns}|{head}".encode()).hexdigest()[:20]


def probe(path: str) -> dict:
    cmd = ["ffprobe", "-v", "error", "-print_format", "json", "-show_streams", "-show_format", path]
    try:
        raw = json.loads(subprocess.run(cmd, capture_output=True, check=False, timeout=60).stdout or b"{}")
    except (OSError, ValueError, subprocess.TimeoutExpired) as exc:
        return {"probe_error": str(exc)[:200]}
    streams = raw.get("streams", [])
    aud = next((s for s in streams if s.get("codec_type") == "audio"), None)
    vids = [s for s in streams if s.get("codec_type") == "video"
            and not (s.get("disposition") or {}).get("attached_pic")]
    out = {"codec": aud.get("codec_name") if aud else None,
           "src_rate": int(aud.get("sample_rate", 0)) if aud else 0,
           "channels": int(aud.get("channels", 0)) if aud else 0, "kbps": None,
           "lossy": bool(aud and aud.get("codec_name") in {"aac", "mp3", "opus", "vorbis"}),
           "duration": round(float((raw.get("format") or {}).get("duration") or 0), 3),
           "has_video": bool(vids)}
    if vids:
        num, _, den = (vids[0].get("avg_frame_rate") or "0/0").partition("/")
        fps = round(float(num) / float(den), 3) if den and float(den) else None
        out["video"] = {"width": vids[0].get("width"), "height": vids[0].get("height"), "fps": fps}
    return out


def load(path, sr=48000, mono=False, start=None, end=None, cache=True):
    if not os.path.exists(path):
        raise AudioError(f"no such file: {path}")
    ch = 1 if mono else 2
    cmd = ["ffmpeg", "-nostdin", "-v", "error", "-i", str(path), "-vn", "-f", "f32le", "-ac", str(ch),
           "-ar", str(sr), "pipe:1"]
    run = subprocess.run(cmd, capture_output=True, check=False)
    if run.returncode != 0 or not run.stdout:
        raise AudioError(f"cannot decode {path}: {run.stderr.decode('utf-8', 'replace').strip()[:200]}")
    x = np.frombuffer(run.stdout, dtype="<f4").copy()
    x = x if mono else x.reshape(-1, 2)
    a = max(0, round((start or 0) * sr))
    b = len(x) if end is None else min(len(x), round(end * sr))
    return x[a:b]


def to_mono(x):
    x = np.asarray(x)
    return x.mean(axis=1) if x.ndim == 2 else x


def duration(path):
    return len(load(path, mono=True)) / SR


def db(v, floor=1e-12):
    return 20 * math.log10(max(abs(float(v)), floor))


def write_wav(path, x, sr=48000, subtype="PCM_24"):
    import soundfile as sf

    Path(path).parent.mkdir(parents=True, exist_ok=True)
    sf.write(str(path), np.asarray(x, dtype=np.float32), sr, subtype=subtype)
    return str(path)


# ---------------------------------------------------------------- hark.core.features

_K = (((1.53512485958697, -2.69169618940638, 1.19839281085285), (1.0, -1.69065929318241, 0.73248077421585)),
      ((1.0, -2.0, 1.0), (1.0, -1.99004745483398, 0.99007225036621)))


def _stereo(x):
    x = np.asarray(x, dtype=np.float64)
    return np.column_stack([x, x]) if x.ndim == 1 else x


def k_weight(x, sr=48000):
    from scipy.signal import lfilter

    y = np.asarray(x, dtype=np.float64)
    for b, a in _K:
        y = lfilter(b, a, y, axis=0)
    return y


def loudness_curve(x, win=0.4, hop=0.1):
    y = k_weight(_stereo(x))
    n, w, h = len(y), int(win * SR), int(hop * SR)
    cs = np.vstack([np.zeros((1, 2)), np.cumsum(y ** 2, axis=0)])
    if n < w:
        ms = cs[-1] / max(n, 1)
        return np.array([n / SR / 2]), np.array([-0.691 + 10 * np.log10(ms.sum() + 1e-20)])
    starts = np.arange(0, n - w + 1, h)
    ms = (cs[starts + w] - cs[starts]) / w
    return (starts + w / 2) / SR, -0.691 + 10 * np.log10(ms.sum(axis=1) + 1e-20)


def _pmean(L):
    return 10 * np.log10(np.mean(10 ** (np.asarray(L) / 10)))


def integrated_lufs(x):
    _, L = loudness_curve(x, 0.4, 0.1)
    L = L[L > -70]
    if not L.size:
        return -70.0
    thr = _pmean(L) - 10
    k = L[L > thr]
    return float(_pmean(k)) if k.size else float(thr + 10)


def lra(x):
    _, L = loudness_curve(x, 3.0, 0.1)
    L = L[L > -70]
    if L.size < 5:
        return None
    k = L[L > _pmean(L) - 20]
    return float(np.percentile(k, 95) - np.percentile(k, 10)) if k.size > 4 else None


def true_peak_dbtp(x):
    from scipy.signal import resample_poly

    up = resample_poly(_stereo(x), 4, 1, axis=0)
    return db(np.abs(up).max())


def onsets(m, sr=48000, delta=0.07):
    m = np.asarray(m, dtype=np.float64)
    hop = 480
    n = len(m) // hop
    if n < 3:
        return []
    e = np.sqrt((m[: n * hop].reshape(n, hop) ** 2).mean(axis=1)) + 1e-9
    d = np.diff(20 * np.log10(e), prepend=-180)
    out, last = [], -1.0
    for i in np.flatnonzero(d > 6):
        t = i * hop / sr
        if t - last > 0.05:
            out.append(round(t, 3))
            last = t
    return out


# ---------------------------------------------------------------- hark.core.report

def fmt_t(t):
    if t is None:
        return "-"
    m, s = divmod(float(t), 60)
    return f"{int(m)}:{s:05.2f}"


def clean(obj):
    if isinstance(obj, dict):
        return {str(k): clean(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple)):
        return [clean(v) for v in obj]
    if isinstance(obj, np.ndarray):
        return clean(obj.tolist())
    if isinstance(obj, (np.floating, float)):
        return None if not math.isfinite(float(obj)) else float(obj)
    if isinstance(obj, np.integer):
        return int(obj)
    if isinstance(obj, (np.bool_,)):
        return bool(obj)
    if obj is None or isinstance(obj, (str, int, bool)):
        return obj
    return str(obj)


def to_json(result, indent=1):
    return json.dumps(clean(result), indent=indent, ensure_ascii=True)


def add_common(parser):
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--out")
    parser.add_argument("--threads", type=int)


def emit(result, args, text=None):
    if getattr(args, "out", None):
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(json.dumps(clean(result), indent=1), encoding="utf-8")
    print(to_json(result) if getattr(args, "json", False) else (text or result.get("text") or result["summary"]))
    return result


def error_line(cmd, msg):
    print(f"hark {cmd}: error: {msg}", file=sys.stderr)


# ---------------------------------------------------------------- hark.models

class MLMissing(RuntimeError):
    def __init__(self, feature="", missing=("torch",), command="pip install torch"):
        super().__init__(f"{feature} needs the ml extras")
        self.feature, self.missing, self.command = feature, tuple(missing), command


def require_ml(feature):
    return None


def _no_model(model_id=None):
    raise MLMissing("model")


# ---------------------------------------------------------------- hark.listen.profile (simplified film metrics)

METRIC_ROWS = [  # key, group, label, unit, decimals
    ("lufs_i", "loudness", "integrated loudness", "LUFS", 1), ("true_peak", "loudness", "true peak", "dBTP", 1),
    ("lra", "loudness", "loudness range", "LU", 1), ("crest_lu", "loudness", "loudest 3 s vs integrated", "LU", 1),
    ("intro_lu", "arc", "opening level", "LU", 1), ("end_lu", "arc", "closing level", "LU", 1),
    ("peak_pos", "arc", "loudest moment position", "0-1", 2), ("first_sound_s", "arc", "time to first sound", "s", 2),
    ("max_gap_s", "arc", "longest gap", "s", 2), ("end_silence_s", "ending", "dead air at the end", "s", 2),
    ("fade_s", "ending", "fade length", "s", 2), ("final_lu", "ending", "final moment level", "LU", 1),
    ("tail_s", "ending", "final ring-out", "s", 2), ("music_frac", "music", "music presence", "share", 2),
    ("speech_frac", "music", "voice presence", "share", 2), ("tempo", "music", "tempo", "BPM", 0),
    ("onset_rate", "density", "onset rate", "/s", 2), ("hit_rate", "density", "salient hit rate", "/s", 2),
    ("stopdowns_min", "density", "stop-downs", "/min", 1), ("cut_rate", "sync", "visual event rate", "/s", 2),
    ("cuts_with_sound", "sync", "sound on visual events", "share", 2),
    ("cut_hit_frac", "sync", "hits on visual events", "share", 2),
    ("cut_frame_sync", "sync", "frame-sync on visual events", "share", 2),
    ("cuts_on_beat", "sync", "visual events on the beat", "share", 2),
    ("band_sub", "spectrum", "sub 20-60 Hz", "dB", 1), ("band_low", "spectrum", "low 60-250 Hz", "dB", 1),
    ("band_lowmid", "spectrum", "low-mid 250 Hz-1 kHz", "dB", 1), ("band_mid", "spectrum", "mid 1-4 kHz", "dB", 1),
    ("band_presence", "spectrum", "presence 4-8 kHz", "dB", 1), ("band_air", "spectrum", "air 8-16 kHz", "dB", 1),
    ("brightness_hz", "spectrum", "brightness", "Hz", 0), ("side_to_mid_db", "stereo", "stereo width", "dB", 1),
    ("correlation", "stereo", "L/R correlation", "r", 2),
]
METRICS = {k: {"group": g, "label": lab, "unit": u, "decimals": d} for k, g, lab, u, d in METRIC_ROWS}
SHAPE_METRICS = ("lra", "crest_lu", "intro_lu", "end_lu", "final_lu")
BANDS = (("band_sub", 20, 60), ("band_low", 60, 250), ("band_lowmid", 250, 1000), ("band_mid", 1000, 4000),
         ("band_presence", 4000, 8000), ("band_air", 8000, 16000))


class ProfileError(ValueError):
    pass


def media_problem(path):
    if not os.path.exists(path):
        return f"file not found: {path}"
    if os.path.isdir(path):
        return f"{path} is a folder, not a media file"
    p = probe(path)
    if "probe_error" in p or (not p.get("codec") and not p.get("has_video")):
        return f"{path} is not a readable audio or video file"
    return None if p.get("codec") else f"{path} has no audio track"


def unscorable(metrics):
    if (metrics.get("duration") or 0) < 3.0:
        return f"shorter than 3 s ({metrics.get('duration', 0):.2f} s)"
    if metrics.get("lufs_i") is None or metrics["lufs_i"] <= -69.5:
        return "silent: nothing above -70 LUFS"
    return None


def fmt_value(key, v):
    if v is None:
        return "n.a."
    row = METRICS.get(key, {"unit": "", "decimals": 2})
    if row["unit"] == "share":
        return f"{100 * v:.0f}%"
    if row["unit"] in ("0-1", "r"):
        return f"{v:.{row['decimals']}f}"
    return f"{v:.{row['decimals']}f} {row['unit']}"


def _spectral(m):
    f = np.fft.rfftfreq(len(m), 1 / SR)
    p = np.abs(np.fft.rfft(m)) ** 2
    tot = p[(f >= 20) & (f <= 16000)].sum() + 1e-20
    out = {k: round(10 * np.log10(p[(f >= lo) & (f < hi)].sum() / tot + 1e-12), 1) for k, lo, hi in BANDS}
    out["brightness_hz"] = int((f * p).sum() / (p.sum() + 1e-20))
    return out


def _stereo_fields(x):
    left, right = x[:, 0].astype(float), x[:, 1].astype(float)
    side, mid = (left - right) / 2, (left + right) / 2
    rs = np.sqrt(np.mean(side ** 2))
    width = -40.0 if rs <= 1e-7 else round(20 * np.log10(rs) - 20 * np.log10(np.sqrt(np.mean(mid ** 2)) + 1e-12), 1)
    corr = float(np.corrcoef(left, right)[0, 1]) if left.std() > 1e-9 and right.std() > 1e-9 else None
    return {"side_to_mid_db": width, "correlation": None if corr is None else round(corr, 2)}, rs <= 1e-7


def _start_edge(m, t):
    """Listen P9 start edge: the first 2 ms frame within 20 dB of the loudest of the 150 ms from the block start."""
    n = len(m) // 96
    e2 = 20 * np.log10(np.sqrt((m[: n * 96].reshape(n, 96) ** 2).mean(axis=1)) + 1e-9)
    a, b = max(0, int((t - 0.05) / 0.002)), min(n, int((t + 0.05) / 0.002) + 1)
    if b <= a:
        return max(0.0, t)
    ok = np.flatnonzero(e2[a:b] >= e2[a: a + 75].max() - 20)
    return round((a + ok[0]) * 0.002, 3) if ok.size else max(0.0, t)


def _arc_fields(x, dur, I):
    tf, Lf = loudness_curve(x, 0.1, 0.02)
    loud = np.flatnonzero(Lf >= I - 20)
    aud = np.flatnonzero(Lf >= I - 30)
    first = round(_start_edge(x.mean(axis=1), tf[loud[0]]), 2) if loud.size else None
    last_end = tf[aud[-1]] + 0.05 if aud.size else dur
    out = {"first_sound_s": first, "end_silence_s": round(max(0.0, dur - last_end), 2)}
    gap, run = 0, 0
    for v in Lf[(loud[0] if loud.size else 0):(aud[-1] if aud.size else 0)]:
        run = run + 1 if v < I - 30 else 0
        gap = max(gap, run)
    out["max_gap_s"] = round(gap * 0.02, 2)
    tm, Lm = loudness_curve(x, 0.4, 0.1)
    full = np.flatnonzero(Lm >= I - 6)
    out["fade_s"] = round(max(0.0, last_end - (tm[full[-1]] if full.size else last_end) - 0.2), 2)
    return out, (tf, Lf)


def _shape_fields(x, dur, I):
    if dur < 10:
        return {k: None for k in SHAPE_METRICS}
    ts, Ls = loudness_curve(x, 3.0, 0.1)
    tm, Lm = loudness_curve(x, 0.4, 0.1)
    span = max(0.1 * dur, 0.5)
    emean = lambda L: float(_pmean(L)) if len(L) else None  # noqa: E731
    return {"lra": round(lra(x) or 0.0, 1), "crest_lu": round(float(Ls.max() - I), 1),
            "intro_lu": round(emean(Lm[tm <= span]) - I, 1), "end_lu": round(emean(Lm[tm >= dur - span]) - I, 1),
            "final_lu": round(float(Lm[tm >= dur - 3].max() - I), 1)}


def film_metrics(path, video=None, use_ast=True):
    x = load(path)
    m = x.mean(axis=1)
    dur = len(x) / SR
    I = integrated_lufs(x)
    metrics = {k: None for k in METRICS}
    metrics.update(lufs_i=round(I, 1), true_peak=round(true_peak_dbtp(x), 1))
    arc, _ = _arc_fields(x, dur, I)
    metrics.update(arc)
    metrics.update(_shape_fields(x, dur, I))
    metrics.update(_spectral(m))
    width, mono = _stereo_fields(x)
    metrics.update(width)
    on = onsets(m)
    metrics["onset_rate"] = round(len(on) / dur, 2)
    metrics["hit_rate"] = round(len(on) / dur, 2)
    metrics["stopdowns_min"] = 0.0
    return {"path": path, "video": video, "duration": round(dur, 3), "fps": None, "codec": "pcm", "lossy": False,
            "mono": bool(mono), "short": dur < 10, "metrics": metrics, "chance": {}, "arc": None,
            "events": {"hits": on, "visual": [], "visual_strength": [], "stopdowns": [], "beats": [],
                       "music_windows": None, "onsets_n": len(on)},
            "seconds": 0.0, "timing": {}, "summary": f"{os.path.basename(path)} {dur:.1f} s"}


def _stats(vals):
    v = [x for x in vals if x is not None]
    if not v:
        return {"n": 0}
    return {"n": len(v), "median": round(float(np.median(v)), 3), "p10": round(float(np.percentile(v, 10)), 3),
            "p90": round(float(np.percentile(v, 90)), 3), "min": round(min(v), 3), "max": round(max(v), 3),
            "values": list(vals)}


def aggregate(films):
    metrics = {k: _stats([f["metrics"].get(k) for f in films]) for k in METRICS}
    warnings = [] if len(films) >= 3 else [f"only {len(films)} references: the ranges are far too narrow"]
    return {"version": 2, "n": len(films), "metrics": metrics, "chance": {}, "arc": None, "films": list(films),
            "warnings": warnings, "summary": f"{len(films)} films"}


def load_profile(path=None):
    if path is None:
        root = Path(__file__).resolve().parents[2] / "hark" / "data" / "timeline" / "launch_profile.json"
        path = str(root)
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def profile(paths, styles=None, use_ast=True, out=None):
    films = []
    for i, p in enumerate([paths] if isinstance(paths, str) else paths):
        f = film_metrics(p, use_ast=use_ast)
        films.append({"id": f"ref_{i + 1}", "style": "", "duration": f["duration"], "has_video": False,
                      "metrics": f["metrics"], "chance": f["chance"], "arc": f["arc"]})
    prof = aggregate(films)
    if out:
        Path(out).write_text(json.dumps(clean(prof), indent=1), encoding="utf-8")
    return prof


# ---------------------------------------------------------------- hark.listen.realness and vocabulary

def load_model(path=None):
    raise FileNotFoundError("no realness model in the stand-in")


def thresholds_of(model, file=False):
    return {"lo": 0.4, "hi": 0.6}


def realness(path, start=None, end=None, ast=False, cache=True):
    raise MLMissing("realness")


def _no_vocab():
    raise MLMissing("vocabulary")


# ---------------------------------------------------------------- install

def _specs() -> list[tuple[str, dict, bool]]:
    return [
        ("hark.core", {}, True),
        ("hark.core.config", {"HOME": os.environ.get("HARK_HOME", str(Path.home() / ".cache" / "hark")),
                              "DATA_DIR": str(Path(__file__).resolve().parents[2] / "hark" / "data")}, False),
        ("hark.core.audio", {"SR": SR, "Audio": np.ndarray, "AudioError": AudioError, "AUDIO_EXTS": AUDIO_EXTS,
                             "VIDEO_EXTS": VIDEO_EXTS, "MEDIA_EXTS": AUDIO_EXTS | VIDEO_EXTS, "load": load,
                             "to_mono": to_mono, "probe": probe, "duration": duration, "file_id": file_id, "db": db,
                             "write_wav": write_wav, "DECODE_VERSION": 2}, False),
        ("hark.core.features", {"k_weight": k_weight, "loudness_curve": loudness_curve,
                                "integrated_lufs": integrated_lufs, "lra": lra, "true_peak_dbtp": true_peak_dbtp,
                                "onsets": onsets}, False),
        ("hark.core.report", {"fmt_t": fmt_t, "clean": clean, "to_json": to_json, "add_common": add_common,
                              "emit": emit, "error_line": error_line}, False),
        ("hark.models", {"MLMissing": MLMissing, "require_ml": require_ml}, True),
        ("hark.models.clap", {"get": _no_model}, False),
        ("hark.models.ast", {"get": _no_model}, False),
        ("hark.listen", {}, True),
        ("hark.listen.vocabulary", {"get": _no_vocab}, False),
        ("hark.listen.profile", {"film_metrics": film_metrics, "load_profile": load_profile, "aggregate": aggregate,
                                 "media_problem": media_problem, "unscorable": unscorable, "fmt_value": fmt_value,
                                 "METRICS": METRICS, "SHAPE_METRICS": SHAPE_METRICS, "MIN_FILM_S": 10.0,
                                 "MIN_SHAPE_S": 3.0, "AST_WIN_S": 5.0, "MUSIC_P": 0.3, "ProfileError": ProfileError,
                                 "profile": profile}, False),
        ("hark.listen.realness", {"load_model": load_model, "thresholds_of": thresholds_of, "realness": realness},
         False),
    ]


def install() -> list[str]:
    """Install every absent module; return the installed names."""
    import hark  # noqa: F401  (the package root exists: T-12)

    done = []
    for name, attrs, package in _specs():
        if _absent(name):
            _module(name, dict(attrs), package)
            done.append(name)
    return done


def parser_with(add_cli) -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="hark")
    add_cli(p.add_subparsers(dest="command"))
    return p


# ---------------------------------------------------------------- fixtures

import pytest  # noqa: E402

STANDINS = install()


class StubClap:
    """Fixed unit vectors: an excerpt maps to a vector from its RMS level, a text to a vector from its hash."""

    dim = 32

    def _unit(self, seed: int) -> np.ndarray:
        v = np.random.default_rng(seed).normal(size=self.dim)
        return v / np.linalg.norm(v)

    def embed_text(self, texts, batch=64):
        return np.vstack([self._unit(int(hashlib.sha1(t.encode()).hexdigest()[:8], 16)) for t in texts])

    def embed_audio(self, arrays, pad="repeatpad", batch=8, per_window=False):
        self.calls = getattr(self, "calls", 0) + len(arrays)
        out = []
        for a in arrays:
            level = int(np.clip(20 * np.log10(np.sqrt(np.mean(np.square(a))) + 1e-9) + 100, 0, 200)) // 6
            out.append(self._unit(level))
        return np.vstack(out).astype(np.float32)


class StubAST:
    labels = ["Music", "Speech", "Silence", "Clicking", "Noise", "Tap"]
    index = {lab: k for k, lab in enumerate(labels)}

    def probs(self, arrays, reduce="max", batch=8):
        return np.tile(np.array([0.1, 0.02, 0.0, 0.6, 0.3, 0.07], dtype=np.float32), (len(arrays), 1))

    def top(self, p, k=10, min_p=0.0):
        order = np.argsort(-np.asarray(p))
        return [(self.labels[i], round(float(p[i]), 3)) for i in order[:k] if p[i] >= min_p]

    def prob_of(self, p, label):
        return float(p[self.index[label]])


class StubVocab:
    """Two terms per section category; calibrate() gives z from fixed cosines."""

    def __init__(self):
        cats = ("music_genre", "music_mood", "instruments", "ambience", "voice")
        self.terms = [f"{c} term {k}" for c in cats for k in (1, 2)]
        self.term_category = [c for c in cats for _ in (1, 2)]
        self.categories = list(cats)
        self.templates = ["{}"]

    def calibrate(self, A, Q=None, method="both"):
        n, T = len(A), len(self.terms)
        C = np.tile(np.linspace(0.3, 0.1, T), (n, 1))
        Z = np.tile(np.linspace(3.0, 0.5, T), (n, 1))
        return types.SimpleNamespace(C=C, Z=Z, U=C, mu=np.zeros(n), sigma=np.ones(n), Cq=None, z_q=None,
                                     percentile=None, rank=None)


@pytest.fixture
def stub_models(monkeypatch):
    """Replace CLAP, AST and the vocabulary with fixed stubs. Returns the CLAP stub (it counts audio calls).

    The stubs need no ML extras, so the CLI handlers' require_ml check is a no-op here.
    """
    import hark.listen.vocabulary as vocabulary
    import hark.models
    import hark.models.ast as ast_mod
    import hark.models.clap as clap_mod

    monkeypatch.setattr(hark.models, "require_ml", lambda feature: None)
    clap, ast, vocab = StubClap(), StubAST(), StubVocab()
    monkeypatch.setattr(clap_mod, "get", lambda model_id=None: clap)
    monkeypatch.setattr(ast_mod, "get", lambda model_id=None: ast)
    monkeypatch.setattr(vocabulary, "get", lambda: vocab)
    return clap


def make_click_track(times, dur, amp=0.5, bed=0.01, seed=0, length=0.05, tau=0.008):
    """Decaying noise bursts at the given times over a noise bed (analysis tests helper)."""
    r = np.random.default_rng(seed)
    x = bed * r.uniform(-1, 1, int(round(dur * SR)))
    n = int(round(length * SR))
    env = np.exp(-np.arange(n) / SR / tau)
    for t in times:
        i = int(round(t * SR))
        j = min(x.size, i + n)
        x[i:j] += amp * r.uniform(-1, 1, j - i) * env[: j - i]
    return x.astype(np.float32)


@pytest.fixture
def click_track():
    return make_click_track


@pytest.fixture
def cli_parser():
    """parser(add_cli) -> argparse parser with that command registered."""
    return parser_with


def make_mp4(path, dur=3.0, split=1.5, fps=30, audio=None, colours=None, size="96x54"):
    """Write an mp4 with ffmpeg: colour clips (default black then white at ``split``), optional audio array."""
    import soundfile as sf

    clips = colours or [("black", split), ("white", dur - split)]
    cmd = ["ffmpeg", "-nostdin", "-v", "error", "-y"]
    for colour, d in clips:
        cmd += ["-f", "lavfi", "-i", f"color=c={colour}:s={size}:r={fps}:d={d}"]
    n = len(clips)
    filt = "".join(f"[{k}:v]" for k in range(n)) + f"concat=n={n}:v=1:a=0[v]"
    maps = ["-map", "[v]"]
    if audio is not None:
        wav_path = str(path) + ".wav"
        sf.write(wav_path, np.asarray(audio, dtype=np.float32), SR)
        cmd += ["-i", wav_path]
        maps += ["-map", f"{n}:a", "-c:a", "pcm_s16le"] if str(path).endswith(".mov") else \
            ["-map", f"{n}:a", "-c:a", "aac", "-b:a", "192k"]
    cmd += ["-filter_complex", filt, *maps, "-c:v", "libx264", "-pix_fmt", "yuv420p", "-r", str(fps), str(path)]
    subprocess.run(cmd, check=True, capture_output=True)
    return str(path)


@pytest.fixture
def mp4():
    return make_mp4
