"""compare and ab: a sound against reference recordings, and two sounds against each other.

``compare`` names the nearest references, says whether the sound is typical of the set (CLAP
typicality) and which measurements stand out (robust z). ``ab`` says which of two sounds or trims
is nearer to a text or a reference set, and which audible measurements of B differ from A.
"""

from __future__ import annotations

import argparse
import os
from collections.abc import Sequence
from typing import Any

import numpy as np

from hark.core import audio, features, report
from hark.listen import vocabulary as voc

SR = 48000
DEFAULT_K = 5
OUTLIER_Z = 2.0
MAX_WORDS = 4
DEAD_BAND = 0.3
CLEARLY = 1.0
LOUDNESS_PAD_S = 0.4
LENGTH_RANGE_DB = 40.0
NOISE_FLOOR_MIN_S = 1.0
NOISE_FLOOR_DB = -120.0
MONO_DB = -40.0
MAD_FACTOR = 1.4826
LOG_KEYS = ("brightness_hz", "bandwidth_hz", "attack_ms", "decay_ms", "length_s")
MIN_SCALE = {"loudness_lufs": 1.0, "peak_dbfs": 1.0, "noise_floor_dbfs": 2.0, "low_end_db": 1.0,
             "width_db": 2.0, "brightness_hz": 0.05, "bandwidth_hz": 0.05, "attack_ms": 0.1,
             "decay_ms": 0.1, "length_s": 0.05}
DEFAULT_MIN_SCALE = 0.1

# key, threshold, kind, up word, down word, label, "with" prefix (order = importance to the ear)
DIFF_TABLE: tuple[tuple[str, float, str, str, str, str, bool], ...] = (
    ("loudness_lufs", 1.5, "db", "louder", "quieter", "loudness", False),
    ("brightness_hz", 1.15, "ratio", "brighter", "darker", "brightness", False),
    ("attack_ms", 1.5, "ratio", "a softer attack", "a sharper attack", "", True),
    ("decay_ms", 1.5, "ratio", "a longer tail", "a shorter tail", "", True),
    ("low_end_db", 3.0, "db", "more low end", "less low end", "energy below 250 Hz", True),
    ("length_s", 1.25, "ratio", "longer", "shorter", "sounding length", False),
    ("noise_floor_dbfs", 6.0, "db", "noisier", "cleaner", "noise floor", False),
    ("width_db", 6.0, "db", "wider", "narrower", "side/mid", False),
    ("bandwidth_hz", 1.4, "ratio", "more top end", "less top end", "bandwidth", True),
)
PEAK_WORDS = ("hotter peaks", "lower peaks")
SIMILARITY = ((0.9, "nearly the same sound"), (0.75, "similar"), (0.5, "related"))


# ---------------------------------------------------------------- trim-proof measurements


def sounding_length(x: np.ndarray) -> float:
    """Seconds from the first to the last 10 ms frame within 40 dB of the loudest (C1)."""
    m = audio.to_mono(np.asarray(x, np.float64))
    if not len(m):
        return 0.0
    lv = 20 * np.log10(np.maximum(features.frame_rms(m, 480), 1e-12))
    act = np.nonzero(lv >= lv.max() - LENGTH_RANGE_DB)[0]
    return round(min(len(m) / SR, (act[-1] - act[0] + 1) * 0.01), 3)


def _padded(x: np.ndarray) -> np.ndarray:
    x = np.asarray(x, np.float64)
    need = int(LOUDNESS_PAD_S * SR) - len(x)
    if need <= 0:
        return x
    return np.concatenate([x, np.zeros((need,) + x.shape[1:])])


def _low_end(bands: dict | None) -> float | None:
    if not bands or bands.get("sub") is None or bands.get("low") is None:
        return None
    return round(10 * np.log10(10 ** (bands["sub"] / 10) + 10 ** (bands["low"] / 10)), 1)


def feats(measurements: dict, audio_x: np.ndarray) -> dict:
    """Measurements that silence around a sound does not change (a key is absent when unknown)."""
    m = measurements
    dur = len(audio_x) / SR
    width = m.get("side_to_mid_db")
    out = {"loudness_lufs": round(float(features.integrated_lufs(_padded(audio_x))), 1),
           "peak_dbfs": m.get("peak_dbfs"), "brightness_hz": m.get("brightness_hz"),
           "bandwidth_hz": m.get("bandwidth_hz"), "attack_ms": m.get("attack_ms"), "decay_ms": m.get("decay_ms"),
           "length_s": sounding_length(audio_x), "duration": round(dur, 3),
           "width_db": MONO_DB if width is None else max(MONO_DB, float(width)),
           "low_end_db": _low_end(m.get("bands_db"))}
    if dur >= NOISE_FLOOR_MIN_S and m.get("noise_floor_dbfs") is not None:
        out["noise_floor_dbfs"] = max(NOISE_FLOOR_DB, float(m["noise_floor_dbfs"]))
    return {k: v for k, v in out.items() if v is not None}


def measure_path(path: str, start: float | None = None, end: float | None = None) -> dict:
    """Trim-proof measurements of a file range (C4)."""
    x = audio.load(path, start=start, end=end)
    if len(x) < 240:
        return {"duration": round(len(x) / SR, 3)}
    return feats(features.measure(x, light=True), x)


# ---------------------------------------------------------------- statistics


def _g(key: str, v: float) -> float:
    return float(np.log(max(v, 0.001))) if key in LOG_KEYS else float(v)


def robust_z(key: str, x: float, refs: Sequence[float]) -> dict | None:
    """Robust z of a value against reference values (C2); None with fewer than 2 references."""
    vals = [float(r) for r in refs if r is not None]
    if len(vals) < 2:
        return None
    g = np.array([_g(key, r) for r in vals])
    med = float(np.median(g))
    mad = float(np.median(np.abs(g - med)))
    scale = max(MAD_FACTOR * mad, MIN_SCALE.get(key, DEFAULT_MIN_SCALE))
    back = float(np.exp(med)) if key in LOG_KEYS else med
    p10, p90 = np.percentile(vals, [10, 90])
    return {"value": x, "refs_median": round(back, 3), "robust_z": round((_g(key, x) - med) / scale, 2),
            "refs_p10_p90": [round(float(p10), 3), round(float(p90), 3)]}


def typicality_words(z: float) -> str:
    """Words of a typicality z."""
    if z >= -1:
        return "fits the set"
    if z >= -2:
        return "somewhat unusual for the set"
    return "an outlier: unlike the references"


def typicality(a: np.ndarray, refs: np.ndarray, k: int = DEFAULT_K) -> dict:
    """How the sound sits among the references, compared the way each reference sits among the rest (C3)."""
    R = np.asarray(refs, np.float64)
    x = np.asarray(a, np.float64)
    n = len(R)
    S = R @ R.T
    np.fill_diagonal(S, -np.inf)
    kk = max(1, min(k, n - 1))
    s = np.sort(S, axis=1)[:, ::-1][:, :kk].mean(axis=1)
    sx = float(np.sort(R @ x)[::-1][:kk].mean())
    z = (sx - s.mean()) / (s.std() + 1e-6)
    return {"mean_cos_knn": round(sx, 3), "z": round(float(z), 2),
            "percentile": round(100 * float((s <= sx).mean()), 1), "k": k, "verdict": typicality_words(z)}


def closer(sa: float, sb: float) -> tuple[str | None, str]:
    """Which of 2 scores is closer, with words (C5)."""
    d = abs(sa - sb)
    if d < DEAD_BAND:
        return None, "neither is closer"
    return ("a" if sa > sb else "b"), ("clearly closer" if d >= CLEARLY else "slightly closer")


def similarity_words(cos: float) -> str:
    """Words for the CLAP cosine of 2 sounds."""
    for lo, words in SIMILARITY:
        if cos >= lo:
            return words
    return "different sounds"


def ordinal(n: int) -> str:
    """1st, 2nd, 3rd, 4th, 11th, 12th, 13th, 21st ..."""
    if 10 <= n % 100 <= 20:
        return f"{n}th"
    return f"{n}{ {1: 'st', 2: 'nd', 3: 'rd'}.get(n % 10, 'th') }".replace(" ", "")


# ---------------------------------------------------------------- differences in words


def fmt_value(key: str, v: float) -> str:
    """A measurement value as text with its unit."""
    if key in ("brightness_hz", "bandwidth_hz"):
        return f"{int(round(v))} Hz" if v < 1000 else f"{v / 1000:.1f} kHz"
    if key in ("attack_ms", "decay_ms"):
        return f"{int(round(v))} ms" if v < 2000 else f"{v / 1000:.1f} s"
    if key == "length_s":
        return f"{v:.2f} s"
    if key == "width_db":
        return "mono" if v <= MONO_DB else f"{int(round(v))} dB"
    unit = {"loudness_lufs": "LUFS", "noise_floor_dbfs": "dBFS", "peak_dbfs": "dBFS"}.get(key, "dB")
    return f"{int(round(v))} {unit}"


def _kept(kind: str, key: str, a: float, b: float, thr: float) -> bool:
    if kind == "db":
        return abs(b - a) >= thr
    if a <= 0 or b <= 0:
        return False
    ok = max(a, b) / min(a, b) >= thr
    return ok and (key != "attack_ms" or abs(b - a) >= 3)


def _phrase(row: tuple, a: float, b: float, vs: str) -> str:
    key, _, _, up, down, label, with_ = row
    word = up if b > a else down
    if key == "loudness_lufs":
        tail = " than their median" if vs else ""
        return f"{int(round(abs(b - a)))} dB {word}{tail}"
    inner = f"{fmt_value(key, b)} vs {vs}{fmt_value(key, a)}"
    text = f"{word} ({label + ' ' if label else ''}{inner})"
    return f"with {text}" if with_ else text


def differences(fa: dict, fb: dict, vs: str = "") -> list[tuple[str, str]]:
    """Up to 4 audible differences of fb against fa, as (key, phrase), in table order (C6)."""
    out = []
    for row in DIFF_TABLE:
        key, thr, kind = row[0], row[1], row[2]
        if fa.get(key) is None or fb.get(key) is None:
            continue
        if _kept(kind, key, float(fa[key]), float(fb[key]), thr):
            out.append((key, _phrase(row, float(fa[key]), float(fb[key]), vs)))
        if len(out) >= MAX_WORDS:
            break
    return out


def join_words(phrases: Sequence[str]) -> str:
    """Plain phrases first, then "with" phrases; the last 2 joined by " and "."""
    ordered = [p for p in phrases if not p.startswith("with ")] + [p for p in phrases if p.startswith("with ")]
    if len(ordered) <= 1:
        return "".join(ordered)
    return ", ".join(ordered[:-1]) + " and " + ordered[-1]


# ---------------------------------------------------------------- names


def trim_name(path: str, start: float | None, end: float | None) -> str:
    """The base name, plus the trim when one was given."""
    name = os.path.basename(path)
    if start is None and end is None:
        return name
    tail = "end" if end is None else report.fmt_t(end)
    return f"{name} [{report.fmt_t(start or 0.0)}-{tail}]"


def _ref_files(refs: Sequence[str], exclude: Sequence[str]) -> tuple[list[str], list[dict]]:
    audio.check_paths(list(refs), what="refs")
    files = [p for p in audio.list_media(list(refs)) if not any(voc.same_path(p, e) for e in exclude)]
    return voc.split_readable(files)


def skipped_note(skipped: Sequence[dict]) -> str:
    """The skipped-files sentence (count, first 3 names, '...' when more)."""
    if not skipped:
        return ""
    names = ", ".join(os.path.basename(s["path"]) for s in skipped[:3]) + (", ..." if len(skipped) > 3 else "")
    return f" Skipped {len(skipped)} unreadable file(s): {names}."


# ---------------------------------------------------------------- compare


def _short_compare(path: str, n: int, skipped: list[dict]) -> dict:
    return {"summary": f"Need at least 2 readable reference files to compare; found {n}.{skipped_note(skipped)}",
            "path": path, "n_refs": n, "nearest": [], "skipped": skipped}


def _feature_table(fx: dict, ref_feats: list[dict]) -> dict:
    table = {}
    for key, value in fx.items():
        if key == "duration":
            continue
        rz = robust_z(key, value, [f.get(key) for f in ref_feats])
        if rz is not None:
            table[key] = rz
    return table


def _outlier_words(table: dict) -> list[str]:
    keys = [row[0] for row in DIFF_TABLE if row[0] in table and abs(table[row[0]]["robust_z"]) >= OUTLIER_Z]
    out = []
    for key in keys[:MAX_WORDS]:
        row = next(r for r in DIFF_TABLE if r[0] == key)
        a, b = table[key]["refs_median"], table[key]["value"]
        out.append(_phrase(row, float(a), float(b), "median "))
    return out


def _compare_summary(name: str, n: int, nearest: list[dict], typ: dict, outliers: list[str],
                     skipped: list[dict]) -> str:
    near = ", ".join(f"{r['name']} ({r['cosine']:.2f})" for r in nearest[:3])
    pct = ordinal(int(round(typ["percentile"])))
    parts = [f"{name} against {n} references. Nearest: {near}.",
             f"Typicality z {voc.zf(typ['z'])} ({pct} percentile of the references' own distances): "
             f"{typ['verdict']}."]
    if outliers:
        parts.append("Stands out: " + join_words(outliers) + ".")
    else:
        parts.append("Every measurement is within the references' spread.")
    return " ".join(parts) + skipped_note(skipped)


def compare(path: str, refs: Sequence[str], k: int = DEFAULT_K, start: float | None = None,
            end: float | None = None) -> dict:
    """Place a sound among reference recordings."""
    refs = [refs] if isinstance(refs, str) else list(refs)
    voc.check_range(path, start, end)
    ok, skipped = _ref_files(refs, [path])
    if len(ok) < 2:
        return _short_compare(path, len(ok), skipped)
    E = voc.embed_files([path] + ok, ranges=[(start, end)] + [(None, None)] * len(ok))
    cos = E[1:] @ E[0]
    order = np.argsort(-cos, kind="stable")[:k]
    nearest = [{"path": ok[i], "name": os.path.basename(ok[i]), "cosine": round(float(cos[i]), 3)} for i in order]
    typ = typicality(E[0], E[1:], k)
    fx = measure_path(path, start, end)
    table = _feature_table(fx, [measure_path(p) for p in ok])
    zs = [abs(v["robust_z"]) for v in table.values()]
    summary = _compare_summary(trim_name(path, start, end), len(ok), nearest, typ, _outlier_words(table), skipped)
    return {"summary": summary, "path": path, "range": [start, end], "n_refs": len(ok), "nearest": nearest,
            "typicality": typ, "features": table,
            "feature_distance": round(float(np.median(zs)), 2) if zs else None, "skipped": skipped}


def compare_text(result: dict) -> str:
    """Text output of compare."""
    lines = [result["summary"]]
    if result.get("nearest"):
        lines += ["", "nearest:"] + [f"  {r['cosine']:.3f}  {r['name']}" for r in result["nearest"]]
        lines.append("features (value / refs median / robust z):")
        for key, v in result.get("features", {}).items():
            lines.append(f"  {key}: {v['value']} / {v['refs_median']} / {v['robust_z']:+.2f}")
    return "\n".join(lines)


# ---------------------------------------------------------------- ab


def _query_part(E: np.ndarray, query: str) -> tuple[dict, str]:
    ra, rb = (row[0] for row in voc.get().scores(E, [query]))
    who, words = closer(ra["z"], rb["z"])
    rec = {"query": query, "a": ra, "b": rb, "closer": who, "dz": round(abs(ra["z"] - rb["z"]), 2)}
    lead = f"{who.upper()} is {words}" if who else "Neither is closer"
    return rec, f"{lead} to '{query}' (A z {voc.zf(ra['z'])}, B z {voc.zf(rb['z'])})."


def _refs_part(E: np.ndarray, refs: Sequence[str], a: str, b: str, k: int) -> tuple[dict | None, str]:
    ok, skipped = _ref_files(refs, [a, b])
    if len(ok) < 2:
        return None, f"Need at least 2 readable references to judge closeness; found {len(ok)}." + \
            skipped_note(skipped)
    R = voc.embed_files(ok)
    ta, tb = typicality(E[0], R, k), typicality(E[1], R, k)
    who, words = closer(ta["z"], tb["z"])
    rec = {"n_refs": len(ok), "a": {"mean_cos_knn": ta["mean_cos_knn"], "z": ta["z"]},
           "b": {"mean_cos_knn": tb["mean_cos_knn"], "z": tb["z"]}, "closer": who, "skipped": skipped}
    lead = f"{who.upper()} is {words}" if who else "Neither is closer"
    text = (f"{lead} to the {len(ok)} references (typicality A z {voc.zf(ta['z'])}, B z {voc.zf(tb['z'])}; "
            f"z >= -1 fits the set).") + skipped_note(skipped)
    return rec, text


def _diff_sentence(diffs: list[tuple[str, str]]) -> str:
    if not diffs:
        return "Measured: nothing audible differs in level, brightness, envelope, low end or noise."
    return "B is " + join_words([p for _, p in diffs]) + "."


def ab(a: str, b: str, query: str | None = None, refs: Sequence[str] | None = None, k: int = DEFAULT_K,
       a_start: float | None = None, a_end: float | None = None, b_start: float | None = None,
       b_end: float | None = None) -> dict:
    """A/B two sounds or two trims of one file."""
    voc.check_range(a, a_start, a_end)
    voc.check_range(b, b_start, b_end)
    if query is not None and not str(query).strip():
        raise ValueError("no query given")
    E = voc.embed_files([a, b], ranges=[(a_start, a_end), (b_start, b_end)])
    cos_ab = float(E[0] @ E[1])
    result: dict[str, Any] = {"a": a, "b": b, "a_range": [a_start, a_end], "b_range": [b_start, b_end],
                              "cosine_ab": round(cos_ab, 3)}
    parts = [f"A = {trim_name(a, a_start, a_end)}, B = {trim_name(b, b_start, b_end)}."]
    if query is not None:
        result["query"], text = _query_part(E, str(query).strip())
        parts.append(text)
    if refs:
        rec, text = _refs_part(E, [refs] if isinstance(refs, str) else list(refs), a, b, k)
        if rec is not None:
            result["refs"] = rec
        parts.append(text)
    parts.append(f"CLAP hears them as {similarity_words(cos_ab)} (cosine {cos_ab:.2f}).")
    fa, fb = measure_path(a, a_start, a_end), measure_path(b, b_start, b_end)
    diffs = differences(fa, fb)
    parts.append(_diff_sentence(diffs))
    result.update({"features": {"a": fa, "b": fb}, "differences": [k for k, _ in diffs]})
    return {"summary": " ".join(parts), **result}


# ---------------------------------------------------------------- CLI


def _require(name: str) -> None:
    import hark.models

    hark.models.require_ml(name)


@report.expected_errors("compare")
def _run_compare(args: argparse.Namespace) -> int | None:
    _require("compare")
    result = compare(args.path, args.refs, k=args.k, start=args.start, end=args.end)
    report.emit(result, args, text=compare_text(result))
    return 0


@report.expected_errors("ab")
def _run_ab(args: argparse.Namespace) -> int | None:
    _require("ab")
    result = ab(args.a, args.b, query=args.query, refs=args.refs, a_start=args.a_start, a_end=args.a_end,
                b_start=args.b_start, b_end=args.b_end)
    report.emit(result, args, text=result["summary"])
    return 0


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``compare`` and ``ab``."""
    p = sub.add_parser("compare", help="Nearest reference recordings, typicality, feature differences.")
    p.add_argument("path")
    p.add_argument("--refs", nargs="+", required=True, help="reference files or folders")
    p.add_argument("-k", type=int, default=DEFAULT_K, help="how many nearest references to list")
    p.add_argument("--start", type=float, default=None)
    p.add_argument("--end", type=float, default=None)
    report.add_common(p)
    p.set_defaults(func=_run_compare)
    q = sub.add_parser("ab", help="A/B two sounds or two trims.")
    q.add_argument("a")
    q.add_argument("b")
    q.add_argument("--query", default=None)
    q.add_argument("--refs", nargs="+", default=None)
    for side in ("a", "b"):
        q.add_argument(f"--{side}-start", type=float, default=None)
        q.add_argument(f"--{side}-end", type=float, default=None)
    report.add_common(q)
    q.set_defaults(func=_run_ab)
