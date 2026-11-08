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

import numpy

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


def sounding_length(sig: numpy.ndarray) -> float:
    """Seconds from the first to the last 10 ms frame within 40 dB of the loudest (C1)."""
    mat = audio.to_mono(numpy.asarray(sig, numpy.float64))
    if not len(mat):
        return 0.0
    lv = 20 * numpy.log10(numpy.maximum(features.frame_rms(mat, 480), 1e-12))
    act = numpy.nonzero(lv >= lv.max() - LENGTH_RANGE_DB)[0]
    return round(min(len(mat) / SR, (act[-1] - act[0] + 1) * 0.01), 3)


def _padded(sig: numpy.ndarray) -> numpy.ndarray:
    sig = numpy.asarray(sig, numpy.float64)
    need = int(LOUDNESS_PAD_S * SR) - len(sig)
    if need <= 0:
        return sig
    return numpy.concatenate([sig, numpy.zeros((need,) + sig.shape[1:])])


def _low_end(bands: dict | None) -> float | None:
    if not bands or bands.get("sub") is None or bands.get("low") is None:
        return None
    return round(10 * numpy.log10(10 ** (bands["sub"] / 10) + 10 ** (bands["low"] / 10)), 1)


COPIED = ("peak_dbfs", "brightness_hz", "bandwidth_hz", "attack_ms", "decay_ms")


def _width(mat: dict) -> float:
    win_n = mat.get("side_to_mid_db")
    return MONO_DB if win_n is None else max(MONO_DB, float(win_n))


def _noise_floor(mat: dict, secs: float) -> float | None:
    if secs < NOISE_FLOOR_MIN_S or mat.get("noise_floor_dbfs") is None:
        return None
    return max(NOISE_FLOOR_DB, float(mat["noise_floor_dbfs"]))


def feats(measurements: dict, audio_x: numpy.ndarray) -> dict:
    """Measurements that silence around a sound does not change (a key is absent when unknown)."""
    mat = measurements
    secs = len(audio_x) / SR
    result = {"loudness_lufs": round(float(features.integrated_lufs(_padded(audio_x))), 1),
           **{kk: mat.get(kk) for kk in COPIED}, "length_s": sounding_length(audio_x), "duration": round(secs, 3),
           "width_db": _width(mat), "low_end_db": _low_end(mat.get("bands_db")),
           "noise_floor_dbfs": _noise_floor(mat, secs)}
    return {kk: vec for kk, vec in result.items() if vec is not None}


def measure_path(fpath: str, t_from: float | None = None, t_to: float | None = None) -> dict:
    """Trim-proof measurements of a file range (C4)."""
    sig = audio.load(fpath, start=t_from, end=t_to)
    if len(sig) < 240:
        return {"duration": round(len(sig) / SR, 3)}
    return feats(features.measure(sig, light=True), sig)


# ---------------------------------------------------------------- statistics


def _g(kname: str, vec: float) -> float:
    return float(numpy.log(max(vec, 0.001))) if kname in LOG_KEYS else float(vec)


def _spread(kname: str, grp: numpy.ndarray) -> tuple[float, float]:
    """Median and scale (MAD-based, floored per key) of transformed values."""
    med = float(numpy.median(grp))
    mad = float(numpy.median(numpy.abs(grp - med)))
    return med, max(MAD_FACTOR * mad, MIN_SCALE.get(kname, DEFAULT_MIN_SCALE))


def robust_z(kname: str, sig: float, references: Sequence[float]) -> dict | None:
    """Robust z of a value against reference values (C2); None with fewer than 2 references."""
    vals = [float(row_) for row_ in references if row_ is not None]
    if len(vals) < 2:
        return None
    med, scale = _spread(kname, numpy.array([_g(kname, row_) for row_ in vals]))
    back = float(numpy.exp(med)) if kname in LOG_KEYS else med
    p10, p90 = numpy.percentile(vals, [10, 90])
    return {"value": sig, "refs_median": round(back, 3), "robust_z": round((_g(kname, sig) - med) / scale, 2),
            "refs_p10_p90": [round(float(p10), 3), round(float(p90), 3)]}


def typicality_words(z: float) -> str:
    """Words of a typicality z."""
    if z >= -1:
        return "fits the set"
    if z >= -2:
        return "somewhat unusual for the set"
    return "an outlier: unlike the references"


def typicality(lhs: numpy.ndarray, references: numpy.ndarray, k: int = DEFAULT_K) -> dict:
    """How the sound sits among the references, compared the way each reference sits among the rest (C3)."""
    R = numpy.asarray(references, numpy.float64)
    sig = numpy.asarray(lhs, numpy.float64)
    count = len(R)
    S = R @ R.T
    numpy.fill_diagonal(S, -numpy.inf)
    kk = max(1, min(k, count - 1))
    sv = numpy.sort(S, axis=1)[:, ::-1][:, :kk].mean(axis=1)
    sx = float(numpy.sort(R @ sig)[::-1][:kk].mean())
    z = (sx - sv.mean()) / (sv.std() + 1e-6)
    return {"mean_cos_knn": round(sx, 3), "z": round(float(z), 2),
            "percentile": round(100 * float((sv <= sx).mean()), 1), "k": k, "verdict": typicality_words(z)}


def closer(sa: float, sb: float) -> tuple[str | None, str]:
    """Which of 2 scores is closer, with words (C5)."""
    dd = abs(sa - sb)
    if dd < DEAD_BAND:
        return None, "neither is closer"
    return ("a" if sa > sb else "b"), ("clearly closer" if dd >= CLEARLY else "slightly closer")


def similarity_words(cos: float) -> str:
    """Words for the CLAP cosine of 2 sounds."""
    for lo, words in SIMILARITY:
        if cos >= lo:
            return words
    return "different sounds"


def ordinal(count: int) -> str:
    """1st, 2nd, 3rd, 4th, 11th, 12th, 13th, 21st ..."""
    if 10 <= count % 100 <= 20:
        return f"{count}th"
    return f"{count}{ {1: 'st', 2: 'nd', 3: 'rd'}.get(count % 10, 'th') }".replace(" ", "")


# ---------------------------------------------------------------- differences in words


def _scaled(vec: float, small: str, big: str, at: float) -> str:
    return f"{int(round(vec))} {small}" if vec < at else f"{vec / 1000:.1f} {big}"


_FORMATS = {
    "brightness_hz": lambda v: _scaled(v, "Hz", "kHz", 1000),
    "bandwidth_hz": lambda v: _scaled(v, "Hz", "kHz", 1000),
    "attack_ms": lambda v: _scaled(v, "ms", "s", 2000),
    "decay_ms": lambda v: _scaled(v, "ms", "s", 2000),
    "length_s": lambda v: f"{v:.2f} s",
    "width_db": lambda v: "mono" if v <= MONO_DB else f"{int(round(v))} dB",
}
_UNITS = {"loudness_lufs": "LUFS", "noise_floor_dbfs": "dBFS", "peak_dbfs": "dBFS"}


def fmt_value(kname: str, vec: float) -> str:
    """A measurement value as text with its unit."""
    fmt = _FORMATS.get(kname)
    return fmt(vec) if fmt else f"{int(round(vec))} {_UNITS.get(kname, 'dB')}"


def _kept(kind: str, kname: str, lhs: float, rhs: float, thr: float) -> bool:
    if kind == "db":
        return abs(rhs - lhs) >= thr
    if lhs <= 0 or rhs <= 0:
        return False
    ok = max(lhs, rhs) / min(lhs, rhs) >= thr
    return ok and (kname != "attack_ms" or abs(rhs - lhs) >= 3)


def _phrase(row: tuple, lhs: float, rhs: float, vs: str) -> str:
    kname, _, _, up, down, label, with_ = row
    word = up if rhs > lhs else down
    if kname == "loudness_lufs":
        tail = " than their median" if vs else ""
        return f"{int(round(abs(rhs - lhs)))} dB {word}{tail}"
    inner = f"{fmt_value(kname, rhs)} vs {vs}{fmt_value(kname, lhs)}"
    text = f"{word} ({label + ' ' if label else ''}{inner})"
    return f"with {text}" if with_ else text


def differences(fa: dict, fb: dict, vs: str = "") -> list[tuple[str, str]]:
    """Up to 4 audible differences of fb against fa, as (key, phrase), in table order (C6)."""
    result = []
    for row in DIFF_TABLE:
        kname, thr, kind = row[0], row[1], row[2]
        if fa.get(kname) is None or fb.get(kname) is None:
            continue
        if _kept(kind, kname, float(fa[kname]), float(fb[kname]), thr):
            result.append((kname, _phrase(row, float(fa[kname]), float(fb[kname]), vs)))
        if len(result) >= MAX_WORDS:
            break
    return result


def join_words(phrases: Sequence[str]) -> str:
    """Plain phrases first, then "with" phrases; the last 2 joined by " and "."""
    ordered = [pth for pth in phrases if not pth.startswith("with ")] + [pth for pth in phrases if pth.startswith("with ")]
    if len(ordered) <= 1:
        return "".join(ordered)
    return ", ".join(ordered[:-1]) + " and " + ordered[-1]


# ---------------------------------------------------------------- names


def trim_name(fpath: str, t_from: float | None, t_to: float | None) -> str:
    """The base name, plus the trim when one was given."""
    name = os.path.basename(fpath)
    if t_from is None and t_to is None:
        return name
    tail = "end" if t_to is None else report.fmt_t(t_to)
    return f"{name} [{report.fmt_t(t_from or 0.0)}-{tail}]"


def _ref_files(references: Sequence[str], exclude: Sequence[str]) -> tuple[list[str], list[dict]]:
    audio.check_paths(list(references), what="refs")
    files = [pth for pth in audio.list_media(list(references)) if not any(voc.same_path(pth, ex) for ex in exclude)]
    return voc.split_readable(files)


def skipped_note(skipped: Sequence[dict]) -> str:
    """The skipped-files sentence (count, first 3 names, '...' when more)."""
    if not skipped:
        return ""
    names = ", ".join(os.path.basename(sv["path"]) for sv in skipped[:3]) + (", ..." if len(skipped) > 3 else "")
    return f" Skipped {len(skipped)} unreadable file(s): {names}."


# ---------------------------------------------------------------- compare


def _short_compare(fpath: str, count: int, skipped: list[dict]) -> dict:
    return {"summary": f"Need at least 2 readable reference files to compare; found {count}.{skipped_note(skipped)}",
            "path": fpath, "n_refs": count, "nearest": [], "skipped": skipped}


def _feature_table(fx: dict, ref_feats: list[dict]) -> dict:
    table = {}
    for kname, value in fx.items():
        if kname == "duration":
            continue
        rz = robust_z(kname, value, [fv.get(kname) for fv in ref_feats])
        if rz is not None:
            table[kname] = rz
    return table


def _outlier_words(table: dict) -> list[str]:
    keys = [row[0] for row in DIFF_TABLE if row[0] in table and abs(table[row[0]]["robust_z"]) >= OUTLIER_Z]
    result = []
    for kname in keys[:MAX_WORDS]:
        row = next(row_ for row_ in DIFF_TABLE if row_[0] == kname)
        lhs, rhs = table[kname]["refs_median"], table[kname]["value"]
        result.append(_phrase(row, float(lhs), float(rhs), "median "))
    return result


def _compare_summary(name: str, count: int, nearest: list[dict], typ: dict, outliers: list[str],
                     skipped: list[dict]) -> str:
    near = ", ".join(f"{row_['name']} ({row_['cosine']:.2f})" for row_ in nearest[:3])
    pct = ordinal(int(round(typ["percentile"])))
    pieces = [f"{name} against {count} references. Nearest: {near}.",
             f"Typicality z {voc.zf(typ['z'])} ({pct} percentile of the references' own distances): "
             f"{typ['verdict']}."]
    if outliers:
        pieces.append("Stands out: " + join_words(outliers) + ".")
    else:
        pieces.append("Every measurement is within the references' spread.")
    return " ".join(pieces) + skipped_note(skipped)


def _nearest(E: numpy.ndarray, ok: Sequence[str], kk: int) -> list[dict]:
    cos = E[1:] @ E[0]
    order = numpy.argsort(-cos, kind="stable")[:kk]
    return [{"path": ok[ix], "name": os.path.basename(ok[ix]), "cosine": round(float(cos[ix]), 3)} for ix in order]


def _median_abs_z(table: dict) -> float | None:
    zs = [abs(vec["robust_z"]) for vec in table.values()]
    return round(float(numpy.median(zs)), 2) if zs else None


def compare(path: str, refs: Sequence[str], k: int = DEFAULT_K, start: float | None = None,
            end: float | None = None) -> dict:
    """Place a sound among reference recordings."""
    refs = [refs] if isinstance(refs, str) else list(refs)
    voc.check_range(path, start, end)
    ok, skipped = _ref_files(refs, [path])
    if len(ok) < 2:
        return _short_compare(path, len(ok), skipped)
    E = voc.embed_files([path] + ok, ranges=[(start, end)] + [(None, None)] * len(ok))
    nearest = _nearest(E, ok, k)
    typ = typicality(E[0], E[1:], k)
    table = _feature_table(measure_path(path, start, end), [measure_path(pth) for pth in ok])
    summary = _compare_summary(trim_name(path, start, end), len(ok), nearest, typ, _outlier_words(table), skipped)
    return {"summary": summary, "path": path, "range": [start, end], "n_refs": len(ok), "nearest": nearest,
            "typicality": typ, "features": table, "feature_distance": _median_abs_z(table), "skipped": skipped}


def compare_text(result: dict) -> str:
    """Text output of compare."""
    lines = [result["summary"]]
    if result.get("nearest"):
        lines += ["", "nearest:"] + [f"  {row_['cosine']:.3f}  {row_['name']}" for row_ in result["nearest"]]
        lines.append("features (value / refs median / robust z):")
        for kname, vec in result.get("features", {}).items():
            lines.append(f"  {kname}: {vec['value']} / {vec['refs_median']} / {vec['robust_z']:+.2f}")
    return "\n".join(lines)


# ---------------------------------------------------------------- ab


def _query_part(E: numpy.ndarray, prompt: str) -> tuple[dict, str]:
    ra, rb = (row[0] for row in voc.get().scores(E, [prompt]))
    who, words = closer(ra["z"], rb["z"])
    rec = {"query": prompt, "a": ra, "b": rb, "closer": who, "dz": round(abs(ra["z"] - rb["z"]), 2)}
    lead = f"{who.upper()} is {words}" if who else "Neither is closer"
    return rec, f"{lead} to '{prompt}' (A z {voc.zf(ra['z'])}, B z {voc.zf(rb['z'])})."


def _refs_part(E: numpy.ndarray, references: Sequence[str], lhs: str, rhs: str, kk: int) -> tuple[dict | None, str]:
    ok, skipped = _ref_files(references, [lhs, rhs])
    if len(ok) < 2:
        return None, f"Need at least 2 readable references to judge closeness; found {len(ok)}." + \
            skipped_note(skipped)
    R = voc.embed_files(ok)
    ta, tb = typicality(E[0], R, kk), typicality(E[1], R, kk)
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
    return "B is " + join_words([pth for _, pth in diffs]) + "."


def _semantic_parts(E: numpy.ndarray, prompt: str | None, references, lhs: str, rhs: str, kk: int) -> tuple[dict, list[str]]:
    """The query and refs parts of an A/B: (result fields, sentences)."""
    rec: dict[str, Any] = {}
    pieces = []
    if prompt is not None:
        rec["query"], text = _query_part(E, str(prompt).strip())
        pieces.append(text)
    if references:
        found, text = _refs_part(E, [references] if isinstance(references, str) else list(references), lhs, rhs, kk)
        if found is not None:
            rec["refs"] = found
        pieces.append(text)
    return rec, pieces


def ab(a: str, b: str, query: str | None = None, refs: Sequence[str] | None = None, k: int = DEFAULT_K,
       a_start: float | None = None, a_end: float | None = None, b_start: float | None = None,
       b_end: float | None = None) -> dict:
    """A/B two sounds or two trims of one file."""
    ra, rb = (a_start, a_end), (b_start, b_end)
    voc.check_range(a, *ra)
    voc.check_range(b, *rb)
    if query is not None and not str(query).strip():
        raise ValueError("no query given")
    E = voc.embed_files([a, b], ranges=[ra, rb])
    cos_ab = float(E[0] @ E[1])
    rec, sem = _semantic_parts(E, query, refs, a, b, k)
    fa, fb = measure_path(a, *ra), measure_path(b, *rb)
    diffs = differences(fa, fb)
    pieces = [f"A = {trim_name(a, *ra)}, B = {trim_name(b, *rb)}.", *sem,
             f"CLAP hears them as {similarity_words(cos_ab)} (cosine {cos_ab:.2f}).", _diff_sentence(diffs)]
    result = {"a": a, "b": b, "a_range": list(ra), "b_range": list(rb), "cosine_ab": round(cos_ab, 3), **rec,
              "features": {"a": fa, "b": fb}, "differences": [kname for kname, _ in diffs]}
    return {"summary": " ".join(pieces), **result}


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
    pth = sub.add_parser("compare", help="Nearest reference recordings, typicality, feature differences.")
    pth.add_argument("path")
    pth.add_argument("--refs", nargs="+", required=True, help="reference files or folders")
    pth.add_argument("-k", type=int, default=DEFAULT_K, help="how many nearest references to list")
    pth.add_argument("--start", type=float, default=None)
    pth.add_argument("--end", type=float, default=None)
    report.add_common(pth)
    pth.set_defaults(func=_run_compare)
    qtext = sub.add_parser("ab", help="A/B two sounds or two trims.")
    qtext.add_argument("a")
    qtext.add_argument("b")
    qtext.add_argument("--query", default=None)
    qtext.add_argument("--refs", nargs="+", default=None)
    for side in ("a", "b"):
        qtext.add_argument(f"--{side}-start", type=float, default=None)
        qtext.add_argument(f"--{side}-end", type=float, default=None)
    report.add_common(qtext)
    qtext.set_defaults(func=_run_ab)
