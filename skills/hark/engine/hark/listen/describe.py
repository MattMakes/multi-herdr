"""describe: what a sound sounds like, in calibrated vocabulary tags, AudioSet labels and measurements.

The sound is embedded once and scored against every vocabulary term. Validated terms get verdicts.
Terms of the unvalidated categories are only hints. A file longer than 20 s is read as a whole and as
up to 6 windows, and up to 3 windows that read differently are named.
"""

from __future__ import annotations

import argparse
import math
from collections.abc import Sequence
from typing import Any

import numpy as np

from hark.core import audio, features, report
from hark.listen import vocabulary as voc

SR = 48000
DEFAULT_TOP = 8
LONG_S = 20.0
LIGHT_S = 60.0
NEAR_SILENT_DB = -70.0
MONO_DB = -40.0
NOISE_FLOOR_MIN_S = 1.0
DIGITAL_SILENCE_DB = -120.0
MUSIC_LINE = 0.5
AUDIOSET_KEEP = (5, 0.05)
AUDIOSET_SURE = 0.2
WINDOW_AUDIOSET_KEEP = (3, 0.1)
MAX_WINDOWS = 3
MAX_OTHER = 2
CHARACTER_Z = 2.5
MAX_CHARACTER = 2
SOUNDS_LIKE_Z = 2.0
HINT_Z = 3.0
TRANSIENT_MS = 15
SWELL_MS = 150
SHORT_S = 2.5
SHORT_ONSETS = 3
BUSY_RATE = 1.5
NOISE_FLATNESS = 0.3
OVERLAP_TOL = 1e-6
NOTE_NAMES = ("C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B")
SHOWN = ("duration", "peak_dbfs", "true_peak_dbtp", "rms_dbfs", "lufs", "lra_lu", "attack_ms", "decay_ms",
         "brightness_hz", "bandwidth_hz", "noise_floor_dbfs", "flatness", "harmonic_ratio", "f0_hz",
         "onset_count", "onsets_per_s", "width", "digital_silence_frac")
READING = ((3.0, "clear reading"), (2.5, "plausible reading"), (2.0, "weak reading"))


# ---------------------------------------------------------------- measurements and shape


def width_text(side_to_mid: float | None) -> str:
    """'mono' when the side is unknown or under -40 dB, else 'side/mid N dB'."""
    if side_to_mid is None or side_to_mid < MONO_DB:
        return "mono"
    return f"side/mid {int(round(side_to_mid))} dB"


def shown_measurements(m: dict, duration: float) -> dict:
    """The measurements describe shows, in a fixed order, without nulls."""
    vals = dict(m, duration=round(duration, 3), width=width_text(m.get("side_to_mid_db")))
    if duration < NOISE_FLOOR_MIN_S:
        vals.pop("noise_floor_dbfs", None)
    return {k: vals[k] for k in SHOWN if vals.get(k) is not None}


def note_name(f0: float) -> str:
    """The note name of a frequency, for example 440 -> 'A4'."""
    midi = int(round(69 + 12 * math.log2(f0 / 440.0)))
    return f"{NOTE_NAMES[midi % 12]}{midi // 12 - 1}"


def time_text(ms: float) -> str:
    """'N ms' below 2000 ms, else 'N.N s'."""
    return f"{int(round(ms))} ms" if ms < 2000 else f"{ms / 1000:.1f} s"


def freq_text(hz: float) -> str:
    """'N Hz' below 1000 Hz, else 'N.N kHz'."""
    return f"{int(round(hz))} Hz" if hz < 1000 else f"{hz / 1000:.1f} kHz"


def _long_shape(m: dict, d: float) -> str:
    n, rate = m.get("onset_count"), m.get("onsets_per_s")
    if n is None:
        first = "long recording"
    elif rate is not None and rate >= BUSY_RATE:
        first = f"busy, {rate:.1f} onsets/s"
    elif n <= d / 10:
        first = "steady, few onsets"
    else:
        first = f"{n} separate events"
    parts = [first]
    if m.get("lra_lu") is not None:
        parts.append(f"loudness range {int(round(m['lra_lu']))} LU")
    return ", ".join(parts)


def _event_shape(m: dict, d: float) -> str:
    n, rate, attack = m.get("onset_count"), m.get("onsets_per_s"), m.get("attack_ms")
    if d < SHORT_S and (n is None or n <= SHORT_ONSETS):
        if attack is not None and attack <= TRANSIENT_MS:
            return "short transient"
        if attack is not None and attack >= SWELL_MS:
            return "short swell"
        return "short one-shot"
    if rate is not None and rate >= BUSY_RATE:
        return f"busy, {rate:.1f} onsets/s"
    return f"{n} separate events" if n is not None and n >= 2 else "sustained"


def _texture(m: dict) -> str | None:
    if m.get("f0_hz"):
        return f"tonal, pitch ~{int(round(m['f0_hz']))} Hz ({note_name(m['f0_hz'])})"
    if m.get("flatness") is not None and m["flatness"] >= NOISE_FLATNESS:
        return "noise-like"
    return None


def _envelope(m: dict) -> str | None:
    parts = []
    if m.get("attack_ms") is not None:
        parts.append(f"attack {int(round(m['attack_ms']))} ms")
    if m.get("decay_ms") is not None:
        parts.append(f"decays in {time_text(m['decay_ms'])}")
    return f"({', '.join(parts)})" if parts else None


def shape_words(m: dict, d: float) -> str:
    """A few words on envelope and texture (D1)."""
    if d > LONG_S:
        return _long_shape(m, d)
    parts = [_event_shape(m, d), _texture(m)]
    text = ", ".join(p for p in parts if p)
    env = _envelope(m)
    return f"{text} {env}" if env else text


# ---------------------------------------------------------------- tags


def character_words(quality: Sequence[dict]) -> list[str]:
    """Up to 2 quality words of terms with z >= 2.5 ('a deep sound' -> 'deep')."""
    out = []
    for t in quality:
        if t["z"] >= CHARACTER_Z and len(out) < MAX_CHARACTER:
            word = t["term"].split(" ", 1)[1] if " " in t["term"] else t["term"]
            out.append(word[:-len(" sound")] if word.endswith(" sound") else word)
    return out


def tag_lists(v: voc.Vocab, z: np.ndarray, c: np.ndarray, top: int) -> dict:
    """top_tags, music_hints and the best 3 terms of every category for one calibrated row."""
    ranked = v.ranked_terms(z, c, len(v.terms))
    top_tags = [{"term": t, "category": cat, "z": zz, "cosine": cc, "verdict": voc.verdict(zz)}
                for t, cat, zz, cc in ranked if cat not in voc.UNVALIDATED + voc.NOT_SOUNDS_LIKE][:top]
    hints = [{"term": t, "category": cat, "z": zz, "cosine": cc}
             for t, cat, zz, cc in ranked if cat in voc.UNVALIDATED][:top]
    tags: dict[str, list] = {cat: [] for cat in v.categories}
    for t, cat, zz, cc in ranked:
        if len(tags[cat]) < 3:
            tags[cat].append({"term": t, "z": zz, "cosine": cc})
    return {"top_tags": top_tags, "music_hints": hints, "tags": tags}


def other_readings(top_tags: Sequence[dict]) -> list[dict]:
    """Up to 2 tags from position 4 on whose categories the first 3 tags do not have."""
    seen = {t["category"] for t in top_tags[:3]}
    out = []
    for t in top_tags[3:]:
        if t["category"] not in seen and len(out) < MAX_OTHER:
            out.append(t)
            seen.add(t["category"])
    return out


def audioset_list(model: Any, p: np.ndarray, keep: tuple[int, float] = AUDIOSET_KEEP) -> list[dict]:
    """Top labels with p >= the line; the first is always kept."""
    k, line = keep
    top = model.top(p, k=k)
    return [{"label": lab, "p": pp} for i, (lab, pp) in enumerate(top) if i == 0 or pp >= line]


# ---------------------------------------------------------------- embedding


def ast_model() -> Any:
    """The process AudioSet tagger."""
    from hark.models import ast

    return ast.get()


def _embed_short(path: str, s: float, e: float) -> dict:
    a = voc.embed_files([path], ranges=[(s, e)])[0]
    model = ast_model()
    return {"a": a, "probs": np.asarray(model.probs_file(path, start=s, end=e)), "model": model}


def _embed_long(path: str, x: np.ndarray, s: float, e: float) -> dict:
    sp = voc.spans(s, e)
    W = voc.span_embeddings([(path, sp)])[0]
    mono = audio.to_mono(np.asarray(x, np.float32))
    segs = [mono[int(round((a - s) * SR)):int(round((b - s) * SR))] for a, b in sp]
    model = ast_model()
    P = np.asarray(model.probs(segs))
    return {"a": voc.unit(W.mean(axis=0)), "probs": P.max(axis=0), "model": model, "spans": sp, "W": W, "P": P}


def distinct_windows(spans: Sequence[tuple[float, float]], W: np.ndarray, terms: Sequence[str]) -> list[int]:
    """Indexes of 2 or 3 spans that read differently, in time order, or [] (D2)."""
    if len(spans) <= 1:
        return []
    u = voc.unit(np.asarray(W, np.float64).mean(axis=0))
    dist = 1 - np.asarray(W, np.float64) @ u
    picked: list[int] = []
    for i in np.argsort(-dist, kind="stable"):
        free = all(spans[i][1] <= spans[j][0] + OVERLAP_TOL or spans[i][0] >= spans[j][1] - OVERLAP_TOL
                   for j in picked)
        if free and all(terms[i] != terms[j] for j in picked):
            picked.append(int(i))
        if len(picked) >= MAX_WINDOWS:
            break
    return sorted(picked, key=lambda i: spans[i][0]) if len(picked) >= 2 else []


def _span_tops(v: voc.Vocab, W: np.ndarray) -> list[dict]:
    cal = v.calibrate(W)
    return [tag_lists(v, cal.Z[i], cal.C[i], 1)["top_tags"][0] for i in range(len(W))]


def window_records(emb: dict, v: voc.Vocab) -> tuple[list[dict], list[dict]]:
    """The distinct windows and the per-span top tags of a long file."""
    tops = _span_tops(v, emb["W"])
    picks = distinct_windows(emb["spans"], emb["W"], [t["term"] for t in tops])
    recs = [{"start": round(emb["spans"][i][0], 3), "end": round(emb["spans"][i][1], 3), "top_tag": tops[i],
             "audioset": audioset_list(emb["model"], emb["P"][i], WINDOW_AUDIOSET_KEEP)} for i in picks]
    return recs, tops


# ---------------------------------------------------------------- summary


def _music_sentence(p_music: float, hints: Sequence[dict]) -> str:
    genre = [h for h in hints if h["category"] == "music_genre"][:2]
    mood = [h for h in hints if h["category"] == "music_mood"][:2]
    named = ", ".join(f"'{h['term']}' (z {voc.zf(h['z'])})" for h in genre + mood)
    text = f"It is music (AudioSet Music p {p_music:.2f})."
    if named:
        text += (f" Unvalidated hints: {named}; mood hints were wrong for about 1 in 5 tracks in a test.")
    return text


def _reading(z: float) -> str:
    return next(words for lo, words in READING if z >= lo)


def _tags_sentence(top_tags: Sequence[dict]) -> str:
    if top_tags and top_tags[0]["z"] >= SOUNDS_LIKE_Z:
        best = ", ".join(f"'{t['term']}' (z {voc.zf(t['z'])})" for t in top_tags[:3])
        text = f"Sounds most like {best} [{_reading(top_tags[0]['z'])}]."
        other = other_readings(top_tags)
        if other:
            text += " Other readings: " + ", ".join(
                f"'{t['term']}' ({t['category']}, z {voc.zf(t['z'])})" for t in other) + "."
        return text
    near = ", ".join(f"'{t['term']}' (z {voc.zf(t['z'])})" for t in top_tags[:2])
    return f"No sound-effect description stands out; closest, tentative: {near}."


def _audioset_sentence(labels: Sequence[dict]) -> str:
    if not labels:
        return ""
    text = ", ".join(f"{a['label']} {a['p']:.2f}" for a in labels[:3])
    return f"AudioSet: {text}." if labels[0]["p"] >= AUDIOSET_SURE else f"AudioSet tagger unsure ({text})."


def _measure_sentence(meas: dict, d: float) -> str:
    parts = [f"peak {meas['peak_dbfs']:.1f} dBFS"] if meas.get("peak_dbfs") is not None else []
    if meas.get("lufs") is not None:
        parts.append(f"{meas['lufs']:.1f} LUFS")
    for key, word in (("brightness_hz", "brightness"), ("bandwidth_hz", "bandwidth")):
        if meas.get(key) is not None:
            parts.append(f"{word} {freq_text(meas[key])}")
    nf = meas.get("noise_floor_dbfs")
    if nf is not None and d <= LONG_S:
        parts.append("digital silence between sounds" if nf < DIGITAL_SILENCE_DB else f"noise floor {nf:.0f} dBFS")
    parts.append(meas.get("width", "mono"))
    return "Measured: " + ", ".join(parts) + "."


def _windows_sentence(wins: Sequence[dict], tops: Sequence[dict]) -> str:
    if not wins:
        return f"Uniform: all {len(tops)} analysed 10 s windows read as '{tops[0]['term']}'." if tops else ""
    items = []
    for w in wins:
        lab = w["audioset"][0] if w["audioset"] else None
        tail = f", AudioSet {lab['label']} {lab['p']:.2f}" if lab else ""
        items.append(f"{report.fmt_t(w['start'])}-{report.fmt_t(w['end'])} '{w['top_tag']['term']}' "
                     f"(z {voc.zf(w['top_tag']['z'])}{tail})")
    return "Windows that read differently: " + "; ".join(items) + "."


def build_summary(r: dict, p_music: float, tops: Sequence[dict]) -> str:
    """The describe paragraph (D3)."""
    d = r["duration"]
    parts = [f"{d:.2f} s {r['shape']}."]
    if p_music >= MUSIC_LINE:
        parts.append(_music_sentence(p_music, r["music_hints"]))
    parts.append(_tags_sentence(r["top_tags"]))
    hints = r["music_hints"]
    if p_music < MUSIC_LINE and hints and hints[0]["z"] >= HINT_Z:
        parts.append(f"Unvalidated hint: '{hints[0]['term']}' (z {voc.zf(hints[0]['z'])}).")
    if r["character"]:
        parts.append("Character: " + ", ".join(r["character"]) + ".")
    parts += [_audioset_sentence(r["audioset"]), _measure_sentence(r["measurements"], d)]
    if d > LONG_S:
        parts.append(_windows_sentence(r.get("distinct_windows", []), tops))
    return " ".join(p for p in parts if p)


# ---------------------------------------------------------------- describe


def _untagged(base: dict, summary: str) -> dict:
    return {**base, "shape": summary, "top_tags": [], "music_hints": [], "tags": {}, "audioset": [],
            "character": [], "summary": summary}


def _too_quiet(base: dict, m: dict, x: np.ndarray, d: float) -> dict | None:
    if m.get("error"):
        base["measurements"] = {"duration": round(d, 3)}
        return _untagged(base, f"Only {d * 1000:.0f} ms of audio; at least 5 ms are needed to describe a sound.")
    peak = voc.array_peak_db(x)
    if peak < NEAR_SILENT_DB:
        base["measurements"] = shown_measurements(m, d)
        return _untagged(base, f"{d:.2f} s of near-silence (peak {peak:.1f} dBFS); model tags of silence "
                               "mean nothing.")
    return None


def describe(path: str, start: float | None = None, end: float | None = None, top: int = DEFAULT_TOP) -> dict:
    """What a sound sounds like: tags, hints, AudioSet labels, measurements and a summary."""
    dur = voc.check_range(path, start, end)
    s, e = voc.clamp_range(dur, start, end)
    x = audio.load(path, start=start, end=end)
    d = len(x) / SR
    m = features.measure(x, light=d > LIGHT_S) if len(x) else {"error": "too short to measure"}
    base: dict[str, Any] = {"path": path, "start": start, "end": end, "duration": round(d, 3)}
    quiet = _too_quiet(base, m, x, d)
    if quiet is not None:
        return quiet
    v = voc.get()
    emb = _embed_long(path, x, s, e) if d > LONG_S else _embed_short(path, s, e)
    cal = v.calibrate(emb["a"][None, :])
    r = {**base, "measurements": shown_measurements(m, d), "shape": shape_words(m, d),
         **tag_lists(v, cal.Z[0], cal.C[0], top), "audioset": audioset_list(emb["model"], emb["probs"])}
    r["character"] = character_words(r["tags"].get("quality", []))
    tops: list[dict] = []
    if d > LONG_S:
        r["distinct_windows"], tops = window_records(emb, v)
    p_music = float(emb["model"].prob_of(emb["probs"], "Music"))
    r["summary"] = build_summary(r, p_music, tops)
    return r


def describe_text(r: dict) -> str:
    """Text output of describe."""
    lines = [r["summary"]]
    if r["top_tags"]:
        lines += ["", "tags (z >= 3 clear, 2.5-3 plausible, 2-2.5 weak, < 2 tentative):"]
        lines += [f"{voc.zf(t['z']):>5}  {t['term']} [{t['category']}] {t['cosine']:.3f}" for t in r["top_tags"]]
        if r["music_hints"]:
            lines.append("unvalidated hints: " + ", ".join(h["term"] for h in r["music_hints"][:5]))
        best = [f"{cat}: {v[0]['term']}" for cat, v in r["tags"].items() if v]
        lines.append("best per category: " + "; ".join(best))
        lines.append("audioset: " + ", ".join(f"{a['label']} {a['p']:.2f}" for a in r["audioset"]))
    lines.append("measurements: " + ", ".join(f"{k} {v}" for k, v in r["measurements"].items()))
    return "\n".join(lines)


# ---------------------------------------------------------------- CLI


@report.expected_errors("describe")
def _run(args: argparse.Namespace) -> int | None:
    import hark.models

    hark.models.require_ml("describe")
    result = describe(args.path, start=args.start, end=args.end, top=args.top)
    report.emit(result, args, text=describe_text(result))
    return 0


def add_cli(sub: argparse._SubParsersAction) -> None:
    """Register ``describe``."""
    p = sub.add_parser("describe", help="What a sound sounds like: tags, AudioSet labels, measurements.")
    p.add_argument("path", help="audio or video file")
    p.add_argument("--start", type=float, default=None, help="describe only from this time (s)")
    p.add_argument("--end", type=float, default=None, help="describe only up to this time (s)")
    p.add_argument("--top", type=int, default=DEFAULT_TOP,
                   help="how many validated tags, and how many unvalidated hints, to list in total")
    report.add_common(p)
    p.set_defaults(func=_run)
