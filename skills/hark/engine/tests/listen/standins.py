"""Test-only stand-ins for modules of other units (core, models, timeline.video).

conftest.py installs a stand-in only when the real module is absent. Each one follows the
ARCHITECTURE.md signature that listen calls, with just enough behaviour for the listen tests.
Nothing here ships in the package.
"""

from __future__ import annotations

import argparse
import functools
import glob
import hashlib
import json
import os
import sys
import types
from pathlib import Path

import numpy as np

SR = 48000


def _module(name: str, doc: str) -> types.ModuleType:
    mod = types.ModuleType(name, doc)
    mod.__file__ = __file__
    return mod


# ---------------------------------------------------------------- hark.core.config

def make_config() -> types.ModuleType:
    m = _module("hark.core.config", "stand-in config")
    home = Path(os.environ.get("HARK_HOME", Path.home() / ".cache" / "hark")).absolute()
    m.HOME = str(home)
    m.HF_CACHE = str(home / "hf")
    m.EMB_CACHE = str(home / "emb")
    m.DEC_CACHE = str(home / "decoded")
    m.PKG_DIR = str(Path(__file__).resolve().parents[2] / "hark")
    m.DATA_DIR = str(Path(m.PKG_DIR) / "data")
    m.CLAP_MODEL = "laion/larger_clap_general"
    m.AST_MODEL = "MIT/ast-finetuned-audioset-10-10-0.4593"
    m.CLAP_REVISION = None
    m.AST_REVISION = None
    m.PRECISION = "bf16"
    m.threads = lambda: 2
    m.ensure_dirs = lambda: None
    return m


# ---------------------------------------------------------------- hark.core.audio

class AudioError(RuntimeError):
    """Decode, probe or path problem."""


AUDIO_EXTS = frozenset(".wav .flac .ogg .oga .opus .mp3 .m4a .aac .aif .aiff .wma".split())
VIDEO_EXTS = frozenset(".mp4 .mov .m4v .webm .mkv .avi".split())
MEDIA_EXTS = AUDIO_EXTS | VIDEO_EXTS


def _read(path: str) -> np.ndarray:
    import soundfile as sf

    if not os.path.exists(path):
        raise AudioError(f"no such file: {path}")
    if os.path.isdir(path):
        raise AudioError(f"cannot decode {path}: is a folder")
    try:
        x, sr = sf.read(path, dtype="float32", always_2d=True)
    except Exception as exc:
        raise AudioError(f"cannot decode {path}: {str(exc).splitlines()[0][:200]}") from None
    if sr != SR:
        import soxr

        x = soxr.resample(x, sr, SR).astype(np.float32)
    return x


@functools.lru_cache(maxsize=64)
def _cached(path: str, mtime: int, size: int) -> np.ndarray:
    return _read(path)


def load(path, sr=48000, mono=False, start=None, end=None, cache=True):
    if not os.path.exists(path):
        raise AudioError(f"no such file: {path}")
    st = os.stat(path)
    x = _cached(os.path.abspath(path), st.st_mtime_ns, st.st_size)
    if x.shape[1] == 1:
        x = np.repeat(x, 2, axis=1) if not mono else x
    if mono:
        x = x.mean(axis=1)
    elif x.shape[1] > 2:
        x = x[:, :2]
    n = len(x)
    a = max(0, int(round((start or 0) * sr)))
    b = n if end is None else min(n, int(round(end * sr)))
    return np.ascontiguousarray(x[a:max(a, b)], dtype=np.float32)


def to_mono(x):
    x = np.asarray(x)
    return x.mean(axis=1) if x.ndim == 2 else x


def resample(x, sr_from, sr_to):
    if sr_from == sr_to:
        return x
    import soxr

    return soxr.resample(np.asarray(x, np.float32), sr_from, sr_to).astype(np.float32)


def duration(path):
    return len(load(path, mono=True)) / SR


def probe(path):
    if not os.path.exists(path):
        return {"probe_error": f"no such file: {path}"}
    try:
        import soundfile as sf

        i = sf.info(path)
        return {"codec": i.subtype.lower(), "src_rate": i.samplerate, "channels": i.channels, "kbps": None,
                "lossy": False, "duration": round(i.duration, 3), "has_video": False}
    except Exception as exc:
        return {"probe_error": str(exc)[:200]}


def windows(x, win, hop, sr=48000, pad_last=False):
    w, h = int(round(win * sr)), int(round(hop * sr))
    n = len(x)
    if n <= w:
        return [(0.0, x)]
    starts = list(range(0, n - w + 1, h))
    if pad_last and starts[-1] + w < n:
        starts.append(n - w)
    return [(s / sr, x[s:s + w]) for s in starts]


def _is_media(f, exts):
    return os.path.splitext(f)[1].lower() in exts


def _walk(p, recursive, exts):
    found = []
    for root, dirs, files in os.walk(p):
        dirs.sort()
        found += [os.path.join(root, f) for f in sorted(files) if _is_media(f, exts)]
        if not recursive:
            break
    return found


def _found(p, recursive, exts):
    if os.path.isdir(p):
        return _walk(p, recursive, exts)
    if os.path.isfile(p):
        return [p]
    return sorted(f for f in glob.glob(p, recursive=True) if os.path.isfile(f) and _is_media(f, exts))


def list_media(paths, recursive=True, exts=MEDIA_EXTS):
    if isinstance(paths, (str, os.PathLike)):
        paths = [paths]
    out, seen = [], set()
    for f in (f for p in map(str, paths) for f in _found(p, recursive, exts)):
        k = os.path.normcase(os.path.abspath(f))
        if k not in seen:
            seen.add(k)
            out.append(f)
    return out


def check_paths(paths, what="paths"):
    if isinstance(paths, (str, os.PathLike)):
        paths = [paths]
    parts = []
    for p in map(str, paths):
        if not list_media([p]):
            parts.append(f"{p} (no such file or folder)" if not os.path.exists(p)
                         else f"{p} (no audio or video files in it)")
    if parts:
        raise AudioError(f"{what}: nothing playable in " + ", ".join(parts[:5]) + (" ..." if len(parts) > 5 else ""))


def file_id(path):
    st = os.stat(path)
    with open(path, "rb") as f:
        digest = hashlib.sha1(f.read()).hexdigest()[:12]
    state = f"{os.path.abspath(path)}|{st.st_size}|{st.st_mtime_ns}|{digest}"
    return hashlib.sha1(state.encode()).hexdigest()[:20]


def array_id(x):
    a = np.ascontiguousarray(x, dtype=np.float32)
    return hashlib.sha1(a.tobytes() + str(a.shape).encode()).hexdigest()[:20]


def replace_quietly(tmp, dst, tries=5):
    try:
        os.replace(tmp, dst)
        return True
    except OSError:
        try:
            os.remove(tmp)
        except OSError:
            pass
        return False


def db(v, floor=1e-12):
    return float(20 * np.log10(max(float(v), floor)))


def write_wav(path, x, sr=48000, subtype="PCM_24"):
    import soundfile as sf

    Path(path).parent.mkdir(parents=True, exist_ok=True)
    sf.write(path, np.asarray(x), sr, subtype=subtype)
    return path


def make_audio() -> types.ModuleType:
    m = _module("hark.core.audio", "stand-in audio")
    for name in ("AudioError", "AUDIO_EXTS", "VIDEO_EXTS", "MEDIA_EXTS", "load", "to_mono", "resample",
                 "duration", "probe", "windows", "list_media", "check_paths", "file_id", "array_id",
                 "replace_quietly", "db", "write_wav"):
        setattr(m, name, globals()[name])
    m.SR = SR
    m.Audio = np.ndarray
    return m


# ---------------------------------------------------------------- hark.core.features

K_SOS = np.array([[1.53512485958697, -2.69169618940638, 1.19839281085285, 1.0, -1.69065929318241, 0.73248077421585],
                  [1.0, -2.0, 1.0, 1.0, -1.99004745483398, 0.99007225036621]])
BANDS = {"sub": (20, 60), "low": (60, 250), "lowmid": (250, 1000), "mid": (1000, 4000),
         "presence": (4000, 8000), "air": (8000, 16000), "top": (16000, 24000)}


def k_weight(x, sr=48000):
    from scipy.signal import sosfilt

    return sosfilt(K_SOS, np.asarray(x, np.float64), axis=0)


def _stereo(x):
    x = np.asarray(x, np.float64)
    return np.column_stack([x, x]) if x.ndim == 1 else x


def loudness_curve(x, win=0.4, hop=0.1):
    y = k_weight(_stereo(x))
    n = len(y)
    w, h = int(win * SR), int(hop * SR)
    if n < w:
        ms = (y ** 2).mean(axis=0) if n else np.zeros(2)
        return np.array([n / SR / 2]), np.array([-0.691 + 10 * np.log10(ms.sum() + 1e-20)])
    c = np.vstack([np.zeros((1, 2)), np.cumsum(y ** 2, axis=0)])
    starts = np.arange(0, n - w + 1, h)
    ms = (c[starts + w] - c[starts]) / w
    return (starts + w / 2) / SR, -0.691 + 10 * np.log10(ms.sum(axis=1) + 1e-20)


def integrated_lufs(x):
    _, L = loudness_curve(x, 0.4, 0.1)
    L = L[L > -70]
    if not len(L):
        return -70.0
    thr = 10 * np.log10(np.mean(10 ** (L / 10))) - 10
    keep = L[L > thr]
    return float(10 * np.log10(np.mean(10 ** (keep / 10)))) if len(keep) else float(thr + 10)


def lra(x):
    _, L = loudness_curve(x, 3.0, 0.1)
    L = L[L > -70]
    if len(L) < 5:
        return None
    thr = 10 * np.log10(np.mean(10 ** (L / 10))) - 20
    keep = L[L > thr]
    return float(np.percentile(keep, 95) - np.percentile(keep, 10)) if len(keep) > 4 else None


def frame_rms(m, win):
    m = np.asarray(m, np.float64)
    if len(m) < win:
        return np.array([np.sqrt(np.mean(m ** 2) + 1e-20)]) if len(m) else np.array([1e-10])
    k = len(m) // win
    return np.sqrt((m[:k * win].reshape(k, win) ** 2).mean(axis=1) + 1e-20)


def smooth_env(m, win_ms):
    m = np.asarray(m, np.float64)
    n = len(m)
    w2 = max(1, int(win_ms * SR / 2000))
    c = np.concatenate([[0.0], np.cumsum(m ** 2)])
    centres = np.arange(0, n, 48) + 24
    a = np.clip(centres - w2, 0, n)
    b = np.clip(centres + w2, 0, n)
    return np.sqrt((c[b] - c[a]) / np.maximum(b - a, 1) + 1e-20)


def _attack(e5):
    i0 = int(np.argmax(e5 >= 0.1 * e5.max()))
    L = e5[i0:i0 + 300]
    return {"attack_at": i0 / 1000, "attack_ms": float(np.argmax(L >= 0.9 * L.max())) if len(L) else None}


def _decay(e10, ip):
    A = e10[ip:]
    pad = np.concatenate([A, np.full(20, A[-1])])
    M = np.array([pad[i:i + 21].max() for i in range(len(A))])
    hit = np.nonzero(M < e10[ip] * 10 ** (-1.5))[0]
    return float(hit[0]) if len(hit) else None


def envelope_times(m):
    e5, e10 = smooth_env(m, 5), smooth_env(m, 10)
    if not len(e5):
        return {"attack_at": None, "attack_ms": None, "peak_at": None, "decay_ms": None}
    ip = int(np.argmax(e10))
    return {**_attack(e5), "peak_at": ip / 1000, "decay_ms": _decay(e10, ip)}


def true_peak_dbtp(x):
    from scipy.signal import resample_poly

    y = resample_poly(np.asarray(x, np.float64), 4, 1, axis=0)
    return db(np.max(np.abs(y)) if y.size else 0)


def brightness_hz(m, sr=48000):
    from scipy.signal import stft

    m = to_mono(np.asarray(m, np.float64))
    if len(m) < 1024:
        return None
    f, _, Z = stft(m, sr, nperseg=1024, noverlap=768, boundary=None)
    A = np.abs(Z)
    e = A.sum(axis=0)
    act = e >= e.max() * 10 ** (-1.5)
    if not act.any() or e.max() <= 0:
        return None
    cen = (f[:, None] * A[:, act]).sum(axis=0) / (A[:, act].sum(axis=0) + 1e-20)
    return int(np.median(cen))


def onsets(m, sr=48000, delta=0.07):
    import librosa

    y = resample(np.asarray(to_mono(m), np.float32), sr, 22050)
    if len(y) < 512:
        return []
    t = librosa.onset.onset_detect(y=y, sr=22050, hop_length=256, backtrack=True, delta=delta, units="time")
    return [round(float(v), 3) for v in t]


def _spectrum(m):
    from scipy.signal import welch

    n = len(m)
    e = int(np.clip(np.ceil(np.log2(max(1024, min(n, 65536)))), 10, 16))
    f, P = welch(m, SR, nperseg=min(2 ** e, n))
    return f, P + 1e-30


def _bandwidth(f, P):
    edges = 20 * 2 ** (np.arange(31) / 3)
    lv = []
    for lo, hi in zip(edges[:-1], edges[1:]):
        sel = (f >= lo) & (f < hi)
        lv.append(10 * np.log10(P[sel].mean()) if sel.any() else -300)
    lv = np.array(lv)
    keep = np.nonzero(lv >= lv.max() - 60)[0]
    return int(edges[keep[-1] + 1])


def _spectral(m, out):
    f, P = _spectrum(m)
    T = P.sum()
    out["centroid_hz"] = int(np.floor((f * P).sum() / T))
    out["bands_db"] = {k: round(float(10 * np.log10(P[(f >= lo) & (f < hi)].sum() / T + 1e-12)), 1)
                       for k, (lo, hi) in BANDS.items()}
    out["bandwidth_hz"] = _bandwidth(f, P)
    out["flatness"] = round(float(np.exp(np.mean(np.log(P))) / np.mean(P)), 4)
    out["brightness_hz"] = brightness_hz(m)


def _stereo_fields(x, out):
    L, R = x[:, 0], x[:, 1]
    side, mid = (L - R) / 2, (L + R) / 2
    rs = np.sqrt(np.mean(side ** 2))
    out["side_to_mid_db"] = round(db(rs) - db(np.sqrt(np.mean(mid ** 2)) + 1e-12), 1) if rs > 1e-7 else None
    if L.std() > 1e-9 and R.std() > 1e-9:
        out["correlation"] = round(float(np.corrcoef(L, R)[0, 1]), 3)


def _f0(yt, hr):
    import librosa

    if hr < 0.8:
        return None
    f0 = librosa.yin(yt, fmin=50, fmax=2000, sr=22050, frame_length=2048)
    rms = librosa.feature.rms(y=yt, frame_length=2048, hop_length=512)[0][:len(f0)]
    v = f0[:len(rms)][rms > 0.1 * rms.max()]
    return round(float(np.median(v)), 1) if len(v) else None


def _harmonic(m, out):
    import librosa

    yt = resample(np.asarray(m, np.float32), SR, 22050)[:30 * 22050]
    if len(yt) < 2048:
        return
    h, p = librosa.effects.hpss(yt)
    hr = float((h ** 2).sum() / ((h ** 2).sum() + (p ** 2).sum() + 1e-20))
    out["harmonic_ratio"] = round(hr, 3)
    f0 = _f0(yt, hr)
    if f0 is not None:
        out["f0_hz"] = f0


def _tonal(m, out):
    _harmonic(m, out)
    on = onsets(m)
    out.update(onsets=on[:200], onset_count=len(on), onsets_per_s=round(len(on) / out["duration"], 2))


def _source(src, start, end):
    if isinstance(src, (str, os.PathLike)):
        return load(src, start=start, end=end)
    x = np.asarray(src)
    if start is None and end is None:
        return x
    return x[int(round((start or 0) * SR)):None if end is None else int(round(end * SR))]


def _levels(x, out):
    out["peak_dbfs"] = round(db(np.max(np.abs(x))), 1)
    out["true_peak_dbtp"] = round(true_peak_dbtp(x), 1)
    out["rms_dbfs"] = round(db(np.sqrt(np.mean(x ** 2))), 1)


def _loudness(x, out):
    n = len(x)
    if n >= 0.4 * SR:
        out["lufs"] = round(integrated_lufs(x), 1)
    if n >= 3 * SR:
        out["short_term_max_lufs"] = round(float(loudness_curve(x, 3.0, 0.5)[1].max()), 1)
    v = lra(x) if n >= 6 * SR else None
    if v is not None:
        out["lra_lu"] = round(v, 1)


def _floor(m, out):
    out["noise_floor_dbfs"] = round(db(np.percentile(frame_rms(m, 960), 10)), 1)
    out["digital_silence_frac"] = round(float(np.mean(np.abs(m) < 1e-6)), 4)


def _rounded_env(m):
    return {k: (round(v) if k.endswith("_ms") and v is not None else v) for k, v in envelope_times(m).items()}


def measure(src, music=False, start=None, end=None, light=False):
    x = _stereo(_source(src, start, end))
    out = {"duration": round(len(x) / SR, 3)}
    if len(x) < 240:
        return {"duration": out["duration"], "error": "too short to measure"}
    m = x.mean(axis=1)
    _levels(x, out)
    _loudness(x, out)
    _floor(m, out)
    _stereo_fields(x, out)
    out.update(_rounded_env(m))
    _spectral(m, out)
    if not light:
        _tonal(m, out)
    return out


def make_features() -> types.ModuleType:
    m = _module("hark.core.features", "stand-in features")
    for name in ("BANDS", "k_weight", "loudness_curve", "integrated_lufs", "lra", "frame_rms", "smooth_env",
                 "envelope_times", "true_peak_dbtp", "brightness_hz", "onsets", "measure"):
        setattr(m, name, globals()[name])
    m.add_cli = lambda sub: None
    return m


# ---------------------------------------------------------------- hark.core.report

def _clean_float(obj):
    v = float(obj)
    return v if np.isfinite(v) else None


_CLEANERS = (
    (dict, lambda o: {str(k): clean(v) for k, v in o.items()}),
    ((list, tuple), lambda o: [clean(v) for v in o]),
    (np.ndarray, lambda o: clean(o.tolist())),
    (np.integer, int),
    ((float, np.floating), _clean_float),
    ((str, int, bool, type(None)), lambda o: o),
)


def clean(obj):
    fn = next((f for t, f in _CLEANERS if isinstance(obj, t)), str)
    return fn(obj)


def to_json(result, indent=1):
    return json.dumps(clean(result), indent=indent, ensure_ascii=True)


def render_text(result):
    lines = [str(result.get("summary", ""))]
    lines += [f"{k}: {json.dumps(clean(v))[:600]}" for k, v in result.items() if k not in ("summary", "text")]
    return "\n".join(lines)


def emit(result, args, text=None):
    out = getattr(args, "out", None)
    if out:
        Path(out).parent.mkdir(parents=True, exist_ok=True)
        Path(out).write_text(json.dumps(clean(result), indent=1, ensure_ascii=False), encoding="utf-8")
    if getattr(args, "json", False):
        print(to_json(result))
    else:
        print(text if text is not None else result.get("text") or render_text(result))
    return result


def error_line(cmd, msg):
    msg = " ".join(str(msg).split()) or "unknown error"
    print(f"hark {cmd}: error: {msg}", file=sys.stderr)


def fmt_t(t):
    if t is None:
        return "-"
    m, s = divmod(float(t), 60)
    return f"{int(m)}:{s:05.2f}"


def add_common(parser):
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--out", default=None)
    parser.add_argument("--threads", type=int, default=0)


def expected_errors(cmd):
    def wrap(handler):
        @functools.wraps(handler)
        def run(args):
            try:
                return handler(args)
            except (AudioError, ValueError, OSError) as exc:
                msg = " ".join(str(exc).split())
                error_line(cmd, msg)
                if getattr(args, "json", False):
                    print(json.dumps({"summary": f"error: {msg}", "error": msg}))
                return 1
        return run
    return wrap


def make_report() -> types.ModuleType:
    m = _module("hark.core.report", "stand-in report")
    for name in ("clean", "to_json", "render_text", "emit", "error_line", "fmt_t", "add_common", "expected_errors"):
        setattr(m, name, globals()[name])
    return m


# ---------------------------------------------------------------- hark.models

class MLMissing(RuntimeError):
    def __init__(self, feature, missing=("torch",), command="pip install torch transformers"):
        super().__init__(f"{feature} needs the ml extras")
        self.feature, self.missing, self.command = feature, tuple(missing), command


def _missing_ml():
    import importlib.util

    return tuple(m for m in ("torch", "transformers") if importlib.util.find_spec(m) is None)


def require_ml(feature):
    if _missing_ml():
        raise MLMissing(feature, _missing_ml())


def make_models() -> types.ModuleType:
    m = _module("hark.models", "stand-in models")
    m.__path__ = []
    m.MLMissing = MLMissing
    m.ML_MODULES = ("torch", "transformers")
    m.missing_ml = _missing_ml
    m.ml_available = lambda: not _missing_ml()
    m.require_ml = require_ml
    m.require_extra = lambda extra, feature: require_ml(feature)
    m.install_command = lambda extra="ml": "pip install torch transformers"
    return m


def make_model_module(name: str, feature: str) -> types.ModuleType:
    m = _module(f"hark.models.{name}", f"stand-in {name}")

    def get(model_id=None):
        require_ml(feature)
        raise RuntimeError(f"stand-in {name}: no real model in the stand-in")

    m.get = get
    return m


# ---------------------------------------------------------------- hark.timeline.video

def _video_frame_diff(path, cache=True):
    raise AudioError(f"{path}: no picture in the stand-in")


def _video_events(d, fps, ratio=4.0, floor=0.012, big=0.15, gap=0.12):
    d = np.asarray(d, float)
    if len(d) < 3:
        return []
    level = ratio * np.median(d) + 0.004
    out = [i for i in range(1, len(d)) if _is_cut(d[i], big, floor, level)]
    return _spaced_frames(out, fps, gap)


def _is_cut(v, big, floor, level):
    return v >= big or (v > floor and v > level)


def _spaced_frames(frames, fps, gap):
    keep, last = [], -1e9
    for i in frames:
        if (i - last) / fps >= gap:
            keep.append(i)
            last = i
    return keep


def make_video() -> types.ModuleType:
    m = _module("hark.timeline.video", "stand-in video")
    m.has_video = lambda path: bool(probe(path).get("has_video"))
    m.info = lambda path: {"duration": probe(path).get("duration"), "fps": None, "has_video": False}
    m.frame_diff = _video_frame_diff
    m.diff_events = _video_events
    return m


FACTORIES = {
    "hark.core.config": make_config,
    "hark.core.audio": make_audio,
    "hark.core.features": make_features,
    "hark.core.report": make_report,
    "hark.models": make_models,
    "hark.models.clap": lambda: make_model_module("clap", "CLAP"),
    "hark.models.ast": lambda: make_model_module("ast", "AudioSet tagger"),
    "hark.timeline.video": make_video,
}
