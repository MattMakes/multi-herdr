"""Optional second opinion in words from an audio language model, through the agy CLI.

hark never reads, sets or passes a vendor API key. agy uses the operator's own sign-in.
Nothing leaves the machine without explicit consent (``--yes-upload`` or ``consent=True``).
"""

from __future__ import annotations

import argparse
import importlib
import io
import json
import os
import re
import shutil
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

from hark.surfaces._flow import raise_if

AGY_VERDICT = "HEARS"                      # docs/specs/hark/agy-audio-probe.md
AGY_DEFAULT_MODEL = "gemini-4-argon-high"  # used when --model and HARK_LLM_MODEL are unset
AGY_TIMEOUT_S = 240
MODEL_ENV = "HARK_LLM_MODEL"
CLIP_SR = 16000
CLIP_CODEC = "wav mono 16 kHz 16-bit"
CLIP_MIME = "audio/wav"
MAX_SECONDS = 600.0
TRANSCRIPT_CAP = 16000
CUT_CAP = 200
RAW_CAP = 20000
RAW_TEXT_CAP = 3000
ERR_CAP = 300
RETRY_WAIT_S = 4.0
SCORE_KEYS = ("sync", "realism", "mix_balance", "loudness", "music")
SEVERITY_ORDER = {"high": 0, "medium": 1, "low": 2}
VIDEO_EXTS_FALLBACK = (".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi")
MEASURED_KEYS = (
    ("integrated loudness", "lufs", " LUFS"),
    ("true peak", "true_peak_dbtp", " dBTP"),
    ("loudness range", "lra_lu", " LU"),
    ("short-term max loudness", "short_term_max_lufs", " LUFS"),
    ("noise floor (10th percentile of 20 ms levels)", "noise_floor_dbfs", " dBFS"),
    ("clipped samples", "clipped_samples", ""),
)
_KEYISH = re.compile(r"API_?KEY|SECRET", re.IGNORECASE)
_SECRET_TEXT = re.compile(r"\b(sk-[A-Za-z0-9_\-]{12,}|AIza[0-9A-Za-z_\-]{20,})")
_RATE_LIMIT = re.compile(r"rate.?limit|RESOURCE_EXHAUSTED|\b429\b|quota", re.IGNORECASE)
_SECONDS = re.compile(r"^\s*(?:(\d+(?:\.\d+)?)\s*:\s*)?(\d+(?:\.\d+)?)\s*s?\s*$")
_TRAILING_COMMA = re.compile(r",\s*([}\]])")
_sleep = time.sleep


class ProviderError(RuntimeError):
    """The agy call failed."""


class AgyCannotHear(ProviderError):
    """The probe verdict says agy does not hear audio files; nothing is sent."""


# ---------------------------------------------------------------- provider


def child_env() -> dict[str, str]:
    """The environment for agy: every variable except key-like names, whose values are never read."""
    return {name: os.environ[name] for name in list(os.environ) if not _KEYISH.search(name)}


def scrub(text: str) -> str:
    """Replace anything that looks like a secret value with ***."""
    return _SECRET_TEXT.sub("***", str(text))


def _agy_usage(usage: Any) -> dict[str, int]:
    if not isinstance(usage, dict):
        return {}
    pairs = (("promptTokenCount", ("promptTokenCount", "input_tokens", "prompt_tokens")),
             ("candidatesTokenCount", ("candidatesTokenCount", "output_tokens", "completion_tokens")),
             ("thoughtsTokenCount", ("thoughtsTokenCount", "thoughts_tokens", "reasoning_tokens")),
             ("totalTokenCount", ("totalTokenCount", "total_tokens")))
    out: dict[str, int] = {}
    for key, names in pairs:
        val = next((usage[n] for n in names if isinstance(usage.get(n), int)), None)
        if val is not None:
            out[key] = val
    return out


def _agy_prompt(wav: str, prompt: str) -> str:
    return (f"Open the audio file {wav} and listen to it. Do not run shell commands or scripts; "
            "use only your own hearing of the audio.\n\n" + prompt)


class AgyProvider:
    """The Antigravity CLI ``agy -p`` as the audio-model provider."""

    name = "agy"
    default_model = AGY_DEFAULT_MODEL

    def executable(self) -> str | None:
        return shutil.which("agy")

    def ready(self) -> bool:
        return self.executable() is not None

    def model(self, model: str | None = None) -> str:
        chosen = model or os.environ.get(MODEL_ENV) or self.default_model
        return chosen[len("models/"):] if chosen.startswith("models/") else chosen

    def how_to_enable(self) -> list[str]:
        return ["Install the agy CLI and sign in with your own account, so `agy` is on PATH.",
                "Run again with --yes-upload (CLI) or consent=True (Python, MCP) only when the user "
                "agrees that this audio leaves the machine."]

    def argv(self, prompt: str, folder: str, model: str) -> list[str]:
        return [self.executable() or "agy", "-p", prompt, "--add-dir", folder, "--model", model,
                "--output-format", "json", "--print-timeout", f"{AGY_TIMEOUT_S}s"]

    def _run_once(self, argv: list[str]) -> subprocess.CompletedProcess:
        try:
            return subprocess.run(argv, capture_output=True, text=True, env=child_env(),
                                  stdin=subprocess.DEVNULL, timeout=AGY_TIMEOUT_S + 30)
        except subprocess.TimeoutExpired:
            raise ProviderError("agy did not answer in time; try a shorter clip with --start / --end") from None
        except OSError as exc:
            raise ProviderError(f"agy could not start: {type(exc).__name__}: {exc}") from None

    def _run(self, argv: list[str]) -> subprocess.CompletedProcess:
        proc = self._run_once(argv)
        if proc.returncode != 0 and _RATE_LIMIT.search((proc.stdout or "") + (proc.stderr or "")):
            _sleep(RETRY_WAIT_S)
            proc = self._run_once(argv)
        return proc

    def generate(self, audio_bytes: bytes, mime: str, prompt: str, model: str) -> dict[str, Any]:
        folder = tempfile.mkdtemp(prefix="hark-llm-")
        try:
            wav = os.path.join(folder, "clip.wav")
            Path(wav).write_bytes(audio_bytes)
            proc = self._run(self.argv(_agy_prompt(wav, prompt), folder, model))
            return self._answer(proc, model)
        finally:
            shutil.rmtree(folder, ignore_errors=True)

    def _answer(self, proc: subprocess.CompletedProcess, model: str) -> dict[str, Any]:
        data = _json_or_none(proc.stdout or "")
        _check_agy_json(data, proc)
        return {"text": data["response"], "finish_reason": data.get("finish_reason") or data.get("stop_reason"),
                "method": "agy", "files_api_deleted": None,
                "model_version": data.get("model") or model or "agy-default",
                "usage": _agy_usage(data.get("usage"))}


def _stopped(data: dict) -> bool:
    return bool(data.get("finished_reason")) or data.get("status") not in (None, "SUCCESS")


def _blank(text: Any) -> bool:
    return not isinstance(text, str) or not text.strip()


def _check_agy_json(data: Any, proc: subprocess.CompletedProcess) -> None:
    """ProviderError when agy gave no JSON, stopped early, or gave an empty answer."""
    err = (proc.stderr or "")[:ERR_CAP]
    code = proc.returncode
    raise_if(not isinstance(data, dict),
             ProviderError(f"agy gave no JSON answer (exit {code}): {err or (proc.stdout or '')[:ERR_CAP]}"))
    raise_if(_stopped(data), ProviderError(f"agy stopped: status {data.get('status')}, "
                                           f"finished_reason {data.get('finished_reason')} (exit {code})"))
    raise_if(_blank(data.get("response")), ProviderError(f"agy gave an empty answer (exit {code}): {err}"))


PROVIDERS: dict[str, type] = {"agy": AgyProvider}


def _json_or_none(text: str) -> Any:
    try:
        return json.loads(text)
    except ValueError:
        return None


def get_provider(provider: Any = "agy") -> Any:
    """A provider object from a name or an object; ValueError for an unknown name."""
    if not isinstance(provider, str):
        return provider
    if provider not in PROVIDERS:
        raise ValueError(f"unknown LLM provider {provider!r}; providers: {', '.join(PROVIDERS)}")
    return PROVIDERS[provider]()


# ---------------------------------------------------------------- clip


def _file_duration(path: str) -> float:
    from hark.core import audio
    try:
        dur = audio.probe(path).get("duration")
        if isinstance(dur, (int, float)) and dur > 0:
            return float(dur)
    except Exception:
        pass
    return float(audio.duration(path))


def _clip_range(dur: float, start: float | None, end: float | None, max_seconds: float) -> tuple[float, float, bool]:
    a = max(0.0, float(start or 0.0))
    b = min(dur, float(end)) if end is not None else dur
    if b <= a:
        raise ValueError(f"empty range {a:.3f}-{b:.3f} s (file is {dur:.3f} s)")
    if b - a > max_seconds:
        return a, a + max_seconds, True
    return a, b, False


def _wav_bytes(x: Any) -> bytes:
    import soundfile as sf
    buf = io.BytesIO()
    sf.write(buf, x, CLIP_SR, format="WAV", subtype="PCM_16")
    return buf.getvalue()


def encode(path: str, start: float | None = None, end: float | None = None,
           max_seconds: float = MAX_SECONDS, kbps: int = 64) -> tuple[bytes, str, dict[str, Any]]:
    """The clip that agy hears: WAV, mono, 16 kHz, 16-bit (agy hears MP3 as silence).

    ``kbps`` is kept for the interface and has no effect on WAV.
    """
    from hark.core import audio
    dur = _file_duration(path)
    a, b, trimmed = _clip_range(dur, start, end, max_seconds)
    x = audio.load(path, sr=CLIP_SR, mono=True, start=a, end=b, cache=False)
    data = _wav_bytes(x)
    if not data:
        raise ProviderError(f"{path}: the clip encode gave no bytes")
    info = {"start": round(a, 3), "end": round(b, 3), "seconds": round(b - a, 3),
            "file_duration": round(dur, 3), "trimmed": trimmed, "bytes": len(data), "codec": CLIP_CODEC}
    return data, CLIP_MIME, info


# ---------------------------------------------------------------- context


def _video_exts() -> tuple[str, ...]:
    try:
        from hark.core import audio
        return tuple(audio.VIDEO_EXTS)
    except Exception:
        return VIDEO_EXTS_FALLBACK


def video_of(path: str, video: str | None) -> str | None:
    """The given video, else the path itself when it is a video file, else None."""
    if video:
        return video
    return path if os.path.splitext(path)[1].lower() in _video_exts() else None


def _clip_line(clip: dict[str, Any]) -> str:
    return (f"The clip is {clip['seconds']:.1f} s long and covers file seconds {clip['start']:.1f} to "
            f"{clip['end']:.1f} of a {clip['file_duration']:.1f} s file ({clip['codec']}).")


def _measured_line(path: str, clip: dict[str, Any]) -> str | None:
    from hark.core import features
    m = features.measure(path, start=clip["start"], end=clip["end"], light=True)
    parts = [f"{label} {m[key]}{unit}" for label, key, unit in MEASURED_KEYS if m.get(key) is not None]
    if not parts:
        return None
    return ("Measured by hark on this clip (exact values; use them and never estimate levels yourself): "
            + ", ".join(parts) + ".")


def _transcript_lines(path: str, clip: dict[str, Any], video: str | None) -> list[str]:
    tl = importlib.import_module("hark.timeline.timeline")
    res = tl.timeline(path, video=video, start=clip["start"], end=clip["end"], detail="low")
    body = str(res.get("text") or res.get("summary") or "")[:TRANSCRIPT_CAP]
    head = "hark's machine-made listening transcript follows, in file seconds"
    if clip["start"] > 0:
        head += f" (the clip starts at file second {clip['start']:g})"
    return [head + ". Take every time, count and level from it:", body]


def _num(v: float) -> str:
    return f"{v:g}"


def _cut_line(clip: dict[str, Any], video: str) -> str | None:
    vid = importlib.import_module("hark.timeline.video")
    a, b = clip["start"], clip["end"]
    kept = [round(c - a, 2) for c in vid.cuts(video) if a <= c < b][:CUT_CAP]
    if not kept:
        return None
    return ("Picture cuts or big visual changes, in clip seconds: " + ", ".join(_num(c) for c in kept)
            + ". The detector finds hard cuts and whips but misses slow crossfades or finds them late.")


def _measured_safe(path: str, clip: dict[str, Any], notes: list[str]) -> list[str]:
    try:
        line = _measured_line(path, clip)
    except Exception as exc:
        notes.append(f"measured levels NOT sent: {type(exc).__name__}: {scrub(exc)}")
        return []
    return [line] if line else []


def _timeline_safe(path: str, clip: dict[str, Any], video: str | None, notes: list[str]) -> list[str] | None:
    try:
        return _transcript_lines(path, clip, video)
    except Exception as exc:
        notes.append(f"listening transcript NOT sent: {type(exc).__name__}: {scrub(exc)}")
        return None


def _cuts_safe(clip: dict[str, Any], video: str, notes: list[str]) -> list[str]:
    try:
        line = _cut_line(clip, video)
    except Exception as exc:
        notes.append(f"picture cut times NOT sent: {type(exc).__name__}")
        return []
    return [line] if line else []


def context(path: str, clip: dict[str, Any], video: str | None = None, with_timeline: bool = False,
            cuts: bool = True) -> tuple[str, list[str]]:
    """The context text sent with the prompt, and notes on what was not sent."""
    notes: list[str] = []
    lines = [_clip_line(clip)] + _measured_safe(path, clip, notes)
    vid = video_of(path, video)
    transcript = _timeline_safe(path, clip, vid, notes) if with_timeline else None
    lines += transcript or []
    if vid and transcript is None and cuts:
        lines += _cuts_safe(clip, vid, notes)
    return "\n".join(lines), notes


# ---------------------------------------------------------------- prompt

_RUBRIC = """You are a seasoned re-recording mixer and sound supervisor for film. You hear the mix, or one stem, of a short product launch film.

Hold it to the standard of launch films from the best studios: every sound is a deliberate choice, the music is a produced mix, effects land on picture events, and the master is loud while it keeps its dynamics.

An effect can be a recording or a designed or synthetic sound. Grade only how well it fits and how well it is made. Listen to the whole clip. Point to concrete moments. Praise only what earns it.

Sync: judge whether hits, accents and whooshes land on their moments. You cannot see the picture, so use the cut and cue times in the context. Without them, judge rhythmic precision and whether sounds drag, rush or smear. Give sync null when nothing is there to judge against.

Realism: name the sounds that come across as recorded, and the ones that come across as made in code, cheap stock or generic: bare sine beeps, raw noise whooshes, chiptune blips, stock risers, one sample fired again and again without change. A synthetic sound is a fault only where it should pass as recorded. When the context or the question says synthesis is intended, grade those sounds as sound design.

Balance: music, effects and ambience against each other. Listen for masking, elements that jump out or disappear, muddy lows and harsh 2-5 kHz. The clip is mono, so do not grade width.

Loudness: take every level from the measured context and never make one up. Listen for over-compression, low level, peaks, pumping and clipping.

Weak spots: the moments an expert would find embarrassing.

Fixes: each fix has a time range and an exact action, for example: swap the harsh beep for a soft tap that fits; pull the music down 4 dB from 12.0 to 15.0; move the hit 80 ms earlier; high-pass the pad at 150 Hz.

Your hearing of times, counts, pitch and levels is coarse. Do not estimate them. Take them from the measured context and the transcript; your own times are only rough pointers.

Scores run from 1 to 10 in whole steps: 10 equals the premium bar, 7 is professional with small faults, 5 is a capable amateur, 3 is plainly cheap, 1 is broken. The realism score grades how convincing and well made the sounds are for the film's intent, not whether they are recordings.

Write every time as seconds from the start of this clip (for example 12.4), never as clock text such as 0:12.

Reply with one JSON object and nothing else, in this schema:
"""

_SCHEMA_HEAD = """{"overall": {"score": 1-10, "verdict": "one sentence"},
 "scores": {"sync": 1-10 or null, "realism": 1-10, "mix_balance": 1-10, "loudness": 1-10, "music": 1-10 or null},
 "notes": {"sync": "", "realism": "", "mix_balance": "", "loudness": "", "music": ""},
 "synthetic_or_cheap": [{"start": s, "end": s, "what": "", "why": ""}],
 "issues": [{"start": s, "end": s, "severity": "high|medium|low", "category": "sync|realism|mix|loudness|music|noise|edit", "problem": "", "fix": ""}],
 "strengths": [""],
 "top_fixes": ["at most 5, most important first"]"""

_INTENT = ("Intent: the procedural or synthetic sound of this film is deliberate. Grade it as sound design. "
           "List a sound in synthetic_or_cheap only for poor craft (cheap, harsh, generic or badly made), "
           "never for being synthetic.")


def _schema(question: bool) -> str:
    tail = ',\n "answer": "your reply to the editor\'s question"}' if question else "}"
    return _SCHEMA_HEAD + tail


def build_prompt(ctx_text: str, question: str | None = None, synth_intended: bool = False) -> str:
    """The full prompt: rubric and schema, the context, the intent and the editor's question."""
    text = _RUBRIC + _schema(bool(question)) + "\n\nContext:\n" + ctx_text
    if synth_intended:
        text += "\n" + _INTENT
    if question:
        text += ("\n\nThe editor asks the question below. Put your reply in \"answer\"; every other field "
                 "is still required.\n" + question)
    return text


# ---------------------------------------------------------------- answer


def _strip_fences(text: str) -> str:
    t = text.strip()
    t = re.sub(r"^```(?:json)?\s*", "", t, flags=re.IGNORECASE)
    return re.sub(r"\s*```\s*$", "", t)


_BRACES = {"{": 1, "}": -1}


def _in_string(state: dict, ch: str) -> bool:
    esc = state["esc"]
    state["esc"] = (not esc) and ch == "\\"
    state["in_str"] = esc or ch != '"'
    return False


def _outside_string(state: dict, ch: str) -> bool:
    state["in_str"] = ch == '"'
    state["depth"] += _BRACES.get(ch, 0)
    return ch == "}" and state["depth"] == 0


def _object_end(text: str, i: int) -> int:
    """The index after the brace that closes the object opened at ``i``, else the text length."""
    state = {"depth": 0, "in_str": False, "esc": False}
    steps = (j for j in range(i, len(text)) if (_in_string if state["in_str"] else _outside_string)(state, text[j]))
    return next(steps, len(text) - 1) + 1


def _outer_object(text: str) -> str | None:
    i = text.find("{")
    return text[i:_object_end(text, i)] if i >= 0 else None


def _load_obj(cand: str) -> dict | None:
    for attempt in (cand, _TRAILING_COMMA.sub(r"\1", cand)):
        obj = _json_or_none(attempt)
        if isinstance(obj, dict):
            return obj
    return None


def parse_json(text: str) -> dict | None:
    """The first JSON object in a model answer (fences, prose and trailing commas allowed), else None."""
    body = _strip_fences(str(text or ""))
    for cand in (body, _outer_object(body)):
        obj = _load_obj(cand) if cand else None
        if obj is not None:
            return obj
    return None


def seconds_of(v: Any) -> float | None:
    """Seconds from a number or a string like 12.4, 0:12.5, 1:02 or 12.4s; else None."""
    number = isinstance(v, (int, float)) and not isinstance(v, bool)
    m = _SECONDS.match(v) if isinstance(v, str) else None
    clock = float(m.group(1) or 0) * 60 + float(m.group(2)) if m else None
    return float(v) if number else clock


def score_of(v: Any) -> float | None:
    """A score clamped to 0..10 with 1 decimal; a string gives its leading number."""
    if isinstance(v, bool):
        return None
    if isinstance(v, str):
        m = re.match(r"\s*(-?\d+(?:\.\d+)?)", v)
        v = float(m.group(1)) if m else None
    if not isinstance(v, (int, float)):
        return None
    return round(min(10.0, max(0.0, float(v))), 1)


def _span(item: dict, offset: float, clip_len: float) -> tuple[float | None, float | None]:
    a, b = seconds_of(item.get("start")), seconds_of(item.get("end"))
    if a is None:
        return None, None
    a = min(max(a, 0.0), clip_len)
    b = a if b is None else min(max(b, a), clip_len)
    return round(a + offset, 2), round(b + offset, 2)


def _timed(items: Any, keys: tuple[str, ...], offset: float, clip_len: float) -> list[dict]:
    out = []
    for item in items if isinstance(items, list) else []:
        if not isinstance(item, dict):
            continue
        start, end = _span(item, offset, clip_len)
        row = {"start": start, "end": end}
        row.update({k: str(item.get(k) or "").strip() for k in keys})
        out.append(row)
    return out


def _issue_key(issue: dict) -> tuple:
    return (SEVERITY_ORDER[issue["severity"]], issue["start"] is None, issue["start"] or 0.0)


def _issues(items: Any, offset: float, clip_len: float) -> list[dict]:
    rows = _timed(items, ("severity", "category", "problem", "fix"), offset, clip_len)
    for row in rows:
        sev = row["severity"].lower()
        row["severity"] = sev if sev in SEVERITY_ORDER else "medium"
    return sorted(rows, key=_issue_key)


def _strings(items: Any, cap: int) -> list[str]:
    vals = [str(s).strip() for s in items] if isinstance(items, list) else []
    return [s for s in vals if s][:cap]


def _overall(obj: dict) -> dict:
    ov = obj.get("overall")
    ov = ov if isinstance(ov, dict) else {"score": ov}
    return {"score": score_of(ov.get("score")), "verdict": str(ov.get("verdict") or "").strip()}


def _dict(v: Any) -> dict:
    return v if isinstance(v, dict) else {}


def _scores_and_notes(obj: dict) -> dict[str, Any]:
    scores, notes = _dict(obj.get("scores")), _dict(obj.get("notes"))
    kept = {k: str(notes.get(k) or "").strip() for k in SCORE_KEYS}
    return {"scores": {k: score_of(scores.get(k)) for k in SCORE_KEYS},
            "notes": {k: v for k, v in kept.items() if v}}


def _lists(obj: dict) -> dict[str, Any]:
    answer = str(obj.get("answer") or "").strip()
    out = {"strengths": _strings(obj.get("strengths"), 8), "top_fixes": _strings(obj.get("top_fixes"), 5)}
    out.update({"answer": answer} if answer else {})
    return out


def normalize(obj: dict, offset: float = 0.0, clip_len: float = 1e9) -> dict[str, Any]:
    """The model answer in hark's fixed shape, with times in file seconds."""
    out: dict[str, Any] = {"overall": _overall(obj)}
    out.update(_scores_and_notes(obj))
    out["synthetic_or_cheap"] = _timed(obj.get("synthetic_or_cheap"), ("what", "why"), offset, clip_len)
    out["issues"] = _issues(obj.get("issues"), offset, clip_len)
    out.update(_lists(obj))
    return out


# ---------------------------------------------------------------- result


def _trim_note(clip: dict[str, Any] | None) -> str:
    if not clip or not clip.get("trimmed"):
        return ""
    return f" [Only the first {clip['seconds']:g} s were sent; use --start / --end for other parts.]"


def _verdict_text(v: str) -> str:
    v = v or "no verdict"
    return v if v.endswith((".", "!", "?")) else v + "."


def parsed_summary(provider: str, model: str, crit: dict, clip: dict | None) -> str:
    score = crit["overall"]["score"]
    rated = f"{_num(score)}/10" if score is not None else "unscored"
    issues = crit["issues"]
    high = sum(1 for i in issues if i["severity"] == "high")
    text = (f"{provider} ({model}) rates the soundtrack {rated}: {_verdict_text(crit['overall']['verdict'])} "
            f"{len(issues)} issues ({high} high).")
    if crit["top_fixes"]:
        text += f" Top fix: {crit['top_fixes'][0]}"
    text += (" This is an LLM opinion and its timestamps are approximate; verify each fix with "
             "`hark timeline` or `hark ab`.")
    return text + _trim_note(clip)


def _fmt_time(t: float) -> str:
    return f"{int(t // 60)}:{t % 60:05.2f}"


def _span_text(item: dict) -> str:
    a, b = item.get("start"), item.get("end")
    if a is None:
        return "(no time)"
    if b is None or b == a:
        return _fmt_time(a)
    return f"{_fmt_time(a)}-{_fmt_time(b)}"


def _headed(head: str, rows: list[str]) -> list[str]:
    return [head] + rows if rows else []


def _joined(head: str, sep: str, items: list[str]) -> list[str]:
    return [head + sep.join(items)] if items else []


def _issue_row(i: dict) -> str:
    return f"  {_span_text(i)} {i['severity']} {i['category']}: {i['problem']} -> fix: {i['fix']}"


def _critique_lines(c: dict) -> list[str]:
    scored = [f"{k} {_num(v)}" for k, v in c["scores"].items() if v is not None]
    return ([f"answer: {c['answer']}"] if c.get("answer") else []) \
        + _joined("scores (1-10, LLM opinion): ", ", ", scored) \
        + [f"  {k}: {v}" for k, v in c["notes"].items()] \
        + _headed("issues (most severe first; times are file seconds, approximate):",
                  [_issue_row(i) for i in c["issues"]]) \
        + _headed("synthetic or cheap moments:",
                  [f"  {_span_text(i)} {i['what']}: {i['why']}" for i in c["synthetic_or_cheap"]]) \
        + _joined("strengths: ", "; ", c["strengths"]) \
        + _joined("top fixes: ", " | ", [f"{n}. {f}" for n, f in enumerate(c["top_fixes"], 1)])


def render(result: dict[str, Any]) -> str:
    """The human text of a critique result."""
    lines = [str(result.get("summary", ""))]
    lines += [f"(note: {n})" for n in result.get("notes", [])]
    crit = result.get("critique")
    if not crit:
        if result.get("raw"):
            lines.append(str(result["raw"])[:RAW_TEXT_CAP])
        return "\n".join(lines)
    return "\n".join(lines + _critique_lines(crit))


def _off(path: str, prov: Any, model: str, summary: str, steps: list[str]) -> dict[str, Any]:
    text = summary + "\n" + "\n".join(f"  {n}. {s}" for n, s in enumerate(steps, 1))
    return {"enabled": False, "ok": False, "path": path, "provider": prov.name, "model": model,
            "summary": summary, "how_to_enable": steps, "text": text}


def _not_ready(path: str, prov: Any, model: str) -> dict[str, Any]:
    summary = (f"The LLM critique is off: the {prov.name} provider is not ready. "
               "The rest of hark works without it.")
    return _off(path, prov, model, summary, prov.how_to_enable())


def _no_consent(path: str, prov: Any, model: str) -> dict[str, Any]:
    summary = (f"Nothing was uploaded. Sending {os.path.basename(path)} to {prov.name} ({model}) "
               "needs the user's explicit consent.")
    steps = [f"Ask the user for permission to send this audio to {prov.name} for an AI review.",
             "With permission, run again with --yes-upload (CLI) or consent=True (Python, MCP)."]
    return _off(path, prov, model, summary, steps)


def _failed(path: str, prov: Any, model: str, clip: dict | None, exc: BaseException) -> dict[str, Any]:
    msg = scrub(f"{exc}" if isinstance(exc, (ProviderError, ValueError)) else f"{type(exc).__name__}: {exc}")
    text = f"LLM critique failed: {msg}"
    return {"enabled": True, "ok": False, "path": path, "provider": prov.name, "model": model,
            "uploaded": clip, "summary": text, "text": text, "error": msg}


def _uploaded(clip: dict, gen: dict) -> dict:
    up = dict(clip, method=gen.get("method"))
    if gen.get("files_api_deleted") is not None:
        up["files_api_deleted"] = gen["files_api_deleted"]
    return up


def _answered(path: str, prov: Any, model: str, clip: dict, gen: dict, sent: dict) -> dict[str, Any]:
    obj = parse_json(gen.get("text") or "")
    shown = gen.get("model_version") or model
    res: dict[str, Any] = {"enabled": True, "ok": obj is not None, "path": path, "provider": prov.name,
                           "model": shown, "uploaded": _uploaded(clip, gen), "seconds": sent["seconds"],
                           "usage": gen.get("usage") or {}, "finish_reason": gen.get("finish_reason"),
                           "context_sent": sent["context"], "notes": sent["notes"]}
    if obj is None:
        res["raw"] = scrub(gen.get("text") or "")[:RAW_CAP]
        res["summary"] = (f"{prov.name} answered, but not in parseable JSON (finish reason "
                          f"{gen.get('finish_reason')}); the raw answer is attached." + _trim_note(clip))
    else:
        res["critique"] = normalize(obj, offset=clip["start"], clip_len=clip["seconds"])
        res["summary"] = parsed_summary(prov.name, shown, res["critique"], clip)
    res["text"] = render(res)
    return res


def _check_hears(prov: Any) -> None:
    if getattr(prov, "name", "") == "agy" and AGY_VERDICT != "HEARS":
        raise AgyCannotHear("agy cannot hear audio files (probe verdict "
                            f"{AGY_VERDICT}); the LLM critique is unavailable and nothing was sent.")


def _check_file(path: str) -> None:
    if os.path.isfile(path):
        return
    from hark.core.audio import AudioError
    raise AudioError(f"no such file: {path}")


def _send(path: str, prov: Any, model: str, opts: dict[str, Any]) -> dict[str, Any]:
    clip = None
    try:
        data, mime, clip = encode(path, opts["start"], opts["end"], opts["max_seconds"])
        ctx, notes = context(path, clip, opts["video"], opts["with_timeline"], opts["cuts"])
        prompt = build_prompt(ctx, opts["prompt"], opts["synth_intended"])
        t0 = time.monotonic()
        gen = prov.generate(data, mime, prompt, model)
        sent = {"seconds": round(time.monotonic() - t0, 1), "context": ctx, "notes": notes}
    except Exception as exc:
        return _failed(path, prov, model, clip, exc)
    return _answered(path, prov, model, clip, gen, sent)


def critique(path: str, prompt: str | None = None, provider: Any = "agy", model: str | None = None,
             consent: bool = False, video: str | None = None, with_timeline: bool = False,
             start: float | None = None, end: float | None = None, max_seconds: float = MAX_SECONDS,
             cuts: bool = True, synth_intended: bool = False) -> dict[str, Any]:
    """An optional verbal review of a soundtrack by an audio LLM through agy.

    Raises AgyCannotHear when the probe verdict says agy does not hear audio.
    """
    prov = get_provider(provider)
    path = str(path)
    _check_file(path)
    _check_hears(prov)
    chosen = prov.model(model)
    if not prov.ready():
        return _not_ready(path, prov, chosen)
    if not consent:
        return _no_consent(path, prov, chosen)
    opts = {"prompt": prompt, "video": video, "with_timeline": with_timeline, "start": start, "end": end,
            "max_seconds": max_seconds, "cuts": cuts, "synth_intended": synth_intended}
    return _send(path, prov, chosen, opts)


# ---------------------------------------------------------------- CLI


def _add_flags(p: argparse.ArgumentParser) -> None:
    p.add_argument("file", help="soundtrack, stem or video; only the audio is sent")
    p.add_argument("--prompt", default=None, help="an extra question for the model")
    p.add_argument("--yes-upload", action="store_true", help="consent: this audio may leave the machine")
    p.add_argument("--model", default=None, help=f"model id (default ${MODEL_ENV}, else {AGY_DEFAULT_MODEL})")
    p.add_argument("--video", default=None, help="the film, for picture-cut times (frames are never sent)")
    p.add_argument("--with-timeline", action="store_true", help="also send hark's listening transcript")
    p.add_argument("--no-cuts", action="store_true", help="do not compute picture cuts")
    p.add_argument("--start", type=float, default=None, help="send from this second")
    p.add_argument("--end", type=float, default=None, help="send up to this second")
    p.add_argument("--max-seconds", type=float, default=MAX_SECONDS, help="trim longer audio to this length")
    p.add_argument("--synth-intended", action="store_true", help="synthesized sound is deliberate here")


def add_cli(sub: Any) -> None:
    """Register ``hark llm``."""
    from hark.core import report
    p = sub.add_parser("llm", help="Optional audio-LLM critique through the agy CLI (needs --yes-upload).")
    _add_flags(p)
    report.add_common(p)
    p.set_defaults(func=handle)


def handle(args: argparse.Namespace) -> int:
    from hark.core import report
    if args.with_timeline:
        from hark.models import require_ml
        require_ml("llm --with-timeline")
    try:
        result = critique(args.file, prompt=args.prompt, model=args.model, consent=args.yes_upload,
                          video=args.video, with_timeline=args.with_timeline, start=args.start, end=args.end,
                          max_seconds=args.max_seconds, cuts=not args.no_cuts, synth_intended=args.synth_intended)
    except AgyCannotHear as exc:
        report.error_line("llm", str(exc))
        return 2
    report.emit(result, args, text=result.get("text"))
    return 0 if result.get("ok") else 1
