"""What hark sends with a clip: the clip itself, measured context and the prompt.

The clip is mono 16 kHz 16-bit WAV, since agy hears MP3 as silence. Context
lines are optional; each one that fails becomes a note instead of an error.
"""

from __future__ import annotations

import importlib
import io
import os
from typing import Any

from hark.surfaces._llm_agy import ProviderError, scrub

CLIP_SR = 16000


CLIP_CODEC = "wav mono 16 kHz 16-bit"


CLIP_MIME = "audio/wav"


MAX_SECONDS = 600.0


TRANSCRIPT_CAP = 16000


CUT_CAP = 200


VIDEO_EXTS_FALLBACK = (".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi")


MEASURED_KEYS = (
    ("integrated loudness", "lufs", " LUFS"),
    ("true peak", "true_peak_dbtp", " dBTP"),
    ("loudness range", "lra_lu", " LU"),
    ("short-term max loudness", "short_term_max_lufs", " LUFS"),
    ("noise floor (10th percentile of 20 ms levels)", "noise_floor_dbfs", " dBFS"),
    ("clipped samples", "clipped_samples", ""),
)


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
