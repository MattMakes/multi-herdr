"""hark critique: a launch-video sound scorecard against a profile of professional launch films (critique part)."""

from __future__ import annotations

import argparse
import functools
import json
import os
import time
from dataclasses import dataclass, field
from typing import Any

import numpy as np

from hark.timeline._dsp import ascii_safe, runs

__all__ = ["critique", "scorecard", "real_thresholds", "SPEC", "add_cli"]

# key -> (weight, tolerance, sides, practice window (lo, hi) or None, why)
SPEC: dict[str, tuple[int, float, str, tuple | None, str]] = {
    "lufs_i": (3, 1.0, "both", (-16.0, -9.0), "social and site players do not normalise, and a major video site "
               "turns masters above -14 LUFS down"),
    "true_peak": (2, 0.5, "upper", (None, -1.0), "above -1 dBTP the master clips once encoded; decoded lossy audio "
                  "above 0 dBTP clips on playback"),
    "p_real": (3, 0.0, "lower", None, "the references use real recordings and produced music; synthesized sound "
               "reads cheap next to them"),
    "music_frac": (3, 0.05, "lower", None, "the references are music-led: one track carries the energy and hides "
                   "the seams"),
    "cuts_with_sound": (2, 0.05, "lower", None, "viewers expect sound on motion; an edit point with no sound feels "
                        "lifeless"),
    "lra": (2, 1.0, "both", None, "launch films keep the body consistently strong"),
    "crest_lu": (2, 0.5, "both", None, "how far the loudest 3 s stands out from the body"),
    "first_sound_s": (2, 0.1, "upper", None, "silence at the start loses viewers on autoplay"),
    "end_silence_s": (2, 0.25, "upper", None, "dead air makes the ending feel unfinished"),
    "max_gap_s": (2, 0.25, "upper", None, "a silent hole mid-film reads as a mistake unless it is a deliberate "
                  "stop-down"),
    "final_lu": (2, 1.5, "both", None, "the logo moment should land at body level or above"),
    "onset_rate": (2, 0.3, "both", None, "too sparse feels empty, too dense feels cluttered"),
    "hit_rate": (2, 0.2, "both", None, "salient hits mark the key moments"),
    "intro_lu": (1, 1.5, "both", None, "the references open close to body level"),
    "end_lu": (1, 1.5, "both", None, "the last 10 % against the body"),
    "fade_s": (1, 0.5, "both", None, "a long fade-out loses the button"),
    "band_sub": (1, 1.5, "both", None, "weight of the sub band"),
    "band_low": (1, 1.5, "both", None, "body of the low end"),
    "band_lowmid": (1, 1.5, "both", None, "muddy against hollow"),
    "band_mid": (1, 1.5, "both", None, "clicks and voice on phone speakers"),
    "band_presence": (1, 1.5, "both", None, "bite of the transients"),
    "band_air": (1, 1.5, "both", None, "openness; without air the mix sounds muffled"),
    "side_to_mid_db": (1, 1.0, "both", None, "near-mono sounds small; too wide collapses on phones"),
    "speech_frac": (0, 0.0, "both", None, "voice share (4 of 7 references have a voice-over)"),
    "tempo": (0, 0.0, "both", None, "music tempo"),
    "peak_pos": (0, 0.0, "both", None, "where the loudest moment sits"),
    "tail_s": (0, 0.0, "both", None, "ring-out of the final hit"),
    "stopdowns_min": (0, 0.0, "both", None, "short drop-outs before hits"),
    "cut_rate": (0, 0.0, "both", None, "edit pace"),
    "cut_hit_frac": (0, 0.0, "both", None, "visual events with a salient hit (the references are near chance)"),
    "cut_frame_sync": (0, 0.0, "both", None, "visual events with an onset within a frame (near chance)"),
    "cuts_on_beat": (0, 0.0, "both", None, "visual events on the beat grid (near chance)"),
    "brightness_hz": (0, 0.0, "both", None, "perceived brightness"),
    "correlation": (0, 0.0, "both", None, "L/R correlation (1 = mono)"),
}
WEIGHT_WORD = {3: "critical", 2: "major", 1: "minor", 0: "info"}
FLOOR_UNITS = ("s", "/s", "/min", "share")
SHAPE_KEYS = ("lra", "crest_lu", "intro_lu", "end_lu", "final_lu")
BAND_ADVICE = {
    "band_sub": ("Thin bottom: add a sub layer (a sub drop or a kick) under the reveals, or use music with real low end",
                 "Too much 20-60 Hz rumble: high-pass SFX and ambience at 30-40 Hz and cut the music's sub by 2-4 dB"),
    "band_low": ("Weak low end: use a fuller track or add a low layer (bass, low hits)",
                 "Boomy: cut 100-200 Hz on the music and the heavy SFX by 2-3 dB"),
    "band_lowmid": ("Hollow: lift 250 Hz-1 kHz on the music or use fuller recordings",
                    "Muddy or boxy: cut 300-500 Hz on the music bed and the roomy SFX by 2-3 dB"),
    "band_mid": ("Recessed mids: lift 1-3 kHz on SFX or voice so they cut through phone speakers",
                 "Honky or harsh: cut 1-3 kHz on the loudest layer by 2 dB"),
    "band_presence": ("Dull: brighten the SFX or use brighter recordings",
                      "Harsh or sibilant: cut 5-7 kHz or de-ess the brightest layer"),
    "band_air": ("No air: use full-band recordings (44.1 or 48 kHz, no low-pass)",
                 "Hissy or fizzy: low-pass or de-noise the noisiest layer"),
}
MATCH_TOL = {"lufs_i": 0.15, "true_peak": 0.15, "brightness_hz": 15.0, "band_mid": 0.15}


def fmt_t(t) -> str:
    from hark.core.report import fmt_t as core_fmt

    return core_fmt(t)


def _times(ts, n: int = 8) -> str:
    ts = list(ts)
    s = ", ".join(fmt_t(t) for t in ts[:n])
    return s + (f" (+{len(ts) - n} more)" if len(ts) > n else "")


def _error(msg: str) -> Exception:
    from hark.listen.profile import ProfileError

    return ProfileError(msg)


# ---------------------------------------------------------------- listen metric table

def metric_info(key: str) -> dict:
    """{group, label, unit, decimals} of a metric from the listen table; p_real is ours."""
    if key == "p_real":
        return {"group": "realness", "label": "realness (p_real)", "unit": "p", "decimals": 2}
    from hark.listen.profile import METRICS

    # listen rows are (key, group, unit, decimals, label)
    row = next((m for m in METRICS if m[0] == key), None)
    if row is None:
        return {"group": "", "label": key, "unit": "", "decimals": 2}
    _, group, unit, decimals, label = row
    return {"group": group, "label": label, "unit": unit, "decimals": decimals}


def fmt_value(key: str, v) -> str:
    from hark.listen.profile import fmt_value as listen_fmt

    return listen_fmt(key, v)


# ---------------------------------------------------------------- realness thresholds

@functools.lru_cache(maxsize=2)
def real_thresholds(file: bool = True) -> dict:
    """{lo, hi} of the realness model for files (or clips); {0.5, 0.5} when the model is absent or fails."""
    try:
        from hark.listen import realness

        th = realness.thresholds_of(realness.load_model(), file=file)
        return {"lo": float(th["lo"]), "hi": float(th["hi"])}
    except Exception:  # noqa: BLE001 - any failure falls back (critique 2.7)
        return {"lo": 0.5, "hi": 0.5}


def fmt_p(v: float) -> str:
    s = f"{v:.2f}"
    for th in {real_thresholds(True)["lo"], real_thresholds(True)["hi"], real_thresholds(False)["lo"],
               real_thresholds(False)["hi"]}:
        if s == f"{th:.2f}" and abs(v - th) > 1e-12:
            return f"{v:.3f}"
    return s


# ---------------------------------------------------------------- scorecard

def _fixed_window(key: str, practice, lossy: bool):
    """(lo, hi, basis) when the range does not come from the references, else None."""
    if key == "p_real":
        return real_thresholds(True)["hi"], None, "realness model"
    if key == "true_peak" and lossy:
        return None, 0.0, "decoded lossy"
    return (practice[0], practice[1], "web") if practice is not None else None


def _floor(key: str, stats: dict, lo: float):
    """Lower edge for a unit that cannot go below 0: half the reference minimum for rates, else none."""
    unit = metric_info(key)["unit"]
    if lo > 0 or unit not in FLOOR_UNITS:
        return lo
    return round(stats["min"] / 2, 3) if unit == "/s" and stats["min"] > 0 else None


def _ref_window(key: str, stats: dict, tol: float, sides: str):
    if not stats or not stats.get("n"):
        return None, None, "no reference data"
    lo, hi = _floor(key, stats, stats["min"] - tol), stats["max"] + tol
    return (lo if sides in ("both", "lower") else None), (hi if sides in ("both", "upper") else None), "references"


def window(key: str, stats: dict, lossy: bool) -> tuple[float | None, float | None, str]:
    """(lo, hi, basis) of the ok range of one metric."""
    _, tol, sides, practice, _ = SPEC[key]
    fixed = _fixed_window(key, practice, lossy)
    return fixed if fixed is not None else _ref_window(key, stats, tol, sides)


def _side(value, lo, hi) -> str:
    if lo is not None and value < lo:
        return "low"
    return "high" if hi is not None and value > hi else "ok"


def status_of(value, lo, hi, basis: str, weight: int) -> str:
    if basis == "no reference data" or value is None:
        return "n.a."
    return "info" if weight == 0 else _side(value, lo, hi)


def final_weight(key: str, value, status: str, lo, hi) -> int:
    weight, tol = SPEC[key][0], SPEC[key][1]
    if key == "p_real":
        return (3 if value < 0.5 else 1) if status == "low" else weight
    if weight == 3 and status in ("low", "high"):
        edge = lo if status == "low" else hi
        if abs(value - edge) <= 2 * tol + 1e-9:
            return 2
    return weight


def _r(v, d: int = 3):
    return None if v is None else round(float(v), d)


def metric_row(key: str, value, stats: dict, chance: dict | None, lossy: bool) -> dict:
    lo, hi, basis = window(key, stats, lossy)
    status = status_of(value, lo, hi, basis, SPEC[key][0])
    w = final_weight(key, value, status, lo, hi)
    info = metric_info(key)
    row = {"metric": key, "label": info["label"], "group": info["group"], "value": value, "unit": info["unit"],
           "range": [_r(lo), _r(hi)], "basis": basis, "median": (stats or {}).get("median"),
           "typical": [stats.get("p10"), stats.get("p90")] if stats and stats.get("n") else None,
           "status": status, "weight": WEIGHT_WORD[w], "why": SPEC[key][4]}
    if chance and key in chance:
        row["chance"] = chance[key]
    return row


WEIGHT_NUM = {"critical": 3, "major": 2, "minor": 1}
VERDICTS = ((0.9, True, "launch-grade"), (0.75, True, "close"), (0.5, False, "needs work"))


def verdict_of(score, critical: int) -> str:
    if score is None:
        return "n.a."
    ok = (v for floor, clean, v in VERDICTS if score >= floor and not (clean and critical))
    return next(ok, "not launch-grade")


def _score(scored: list[dict]) -> float | None:
    total = sum(WEIGHT_NUM[r["weight"]] for r in scored)
    ok = sum(WEIGHT_NUM[r["weight"]] for r in scored if r["status"] == "ok")
    return round(ok / total, 3) if total else None


def _issues(scored: list[dict], weight: str) -> int:
    return sum(1 for r in scored if r["weight"] == weight and r["status"] != "ok")


def scorecard(values: dict, profile: dict, chance: dict | None = None, lossy: bool = False) -> dict:
    """Score metric values against a profile. No side effects."""
    stats = profile.get("metrics", {})
    rows = [metric_row(k, values.get(k), stats.get(k, {}), chance, lossy) for k in SPEC]
    scored = [r for r in rows if r["weight"] != "info" and r["status"] != "n.a."]
    score, crit = _score(scored), _issues(scored, "critical")
    return {"metrics": rows, "score": score, "verdict": verdict_of(score, crit),
            "in_range": sum(1 for r in scored if r["status"] == "ok"), "scored": len(scored),
            "critical_issues": crit, "major_issues": _issues(scored, "major")}


# ---------------------------------------------------------------- profile, match, leave-one-out

def _read_profile(path: str) -> dict:
    if not os.path.isfile(path):
        raise _error(f"profile not found: {path}")
    how = "make one with: hark profile <reference films> -o profile.json"
    try:
        with open(path, encoding="utf-8") as fh:
            data = json.load(fh)
    except (ValueError, UnicodeDecodeError) as exc:
        raise _error(f"{path} is not a profile JSON ({exc}); {how}") from None
    if not isinstance(data, dict) or not isinstance(data.get("metrics"), dict):
        raise _error(f"{path} is not a profile JSON: 'metrics' is missing; {how}")
    return data


def load_profile_arg(profile) -> tuple[dict, str]:
    from hark.listen.profile import load_profile

    if isinstance(profile, dict):
        return profile, "custom"
    if profile is None:
        return load_profile(), "bundled launch films"
    path = os.fspath(profile)
    return _read_profile(path), path


def _same_metrics(fm: dict, film: dict) -> bool:
    ref = film.get("metrics") or {}
    pairs = [(fm["metrics"].get(k), ref.get(k), tol) for k, tol in MATCH_TOL.items()]
    pairs = [(a, b, tol) for a, b, tol in pairs if a is not None and b is not None]
    return len(pairs) >= 3 and all(abs(a - b) <= tol + 1e-9 for a, b, tol in pairs)


def _is_match(fm: dict, film: dict) -> bool:
    d = film.get("duration")
    return d is not None and abs(d - fm["duration"]) <= 0.05 and _same_metrics(fm, film)


def reference_match(fm: dict, profile: dict) -> str | None:
    """The id of the profile film this file is, by duration and 4 metrics."""
    film = next((f for f in profile.get("films") or [] if _is_match(fm, f)), None)
    return None if film is None else (film.get("id") or "a reference")


def leave_one_out(profile: dict, match: str) -> dict:
    from hark.listen.profile import aggregate

    others = [dict(f, video=f.get("has_video", True), style=f.get("style") or "")
              for f in profile.get("films") or [] if (f.get("id") or "a reference") != match]
    if len(others) < 2:
        return profile
    loo = aggregate(others)
    loo.update(n=len(others), films=others, loo_of=match, n_full=profile.get("n"))
    return loo


# ---------------------------------------------------------------- measurement context

@dataclass
class Film:
    """Everything the fix rules read: film metrics, our own audio pass, cut sync, realness."""

    path: str
    video: str | None
    x: np.ndarray
    dur: float
    lufs: float
    fm: dict = field(default_factory=dict)
    onsets: list = field(default_factory=list)
    fast: tuple = ()
    slow: tuple = ()
    sync: dict | None = None
    realness: dict | None = None
    values: dict = field(default_factory=dict)
    profile: dict = field(default_factory=dict)
    fast_mode: bool = False

    @property
    def visual(self) -> list[float]:
        return list(self.fm.get("events", {}).get("visual") or [])

    def nosound(self) -> list[float] | None:
        if self.sync is None:
            return None
        return [it["t"] for it in self.sync["items"] if it["status"] in ("no sound", "off")]

    def peak_only(self) -> list[float] | None:
        if self.sync is None:
            return None
        return [it["t"] for it in self.sync["items"] if it["status"] == "peak"]


def _decode(path: str) -> tuple[np.ndarray, float, float]:
    from hark.core import audio
    from hark.core.features import integrated_lufs

    x = audio.load(path)
    dur = len(x) / 48000
    return x, dur, (float(integrated_lufs(x)) if dur >= 0.4 else -70.0)


def _audio_pass(f: Film) -> None:
    from hark.core.audio import to_mono
    from hark.core.features import loudness_curve, onsets

    m = np.asarray(to_mono(f.x), dtype=np.float64)
    f.onsets = list(onsets(m))
    f.fast = tuple(np.asarray(v, dtype=float) for v in loudness_curve(f.x, 0.1, 0.02))
    f.slow = tuple(np.asarray(v, dtype=float) for v in loudness_curve(f.x, 3.0, 0.1))


def cut_sync(x: np.ndarray, visual: list[float], fps: float | None) -> dict | None:
    """Sync of the visual events by the timeline matcher, or None."""
    if not visual:
        return None
    try:
        from hark.timeline.timeline import detect_events, level_envs, sync

        return sync(visual, detect_events(x), None, 1 / fps if fps else None, level_envs(x))
    except Exception:  # noqa: BLE001 - the onset lists are the fallback (critique 2.5)
        return None


def _realness(path: str) -> dict:
    try:
        from hark.listen.realness import realness

        r = realness(path)
        return {k: r.get(k) for k in ("p_real", "verdict", "synth_windows", "segments", "summary")}
    except Exception as exc:  # noqa: BLE001 - kept as an error row (critique 2.2 step 8)
        return {"error": f"{type(exc).__name__}: {exc}"}


# ---------------------------------------------------------------- fix text helpers

def _rng(f: Film, key: str) -> str:
    st = f.profile.get("metrics", {}).get(key) or {}
    if not st.get("n"):
        return ""
    return f"refs {fmt_value(key, st['min'])} .. {fmt_value(key, st['max'])}"


def _end(text: str, rng: str) -> str:
    return f"{text} ({rng})." if rng else f"{text}."


def _pmean(v: np.ndarray) -> float:
    return float(10 * np.log10(np.mean(10 ** (v / 10)))) if v.size else -np.inf


def body_extremes(f: Film) -> tuple | None:
    """(quiet start, quiet end, quiet level, loud t, loud level) on the 3 s curve, or None when the body is empty."""
    ts, Ls = f.slow
    body = np.flatnonzero((ts > 0.1 * f.dur) & (ts < 0.9 * f.dur))
    if not body.size:
        return None
    q = body[int(np.argmin(Ls[body]))]
    k = int(np.argmax(Ls))
    return max(0.0, ts[q] - 1.5), ts[q] + 1.5, round(float(Ls[q]), 1), float(ts[k]), round(float(Ls[k]), 1)


def _hole_ok(tf: np.ndarray, s: int, e: int, keep: tuple) -> bool:
    """A closed run of 0.5 s or more between the first sound and the end silence. keep is (hop, first, last, n)."""
    hop, first, last, n = keep
    return e < n and (e - s) * hop >= 0.5 and tf[s] > first and tf[e] < last


def holes(f: Film) -> list[tuple[float, float]]:
    tf, Lf = f.fast
    first = f.values.get("first_sound_s") or 0.0
    last = f.dur - (f.values.get("end_silence_s") or 0.0)
    keep = (float(tf[1] - tf[0]) if tf.size > 1 else 0.02, first, last, Lf.size)
    return [(round(tf[s] - 0.05, 2), round(tf[e - 1] + 0.05, 2)) for s, e in runs(Lf < f.lufs - 30, 1)
            if _hole_ok(tf, s, e, keep)]


def without(times, targets, r: float = 0.1) -> list[float]:
    tg = np.asarray(sorted(targets), dtype=float)
    return [t for t in times if not tg.size or np.min(np.abs(tg - t)) > r + 1e-9]


def music_holes(f: Film) -> list[tuple[float, float]]:
    wins = f.fm.get("events", {}).get("music_windows") or []
    spans: list[list[float]] = []
    for t, p_music, *_ in wins:
        if p_music < 0.3:
            a, b = t, min(f.dur, t + 5)
            if spans and a <= spans[-1][1] + 1e-6:
                spans[-1][1] = b
            else:
                spans.append([a, b])
    return [(a, b) for a, b in spans[:6]]


# ---------------------------------------------------------------- actions per metric

def act_lufs(f: Film, key: str, v: float, status: str) -> str:
    if status == "low":
        return _end(f"Raise the master by {-14 - v:+.1f} dB to about -14 LUFS with a true-peak limiter at a "
                    f"-1 dBTP ceiling; if the limiter would work harder than about 3 dB, compress the music bus "
                    f"first", _rng(f, key))
    return _end(f"Turn the master down by {v + 12:.1f} dB to about -12 LUFS: above -9 LUFS the normalising "
                f"platforms turn it down anyway, and the limiter flattens every hit", _rng(f, key))


def act_true_peak(f: Film, key: str, v: float, status: str) -> str:
    lossy = ", so the decoded lossy audio already clips on playback" if f.fm.get("lossy") else ""
    return (f"True peak is {v:+.1f} dBTP{lossy}; end the chain with a true-peak limiter at -1.0 dBTP and "
            f"re-encode.")


def act_music(f: Film, key: str, v: float, status: str) -> str:
    gaps = music_holes(f)
    where = "; no music at " + ", ".join(f"{fmt_t(a)}-{fmt_t(b)}" for a, b in gaps) if gaps else ""
    return _end(f"Music plays in only {100 * v:.0f}% of the film{where}. Run one continuous track under the whole "
                f"film and duck it under the voice instead of stopping it", _rng(f, key))


def act_cuts(f: Film, key: str, v: float, status: str) -> str:
    vis = f.visual
    missing = without(vis, f.onsets)
    head = _end(f"{len(missing)} of {len(vis)} visual events have no onset within 100 ms", _rng(f, key))
    advice = "put a whoosh into the transition that peaks on the cut, or a hit or a UI click there"
    nosound = f.nosound()
    if nosound is None:
        return f"{head} Missing at {_times(missing)}: {advice}."
    text = f"{head} No sound at all at {_times(nosound)}: {advice}." if nosound else \
        f"{head} Every one of them has some sound under it."
    extra = len(missing) - len(nosound)
    if extra > 0:
        text += (f" {extra} have sound playing through them (a swell, music or a level peak) but no new attack; "
                 f"add an accent on the ones that matter.")
    return text


def _span(a: float, b: float) -> str:
    return f"{fmt_t(a)}-{fmt_t(b)}"


def _shape_high(f: Film, key: str, v: float, ext: tuple) -> str:
    qa, qb, ql, lt, ll = ext
    loud = _span(max(0, lt - 1.5), lt + 1.5)
    if key == "lra":
        return _end(f"The level swings {v:.1f} LU: the body is quietest at {_span(qa, qb)} ({ql:.1f} LUFS) and "
                    f"the loudest 3 s is {loud} ({ll:.1f} LUFS); lift the quiet parts or compress the music bus "
                    f"2-4 dB", _rng(f, key))
    return _end(f"The loudest 3 s at {loud} ({ll:.1f} LUFS) stands {v:.1f} LU above integrated, so the rest plays "
                f"quiet next to it; lift the body (quietest at {_span(qa, qb)}) or tame that peak", _rng(f, key))


def _shape_flat(f: Film, key: str, v: float) -> str:
    if key == "lra":
        return _end(f"LRA is {v:.1f} LU, so the level never moves; give the film sections (a calmer voice or UI "
                    f"passage, a lift into the reveal) or ease the limiter", _rng(f, key))
    return _end(f"The loudest 3 s is only {v:.1f} LU above integrated, so no moment stands out; let the main "
                f"reveal or the logo hit rise 2-3 LU above the body, or ease the limiter", _rng(f, key))


def act_shape(f: Film, key: str, v: float, status: str) -> str:
    ext = body_extremes(f) if status == "high" else None
    return _shape_high(f, key, v, ext) if ext is not None else _shape_flat(f, key, v)


def act_first(f: Film, key: str, v: float, status: str) -> str:
    tf, Lf = f.fast
    before = _pmean(Lf[tf < v])
    if before > -60:
        return _end(f"Nothing reaches body level until {fmt_t(v)}: before it the level is {before:.0f} LUFS, "
                    f"{f.lufs - before:.0f} LU under the body; raise the opening or start the music at 0:00",
                    _rng(f, key))
    return _end(f"Silence until {fmt_t(v)}; start music, ambience or a riser at 0:00", _rng(f, key))


def act_end_silence(f: Film, key: str, v: float, status: str) -> str:
    return _end(f"{v:.2f} s of dead air after the sound ends at {fmt_t(f.dur - v)}; trim the picture or let the "
                f"final sting ring out over the end card", _rng(f, key))


def act_gap(f: Film, key: str, v: float, status: str) -> str:
    hs = holes(f)
    where = " (" + ", ".join(_span(a, b) for a, b in hs[:6]) + ")" if hs else ""
    return _end(f"The longest hole is {v:.2f} s{where}; keep a music bed or room tone under it, or make each hole "
                f"a deliberate stop-down that ends on a hit", _rng(f, key))


def act_final(f: Film, key: str, v: float, status: str) -> str:
    if status == "high":
        return _end(f"The final hit is {v:.1f} LU above the body; pull it down 2-3 dB", _rng(f, key))
    at = f" at the last visual event ({fmt_t(f.visual[-1])})" if f.visual else ""
    return _end(f"The final moment sits {-v:.1f} LU under the body; end on a logo sting or a hit{at} instead of "
                f"fading away", _rng(f, key))


def act_onsets(f: Film, key: str, v: float, status: str) -> str:
    if status == "low":
        return _end(f"Sparse: {v:.2f} onsets per second; use music with more drive, or add UI foley (clicks, "
                    f"typing, ticks) on the on-screen actions", _rng(f, key))
    return _end(f"Busy: {v:.2f} onsets per second; thin out the SFX layer and keep the accents for the important "
                f"moments", _rng(f, key))


def biggest_without_hit(f: Film) -> list[float]:
    ev = f.fm.get("events", {})
    pairs = sorted(zip(ev.get("visual") or [], ev.get("visual_strength") or []), key=lambda p: -p[1])
    hits = ev.get("hits") or []
    return sorted([t for t, _ in pairs if without([t], hits)][:6])


def act_hits(f: Film, key: str, v: float, status: str) -> str:
    if status == "high":
        return _end(f"Too many salient hits ({v:.2f} per second); keep the biggest for the reveals and soften the "
                    f"rest by 3-6 dB", _rng(f, key))
    word = "No" if v == 0 else "Few"
    text = _end(f"{word} salient hits ({v:.2f} per second); add impacts or accents on the key moments", _rng(f, key))
    big = biggest_without_hit(f)
    return text + (f" The biggest visual events without a hit: {_times(big)}." if big else "")


def act_edges(f: Film, key: str, v: float, status: str) -> str:
    span = _span(0, 0.1 * f.dur) if key == "intro_lu" else _span(0.9 * f.dur, f.dur)
    part = "opening" if key == "intro_lu" else "ending"
    if status == "high":
        return _end(f"The {part} ({span}) is {v:.1f} LU louder than the body; bring it down or lift the body",
                    _rng(f, key))
    fix = "start closer to full level or shorten the build-up" if key == "intro_lu" else \
        "keep the energy up into the end card and finish on a sting"
    return _end(f"The {part} ({span}) sits {-v:.1f} LU under the body; {fix}", _rng(f, key))


def act_fade(f: Film, key: str, v: float, status: str) -> str:
    if status == "high":
        return _end(f"The fade-out takes {v:.2f} s; end on a sting with a short ring-out, or cut hard on the last "
                    f"picture cut", _rng(f, key))
    return _end(f"The end is cut hard ({v:.2f} s); let the last hit ring out briefly", _rng(f, key))


def act_band(f: Film, key: str, v: float, status: str) -> str:
    advice = BAND_ADVICE[key][0 if status == "low" else 1]
    rng = _rng(f, key)
    return f"{advice} (now {fmt_value(key, v)}" + (f"; {rng})." if rng else ").")


def act_width(f: Film, key: str, v: float, status: str) -> str:
    if status == "high":
        return _end(f"The mix is very wide ({fmt_value(key, v)}); check the mono fold-down and narrow the wide "
                    f"ambience or reverbs", _rng(f, key))
    head = "The mix is mono: left and right are identical" if f.fm.get("mono") else \
        f"The mix is nearly mono ({fmt_value(key, v)})"
    return _end(f"{head}; use stereo music and ambience, and pan the SFX with the motion", _rng(f, key))


ACTIONS = {"lufs_i": act_lufs, "true_peak": act_true_peak, "music_frac": act_music, "cuts_with_sound": act_cuts,
           "lra": act_shape, "crest_lu": act_shape, "first_sound_s": act_first, "end_silence_s": act_end_silence,
           "max_gap_s": act_gap, "final_lu": act_final, "onset_rate": act_onsets, "hit_rate": act_hits,
           "intro_lu": act_edges, "end_lu": act_edges, "fade_s": act_fade, "side_to_mid_db": act_width,
           **{k: act_band for k in BAND_ADVICE}}
NO_ACTION = {("true_peak", "low"), ("music_frac", "high"), ("cuts_with_sound", "high"), ("first_sound_s", "low"),
             ("end_silence_s", "low"), ("max_gap_s", "low")}


# ---------------------------------------------------------------- realness action

def _segments(f: Film) -> list[dict]:
    return [s for s in (f.realness or {}).get("segments") or [] if s.get("p_real") is not None]


def quiet_miss(f: Film, p: float) -> bool:
    return p >= 0.5 and not any(s["p_real"] < 0.5 for s in _segments(f))


def _pick_windows(segs: list[dict], keep: int) -> list[dict]:
    kept: list[dict] = []
    for s in segs:
        if all(abs(s["start"] - k["start"]) >= 2.0 for k in kept):
            kept.append(s)
        if len(kept) >= keep:
            break
    return sorted(kept, key=lambda s: s["start"])


def _realness_where(f: Film, tw: dict) -> str:
    segs = sorted(_segments(f), key=lambda s: s["p_real"])
    if not segs:
        return ""
    flagged = [s for s in segs if s["p_real"] <= tw["lo"]]
    picked = _pick_windows(flagged or segs, 6 if flagged else 3)
    listed = ", ".join(f"{_span(s['start'], s['end'])} ({s['p_real']:.2f})" for s in picked)
    if flagged:
        more = len(flagged) - len(picked)
        return (f" 4 s windows at or below the window threshold {tw['lo']:.2f}: {listed}"
                + (f" (+{more} more)" if more > 0 else "") + ".")
    return f" Lowest-scoring 4 s windows: {listed}."


def act_realness(f: Film, key: str, p: float, status: str) -> str | None:
    if quiet_miss(f, p):
        return None
    t = real_thresholds(True) if _segments(f) else real_thresholds(False)
    tw = real_thresholds(False)
    if p <= t["lo"]:
        head = f"Realness {fmt_p(p)} is at or below {t['lo']:.2f}: likely code-synthesized."
    elif p < 0.5:
        head = (f"Realness {fmt_p(p)} leans code-synthesized (the realness model calls {t['lo']:.2f}..{t['hi']:.2f} "
                f"uncertain).")
    else:
        head = f"Realness {fmt_p(p)} is uncertain; real or pro-produced sound scores at least {t['hi']:.2f}."
    fix = (" If the film should sound recorded or produced, replace the oscillator or noise layers with real "
           "recordings and produced music (the sound-sourcing skill covers sourcing and licences)." if p < 0.5 else
           " If the film should sound recorded or produced, find and replace the layer that sounds synthetic.")
    return head + _realness_where(f, tw) + fix + (" Judge each stem with `hark realness stem.wav`: a finished mix "
                                                  "is harder to judge than its sources.")


ACTIONS["p_real"] = act_realness


def _action_text(f: Film, row: dict) -> str | None:
    key, status = row["metric"], row["status"]
    usable = status in ("low", "high") and (key, status) not in NO_ACTION and key in ACTIONS
    return ACTIONS[key](f, key, row["value"], status) if usable else None


def _action(f: Film, row: dict) -> dict | None:
    """The action of one out-of-range row (also written into the row), or None."""
    key, status = row["metric"], row["status"]
    text = _action_text(f, row)
    if not text:
        return None
    edge = row["range"][0] if status == "low" else row["range"][1]
    dist = abs(row["value"] - edge) / max(SPEC[key][1], 0.001)
    row["actions"] = [ascii_safe(text)]
    return {"metric": key, "weight": row["weight"], "w": WEIGHT_NUM[row["weight"]], "dist": round(dist, 3),
            "action": ascii_safe(text)}


def build_actions(f: Film, rows: list[dict]) -> list[dict]:
    out = [a for a in (_action(f, row) for row in rows) if a is not None]
    return sorted(out, key=lambda a: (-a["w"], -a["dist"]))


# ---------------------------------------------------------------- tips and not measured

def _cut_tip(f: Film, acted: set) -> str | None:
    nos, peaks = f.nosound(), f.peak_only()
    if not nos or f.values.get("cuts_with_sound") is None or "cuts_with_sound" in acted:
        return None
    s = (f"{len(nos)} of {len(f.visual)} visual events get no sound at all (no onset within 100 ms, no whoosh "
         f"or riser peak within 300 ms, no level peak within 150 ms): {_times(nos)}. Give the ones that matter "
         f"a whoosh into the cut, a hit or a UI click, or keep one as a deliberate stop-down.")
    more = f" {len(peaks)} more are met only by a level peak; check them on the sheet: {_times(peaks, 4)}."
    return s + (more if peaks else "")


def _realness_tip(f: Film, acted: set) -> str | None:
    p, line = f.values.get("p_real"), real_thresholds(True)["hi"]
    if p is None or "p_real" in acted or p >= line or not quiet_miss(f, p):
        return None
    segs = _segments(f)
    low = f" (lowest window {min(s['p_real'] for s in segs):.2f})" if segs else ""
    return (f"Realness {fmt_p(p)} is under the {line:.2f} line for real sound, but nothing synthetic was "
            f"found{low}; there is no layer to replace on this evidence. Check the stems with `hark realness`.")


def _stopdown_tip(f: Film) -> str | None:
    if f.values.get("stopdowns_min") != 0 or not f.visual:
        return None
    ev = f.fm.get("events", {})
    pairs = list(zip(ev.get("visual") or [], ev.get("visual_strength") or []))
    big = max(pairs, key=lambda q: q[1])[0] if pairs else f.visual[0]
    return (f"No stop-downs: 4 of 7 references drop the sound out for 0.2-1 s right before a big hit; try one "
            f"before the biggest visual change at {fmt_t(big)}.")


def _voice_tip(f: Film) -> str | None:
    sp = f.values.get("speech_frac")
    if sp is None or sp < 0.5:
        return None
    return (f"Voice in {100 * sp:.0f}% of the film: duck the music 3-6 dB under the voice and keep the SFX "
            f"out of 1-4 kHz while it speaks.")


def tips(f: Film, acted: set) -> list[str]:
    found = (_cut_tip(f, acted), _realness_tip(f, acted), _stopdown_tip(f), _voice_tip(f))
    return [ascii_safe(t) for t in found if t]


NOT_MEASURED = {"p_real": "realness unavailable", "music_frac": "no AudioSet windows"}


def not_measured_reason(f: Film, key: str) -> str:
    if key in NOT_MEASURED and f.fast_mode:
        return "skipped by --fast"
    reason = NOT_MEASURED.get(key)
    if key == "cuts_with_sound":
        reason = "fewer than 3 visual events" if f.video else "no picture (pass --video film.mp4)"
    if reason is None and key in SHAPE_KEYS and f.dur < 10:
        reason = "clip under 10 s; the references are 20-93 s films"
    return reason or "not measurable on this file"


def not_measured(f: Film, rows: list[dict]) -> dict:
    out: dict[str, list[str]] = {}
    for row in rows:
        if row["status"] == "n.a." and row["weight"] != "info":
            out.setdefault(not_measured_reason(f, row["metric"]), []).append(row["metric"])
    return out


# ---------------------------------------------------------------- text

def range_text(row: dict) -> str:
    if row["weight"] == "info":
        t = row.get("typical")
        return f"refs p10-p90 {fmt_value(row['metric'], t[0])} .. {fmt_value(row['metric'], t[1])}" if t else ""
    lo, hi = row["range"]
    key = row["metric"]
    fv = (lambda v: fmt_p(v)) if key == "p_real" else (lambda v: fmt_value(key, v))
    if lo is None and hi is None:
        return row["basis"]
    body = f">= {fv(lo)}" if hi is None else f"<= {fv(hi)}" if lo is None else f"{fv(lo)} .. {fv(hi)}"
    return body + (f" ({row['basis']})" if row["basis"] != "references" else "")


def verdict_line(res: dict) -> str:
    s = res["verdict"]
    if res["partial"]:
        s += f" (partial: {' and '.join(res['partial'])} not checked)"
    if res["score"] is not None:
        s += (f" - {res['in_range']} of {res['scored']} scored metrics in range (weighted {100 * res['score']:.0f}%),"
              f" {res['critical_issues']} critical, {res['major_issues']} major")
    return s


def _vs(res: dict) -> str:
    return f"vs {res['profile_n']} reference films ({res['profile']})"


def _loo_note(res: dict) -> str | None:
    m = res.get("reference_match")
    if not m:
        return None
    return (f"This file is reference {m['id']} of the profile, so it is scored against the other "
            f"{m['scored_against']} references; its own values do not set the ranges.")


def _nm_text(nm: dict) -> str:
    return "; ".join(f"{reason}: {', '.join(keys)}" for reason, keys in nm.items())


def summary_text(res: dict) -> str:
    parts = [f"{os.path.basename(res['path'])} {_vs(res)}.", _loo_note(res), f"Verdict: {verdict_line(res)}."]
    if res["actions"]:
        first = res["actions"][0]["action"]
        parts.append("Fix first: " + first + ("" if first.endswith(".") else "."))
    else:
        parts.append("Nothing needs fixing against the references.")
    if res["not_measured"]:
        parts.append(f"Not measured: {_nm_text(res['not_measured'])}.")
    return ascii_safe(" ".join(p for p in parts if p))


def _scorecard_lines(rows: list[dict]) -> list[str]:
    lines = ["SCORECARD", f"{'':8} {'metric':30} {'value':>12}  {'ok range':30} {'ref median':>12}  status"]
    for row in rows:
        if row["weight"] == "info":
            continue
        key = row["metric"]
        val = "n.a." if row["value"] is None else fmt_p(row["value"]) if key == "p_real" else \
            fmt_value(key, row["value"])
        med = "" if row["median"] is None else fmt_value(key, row["median"]) if key != "p_real" else fmt_p(row["median"])
        st = row["status"] + (f" ({row['weight']})" if row["status"] in ("low", "high") else "")
        lines.append(f"{row['group'][:8]:8} {row['label'][:30]:30} {val:>12}  {range_text(row)[:30]:30} {med:>12}  {st}")
    return lines


def _arc_lines(arc: dict | None) -> list[str]:
    if not arc or not arc.get("film") or not arc.get("refs_median"):
        return []

    def row(vals):
        return " ".join("     -" if v is None else f"{v:+6.1f}" for v in vals)

    return ["", "LOUDNESS ARC (LU vs integrated at 0%, 10% .. 100%; not scored)",
            f"  this film  {row(arc['film'])}", f"  ref median {row(arc['refs_median'])}"]


def _info_lines(rows: list[dict]) -> list[str]:
    out = ["", "NOT SCORED (style choices)"]
    for row in rows:
        if row["weight"] == "info" and row["value"] is not None:
            ch = f" (chance {fmt_value(row['metric'], row['chance'])})" if row.get("chance") is not None else ""
            out.append(f"  {row['label']}: {fmt_value(row['metric'], row['value'])}{ch}  {range_text(row)}")
    return out


def _partial_note(partial: list[str]) -> str | None:
    if len(partial) == 2:
        return ("  Run without --fast before shipping: realness and music presence are critical checks that this "
                "run skipped.")
    if partial:
        return f"  {partial[0]} could not be measured."
    return None


def _fix_lines(res: dict) -> list[str]:
    lines = []
    if res["actions"]:
        lines.append("FIX FIRST (most important first):")
        lines += [f"  {k}. [{a['weight']}] {a['action']}" for k, a in enumerate(res["actions"], 1)]
    lines += [f"  tip: {t}" for t in res["tips"]]
    if res["not_measured"]:
        lines.append(f"NOT MEASURED: {_nm_text(res['not_measured'])}")
    return lines


def _realness_lines(real: dict | None) -> list[str]:
    real = real or {}
    if real.get("summary"):
        return ["", f"realness: {real['summary']}"]
    return ["", f"realness: not available ({real['error'][:120]})"] if real.get("error") else []


def critique_text(res: dict) -> str:
    lines = [f"hark critique: {os.path.basename(res['path'])} ({res['duration']:.1f} s) {_vs(res)}", _loo_note(res),
             f"VERDICT: {verdict_line(res)}", _partial_note(res["partial"]), *_fix_lines(res), "",
             *_scorecard_lines(res["metrics"]), *_arc_lines(res.get("arc")), *_info_lines(res["metrics"]),
             *_realness_lines(res.get("realness"))]
    return ascii_safe("\n".join(line for line in lines if line is not None))


# ---------------------------------------------------------------- the run

def _check_video(video: str) -> None:
    from hark.core.audio import probe
    from hark.timeline.timeline import is_picture

    if not os.path.exists(video):
        raise _error(f"file not found: {video}")
    if not is_picture(video, probe(video)):
        raise _error(f"{video} has no picture (no video stream)")


def _check_media(path: str, video: str | None) -> str | None:
    from hark.core.audio import probe
    from hark.listen.profile import media_problem
    from hark.timeline.timeline import is_picture

    problem = media_problem(path)
    if problem:
        raise _error(f"file not found: {path}" if problem == "not found" else problem)
    if video and os.path.abspath(video) == os.path.abspath(path) and not is_picture(path, probe(path)):
        video = None
    if video:
        _check_video(video)
    return video or (path if is_picture(path, probe(path)) else None)


def unscored(path: str, video, dur: float, reason: str) -> dict:
    text = ascii_safe(f"{os.path.basename(path)}: {reason}, so there is nothing to score against launch films. "
                      f"Use the single-sound commands (hark describe, hark realness) for a short or silent clip.")
    return {"path": path, "video": video, "duration": round(dur, 3), "verdict": "n.a.", "score": None, "in_range": 0,
            "scored": 0, "critical_issues": 0, "major_issues": 0, "partial": [], "actions": [], "tips": [],
            "metrics": [], "summary": text, "text": text}


def _unscorable_reason(dur: float, lufs: float) -> str | None:
    if dur < 3.0:
        return f"it is shorter than 3 s ({dur:.2f} s)"
    if lufs <= -69.5:
        return "it is silent (nothing above -70 LUFS)"
    return None


def _measure(f: Film, use_ast: bool, use_realness: bool) -> None:
    from hark.listen.profile import film_metrics

    f.fm = film_metrics(f.path, video=f.video, use_ast=use_ast)
    _audio_pass(f)
    match = reference_match(f.fm, f.profile)
    if match:
        f.profile = leave_one_out(f.profile, match)
    f.sync = cut_sync(f.x, f.visual, f.fm.get("fps"))
    f.values = dict(f.fm["metrics"], p_real=None)
    if use_realness:
        f.realness = _realness(f.path)
        f.values["p_real"] = f.realness.get("p_real")


def _events_block(f: Film) -> dict:
    ev = f.fm.get("events", {})
    return {"visual": f.visual, "visual_without_onset": without(f.visual, f.onsets),
            "visual_without_any_sound": f.nosound(), "visual_without_hit": without(f.visual, ev.get("hits") or []),
            "hits": ev.get("hits") or [], "stopdowns": ev.get("stopdowns") or []}


CARD_KEYS = ("verdict", "score", "in_range", "scored", "critical_issues", "major_issues")


def _arc_block(f: Film) -> dict:
    arc = f.profile.get("arc") or {}
    return {"film": f.fm.get("arc"), "refs_median": arc.get("median"), "refs_p10": arc.get("p10"),
            "refs_p90": arc.get("p90")}


def _match_block(f: Film) -> dict | None:
    if not f.profile.get("loo_of"):
        return None
    return {"id": f.profile["loo_of"], "scored_against": f.profile.get("n")}


def _partial(f: Film) -> list[str]:
    return [n for n, k in (("realness", "p_real"), ("music presence", "music_frac")) if f.values.get(k) is None]


def _result(f: Film, card: dict, label: str, t_start: float) -> dict:
    from hark.core.report import clean

    actions = build_actions(f, card["metrics"])
    res = {"path": f.path, "video": f.video, "duration": round(f.dur, 3), "profile_n": f.profile.get("n"),
           "lossy": bool(f.fm.get("lossy")), "profile": label, **{k: card[k] for k in CARD_KEYS},
           "partial": _partial(f), "actions": [{k: a[k] for k in ("weight", "metric", "action")} for a in actions],
           "tips": tips(f, {a["metric"] for a in actions}), "metrics": card["metrics"],
           "reference_match": _match_block(f), "events": _events_block(f), "arc": _arc_block(f),
           "realness": f.realness, "not_measured": not_measured(f, card["metrics"]),
           "seconds": round(time.perf_counter() - t_start, 1)}
    res = clean(res)
    res["summary"] = summary_text(res)
    res["text"] = critique_text(res)
    return res


def critique(path, profile=None, video=None, use_ast=True, use_realness=True) -> dict:
    """Score a soundtrack or film against a launch-film profile and say what to fix."""
    t_start = time.perf_counter()
    path = os.fspath(path)
    video = _check_media(path, os.fspath(video) if video else None)
    prof, label = load_profile_arg(profile)
    x, dur, lufs = _decode(path)
    reason = _unscorable_reason(dur, lufs)
    if reason:
        return unscored(path, video, dur, reason)
    f = Film(path, video, x, dur, lufs, profile=prof, fast_mode=not (use_ast or use_realness))
    _measure(f, use_ast, use_realness)
    card = scorecard(f.values, f.profile, f.fm.get("chance"), bool(f.fm.get("lossy")))
    return _result(f, card, label, t_start)


# ---------------------------------------------------------------- CLI

def _handler(args: argparse.Namespace) -> int:
    from hark.core.audio import AudioError
    from hark.core.report import emit, error_line
    from hark.listen.profile import ProfileError

    try:
        if not args.fast:
            from hark.models import require_ml

            require_ml("critique")
        res = critique(args.file, profile=args.profile, video=args.video, use_ast=not args.fast,
                       use_realness=not args.fast)
    except (ProfileError, AudioError) as exc:
        error_line("critique", str(exc))
        return 1
    emit(res, args)
    return 0


def add_cli(sub: Any) -> None:
    """Register `hark critique`."""
    from hark.core.report import add_common

    p = sub.add_parser("critique", help="launch-video sound scorecard against professional launch films")
    p.add_argument("file", help="soundtrack or film")
    p.add_argument("--video", help="the picture for the visual events (default: FILE when it is a video)")
    p.add_argument("--profile", help="a profile JSON made by `hark profile` (default: the bundled launch films)")
    p.add_argument("--fast", action="store_true", help="skip the AudioSet and realness passes (partial verdict)")
    add_common(p)
    p.set_defaults(func=_handler)
