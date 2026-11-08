"""hark as an MCP server over stdio: listening, review and bank tools for a coding agent.

The MCP SDK is imported only when the server starts. Every backing module is imported inside its tool
call, so a tool whose unit is not installed returns a tool error and the rest keep working.
"""

from __future__ import annotations

import argparse
import contextlib
import importlib
import importlib.util
import inspect
import io
import json
import math
import os
import re
import sys
import threading
import time
import types
from dataclasses import dataclass
from typing import Any, Callable

from hark.surfaces._flow import attempt, raise_if

SERVER_NAME = "hark"
COMPACT_LIMIT = 7000
BODY_LIMIT = 24000
LIST_CAPS = (24, 12, 6, 3, 1)
STRING_CAP = 400
IMAGE_EDGE = 1600
IMAGE_QUALITY = 88
SILENT_DBFS = -70.0
VIDEO_EXTS = (".mp4", ".mov", ".m4v", ".webm", ".mkv", ".avi")
DETAILS = ("low", "normal", "high")
FIND_KINDS = ("auto", "event", "texture")
CUE_TEXT_START = set("[{0123456789.-")
REGISTER_HINT = "claude mcp add hark --scope user -e HARK_THREADS=3 -- hark mcp"

_LOCK = threading.RLock()


class ToolError(Exception):
    """A tool failure with one readable message; the client sees an MCP error result."""


@dataclass(frozen=True)
class Tool:
    name: str
    module: str
    func: str
    read_only: bool
    network: bool


TOOLS: tuple[Tool, ...] = (
    Tool("measure", "hark.core.features", "measure", True, False),
    Tool("describe", "hark.listen.describe", "describe", True, False),
    Tool("match", "hark.listen.match", "match", True, False),
    Tool("rank", "hark.listen.search", "rank", True, False),
    Tool("find", "hark.listen.search", "find", True, False),
    Tool("compare", "hark.listen.compare", "compare", True, False),
    Tool("ab", "hark.listen.compare", "ab", True, False),
    Tool("realness", "hark.listen.realness", "realness", True, False),
    Tool("timeline", "hark.timeline.timeline", "timeline", True, False),
    Tool("sheet", "hark.timeline.sheet", "sheet", False, False),
    Tool("critique", "hark.timeline.critique", "critique", True, False),
    Tool("doctor", "hark.doctor", "check", True, False),
    Tool("llm_critique", "hark.surfaces.llm", "critique", False, True),
    Tool("sfx_list", "hark.forge.bank.sfx", "sfx_list", True, False),
    Tool("sfx_render", "hark.forge.bank.sfx", "sfx_render", False, False),
    Tool("kit_render", "hark.forge.bank.kit", "render", False, False),
)
TOOL_BY_NAME = {t.name: t for t in TOOLS}

DESCRIPTIONS: dict[str, str] = {
    "measure": """Plain meter readings of 1 audio or video file, with no models, so it is quick. It reports length,
        integrated LUFS, true peak in dBTP, loudness range, noise floor, the count of clipped samples, the share of
        exact digital zeros, attack and decay times, brightness, bandwidth, band energies and onsets; music=true
        also estimates tempo, beat times and key. The values are measurements and can be trusted. Use it for
        loudness, peak, clipping and silence checks, and to compare levels before and after a mix edit.""",
    "describe": """What a sound is: calibrated tags grouped by category (for example source, material, action and
        character), the top AudioSet classes, the main measurements and a short paragraph. Use it first on an
        unknown file, or to confirm that a downloaded asset matches its title. It needs the CLAP and AudioSet
        models, so the first call is slow (20 to 60 s); later calls are quick.""",
    "match": """How well 1 sound fits 1 or more text descriptions. It returns z with its verdict band (strong at
        z >= 3, plausible at z >= 2.5, weak at z >= 2, else no match), the percentile among the vocabulary,
        p_vs_alts (a forced choice among the alternatives you pass, not calibrated), the raw cosine (not a
        probability) and the nearest vocabulary terms. Judge by z and the verdict. Pass believable alternatives.
        The first call loads CLAP (20 to 60 s).""",
    "rank": """Orders candidate files by calibrated fit to a text description. Paths can be files, folders or
        globs. Next steps: find on the best file, then describe and realness to confirm it. Embeddings are
        cached, so a second query over the same files is quick; the first call loads CLAP (20 to 60 s).""",
    "find": """The best moments for a description inside a long recording, as ranked segments with start, end, z,
        verdict, kind, head_db, tail_db and cut, in seconds. Event segments begin 5 ms ahead of the attack and
        close after the decay. Texture windows snap their edges to the sound when the query names a one-shot.
        cut is 'head cut' or 'tail cut' when a segment edge falls inside a sound, and such segments rank lower.
        The first call loads CLAP (20 to 60 s).""",
    "compare": """Places 1 sound against a set of reference files: the closest references, a typicality z (>= -1
        belongs to the set, < -2 is an outlier) and the measured differences in plain words. The first call loads
        CLAP (20 to 60 s).""",
    "ab": """Says which of 2 sounds fits a text or a reference set better, and how they differ in measured terms
        (level, brightness, decay length). The same file twice with different trims is allowed. For files with
        several takes, pass trims: a whole-file comparison includes every take and the gaps. The first call loads
        CLAP (20 to 60 s).""",
    "realness": """Real recording or professionally produced audio versus simple code-synthesized sound. It returns
        p_real (a probability), a verdict and the evidence in words; a file longer than 6 s also gets verdicts per
        window, and a folder or glob gives 1 row per file. The target is simple procedural audio, so electronic
        music and professional sound design count as real. Synthetic sound can be the goal: report the verdict,
        and call it a fault only when real sound is wanted. The result is strong evidence, not proof.""",
    "timeline": """A transcript of a whole soundtrack or film: events with time, level, tags and AudioSet classes,
        the loudness curve, picture cuts when there is video, and sync statistics against cuts and cues. A video
        file supplies its own picture unless video names another file. It loads the models on first use (20 to
        60 s); long films take longer.""",
    "sheet": """Draws 1 annotated picture of a soundtrack on a shared time axis: video frames, sections, labelled
        events, a spectrogram, the waveform and loudness, beats, and cue markers whose colour shows sync (green
        inside 1 frame, amber inside 100 ms, red for no hit); cuts are dashed. start and end zoom in. It writes a
        PNG and returns it as an image; use timeline to confirm details. It loads the models on first use.""",
    "critique": """Scores every soundtrack metric (for example loudness, true peak, dynamics, spectral balance,
        event density, sync and realness) against the range of professional launch films. Each metric has a
        status (ok, low, high), a weight, the reason it matters and timed actions, plus 1 overall verdict:
        launch-grade, close, needs work or not launch-grade. Run it before delivery and after each fix round.
        fast=true skips the AudioSet and realness passes.""",
    "doctor": """Checks the hark install: packages, ffmpeg, both models (pinned revision, cache state, offline
        flag), CPU threads and whether the LLM provider (agy) is ready. Run it after a tool failure, or once
        before first use. load=true also loads both models and times 1 window.""",
    "llm_critique": """An optional verbal review by an audio language model (through the agy CLI) that covers sync,
        realism, balance, loudness, cheap or synthetic moments and timed fixes. The audio leaves the machine, so
        set consent=true only after the user said yes. Without a ready provider or without consent nothing is
        sent, and the result explains how to enable it. It is opinion with coarse timing: confirm each fix with
        timeline or ab. A call can take several minutes.""",
    "sfx_list": """Lists the sounds of hark's shipped bank, with length, peak time, kinds and the knobs each sound
        takes (dur, apex, pitch). No models; instant.""",
    "sfx_render": """Writes 1 bank sound as a 24-bit WAV in about 1 s. The length equals dur; apex sets the time of
        the loudest point of a whoosh; pitch changes the recipe's own pitch parameter and never resamples. The
        result has the measured onset, peak and end and the model-free audit.""",
    "kit_render": """Renders a film's sound kit: the takes of every role and a lock with the measured onset, peak,
        end and loudness of each take. The cache key is the sound, seed, edits and bank version, so an unchanged
        kit renders nothing. A take that fails the audit gets a new seed and renders again. No models.""",
}

INSTRUCTIONS = """You cannot hear, so use these hark tools whenever you must judge, choose or check audio: effects,
music, ambience, mixes, or the soundtrack of a video. Pass absolute paths.

Typical chains:
- choosing an asset: rank across the candidates, find inside the winner, describe to confirm, and realness too
  when real or professionally produced sound is wanted;
- reviewing a mix or a film: timeline, sheet, critique;
- checking an edit: ab between the old and the new render, then timeline on the new render;
- plain meters without models: measure;
- llm_critique is optional, uploads the audio and needs the user's explicit yes.

Reading the numbers: z is computed against a few hundred vocabulary phrases on the same sound; strong is
z >= 3, plausible z >= 2.5, weak z >= 2, else no match. p_real and AudioSet scores are probabilities. p_vs_alts
is a forced choice, uncalibrated and only as useful as the alternatives. Cosine is raw CLAP similarity and is
comparable only for 1 sound and 1 query. Trust measurements most, then verdicts and z, and cosine least.

When realness cannot run, report that real versus synthetic is unchecked; the synth classes of describe are
weaker evidence.

The first call that loads a model takes 20 to 60 s; later calls are quick.

Film sounds without models: sfx_list for the bank, then sfx_render for 1 take aimed at a cue (dur, apex,
pitch), or kit_render for a whole kit (takes plus a lock with the measured onset, peak and end)."""


def description(name: str) -> str:
    """The tool description with whitespace collapsed to single spaces."""
    return " ".join(DESCRIPTIONS[name].split())


def _first_sentence(text: str) -> str:
    head = re.split(r"(?<=\.)\s", text, maxsplit=1)[0]
    return head if head.endswith(".") else head + "."


def tool_list() -> dict[str, Any]:
    """The 16 tools as ``{summary, tools, text}`` (``hark mcp --tools``)."""
    rows = [{"name": t.name, "read_only": t.read_only, "network": t.network, "description": description(t.name)}
            for t in TOOLS]
    head = f"hark MCP server: {len(rows)} tools. Register it with: {REGISTER_HINT}"
    lines = [head] + [f"{r['name']:<13}{_first_sentence(r['description'])}" for r in rows]
    return {"summary": head, "tools": rows, "text": "\n".join(lines)}


# ---------------------------------------------------------------- JSON shaping


def clean(obj: Any) -> Any:
    """JSON-safe copy: string keys, lists for tuples, native numbers, null for NaN and infinity."""
    if isinstance(obj, dict):
        return {str(k): clean(v) for k, v in obj.items()}
    if isinstance(obj, (list, tuple, set)):
        return [clean(v) for v in obj]
    return _clean_scalar(obj)


def _native(obj: Any) -> Any:
    finite = not (isinstance(obj, float) and not math.isfinite(obj))
    plain = obj is None or isinstance(obj, (bool, int, float, str))
    return (obj if plain else str(obj)) if finite else None


def _clean_scalar(obj: Any) -> Any:
    array = hasattr(obj, "tolist") and not isinstance(obj, (str, bytes))
    return clean(obj.tolist()) if array else _native(obj)


def _shrink_str(v: str, cap: int, top: bool) -> str:
    return v[:STRING_CAP] + " ..." if len(v) > STRING_CAP else v


def _shrink_list(v: list, cap: int, top: bool) -> list:
    out = [_shrink(x, cap) for x in v[:cap]]
    return out + [f"... +{len(v) - cap} more"] if len(v) > cap else out


def _shrink_dict(v: dict, cap: int, top: bool) -> dict:
    return {k: _shrink(x, cap) for k, x in v.items() if not (top and k in ("summary", "text"))}


_SHRINKERS: tuple[tuple[type, Callable[[Any, int, bool], Any]], ...] = (
    (float, lambda v, cap, top: round(v, 3)), (str, _shrink_str), (list, _shrink_list), (dict, _shrink_dict))


def _shrink(v: Any, cap: int, top: bool = False) -> Any:
    fn = next((f for kind, f in _SHRINKERS if isinstance(v, kind)), None)
    return fn(v, cap, top) if fn else v


def _dump(v: Any) -> str:
    return json.dumps(v, separators=(",", ":"), ensure_ascii=False)


def _drop_fields(value: dict) -> str:
    order = sorted(value, key=lambda k: len(_dump(value[k])), reverse=True)
    rest, cut, out = dict(value), [], ""
    for key in order:
        rest.pop(key)
        cut.append(key)
        out = _dump(dict(rest, cut_fields=cut, note="fields were left out; call again with full=true"))
        if len(out) <= COMPACT_LIMIT:
            return out
    return out


def compact_json(result: Any) -> str:
    """The compact JSON view of a result, at most 7000 characters when it can be."""
    value = clean(result)
    small: Any = value
    for cap in LIST_CAPS:
        small = _shrink(value, cap, top=True)
        out = _dump(small)
        if len(out) <= COMPACT_LIMIT:
            return out
    if not isinstance(small, dict):
        return _dump({"cut": "the result does not fit the compact view; full=true returns it"})
    return _drop_fields(small)


def _head_body(res: dict, full: bool) -> tuple[str, str]:
    head, body = str(res.get("summary") or "").strip(), str(res.get("text") or "").strip()
    cut = not full and len(body) > BODY_LIMIT
    body = body[:BODY_LIMIT] + f"\n(text cut at {BODY_LIMIT} characters; full=true gives all of it)" if cut else body
    return ("" if head and body.startswith(head) else head), body


def _json_part(res: dict, full: bool) -> str:
    return ("JSON: " + json.dumps(clean(res), ensure_ascii=False)) if full else "JSON (compact): " + compact_json(res)


def format_result(result: Any, full: bool = False, ignored: list[str] | None = None) -> str:
    """Tool text: summary, body, ignored-arguments note and the JSON."""
    res = result if isinstance(result, dict) else {"summary": str(result)}
    head, body = _head_body(res, full)
    note = f"(this hark version ignored: {', '.join(ignored)})" if ignored else ""
    return "\n\n".join(p for p in (head, body, note, _json_part(res, full)) if p)


# ---------------------------------------------------------------- measure summary


def _silence_sentence(m: dict, d: str) -> str:
    kind = "digitally silent" if (m.get("digital_silence_frac") or 0) >= 0.999 else "near-silent"
    return (f"{d} s, {kind} (peak {m['peak_dbfs']:.0f} dBFS). Loudness, spectrum and envelope values of "
            "silence mean nothing.")


def _loud_part(m: dict) -> str:
    if m.get("lufs") is None:
        return f"RMS {m.get('rms_dbfs')} dBFS (too short for LUFS)"
    part = f"{m['lufs']} LUFS integrated"
    return part + (f" (short-term max {m['short_term_max_lufs']})" if m.get("short_term_max_lufs") is not None else "")


def _peak_part(m: dict) -> str:
    part = f"true peak {m.get('true_peak_dbtp')} dBTP"
    return part + (f", {m['clipped_samples']} clipped samples" if (m.get("clipped_samples") or 0) > 0 else "")


def _khz(m: dict, key: str, label: str) -> list[str]:
    return [f"{label} {m[key] / 1000:.1f} kHz"] if m.get(key) else []


def _env_parts(m: dict) -> list[str]:
    parts = []
    if m.get("attack_ms") is not None:
        decay = f", decay {m['decay_ms']} ms" if m.get("decay_ms") is not None else ""
        parts.append(f"attack {m['attack_ms']} ms{decay}")
    if m.get("onset_count") is not None:
        parts.append(f"{m['onset_count']} onsets")
    if m.get("tempo") is not None:
        key = (m.get("key") or [{}])[0].get("key", "?") if m.get("key") else "?"
        parts.append(f"tempo ~{m['tempo']} BPM, key {key}")
    return parts


def _level_parts(m: dict, d: str) -> list[str]:
    parts = [f"{d} s", _loud_part(m), _peak_part(m)]
    if m.get("lra_lu") is not None:
        parts.append(f"LRA {m['lra_lu']} LU")
    if m.get("noise_floor_dbfs") is not None:
        parts.append(f"noise floor {m['noise_floor_dbfs']} dBFS")
    return parts + _khz(m, "brightness_hz", "brightness") + _khz(m, "bandwidth_hz", "bandwidth")


def measure_summary(m: dict) -> str:
    """The 1-sentence plain summary of a measurement dict."""
    d = f"{float(m.get('duration') or 0):.3f}"
    if m.get("error"):
        return f"{d} s: {m['error']}."
    peak = m.get("peak_dbfs")
    if peak is not None and peak < SILENT_DBFS:
        return _silence_sentence(m, d)
    text = ", ".join(_level_parts(m, d) + _env_parts(m)) + "."
    zeros = m.get("digital_silence_frac") or 0
    if zeros > 0.01:
        text += (f" {zeros * 100:.1f}% exact digital zeros, usually edit, fade or gate padding; weak evidence "
                 "alone, so use realness for real versus synthesized.")
    return text


# ---------------------------------------------------------------- argument checks


def _cwd_note() -> str:
    return f"pass an absolute path (the server's working folder is {os.getcwd()})"


def _norm(p: str) -> str:
    return os.path.abspath(os.path.expanduser(p.strip().strip('"')))


def file_arg(value: Any, what: str = "path") -> str:
    """An existing file path, normalized; ToolError otherwise."""
    raise_if(not isinstance(value, str) or not value.strip().strip('"'), ToolError(f"{what}: a file path is required"))
    path = _norm(value)
    raise_if(os.path.isdir(path), ToolError(f"{what}: {path} is a folder; a file is expected"))
    raise_if(not os.path.exists(path), ToolError(f"{what}: no such file {path}; {_cwd_note()}"))
    return path


def optional_file(value: Any, what: str) -> str | None:
    return None if value in (None, "") else file_arg(value, what)


def out_arg(value: Any, what: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ToolError(f"{what}: an output path is required")
    return _norm(value)


def _media_of(item: str) -> list[str]:
    from hark.core import audio
    try:
        return list(audio.list_media([item]))
    except Exception:
        return []


def paths_arg(value: Any, what: str) -> list[str]:
    """Files, folders and globs to 1 media list; every item must yield media."""
    items = [value] if isinstance(value, str) else list(value or [])
    if not items:
        raise ToolError(f"{what}: give at least 1 file, folder or glob")
    found, empty = [], []
    for raw in items:
        item = _norm(str(raw))
        media = _media_of(item)
        (found.extend(media) if media else empty.append(item))
    if empty:
        raise ToolError(f"{what}: no media files in {', '.join(empty[:5])}; {_cwd_note()}")
    return found


def choice_arg(value: str, allowed: tuple[str, ...], what: str) -> str:
    if value not in allowed:
        raise ToolError(f"{what} must be one of {', '.join(allowed)} (got {value!r})")
    return value


def cues_arg(value: Any) -> Any:
    if value in (None, "", []):
        return None
    if isinstance(value, str) and value.strip()[:1] not in CUE_TEXT_START:
        return file_arg(value, "cues")
    return value


def video_default(path: str, video: Any) -> str | None:
    if video not in (None, ""):
        return file_arg(video, "video")
    return path if os.path.splitext(path)[1].lower() in VIDEO_EXTS else None


def range_arg(value: Any, what: str, lo: float, hi: float | None = None) -> Any:
    if value is None:
        return None
    if value < lo or (hi is not None and value > hi):
        raise ToolError(f"{what} must be in {lo}..{hi if hi is not None else 'inf'} (got {value})")
    return value


# ---------------------------------------------------------------- backing calls


def _module_present(name: str) -> bool:
    if name in sys.modules:
        return True
    try:
        return importlib.util.find_spec(name) is not None
    except (ImportError, ValueError):
        return False


def present_tools() -> list[str]:
    return [t.name for t in TOOLS if _module_present(t.module)]


def _absent(exc: ModuleNotFoundError, module: str) -> bool:
    name = exc.name or ""
    return bool(name) and (module == name or module.startswith(name + "."))


def backing(tool: str, func: str | None = None) -> Callable:
    """The backing function of a tool; ToolError when its module is absent or broken."""
    t = TOOL_BY_NAME[tool]
    try:
        mod = importlib.import_module(t.module)
    except ModuleNotFoundError as exc:
        if not _absent(exc, t.module):
            raise ToolError(_broken(t, exc)) from None
        raise ToolError(f"{tool} is not part of this install (module {t.module} is missing); "
                        f"present tools: {', '.join(present_tools()) or 'none'}") from None
    except Exception as exc:
        raise ToolError(_broken(t, exc)) from None
    fn = getattr(mod, func or t.func, None)
    if fn is None:
        raise ToolError(f"{tool} is not part of this install ({t.module}.{func or t.func} is missing)")
    return fn


def _broken(t: Tool, exc: BaseException) -> str:
    return (f"{t.name}: module {t.module} failed to load ({type(exc).__name__}: {exc}); retry if hark is "
            "being updated, else run hark doctor")


def filter_kwargs(fn: Callable, kwargs: dict[str, Any]) -> tuple[dict[str, Any], list[str]]:
    """Drop null values, then keep only the names that ``fn`` accepts; return the ignored names."""
    given = {k: v for k, v in kwargs.items() if v is not None}
    params = _params_of(fn)
    names = given if params is None else params
    return {k: v for k, v in given.items() if k in names}, [k for k in given if k not in names]


def _params_of(fn: Callable) -> dict | None:
    """The named parameters of fn, or None when it takes any keyword (or has no signature)."""
    params = attempt(lambda: inspect.signature(fn).parameters)
    open_kw = params is None or any(p.kind is p.VAR_KEYWORD for p in params.values())
    return None if open_kw else dict(params)


def _ml_message(exc: BaseException) -> str:
    feature = getattr(exc, "feature", "this tool")
    command = getattr(exc, "command", "")
    return f"MLMissing: {feature} needs the ml extras. Install them with: {command}"


def call_backing(tool: str, fn: Callable, kwargs: dict[str, Any]) -> tuple[Any, list[str]]:
    """Run a backing function under the lock with stdout sent to stderr."""
    args, ignored = filter_kwargs(fn, kwargs)
    with _LOCK, contextlib.redirect_stdout(sys.stderr):
        try:
            return fn(**args), ignored
        except ToolError:
            raise
        except Exception as exc:
            if type(exc).__name__ == "MLMissing":
                raise ToolError(f"{tool} failed: {_ml_message(exc)}") from None
            raise ToolError(f"{tool} failed: {type(exc).__name__}: {exc}") from None


@dataclass
class ToolResult:
    text: str
    image: bytes | None = None


def _run(tool: str, kwargs: dict[str, Any], full: bool) -> ToolResult:
    res, ignored = call_backing(tool, backing(tool), kwargs)
    return ToolResult(format_result(res, full, ignored))


# ---------------------------------------------------------------- per-tool logic


def t_measure(path: str, start: float | None = None, end: float | None = None, music: bool = False,
              light: bool = False, full: bool = False) -> ToolResult:
    p = file_arg(path)
    res, ignored = call_backing("measure", backing("measure"),
                               {"src": p, "music": music, "start": start, "end": end, "light": light})
    res = dict(res) if isinstance(res, dict) else {"summary": str(res)}
    res["summary"] = measure_summary(res)
    return ToolResult(format_result(res, full, ignored))


def t_describe(path: str, start: float | None = None, end: float | None = None, top: int = 8,
               full: bool = False) -> ToolResult:
    kw = {"path": file_arg(path), "start": start, "end": end, "top": range_arg(top, "top", 1, 30)}
    return _run("describe", kw, full)


def t_match(path: str, query: Any, alts: list[str] | None = None, start: float | None = None,
            end: float | None = None, full: bool = False) -> ToolResult:
    return _run("match", {"path": file_arg(path), "query": query, "alts": alts or None,
                          "start": start, "end": end}, full)


def t_rank(paths: Any, query: str, top: int | None = 20, full: bool = False) -> ToolResult:
    kw = {"paths": paths_arg(paths, "paths"), "query": query, "top": range_arg(top, "top", 1)}
    return _run("rank", kw, full)


def t_find(path: str, query: str, top: int = 5, kind: str = "auto", win: float | None = None,
           hop: float | None = None, start: float | None = None, end: float | None = None,
           full: bool = False) -> ToolResult:
    kw = {"path": file_arg(path), "query": query, "top": range_arg(top, "top", 1, 50),
          "kind": choice_arg(kind, FIND_KINDS, "kind"), "win": win, "hop": hop, "start": start, "end": end}
    return _run("find", kw, full)


def t_compare(path: str, refs: Any, k: int = 5, start: float | None = None, end: float | None = None,
              full: bool = False) -> ToolResult:
    kw = {"path": file_arg(path), "refs": paths_arg(refs, "refs"), "k": range_arg(k, "k", 1, 50),
          "start": start, "end": end}
    return _run("compare", kw, full)


def t_ab(a: str, b: str, query: str | None = None, refs: Any = None, a_start: float | None = None,
         a_end: float | None = None, b_start: float | None = None, b_end: float | None = None,
         full: bool = False) -> ToolResult:
    kw = {"a": file_arg(a, "a"), "b": file_arg(b, "b"), "query": query,
          "refs": paths_arg(refs, "refs") if refs not in (None, "", []) else None,
          "a_start": range_arg(a_start, "a_start", 0), "a_end": a_end,
          "b_start": range_arg(b_start, "b_start", 0), "b_end": b_end}
    return _run("ab", kw, full)


def _realness_row(r: dict) -> str:
    p = r.get("p_real")
    cell = f"{p:6.2f}" if isinstance(p, (int, float)) else f"{'-':>6}"
    name = os.path.basename(str(r.get("path") or r.get("file") or ""))
    return f"{cell}  {str(r.get('verdict') or '-'):<24} {name}: {r.get('why') or r.get('summary') or ''}"


def _realness_text(res: dict) -> str:
    rows = [_realness_row(r) for r in res.get("files") or res.get("results") or []]
    return "\n".join([str(res.get("summary") or ""), "p_real  verdict                  file: main reason"] + rows)


def _realness_ready() -> None:
    mod = sys.modules.get("hark.listen.realness") or importlib.import_module("hark.listen.realness")
    model = getattr(mod, "MODEL_PATH", None)
    if model is not None and not os.path.exists(str(model)):
        raise ToolError("realness failed: the realness model file is missing, so real versus synthesized was "
                        "NOT checked; tell the user so. describe (synth-type AudioSet classes) and measure are "
                        "weaker evidence.")


def _realness_many(p: str, start: Any, end: Any, ast: bool, full: bool) -> ToolResult:
    if not os.path.isdir(p) and not any(c in p for c in "*?["):
        file_arg(p)
    paths = paths_arg(p, "path")
    try:
        fn = backing("realness", "realness_many")
    except ToolError:
        raise ToolError("realness: this hark version judges 1 file per call; pass a file") from None
    res, ignored = call_backing("realness", fn, {"paths": paths, "ast": ast or None, "start": start, "end": end})
    res = dict(res) if isinstance(res, dict) else {"summary": str(res)}
    res["text"] = _realness_text(res)
    return ToolResult(format_result(res, full, ignored))


def t_realness(path: str, start: float | None = None, end: float | None = None, ast: bool = False,
               full: bool = False) -> ToolResult:
    backing("realness")
    _realness_ready()
    if not isinstance(path, str) or not path.strip():
        raise ToolError("path: a file path is required")
    p = _norm(path)
    if os.path.isfile(p):
        return _run("realness", {"path": p, "start": start, "end": end, "ast": ast or None}, full)
    return _realness_many(p, start, end, ast, full)


def _timeline_kw(path: str, video: Any, cues: Any, start: Any, end: Any, detail: str) -> dict[str, Any]:
    p = file_arg(path)
    return {"path": p, "video": video_default(p, video), "cues": cues_arg(cues), "start": start, "end": end,
            "detail": choice_arg(detail, DETAILS, "detail")}


def t_timeline(path: str, video: str | None = None, cues: Any = None, start: float | None = None,
               end: float | None = None, detail: str = "normal", full: bool = False) -> ToolResult:
    return _run("timeline", _timeline_kw(path, video, cues, start, end, detail), full)


def _home() -> str:
    env = os.environ.get("HARK_HOME")
    if env:
        return os.path.abspath(os.path.expanduser(env))
    from hark.core import config
    return str(config.HOME)


def _short(v: float) -> str:
    return f"{v:g}"


def sheet_path(path: str, start: float | None, end: float | None) -> str:
    """Default PNG path of a sheet: <HARK_HOME>/sheets/<stem>_<id8>[<span>].png."""
    from hark.core import audio
    span = "" if start is None and end is None else f"_{_short(start or 0)}-{_short(end) if end is not None else 'end'}"
    stem = os.path.splitext(os.path.basename(path))[0]
    folder = os.path.join(_home(), "sheets")
    os.makedirs(folder, exist_ok=True)
    return os.path.join(folder, f"{stem}_{audio.file_id(path)[:8]}{span}.png")


def jpeg_of(png: str) -> bytes:
    """The PNG as a JPEG with the long edge at most 1600 px."""
    from PIL import Image
    with Image.open(png) as im:
        im = im.convert("RGB")
        im.thumbnail((IMAGE_EDGE, IMAGE_EDGE))
        buf = io.BytesIO()
        im.save(buf, "JPEG", quality=IMAGE_QUALITY, optimize=True)
    return buf.getvalue()


def t_sheet(path: str, video: str | None = None, cues: Any = None, start: float | None = None,
            end: float | None = None, detail: str = "normal", out_png: str | None = None,
            include_image: bool = True, full: bool = False) -> ToolResult:
    kw = _timeline_kw(path, video, cues, start, end, detail)
    kw["out_png"] = out_arg(out_png, "out_png") if out_png else sheet_path(kw["path"], start, end)
    res, ignored = call_backing("sheet", backing("sheet"), kw)
    body = format_result(res, full, ignored)
    png = (res.get("png") if isinstance(res, dict) else None) or kw["out_png"]
    if not os.path.isfile(png):
        return ToolResult(f"{body}\n(no PNG found at {png})")
    image = jpeg_of(png) if include_image else None
    return ToolResult(f"PNG: {png}\n\n{body}", image)


def t_critique(path: str, video: str | None = None, profile: str | None = None, fast: bool = False,
               full: bool = False) -> ToolResult:
    p = file_arg(path)
    kw = {"path": p, "video": video_default(p, video), "profile": optional_file(profile, "profile"),
          "use_ast": False if fast else None, "use_realness": False if fast else None}
    return _run("critique", kw, full)


def t_doctor(load: bool = False, full: bool = False) -> ToolResult:
    return _run("doctor", {"load": load}, full)


def t_llm_critique(path: str, consent: bool = False, prompt: str | None = None, video: str | None = None,
                   with_timeline: bool = False, model: str | None = None, synth_intended: bool = False,
                   start: float | None = None, end: float | None = None, full: bool = False) -> ToolResult:
    kw = {"path": file_arg(path), "consent": consent, "prompt": prompt, "video": optional_file(video, "video"),
          "with_timeline": with_timeline, "model": model, "start": start, "end": end,
          "synth_intended": synth_intended}
    return _run("llm_critique", kw, full)


def _knob_extra(knobs: dict) -> str:
    pitch = knobs.get("pitch")
    note = pitch.get("note") if isinstance(pitch, dict) else None
    return (", apex" if knobs.get("apex") else "") + (f", pitch {note}" if note else "")


def _sfx_line(s: dict) -> str:
    knobs = s.get("knobs") or {}
    extra = _knob_extra(knobs)
    kinds = ", ".join(s.get("kinds") or []) or "-"
    return (f"{s.get('id')}: {float(s.get('dur_s') or 0):.3f} s, peak {float(s.get('peak_s') or 0):.3f} s; "
            f"{kinds}; dur {knobs.get('dur')}{extra}")


def t_sfx_list(category: str | None = None, kind: str | None = None, full: bool = False) -> ToolResult:
    res, ignored = call_backing("sfx_list", backing("sfx_list"), {"category": category, "kind": kind})
    res = dict(res) if isinstance(res, dict) else {"summary": str(res)}
    res["text"] = "\n".join([str(res.get("summary") or "")] + [_sfx_line(s) for s in res.get("sounds") or []])
    return ToolResult(format_result(res, full, ignored))


def t_sfx_render(sound: str, out: str, seed: int | None = None, dur: float | None = None,
                 apex: float | None = None, pitch: str | None = None, peak_db: float = -1.0,
                 full: bool = False) -> ToolResult:
    if peak_db is not None and peak_db > 0:
        raise ToolError("peak_db must be <= 0")
    kw = {"sound": sound, "out": out_arg(out, "out"), "seed": seed, "dur": range_arg(dur, "dur", 1e-9),
          "apex": range_arg(apex, "apex", 1e-9), "pitch": pitch, "peak_db": peak_db}
    return _run("sfx_render", kw, full)


def t_kit_render(kit: str, out: str, force: bool = False, full: bool = False) -> ToolResult:
    return _run("kit_render", {"kit": file_arg(kit, "kit"), "out": out_arg(out, "out"), "force": force}, full)


HANDLERS: dict[str, Callable[..., ToolResult]] = {
    "measure": t_measure, "describe": t_describe, "match": t_match, "rank": t_rank, "find": t_find,
    "compare": t_compare, "ab": t_ab, "realness": t_realness, "timeline": t_timeline, "sheet": t_sheet,
    "critique": t_critique, "doctor": t_doctor, "llm_critique": t_llm_critique, "sfx_list": t_sfx_list,
    "sfx_render": t_sfx_render, "kit_render": t_kit_render,
}


def call_tool(name: str, **kwargs: Any) -> ToolResult:
    """Run 1 tool in process (the server and the tests use this)."""
    if name not in HANDLERS:
        raise ToolError(f"unknown tool {name!r}")
    if name == "llm_critique":
        return _llm_call(kwargs)
    return HANDLERS[name](**kwargs)


def _llm_call(kwargs: dict[str, Any]) -> ToolResult:
    try:
        return HANDLERS["llm_critique"](**kwargs)
    except ToolError as exc:
        msg = str(exc)
        raise ToolError(msg.replace("llm_critique failed: AgyCannotHear: ", "")) from None


# ---------------------------------------------------------------- server


def hold_threads(threads: int | None = None) -> int:
    """Set the BLAS thread count before numpy loads, unless the user set it."""
    if threads:
        os.environ["HARK_THREADS"] = str(threads)
    n = _thread_count()
    os.environ.setdefault("OPENBLAS_NUM_THREADS", str(n))
    return n


def _thread_count() -> int:
    raw = os.environ.get("HARK_THREADS")
    if raw:
        try:
            return max(1, int(raw))
        except ValueError:
            pass
    return max(1, min(4, (os.cpu_count() or 2) // 2))


def _sdk_tool(name: str) -> Callable:
    handler = HANDLERS[name]

    def run(**kwargs: Any) -> Any:
        res = call_tool(name, **kwargs)
        if res.image is None:
            return res.text
        return [res.text, _sdk().Image(data=res.image, format="jpeg")]

    run.__name__ = name
    run.__doc__ = description(name)
    run.__signature__ = _public_signature(handler)  # type: ignore[attr-defined]
    run.__annotations__ = dict(getattr(handler, "__annotations__", {}))
    return run


def _public_signature(fn: Callable) -> inspect.Signature:
    sig = inspect.signature(fn)
    return sig.replace(return_annotation=inspect.Signature.empty)


def _annotations(t: Tool) -> Any:
    from mcp.types import ToolAnnotations
    return ToolAnnotations(title=f"hark {t.name}", readOnlyHint=t.read_only, destructiveHint=False,
                           openWorldHint=t.network)


def _register(server: Any, t: Tool) -> None:
    fn = _sdk_tool(t.name)
    base = {"name": t.name, "description": description(t.name), "annotations": _annotations(t)}
    try:
        server.add_tool(fn, structured_output=False, **base)
    except TypeError:
        server.add_tool(fn, **base)


def _version() -> str:
    import hark
    return hark.__version__


def _sdk() -> Any:
    """The server module of the MCP SDK: ``mcpserver`` (mcp 2.x), else ``fastmcp`` (mcp 1.x)."""
    name = "mcp.server.mcpserver" if importlib.util.find_spec("mcp.server.mcpserver") else "mcp.server.fastmcp"
    mod = importlib.import_module(name)
    return types.SimpleNamespace(Server=getattr(mod, "MCPServer", None) or mod.FastMCP, Image=mod.Image)


def _new_server() -> Any:
    server_cls = _sdk().Server
    try:
        return server_cls(SERVER_NAME, instructions=INSTRUCTIONS, log_level="WARNING", version=_version())
    except TypeError:
        server = server_cls(SERVER_NAME, instructions=INSTRUCTIONS, log_level="WARNING")
        low = getattr(server, "_mcp_server", None)
        if low is not None and hasattr(low, "version"):
            low.version = _version()
        return server


def build_server(warm: bool = False) -> Any:
    """The FastMCP server with the 16 tools; ImportError when the mcp extra is missing."""
    server = _new_server()
    for t in TOOLS:
        _register(server, t)
    if warm:
        threading.Thread(target=warm_up, name="hark-warm", daemon=True).start()
    return server


def warm_up() -> None:
    """Load both models, run 1 measurement and load the vocabulary; report on stderr."""
    t0 = time.monotonic()
    with _LOCK:
        try:
            _warm_steps()
            print(f"models warm ({time.monotonic() - t0:.1f} s)", file=sys.stderr)
        except Exception as exc:
            print(f"warm-up failed: {type(exc).__name__}: {exc}", file=sys.stderr)


def _warm_steps() -> None:
    import numpy as np
    from hark.core import features
    from hark.models import ast, clap
    clap.get()
    ast.get()
    features.measure(0.1 * np.random.default_rng(0).standard_normal(48000).astype(np.float32))
    if _module_present("hark.listen.vocabulary"):
        importlib.import_module("hark.listen.vocabulary").get()


def _mcp_missing_line() -> str:
    try:
        from hark.models import install_command
        cmd = install_command("mcp")
    except Exception:
        cmd = f'{sys.executable} -m pip install "mcp>=2"'
    return f"hark mcp: error: MLMissing: the MCP server needs the mcp extras. Install them with: {cmd}"


def serve(warm: bool = False) -> int:
    """Run the server on stdio until the client closes the pipe."""
    n = hold_threads()
    if importlib.util.find_spec("mcp") is None:
        print(_mcp_missing_line(), file=sys.stderr)
        return 3
    server = build_server(warm=warm)
    print(f"hark {_version()} MCP server: {len(TOOLS)} tools on stdio, {n} threads", file=sys.stderr)
    server.run("stdio")
    return 0


# ---------------------------------------------------------------- CLI


def add_cli(sub: Any) -> None:
    """Register ``hark mcp``."""
    from hark.core import report
    p = sub.add_parser("mcp", help="Run hark as an MCP server over stdio.")
    p.add_argument("--warm", action="store_true", help="load the models in the background at start")
    p.add_argument("--tools", action="store_true", help="print the tool list and exit")
    report.add_common(p)
    p.set_defaults(func=handle)


def handle(args: argparse.Namespace) -> int:
    if args.tools:
        from hark.core import report
        res = tool_list()
        report.emit(res, args, text=res["text"])
        return 0
    if importlib.util.find_spec("mcp") is None:
        from hark.models import require_extra
        require_extra("mcp", "MCP server")
    return serve(warm=args.warm)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(prog="python -m hark.surfaces.mcp_server")
    p.add_argument("--warm", action="store_true")
    return serve(warm=p.parse_args(argv).warm)


if __name__ == "__main__":
    sys.exit(main())
