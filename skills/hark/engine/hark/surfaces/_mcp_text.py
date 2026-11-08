"""Text that hark's MCP tools send back: JSON made safe and small, and a meter summary.

A tool reply is the summary, the readable body, a note on ignored arguments and
then JSON. Without ``full`` the JSON is shrunk step by step until it fits 7000
characters: numbers rounded, long strings cut, lists capped, then the largest
fields dropped.
"""

from __future__ import annotations

import json
import math
from typing import Any

COMPACT_LIMIT = 7000
BODY_LIMIT = 24000
LIST_CAPS = (24, 12, 6, 3, 1)
STRING_CAP = 400
SILENT_DBFS = -70.0


def clean(obj: Any) -> Any:
    """A copy that json can write: text keys, lists for tuples and sets, NaN and inf as null."""
    if isinstance(obj, dict):
        return {str(key): clean(item) for key, item in obj.items()}
    if isinstance(obj, (list, tuple, set)):
        return [clean(item) for item in obj]
    return _clean_leaf(obj)


def _clean_leaf(obj: Any) -> Any:
    """Arrays become lists, non-finite floats become None, other objects their text."""
    if hasattr(obj, "tolist") and not isinstance(obj, (str, bytes)):
        return clean(obj.tolist())
    if isinstance(obj, float) and not math.isfinite(obj):
        return None
    return obj if obj is None or isinstance(obj, (bool, int, float, str)) else str(obj)


def _squeeze_text(value: str, cap: int, outer: bool) -> str:
    return value[:STRING_CAP] + " ..." if len(value) > STRING_CAP else value


def _squeeze_list(value: list, cap: int, outer: bool) -> list:
    kept = [_squeeze(item, cap) for item in value[:cap]]
    if len(value) > cap:
        kept.append(f"... +{len(value) - cap} more")
    return kept


def _squeeze_dict(value: dict, cap: int, outer: bool) -> dict:
    return {key: _squeeze(item, cap) for key, item in value.items()
            if not (outer and key in ("summary", "text"))}


_SQUEEZERS = (
    (float, lambda value, cap, outer: round(value, 3)),
    (str, _squeeze_text),
    (list, _squeeze_list),
    (dict, _squeeze_dict),
)


def _squeeze(value: Any, cap: int, outer: bool = False) -> Any:
    """One shrink pass: round floats, cut strings, cap lists; the outer dict loses summary and text."""
    handler = next((fn for kind, fn in _SQUEEZERS if isinstance(value, kind)), None)
    return value if handler is None else handler(value, cap, outer)


def _tight(value: Any) -> str:
    return json.dumps(value, separators=(",", ":"), ensure_ascii=False)


def _without_biggest(record: dict) -> str:
    """Drop fields, largest JSON first, until the rest fits; the dropped names are listed."""
    biggest_first = sorted(record, key=lambda key: len(_tight(record[key])), reverse=True)
    remaining = dict(record)
    dropped: list[str] = []
    text = ""
    for key in biggest_first:
        del remaining[key]
        dropped.append(key)
        text = _tight(dict(remaining, cut_fields=dropped,
                           note="fields were left out; call again with full=true"))
        if len(text) <= COMPACT_LIMIT:
            break
    return text


def compact_json(result: Any) -> str:
    """Compact JSON of a result, within 7000 characters whenever that can be done."""
    safe = clean(result)
    shrunk: Any = safe
    for cap in LIST_CAPS:
        shrunk = _squeeze(safe, cap, outer=True)
        text = _tight(shrunk)
        if len(text) <= COMPACT_LIMIT:
            return text
    if isinstance(shrunk, dict):
        return _without_biggest(shrunk)
    return _tight({"cut": "the result does not fit the compact view; full=true returns it"})


def _body(record: dict, full: bool) -> str:
    body = str(record.get("text") or "").strip()
    if full or len(body) <= BODY_LIMIT:
        return body
    return body[:BODY_LIMIT] + f"\n(text cut at {BODY_LIMIT} characters; full=true gives all of it)"


def _payload(record: dict, full: bool) -> str:
    if full:
        return "JSON: " + json.dumps(clean(record), ensure_ascii=False)
    return "JSON (compact): " + compact_json(record)


def format_result(result: Any, full: bool = False, ignored: list[str] | None = None) -> str:
    """The reply text of a tool: summary, body, a note on ignored arguments, then JSON."""
    record = result if isinstance(result, dict) else {"summary": str(result)}
    summary = str(record.get("summary") or "").strip()
    body = _body(record, full)
    if summary and body.startswith(summary):
        summary = ""
    note = f"(this hark version ignored: {', '.join(ignored)})" if ignored else ""
    return "\n\n".join(part for part in (summary, body, note, _payload(record, full)) if part)


def _loudness(m: dict) -> str:
    if m.get("lufs") is None:
        return f"RMS {m.get('rms_dbfs')} dBFS (too short for LUFS)"
    text = f"{m['lufs']} LUFS integrated"
    if m.get("short_term_max_lufs") is not None:
        text += f" (short-term max {m['short_term_max_lufs']})"
    return text


def _peaks(m: dict) -> str:
    text = f"true peak {m.get('true_peak_dbtp')} dBTP"
    if (m.get("clipped_samples") or 0) > 0:
        text += f", {m['clipped_samples']} clipped samples"
    return text


def _level_clauses(m: dict, secs: str) -> list[str]:
    said = [f"{secs} s", _loudness(m), _peaks(m)]
    if m.get("lra_lu") is not None:
        said.append(f"LRA {m['lra_lu']} LU")
    if m.get("noise_floor_dbfs") is not None:
        said.append(f"noise floor {m['noise_floor_dbfs']} dBFS")
    for field, word in (("brightness_hz", "brightness"), ("bandwidth_hz", "bandwidth")):
        if m.get(field):
            said.append(f"{word} {m[field] / 1000:.1f} kHz")
    return said


def _shape_clauses(m: dict) -> list[str]:
    said = []
    if m.get("attack_ms") is not None:
        tail = f", decay {m['decay_ms']} ms" if m.get("decay_ms") is not None else ""
        said.append(f"attack {m['attack_ms']} ms{tail}")
    if m.get("onset_count") is not None:
        said.append(f"{m['onset_count']} onsets")
    if m.get("tempo") is not None:
        tonic = (m.get("key") or [{}])[0].get("key", "?") if m.get("key") else "?"
        said.append(f"tempo ~{m['tempo']} BPM, key {tonic}")
    return said


def _clauses(m: dict, secs: str) -> list[str]:
    return _level_clauses(m, secs) + _shape_clauses(m)


def measure_summary(m: dict) -> str:
    """One plain sentence that sums up a measurement record."""
    secs = f"{float(m.get('duration') or 0):.3f}"
    if m.get("error"):
        return f"{secs} s: {m['error']}."
    top = m.get("peak_dbfs")
    zeros = m.get("digital_silence_frac") or 0
    if top is not None and top < SILENT_DBFS:
        quiet = "digitally silent" if zeros >= 0.999 else "near-silent"
        return (f"{secs} s, {quiet} (peak {m['peak_dbfs']:.0f} dBFS). Loudness, spectrum and envelope values of "
                "silence mean nothing.")
    sentence = ", ".join(_clauses(m, secs)) + "."
    if zeros > 0.01:
        sentence += (f" {zeros * 100:.1f}% exact digital zeros, usually edit, fade or gate padding; weak evidence "
                     "alone, so use realness for real versus synthesized.")
    return sentence
