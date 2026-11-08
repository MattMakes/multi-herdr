"""Reading an audio model's reply: find the JSON, fix its shape, write it as text.

Times in the reply are clip seconds; hark shifts them to file seconds.
"""

from __future__ import annotations

import json
import re
from typing import Any

SCORE_KEYS = ("sync", "realism", "mix_balance", "loudness", "music")
SEVERITY_ORDER = {"high": 0, "medium": 1, "low": 2}
RAW_TEXT_CAP = 3000

_CLOCK = re.compile(r"^\s*(?:(\d+(?:\.\d+)?)\s*:\s*)?(\d+(?:\.\d+)?)\s*s?\s*$")
_LEADING_NUMBER = re.compile(r"\s*(-?\d+(?:\.\d+)?)")
_DANGLING_COMMA = re.compile(r",\s*([}\]])")
_FENCE_OPEN = re.compile(r"^```(?:json)?\s*", re.IGNORECASE)
_FENCE_CLOSE = re.compile(r"\s*```\s*$")


def _g(v: float) -> str:
    return f"{v:g}"


# finding the JSON ----------------------------------------------------


def _first_object(text: str) -> str | None:
    """The text from the first "{" to its matching "}", skipping braces inside strings.

    Without a match the slice runs to the end of the text.
    """
    begin = text.find("{")
    if begin < 0:
        return None
    level = 0
    quoted = False
    escaped = False
    for at in range(begin, len(text)):
        ch = text[at]
        if quoted:
            if escaped:
                escaped = False
            elif ch == "\\":
                escaped = True
            elif ch == '"':
                quoted = False
            continue
        if ch == '"':
            quoted = True
        elif ch == "{":
            level += 1
        elif ch == "}":
            level -= 1
            if level == 0:
                return text[begin: at + 1]
    return text[begin:]


def _as_dict(candidate: str) -> dict | None:
    for version in (candidate, _DANGLING_COMMA.sub(r"\1", candidate)):
        try:
            loaded = json.loads(version)
        except ValueError:
            continue
        if isinstance(loaded, dict):
            return loaded
    return None


def parse_json(text: str) -> dict | None:
    """The first JSON object in a reply; code fences, prose around it and trailing commas are fine."""
    stripped = _FENCE_CLOSE.sub("", _FENCE_OPEN.sub("", str(text or "").strip()))
    for candidate in (stripped, _first_object(stripped)):
        if candidate:
            found = _as_dict(candidate)
            if found is not None:
                return found
    return None


# fixing the shape ----------------------------------------------------


def seconds_of(v: Any) -> float | None:
    """Seconds from a number or from text such as 12.4, 0:12.5, 1:02 or 12.4s; None otherwise."""
    if isinstance(v, (int, float)) and not isinstance(v, bool):
        return float(v)
    if not isinstance(v, str):
        return None
    hit = _CLOCK.match(v)
    if hit is None:
        return None
    return float(hit.group(1) or 0) * 60 + float(hit.group(2))


def score_of(v: Any) -> float | None:
    """A score held to 0..10 at 1 decimal; text gives its leading number."""
    if isinstance(v, bool):
        return None
    if isinstance(v, str):
        hit = _LEADING_NUMBER.match(v)
        v = float(hit.group(1)) if hit else None
    if not isinstance(v, (int, float)):
        return None
    return round(min(10.0, max(0.0, float(v))), 1)


def _text(value: Any) -> str:
    return str(value or "").strip()


def _placed(entries: Any, fields: tuple[str, ...], shift: float, length: float) -> list[dict]:
    """Rows with start and end clamped to the clip, shifted to file seconds, plus text fields."""
    rows = []
    for entry in entries if isinstance(entries, list) else []:
        if not isinstance(entry, dict):
            continue
        begin, finish = seconds_of(entry.get("start")), seconds_of(entry.get("end"))
        if begin is None:
            row: dict[str, Any] = {"start": None, "end": None}
        else:
            begin = min(max(begin, 0.0), length)
            finish = begin if finish is None else min(max(finish, begin), length)
            row = {"start": round(begin + shift, 2), "end": round(finish + shift, 2)}
        for name in fields:
            row[name] = _text(entry.get(name))
        rows.append(row)
    return rows


def _top_texts(entries: Any, limit: int) -> list[str]:
    if not isinstance(entries, list):
        return []
    return [t for t in (str(e).strip() for e in entries) if t][:limit]


def normalize(obj: dict, offset: float = 0.0, clip_len: float = 1e9) -> dict[str, Any]:
    """The reply in hark's fixed shape, every time in file seconds."""
    head = obj.get("overall")
    if not isinstance(head, dict):
        head = {"score": head}
    marks = obj.get("scores") if isinstance(obj.get("scores"), dict) else {}
    remarks = obj.get("notes") if isinstance(obj.get("notes"), dict) else {}
    problems = _placed(obj.get("issues"), ("severity", "category", "problem", "fix"), offset, clip_len)
    for row in problems:
        level = row["severity"].lower()
        row["severity"] = level if level in SEVERITY_ORDER else "medium"
    problems.sort(key=lambda r: (SEVERITY_ORDER[r["severity"]], r["start"] is None, r["start"] or 0.0))
    shaped: dict[str, Any] = {
        "overall": {"score": score_of(head.get("score")), "verdict": _text(head.get("verdict"))},
        "scores": {k: score_of(marks.get(k)) for k in SCORE_KEYS},
        "notes": {k: _text(remarks.get(k)) for k in SCORE_KEYS if _text(remarks.get(k))},
        "synthetic_or_cheap": _placed(obj.get("synthetic_or_cheap"), ("what", "why"), offset, clip_len),
        "issues": problems,
        "strengths": _top_texts(obj.get("strengths"), 8),
        "top_fixes": _top_texts(obj.get("top_fixes"), 5),
    }
    reply = _text(obj.get("answer"))
    if reply:
        shaped["answer"] = reply
    return shaped


# writing it as text --------------------------------------------------


def _trim_note(clip: dict[str, Any] | None) -> str:
    if not clip or not clip.get("trimmed"):
        return ""
    return f" [Only the first {clip['seconds']:g} s were sent; use --start / --end for other parts.]"


def parsed_summary(provider: str, model: str, crit: dict, clip: dict | None) -> str:
    """One paragraph: the score, the verdict, the issue counts and the first fix."""
    score = crit["overall"]["score"]
    grade = "unscored" if score is None else f"{_g(score)}/10"
    verdict = crit["overall"]["verdict"] or "no verdict"
    if not verdict.endswith((".", "!", "?")):
        verdict += "."
    problems = crit["issues"]
    severe = len([p for p in problems if p["severity"] == "high"])
    text = f"{provider} ({model}) rates the soundtrack {grade}: {verdict} {len(problems)} issues ({severe} high)."
    if crit["top_fixes"]:
        text += f" Top fix: {crit['top_fixes'][0]}"
    text += (" This is an LLM opinion and its timestamps are approximate; verify each fix with "
             "`hark timeline` or `hark ab`.")
    return text + _trim_note(clip)


def _clock(t: float) -> str:
    return f"{int(t // 60)}:{t % 60:05.2f}"


def _span_text(item: dict) -> str:
    begin, finish = item.get("start"), item.get("end")
    if begin is None:
        return "(no time)"
    if finish is None or finish == begin:
        return _clock(begin)
    return f"{_clock(begin)}-{_clock(finish)}"


def _critique_lines(c: dict) -> list[str]:
    out: list[str] = []
    if c.get("answer"):
        out.append(f"answer: {c['answer']}")
    graded = [f"{k} {_g(v)}" for k, v in c["scores"].items() if v is not None]
    if graded:
        out.append("scores (1-10, LLM opinion): " + ", ".join(graded))
    out.extend(f"  {k}: {v}" for k, v in c["notes"].items())
    if c["issues"]:
        out.append("issues (most severe first; times are file seconds, approximate):")
        out.extend(f"  {_span_text(p)} {p['severity']} {p['category']}: {p['problem']} -> fix: {p['fix']}"
                   for p in c["issues"])
    if c["synthetic_or_cheap"]:
        out.append("synthetic or cheap moments:")
        out.extend(f"  {_span_text(s)} {s['what']}: {s['why']}" for s in c["synthetic_or_cheap"])
    if c["strengths"]:
        out.append("strengths: " + "; ".join(c["strengths"]))
    if c["top_fixes"]:
        out.append("top fixes: " + " | ".join(f"{n}. {fix}" for n, fix in enumerate(c["top_fixes"], 1)))
    return out


def render(result: dict[str, Any]) -> str:
    """The readable text of a critique result."""
    lines = [str(result.get("summary", ""))]
    lines.extend(f"(note: {n})" for n in result.get("notes", []))
    crit = result.get("critique")
    if crit:
        lines.extend(_critique_lines(crit))
    elif result.get("raw"):
        lines.append(str(result["raw"])[:RAW_TEXT_CAP])
    return "\n".join(lines)
