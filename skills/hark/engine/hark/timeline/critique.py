"""hark critique: a launch-video sound scorecard against a profile of professional launch films (critique part)."""

from __future__ import annotations

import argparse
import functools
import json
import os
import time
from dataclasses import dataclass, field
from typing import Any

import numpy

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


def fmt_t(tx) -> str:
    from hark.core.report import fmt_t as core_fmt

    return core_fmt(tx)


def _times(times_, n: int = 8) -> str:
    times_ = list(times_)
    sv = ", ".join(fmt_t(tx) for tx in times_[:n])
    return sv + (f" (+{len(times_) - n} more)" if len(times_) > n else "")


def _error(msg: str) -> Exception:
    from hark.listen.profile import ProfileError

    return ProfileError(msg)


# ---------------------------------------------------------------- listen metric table

def metric_info(mkey: str) -> dict:
    """{group, label, unit, decimals} of a metric from the listen table; p_real is ours."""
    if mkey == "p_real":
        return {"group": "realness", "label": "realness (p_real)", "unit": "p", "decimals": 2}
    from hark.listen.profile import METRICS

    # listen rows are (key, group, unit, decimals, label)
    entry = next((m for m in METRICS if m[0] == mkey), None)
    if entry is None:
        return {"group": "", "label": mkey, "unit": "", "decimals": 2}
    _, group, unit, decimals, label = entry
    return {"group": group, "label": label, "unit": unit, "decimals": decimals}


def fmt_value(mkey: str, val) -> str:
    from hark.listen.profile import fmt_value as listen_fmt

    return listen_fmt(mkey, val)


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


def fmt_p(val: float) -> str:
    sv = f"{val:.2f}"
    for th in {real_thresholds(True)["lo"], real_thresholds(True)["hi"], real_thresholds(False)["lo"],
               real_thresholds(False)["hi"]}:
        if sv == f"{th:.2f}" and abs(val - th) > 1e-12:
            return f"{val:.3f}"
    return sv


# ---------------------------------------------------------------- scorecard

def _fixed_window(mkey: str, practice, lossy: bool):
    """(lo, hi, basis) when the range does not come from the references, else None."""
    if mkey == "p_real":
        return real_thresholds(True)["hi"], None, "realness model"
    if mkey == "true_peak" and lossy:
        return None, 0.0, "decoded lossy"
    return (practice[0], practice[1], "web") if practice is not None else None


def _floor(mkey: str, ref_stats: dict, low: float):
    """Lower edge for a unit that cannot go below 0: half the reference minimum for rates, else none."""
    unit = metric_info(mkey)["unit"]
    if low > 0 or unit not in FLOOR_UNITS:
        return low
    return round(ref_stats["min"] / 2, 3) if unit == "/s" and ref_stats["min"] > 0 else None


def _ref_window(mkey: str, ref_stats: dict, slack: float, sides: str):
    if not ref_stats or not ref_stats.get("n"):
        return None, None, "no reference data"
    low, high = _floor(mkey, ref_stats, ref_stats["min"] - slack), ref_stats["max"] + slack
    return (low if sides in ("both", "lower") else None), (high if sides in ("both", "upper") else None), "references"


def window(mkey: str, ref_stats: dict, lossy: bool) -> tuple[float | None, float | None, str]:
    """(lo, hi, basis) of the ok range of one metric."""
    _, slack, sides, practice, _ = SPEC[mkey]
    fixed = _fixed_window(mkey, practice, lossy)
    return fixed if fixed is not None else _ref_window(mkey, ref_stats, slack, sides)


def _side(amount, low, high) -> str:
    if low is not None and amount < low:
        return "low"
    return "high" if high is not None and amount > high else "ok"


def status_of(amount, low, high, basis: str, weight: int) -> str:
    if basis == "no reference data" or amount is None:
        return "n.a."
    return "info" if weight == 0 else _side(amount, low, high)


def final_weight(mkey: str, amount, state: str, low, high) -> int:
    weight, slack = SPEC[mkey][0], SPEC[mkey][1]
    if mkey == "p_real":
        return (3 if amount < 0.5 else 1) if state == "low" else weight
    if weight == 3 and state in ("low", "high"):
        edge = low if state == "low" else high
        if abs(amount - edge) <= 2 * slack + 1e-9:
            return 2
    return weight


def _r(val, d: int = 3):
    return None if val is None else round(float(val), d)


def metric_row(mkey: str, amount, ref_stats: dict, chance: dict | None, lossy: bool) -> dict:
    low, high, basis = window(mkey, ref_stats, lossy)
    state = status_of(amount, low, high, basis, SPEC[mkey][0])
    w = final_weight(mkey, amount, state, low, high)
    info = metric_info(mkey)
    entry = {"metric": mkey, "label": info["label"], "group": info["group"], "value": amount, "unit": info["unit"],
           "range": [_r(low), _r(high)], "basis": basis, "median": (ref_stats or {}).get("median"),
           "typical": [ref_stats.get("p10"), ref_stats.get("p90")] if ref_stats and ref_stats.get("n") else None,
           "status": state, "weight": WEIGHT_WORD[w], "why": SPEC[mkey][4]}
    if chance and mkey in chance:
        entry["chance"] = chance[mkey]
    return entry


WEIGHT_NUM = {"critical": 3, "major": 2, "minor": 1}
VERDICTS = ((0.9, True, "launch-grade"), (0.75, True, "close"), (0.5, False, "needs work"))


def verdict_of(score, critical: int) -> str:
    if score is None:
        return "n.a."
    ok = (val for floor, clean, val in VERDICTS if score >= floor and not (clean and critical))
    return next(ok, "not launch-grade")


def _score(scored: list[dict]) -> float | None:
    total = sum(WEIGHT_NUM[rr["weight"]] for rr in scored)
    ok = sum(WEIGHT_NUM[rr["weight"]] for rr in scored if rr["status"] == "ok")
    return round(ok / total, 3) if total else None


def _issues(scored: list[dict], weight: str) -> int:
    return sum(1 for rr in scored if rr["weight"] == weight and rr["status"] != "ok")


def scorecard(values: dict, profile: dict, chance: dict | None = None, lossy: bool = False) -> dict:
    """Score metric values against a profile. No side effects."""
    ref_stats = profile.get("metrics", {})
    rows = [metric_row(kk, values.get(kk), ref_stats.get(kk, {}), chance, lossy) for kk in SPEC]
    scored = [rr for rr in rows if rr["weight"] != "info" and rr["status"] != "n.a."]
    score, crit = _score(scored), _issues(scored, "critical")
    return {"metrics": rows, "score": score, "verdict": verdict_of(score, crit),
            "in_range": sum(1 for rr in scored if rr["status"] == "ok"), "scored": len(scored),
            "critical_issues": crit, "major_issues": _issues(scored, "major")}


# ---------------------------------------------------------------- profile, match, leave-one-out

def _read_profile(fpath: str) -> dict:
    if not os.path.isfile(fpath):
        raise _error(f"profile not found: {fpath}")
    how = "make one with: hark profile <reference films> -o profile.json"
    try:
        with open(fpath, encoding="utf-8") as fh:
            data = json.load(fh)
    except (ValueError, UnicodeDecodeError) as exc:
        raise _error(f"{fpath} is not a profile JSON ({exc}); {how}") from None
    if not isinstance(data, dict) or not isinstance(data.get("metrics"), dict):
        raise _error(f"{fpath} is not a profile JSON: 'metrics' is missing; {how}")
    return data


def load_profile_arg(prof_) -> tuple[dict, str]:
    from hark.listen.profile import load_profile

    if isinstance(prof_, dict):
        return prof_, "custom"
    if prof_ is None:
        return load_profile(), "bundled launch films"
    fpath = os.fspath(prof_)
    return _read_profile(fpath), fpath


def _same_metrics(fm: dict, film: dict) -> bool:
    ref = film.get("metrics") or {}
    pairs = [(fm["metrics"].get(kk), ref.get(kk), slack) for kk, slack in MATCH_TOL.items()]
    pairs = [(lhs, b, slack) for lhs, b, slack in pairs if lhs is not None and b is not None]
    return len(pairs) >= 3 and all(abs(lhs - b) <= slack + 1e-9 for lhs, b, slack in pairs)


def _is_match(fm: dict, film: dict) -> bool:
    d = film.get("duration")
    return d is not None and abs(d - fm["duration"]) <= 0.05 and _same_metrics(fm, film)


def reference_match(fm: dict, prof_: dict) -> str | None:
    """The id of the profile film this file is, by duration and 4 metrics."""
    film = next((f for f in prof_.get("films") or [] if _is_match(fm, f)), None)
    return None if film is None else (film.get("id") or "a reference")


def leave_one_out(prof_: dict, match: str) -> dict:
    from hark.listen.profile import aggregate

    others = [dict(film, video=film.get("has_video", True), style=film.get("style") or "")
              for film in prof_.get("films") or [] if (film.get("id") or "a reference") != match]
    if len(others) < 2:
        return prof_
    loo = aggregate(others)
    loo.update(n=len(others), films=others, loo_of=match, n_full=prof_.get("n"))
    return loo


# ---------------------------------------------------------------- measurement context

@dataclass
class Film:
    """Everything the fix rules read: film metrics, our own audio pass, cut sync, realness."""

    path: str
    video: str | None
    x: numpy.ndarray
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


def _decode(fpath: str) -> tuple[numpy.ndarray, float, float]:
    from hark.core import audio
    from hark.core.features import integrated_lufs

    sig = audio.load(fpath)
    secs = len(sig) / 48000
    return sig, secs, (float(integrated_lufs(sig)) if secs >= 0.4 else -70.0)


def _audio_pass(film: Film) -> None:
    from hark.core.audio import to_mono
    from hark.core.features import loudness_curve, onsets

    m = numpy.asarray(to_mono(film.x), dtype=numpy.float64)
    film.onsets = list(onsets(m))
    film.fast = tuple(numpy.asarray(val, dtype=float) for val in loudness_curve(film.x, 0.1, 0.02))
    film.slow = tuple(numpy.asarray(val, dtype=float) for val in loudness_curve(film.x, 3.0, 0.1))


def cut_sync(sig: numpy.ndarray, vis_t: list[float], fps: float | None) -> dict | None:
    """Sync of the visual events by the timeline matcher, or None."""
    if not vis_t:
        return None
    try:
        from hark.timeline.timeline import detect_events, level_envs, sync

        return sync(vis_t, detect_events(sig), None, 1 / fps if fps else None, level_envs(sig))
    except Exception:  # noqa: BLE001 - the onset lists are the fallback (critique 2.5)
        return None


def _realness(fpath: str) -> dict:
    try:
        from hark.listen.realness import realness

        rr = realness(fpath)
        return {kk: rr.get(kk) for kk in ("p_real", "verdict", "synth_windows", "segments", "summary")}
    except Exception as exc:  # noqa: BLE001 - kept as an error row (critique 2.2 step 8)
        return {"error": f"{type(exc).__name__}: {exc}"}


# ---------------------------------------------------------------- fix text helpers

def _rng(film: Film, mkey: str) -> str:
    st = film.profile.get("metrics", {}).get(mkey) or {}
    if not st.get("n"):
        return ""
    return f"refs {fmt_value(mkey, st['min'])} .. {fmt_value(mkey, st['max'])}"


def _end(text: str, rng: str) -> str:
    return f"{text} ({rng})." if rng else f"{text}."


def _pmean(val: numpy.ndarray) -> float:
    return float(10 * numpy.log10(numpy.mean(10 ** (val / 10)))) if val.size else -numpy.inf


def body_extremes(film: Film) -> tuple | None:
    """(quiet start, quiet end, quiet level, loud t, loud level) on the 3 s curve, or None when the body is empty."""
    times_, Ls = film.slow
    body = numpy.flatnonzero((times_ > 0.1 * film.dur) & (times_ < 0.9 * film.dur))
    if not body.size:
        return None
    q = body[int(numpy.argmin(Ls[body]))]
    kk = int(numpy.argmax(Ls))
    return max(0.0, times_[q] - 1.5), times_[q] + 1.5, round(float(Ls[q]), 1), float(times_[kk]), round(float(Ls[kk]), 1)


def _hole_ok(tf: numpy.ndarray, sv: int, e: int, keep: tuple) -> bool:
    """A closed run of 0.5 s or more between the first sound and the end silence. keep is (hop, first, last, n)."""
    hop, first, last, n = keep
    return e < n and (e - sv) * hop >= 0.5 and tf[sv] > first and tf[e] < last


def holes(film: Film) -> list[tuple[float, float]]:
    tf, Lf = film.fast
    first = film.values.get("first_sound_s") or 0.0
    last = film.dur - (film.values.get("end_silence_s") or 0.0)
    keep = (float(tf[1] - tf[0]) if tf.size > 1 else 0.02, first, last, Lf.size)
    return [(round(tf[sv] - 0.05, 2), round(tf[e - 1] + 0.05, 2)) for sv, e in runs(Lf < film.lufs - 30, 1)
            if _hole_ok(tf, sv, e, keep)]


def without(times, targets, rr: float = 0.1) -> list[float]:
    tg = numpy.asarray(sorted(targets), dtype=float)
    return [tx for tx in times if not tg.size or numpy.min(numpy.abs(tg - tx)) > rr + 1e-9]


def music_holes(film: Film) -> list[tuple[float, float]]:
    wins = film.fm.get("events", {}).get("music_windows") or []
    spans: list[list[float]] = []
    for tx, p_music, *_ in wins:
        if p_music < 0.3:
            lhs, b = tx, min(film.dur, tx + 5)
            if spans and lhs <= spans[-1][1] + 1e-6:
                spans[-1][1] = b
            else:
                spans.append([lhs, b])
    return [(lhs, b) for lhs, b in spans[:6]]


# ---------------------------------------------------------------- actions per metric

def act_lufs(film: Film, mkey: str, val: float, state: str) -> str:
    if state == "low":
        return _end(f"Raise the master by {-14 - val:+.1f} dB to about -14 LUFS with a true-peak limiter at a "
                    f"-1 dBTP ceiling; if the limiter would work harder than about 3 dB, compress the music bus "
                    f"first", _rng(film, mkey))
    return _end(f"Turn the master down by {val + 12:.1f} dB to about -12 LUFS: above -9 LUFS the normalising "
                f"platforms turn it down anyway, and the limiter flattens every hit", _rng(film, mkey))


def act_true_peak(film: Film, mkey: str, val: float, state: str) -> str:
    lossy = ", so the decoded lossy audio already clips on playback" if film.fm.get("lossy") else ""
    return (f"True peak is {val:+.1f} dBTP{lossy}; end the chain with a true-peak limiter at -1.0 dBTP and "
            f"re-encode.")


def act_music(film: Film, mkey: str, val: float, state: str) -> str:
    gaps = music_holes(film)
    where = "; no music at " + ", ".join(f"{fmt_t(lhs)}-{fmt_t(b)}" for lhs, b in gaps) if gaps else ""
    return _end(f"Music plays in only {100 * val:.0f}% of the film{where}. Run one continuous track under the whole "
                f"film and duck it under the voice instead of stopping it", _rng(film, mkey))


def act_cuts(film: Film, mkey: str, val: float, state: str) -> str:
    vis = film.visual
    missing = without(vis, film.onsets)
    head = _end(f"{len(missing)} of {len(vis)} visual events have no onset within 100 ms", _rng(film, mkey))
    advice = "put a whoosh into the transition that peaks on the cut, or a hit or a UI click there"
    nosound = film.nosound()
    if nosound is None:
        return f"{head} Missing at {_times(missing)}: {advice}."
    text = f"{head} No sound at all at {_times(nosound)}: {advice}." if nosound else \
        f"{head} Every one of them has some sound under it."
    extra = len(missing) - len(nosound)
    if extra > 0:
        text += (f" {extra} have sound playing through them (a swell, music or a level peak) but no new attack; "
                 f"add an accent on the ones that matter.")
    return text


def _span(lhs: float, b: float) -> str:
    return f"{fmt_t(lhs)}-{fmt_t(b)}"


def _shape_high(film: Film, mkey: str, val: float, ext: tuple) -> str:
    qa, qb, ql, lt, ll = ext
    loud = _span(max(0, lt - 1.5), lt + 1.5)
    if mkey == "lra":
        return _end(f"The level swings {val:.1f} LU: the body is quietest at {_span(qa, qb)} ({ql:.1f} LUFS) and "
                    f"the loudest 3 s is {loud} ({ll:.1f} LUFS); lift the quiet parts or compress the music bus "
                    f"2-4 dB", _rng(film, mkey))
    return _end(f"The loudest 3 s at {loud} ({ll:.1f} LUFS) stands {val:.1f} LU above integrated, so the rest plays "
                f"quiet next to it; lift the body (quietest at {_span(qa, qb)}) or tame that peak", _rng(film, mkey))


def _shape_flat(film: Film, mkey: str, val: float) -> str:
    if mkey == "lra":
        return _end(f"LRA is {val:.1f} LU, so the level never moves; give the film sections (a calmer voice or UI "
                    f"passage, a lift into the reveal) or ease the limiter", _rng(film, mkey))
    return _end(f"The loudest 3 s is only {val:.1f} LU above integrated, so no moment stands out; let the main "
                f"reveal or the logo hit rise 2-3 LU above the body, or ease the limiter", _rng(film, mkey))


def act_shape(film: Film, mkey: str, val: float, state: str) -> str:
    ext = body_extremes(film) if state == "high" else None
    return _shape_high(film, mkey, val, ext) if ext is not None else _shape_flat(film, mkey, val)


def act_first(film: Film, mkey: str, val: float, state: str) -> str:
    tf, Lf = film.fast
    before = _pmean(Lf[tf < val])
    if before > -60:
        return _end(f"Nothing reaches body level until {fmt_t(val)}: before it the level is {before:.0f} LUFS, "
                    f"{film.lufs - before:.0f} LU under the body; raise the opening or start the music at 0:00",
                    _rng(film, mkey))
    return _end(f"Silence until {fmt_t(val)}; start music, ambience or a riser at 0:00", _rng(film, mkey))


def act_end_silence(film: Film, mkey: str, val: float, state: str) -> str:
    return _end(f"{val:.2f} s of dead air after the sound ends at {fmt_t(film.dur - val)}; trim the picture or let the "
                f"final sting ring out over the end card", _rng(film, mkey))


def act_gap(film: Film, mkey: str, val: float, state: str) -> str:
    hs = holes(film)
    where = " (" + ", ".join(_span(lhs, b) for lhs, b in hs[:6]) + ")" if hs else ""
    return _end(f"The longest hole is {val:.2f} s{where}; keep a music bed or room tone under it, or make each hole "
                f"a deliberate stop-down that ends on a hit", _rng(film, mkey))


def act_final(film: Film, mkey: str, val: float, state: str) -> str:
    if state == "high":
        return _end(f"The final hit is {val:.1f} LU above the body; pull it down 2-3 dB", _rng(film, mkey))
    at = f" at the last visual event ({fmt_t(film.visual[-1])})" if film.visual else ""
    return _end(f"The final moment sits {-val:.1f} LU under the body; end on a logo sting or a hit{at} instead of "
                f"fading away", _rng(film, mkey))


def act_onsets(film: Film, mkey: str, val: float, state: str) -> str:
    if state == "low":
        return _end(f"Sparse: {val:.2f} onsets per second; use music with more drive, or add UI foley (clicks, "
                    f"typing, ticks) on the on-screen actions", _rng(film, mkey))
    return _end(f"Busy: {val:.2f} onsets per second; thin out the SFX layer and keep the accents for the important "
                f"moments", _rng(film, mkey))


def biggest_without_hit(film: Film) -> list[float]:
    ev = film.fm.get("events", {})
    pairs = sorted(zip(ev.get("visual") or [], ev.get("visual_strength") or []), key=lambda pv: -pv[1])
    hits = ev.get("hits") or []
    return sorted([tx for tx, _ in pairs if without([tx], hits)][:6])


def act_hits(film: Film, mkey: str, val: float, state: str) -> str:
    if state == "high":
        return _end(f"Too many salient hits ({val:.2f} per second); keep the biggest for the reveals and soften the "
                    f"rest by 3-6 dB", _rng(film, mkey))
    word = "No" if val == 0 else "Few"
    text = _end(f"{word} salient hits ({val:.2f} per second); add impacts or accents on the key moments", _rng(film, mkey))
    big = biggest_without_hit(film)
    return text + (f" The biggest visual events without a hit: {_times(big)}." if big else "")


def act_edges(film: Film, mkey: str, val: float, state: str) -> str:
    span = _span(0, 0.1 * film.dur) if mkey == "intro_lu" else _span(0.9 * film.dur, film.dur)
    part = "opening" if mkey == "intro_lu" else "ending"
    if state == "high":
        return _end(f"The {part} ({span}) is {val:.1f} LU louder than the body; bring it down or lift the body",
                    _rng(film, mkey))
    fix = "start closer to full level or shorten the build-up" if mkey == "intro_lu" else \
        "keep the energy up into the end card and finish on a sting"
    return _end(f"The {part} ({span}) sits {-val:.1f} LU under the body; {fix}", _rng(film, mkey))


def act_fade(film: Film, mkey: str, val: float, state: str) -> str:
    if state == "high":
        return _end(f"The fade-out takes {val:.2f} s; end on a sting with a short ring-out, or cut hard on the last "
                    f"picture cut", _rng(film, mkey))
    return _end(f"The end is cut hard ({val:.2f} s); let the last hit ring out briefly", _rng(film, mkey))


def act_band(film: Film, mkey: str, val: float, state: str) -> str:
    advice = BAND_ADVICE[mkey][0 if state == "low" else 1]
    rng = _rng(film, mkey)
    return f"{advice} (now {fmt_value(mkey, val)}" + (f"; {rng})." if rng else ").")


def act_width(film: Film, mkey: str, val: float, state: str) -> str:
    if state == "high":
        return _end(f"The mix is very wide ({fmt_value(mkey, val)}); check the mono fold-down and narrow the wide "
                    f"ambience or reverbs", _rng(film, mkey))
    head = "The mix is mono: left and right are identical" if film.fm.get("mono") else \
        f"The mix is nearly mono ({fmt_value(mkey, val)})"
    return _end(f"{head}; use stereo music and ambience, and pan the SFX with the motion", _rng(film, mkey))


ACTIONS = {"lufs_i": act_lufs, "true_peak": act_true_peak, "music_frac": act_music, "cuts_with_sound": act_cuts,
           "lra": act_shape, "crest_lu": act_shape, "first_sound_s": act_first, "end_silence_s": act_end_silence,
           "max_gap_s": act_gap, "final_lu": act_final, "onset_rate": act_onsets, "hit_rate": act_hits,
           "intro_lu": act_edges, "end_lu": act_edges, "fade_s": act_fade, "side_to_mid_db": act_width,
           **{k: act_band for k in BAND_ADVICE}}
NO_ACTION = {("true_peak", "low"), ("music_frac", "high"), ("cuts_with_sound", "high"), ("first_sound_s", "low"),
             ("end_silence_s", "low"), ("max_gap_s", "low")}


# ---------------------------------------------------------------- realness action

def _segments(film: Film) -> list[dict]:
    return [sv for sv in (film.realness or {}).get("segments") or [] if sv.get("p_real") is not None]


def quiet_miss(film: Film, pv: float) -> bool:
    return pv >= 0.5 and not any(sv["p_real"] < 0.5 for sv in _segments(film))


def _pick_windows(segs: list[dict], keep: int) -> list[dict]:
    kept: list[dict] = []
    for sv in segs:
        if all(abs(sv["start"] - kk["start"]) >= 2.0 for kk in kept):
            kept.append(sv)
        if len(kept) >= keep:
            break
    return sorted(kept, key=lambda sv: sv["start"])


def _realness_where(film: Film, tw: dict) -> str:
    segs = sorted(_segments(film), key=lambda sv: sv["p_real"])
    if not segs:
        return ""
    flagged = [sv for sv in segs if sv["p_real"] <= tw["lo"]]
    picked = _pick_windows(flagged or segs, 6 if flagged else 3)
    listed = ", ".join(f"{_span(sv['start'], sv['end'])} ({sv['p_real']:.2f})" for sv in picked)
    if flagged:
        more = len(flagged) - len(picked)
        return (f" 4 s windows at or below the window threshold {tw['lo']:.2f}: {listed}"
                + (f" (+{more} more)" if more > 0 else "") + ".")
    return f" Lowest-scoring 4 s windows: {listed}."


def act_realness(film: Film, mkey: str, pv: float, state: str) -> str | None:
    if quiet_miss(film, pv):
        return None
    tx = real_thresholds(True) if _segments(film) else real_thresholds(False)
    tw = real_thresholds(False)
    if pv <= tx["lo"]:
        head = f"Realness {fmt_p(pv)} is at or below {tx['lo']:.2f}: likely code-synthesized."
    elif pv < 0.5:
        head = (f"Realness {fmt_p(pv)} leans code-synthesized (the realness model calls {tx['lo']:.2f}..{tx['hi']:.2f} "
                f"uncertain).")
    else:
        head = f"Realness {fmt_p(pv)} is uncertain; real or pro-produced sound scores at least {tx['hi']:.2f}."
    fix = (" If the film should sound recorded or produced, replace the oscillator or noise layers with real "
           "recordings and produced music (the sound-sourcing skill covers sourcing and licences)." if pv < 0.5 else
           " If the film should sound recorded or produced, find and replace the layer that sounds synthetic.")
    return head + _realness_where(film, tw) + fix + (" Judge each stem with `hark realness stem.wav`: a finished mix "
                                                  "is harder to judge than its sources.")


ACTIONS["p_real"] = act_realness


def _action_text(film: Film, entry: dict) -> str | None:
    mkey, state = entry["metric"], entry["status"]
    usable = state in ("low", "high") and (mkey, state) not in NO_ACTION and mkey in ACTIONS
    return ACTIONS[mkey](film, mkey, entry["value"], state) if usable else None


def _action(film: Film, entry: dict) -> dict | None:
    """The action of one out-of-range row (also written into the row), or None."""
    mkey, state = entry["metric"], entry["status"]
    text = _action_text(film, entry)
    if not text:
        return None
    edge = entry["range"][0] if state == "low" else entry["range"][1]
    dist = abs(entry["value"] - edge) / max(SPEC[mkey][1], 0.001)
    entry["actions"] = [ascii_safe(text)]
    return {"metric": mkey, "weight": entry["weight"], "w": WEIGHT_NUM[entry["weight"]], "dist": round(dist, 3),
            "action": ascii_safe(text)}


def build_actions(film: Film, rows: list[dict]) -> list[dict]:
    out = [lhs for lhs in (_action(film, entry) for entry in rows) if lhs is not None]
    return sorted(out, key=lambda lhs: (-lhs["w"], -lhs["dist"]))


# ---------------------------------------------------------------- tips and not measured

def _cut_tip(film: Film, acted: set) -> str | None:
    nos, peaks = film.nosound(), film.peak_only()
    if not nos or film.values.get("cuts_with_sound") is None or "cuts_with_sound" in acted:
        return None
    sv = (f"{len(nos)} of {len(film.visual)} visual events get no sound at all (no onset within 100 ms, no whoosh "
         f"or riser peak within 300 ms, no level peak within 150 ms): {_times(nos)}. Give the ones that matter "
         f"a whoosh into the cut, a hit or a UI click, or keep one as a deliberate stop-down.")
    more = f" {len(peaks)} more are met only by a level peak; check them on the sheet: {_times(peaks, 4)}."
    return sv + (more if peaks else "")


def _realness_tip(film: Film, acted: set) -> str | None:
    pv, line = film.values.get("p_real"), real_thresholds(True)["hi"]
    if pv is None or "p_real" in acted or pv >= line or not quiet_miss(film, pv):
        return None
    segs = _segments(film)
    low = f" (lowest window {min(sv['p_real'] for sv in segs):.2f})" if segs else ""
    return (f"Realness {fmt_p(pv)} is under the {line:.2f} line for real sound, but nothing synthetic was "
            f"found{low}; there is no layer to replace on this evidence. Check the stems with `hark realness`.")


def _stopdown_tip(film: Film) -> str | None:
    if film.values.get("stopdowns_min") != 0 or not film.visual:
        return None
    ev = film.fm.get("events", {})
    pairs = list(zip(ev.get("visual") or [], ev.get("visual_strength") or []))
    big = max(pairs, key=lambda q: q[1])[0] if pairs else film.visual[0]
    return (f"No stop-downs: 4 of 7 references drop the sound out for 0.2-1 s right before a big hit; try one "
            f"before the biggest visual change at {fmt_t(big)}.")


def _voice_tip(film: Film) -> str | None:
    sp = film.values.get("speech_frac")
    if sp is None or sp < 0.5:
        return None
    return (f"Voice in {100 * sp:.0f}% of the film: duck the music 3-6 dB under the voice and keep the SFX "
            f"out of 1-4 kHz while it speaks.")


def tips(film: Film, acted: set) -> list[str]:
    found = (_cut_tip(film, acted), _realness_tip(film, acted), _stopdown_tip(film), _voice_tip(film))
    return [ascii_safe(tx) for tx in found if tx]


NOT_MEASURED = {"p_real": "realness unavailable", "music_frac": "no AudioSet windows"}


def not_measured_reason(film: Film, mkey: str) -> str:
    if mkey in NOT_MEASURED and film.fast_mode:
        return "skipped by --fast"
    reason = NOT_MEASURED.get(mkey)
    if mkey == "cuts_with_sound":
        reason = "fewer than 3 visual events" if film.video else "no picture (pass --video film.mp4)"
    if reason is None and mkey in SHAPE_KEYS and film.dur < 10:
        reason = "clip under 10 s; the references are 20-93 s films"
    return reason or "not measurable on this file"


def not_measured(film: Film, rows: list[dict]) -> dict:
    out: dict[str, list[str]] = {}
    for entry in rows:
        if entry["status"] == "n.a." and entry["weight"] != "info":
            out.setdefault(not_measured_reason(film, entry["metric"]), []).append(entry["metric"])
    return out


# ---------------------------------------------------------------- text

def range_text(entry: dict) -> str:
    if entry["weight"] == "info":
        tx = entry.get("typical")
        return f"refs p10-p90 {fmt_value(entry['metric'], tx[0])} .. {fmt_value(entry['metric'], tx[1])}" if tx else ""
    low, high = entry["range"]
    mkey = entry["metric"]
    fv = (lambda val: fmt_p(val)) if mkey == "p_real" else (lambda val: fmt_value(mkey, val))
    if low is None and high is None:
        return entry["basis"]
    body = f">= {fv(low)}" if high is None else f"<= {fv(high)}" if low is None else f"{fv(low)} .. {fv(high)}"
    return body + (f" ({entry['basis']})" if entry["basis"] != "references" else "")


def verdict_line(outcome: dict) -> str:
    sv = outcome["verdict"]
    if outcome["partial"]:
        sv += f" (partial: {' and '.join(outcome['partial'])} not checked)"
    if outcome["score"] is not None:
        sv += (f" - {outcome['in_range']} of {outcome['scored']} scored metrics in range (weighted {100 * outcome['score']:.0f}%),"
              f" {outcome['critical_issues']} critical, {outcome['major_issues']} major")
    return sv


def _vs(outcome: dict) -> str:
    return f"vs {outcome['profile_n']} reference films ({outcome['profile']})"


def _loo_note(outcome: dict) -> str | None:
    m = outcome.get("reference_match")
    if not m:
        return None
    return (f"This file is reference {m['id']} of the profile, so it is scored against the other "
            f"{m['scored_against']} references; its own values do not set the ranges.")


def _nm_text(nm: dict) -> str:
    return "; ".join(f"{reason}: {', '.join(keys)}" for reason, keys in nm.items())


def summary_text(outcome: dict) -> str:
    parts = [f"{os.path.basename(outcome['path'])} {_vs(outcome)}.", _loo_note(outcome), f"Verdict: {verdict_line(outcome)}."]
    if outcome["actions"]:
        first = outcome["actions"][0]["action"]
        parts.append("Fix first: " + first + ("" if first.endswith(".") else "."))
    else:
        parts.append("Nothing needs fixing against the references.")
    if outcome["not_measured"]:
        parts.append(f"Not measured: {_nm_text(outcome['not_measured'])}.")
    return ascii_safe(" ".join(pv for pv in parts if pv))


def _scorecard_lines(rows: list[dict]) -> list[str]:
    lines = ["SCORECARD", f"{'':8} {'metric':30} {'value':>12}  {'ok range':30} {'ref median':>12}  status"]
    for entry in rows:
        if entry["weight"] == "info":
            continue
        mkey = entry["metric"]
        val = "n.a." if entry["value"] is None else fmt_p(entry["value"]) if mkey == "p_real" else \
            fmt_value(mkey, entry["value"])
        med = "" if entry["median"] is None else fmt_value(mkey, entry["median"]) if mkey != "p_real" else fmt_p(entry["median"])
        st = entry["status"] + (f" ({entry['weight']})" if entry["status"] in ("low", "high") else "")
        lines.append(f"{entry['group'][:8]:8} {entry['label'][:30]:30} {val:>12}  {range_text(entry)[:30]:30} {med:>12}  {st}")
    return lines


def _arc_lines(arc: dict | None) -> list[str]:
    if not arc or not arc.get("film") or not arc.get("refs_median"):
        return []

    def row(vals):
        return " ".join("     -" if val is None else f"{val:+6.1f}" for val in vals)

    return ["", "LOUDNESS ARC (LU vs integrated at 0%, 10% .. 100%; not scored)",
            f"  this film  {row(arc['film'])}", f"  ref median {row(arc['refs_median'])}"]


def _info_lines(rows: list[dict]) -> list[str]:
    out = ["", "NOT SCORED (style choices)"]
    for entry in rows:
        if entry["weight"] == "info" and entry["value"] is not None:
            ch = f" (chance {fmt_value(entry['metric'], entry['chance'])})" if entry.get("chance") is not None else ""
            out.append(f"  {entry['label']}: {fmt_value(entry['metric'], entry['value'])}{ch}  {range_text(entry)}")
    return out


def _partial_note(partial: list[str]) -> str | None:
    if len(partial) == 2:
        return ("  Run without --fast before shipping: realness and music presence are critical checks that this "
                "run skipped.")
    if partial:
        return f"  {partial[0]} could not be measured."
    return None


def _fix_lines(outcome: dict) -> list[str]:
    lines = []
    if outcome["actions"]:
        lines.append("FIX FIRST (most important first):")
        lines += [f"  {kk}. [{lhs['weight']}] {lhs['action']}" for kk, lhs in enumerate(outcome["actions"], 1)]
    lines += [f"  tip: {tx}" for tx in outcome["tips"]]
    if outcome["not_measured"]:
        lines.append(f"NOT MEASURED: {_nm_text(outcome['not_measured'])}")
    return lines


def _realness_lines(real: dict | None) -> list[str]:
    real = real or {}
    if real.get("summary"):
        return ["", f"realness: {real['summary']}"]
    return ["", f"realness: not available ({real['error'][:120]})"] if real.get("error") else []


def critique_text(outcome: dict) -> str:
    lines = [f"hark critique: {os.path.basename(outcome['path'])} ({outcome['duration']:.1f} s) {_vs(outcome)}", _loo_note(outcome),
             f"VERDICT: {verdict_line(outcome)}", _partial_note(outcome["partial"]), *_fix_lines(outcome), "",
             *_scorecard_lines(outcome["metrics"]), *_arc_lines(outcome.get("arc")), *_info_lines(outcome["metrics"]),
             *_realness_lines(outcome.get("realness"))]
    return ascii_safe("\n".join(line for line in lines if line is not None))


# ---------------------------------------------------------------- the run

def _check_video(vid_path: str) -> None:
    from hark.core.audio import probe
    from hark.timeline.timeline import is_picture

    if not os.path.exists(vid_path):
        raise _error(f"file not found: {vid_path}")
    if not is_picture(vid_path, probe(vid_path)):
        raise _error(f"{vid_path} has no picture (no video stream)")


def _check_media(fpath: str, vid_path: str | None) -> str | None:
    from hark.core.audio import probe
    from hark.listen.profile import media_problem
    from hark.timeline.timeline import is_picture

    problem = media_problem(fpath)
    if problem:
        raise _error(f"file not found: {fpath}" if problem == "not found" else problem)
    if vid_path and os.path.abspath(vid_path) == os.path.abspath(fpath) and not is_picture(fpath, probe(fpath)):
        vid_path = None
    if vid_path:
        _check_video(vid_path)
    return vid_path or (fpath if is_picture(fpath, probe(fpath)) else None)


def unscored(fpath: str, vid_path, secs: float, reason: str) -> dict:
    text = ascii_safe(f"{os.path.basename(fpath)}: {reason}, so there is nothing to score against launch films. "
                      f"Use the single-sound commands (hark describe, hark realness) for a short or silent clip.")
    return {"path": fpath, "video": vid_path, "duration": round(secs, 3), "verdict": "n.a.", "score": None, "in_range": 0,
            "scored": 0, "critical_issues": 0, "major_issues": 0, "partial": [], "actions": [], "tips": [],
            "metrics": [], "summary": text, "text": text}


def _unscorable_reason(secs: float, lufs: float) -> str | None:
    if secs < 3.0:
        return f"it is shorter than 3 s ({secs:.2f} s)"
    if lufs <= -69.5:
        return "it is silent (nothing above -70 LUFS)"
    return None


def _measure(film: Film, use_ast: bool, use_realness: bool) -> None:
    from hark.listen.profile import film_metrics

    film.fm = film_metrics(film.path, video=film.video, use_ast=use_ast)
    _audio_pass(film)
    match = reference_match(film.fm, film.profile)
    if match:
        film.profile = leave_one_out(film.profile, match)
    film.sync = cut_sync(film.x, film.visual, film.fm.get("fps"))
    film.values = dict(film.fm["metrics"], p_real=None)
    if use_realness:
        film.realness = _realness(film.path)
        film.values["p_real"] = film.realness.get("p_real")


def _events_block(film: Film) -> dict:
    ev = film.fm.get("events", {})
    return {"visual": film.visual, "visual_without_onset": without(film.visual, film.onsets),
            "visual_without_any_sound": film.nosound(), "visual_without_hit": without(film.visual, ev.get("hits") or []),
            "hits": ev.get("hits") or [], "stopdowns": ev.get("stopdowns") or []}


CARD_KEYS = ("verdict", "score", "in_range", "scored", "critical_issues", "major_issues")


def _arc_block(film: Film) -> dict:
    arc = film.profile.get("arc") or {}
    return {"film": film.fm.get("arc"), "refs_median": arc.get("median"), "refs_p10": arc.get("p10"),
            "refs_p90": arc.get("p90")}


def _match_block(film: Film) -> dict | None:
    if not film.profile.get("loo_of"):
        return None
    return {"id": film.profile["loo_of"], "scored_against": film.profile.get("n")}


def _partial(film: Film) -> list[str]:
    return [n for n, kk in (("realness", "p_real"), ("music presence", "music_frac")) if film.values.get(kk) is None]


def _result(film: Film, card: dict, label: str, t_start: float) -> dict:
    from hark.core.report import clean

    actions = build_actions(film, card["metrics"])
    outcome = {"path": film.path, "video": film.video, "duration": round(film.dur, 3), "profile_n": film.profile.get("n"),
           "lossy": bool(film.fm.get("lossy")), "profile": label, **{kk: card[kk] for kk in CARD_KEYS},
           "partial": _partial(film), "actions": [{kk: lhs[kk] for kk in ("weight", "metric", "action")} for lhs in actions],
           "tips": tips(film, {lhs["metric"] for lhs in actions}), "metrics": card["metrics"],
           "reference_match": _match_block(film), "events": _events_block(film), "arc": _arc_block(film),
           "realness": film.realness, "not_measured": not_measured(film, card["metrics"]),
           "seconds": round(time.perf_counter() - t_start, 1)}
    outcome = clean(outcome)
    outcome["summary"] = summary_text(outcome)
    outcome["text"] = critique_text(outcome)
    return outcome


def critique(path, profile=None, video=None, use_ast=True, use_realness=True) -> dict:
    """Score a soundtrack or film against a launch-film profile and say what to fix."""
    t_start = time.perf_counter()
    path = os.fspath(path)
    video = _check_media(path, os.fspath(video) if video else None)
    prof, label = load_profile_arg(profile)
    sig, secs, lufs = _decode(path)
    reason = _unscorable_reason(secs, lufs)
    if reason:
        return unscored(path, video, secs, reason)
    film = Film(path, video, sig, secs, lufs, profile=prof, fast_mode=not (use_ast or use_realness))
    _measure(film, use_ast, use_realness)
    card = scorecard(film.values, film.profile, film.fm.get("chance"), bool(film.fm.get("lossy")))
    return _result(film, card, label, t_start)


# ---------------------------------------------------------------- CLI

def _handler(args: argparse.Namespace) -> int:
    from hark.core.audio import AudioError
    from hark.core.report import emit, error_line
    from hark.listen.profile import ProfileError

    try:
        if not args.fast:
            from hark.models import require_ml

            require_ml("critique")
        outcome = critique(args.file, profile=args.profile, video=args.video, use_ast=not args.fast,
                       use_realness=not args.fast)
    except (ProfileError, AudioError) as exc:
        error_line("critique", str(exc))
        return 1
    emit(outcome, args)
    return 0


def add_cli(sub: Any) -> None:
    """Register `hark critique`."""
    from hark.core.report import add_common

    pv = sub.add_parser("critique", help="launch-video sound scorecard against professional launch films")
    pv.add_argument("file", help="soundtrack or film")
    pv.add_argument("--video", help="the picture for the visual events (default: FILE when it is a video)")
    pv.add_argument("--profile", help="a profile JSON made by `hark profile` (default: the bundled launch films)")
    pv.add_argument("--fast", action="store_true", help="skip the AudioSet and realness passes (partial verdict)")
    add_common(pv)
    pv.set_defaults(func=_handler)
