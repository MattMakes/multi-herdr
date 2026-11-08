"""An optional spoken-style review of a soundtrack by an audio language model, via agy.

hark never reads, sets or passes a vendor API key; agy works from the operator's
own sign-in. No audio leaves the machine without explicit consent
(``--yes-upload`` on the command line, ``consent=True`` in Python and MCP).

The provider client, the clip and prompt, and the reply parsing live in the
``_llm_*`` modules beside this one; this module holds the gates and the CLI.
"""

from __future__ import annotations

import argparse
import os
import time
from typing import Any

from hark.surfaces._llm_agy import (  # noqa: F401  (re-exported names)
    AGY_DEFAULT_MODEL,
    MODEL_ENV,
    PROVIDERS,
    AgyCannotHear,
    AgyProvider,
    ProviderError,
    child_env,
    get_provider,
    scrub,
)
from hark.surfaces._llm_answer import (  # noqa: F401  (re-exported names)
    _span_text,
    normalize,
    parse_json,
    parsed_summary,
    render,
    score_of,
    seconds_of,
)
from hark.surfaces._llm_brief import (  # noqa: F401  (re-exported names)
    _INTENT,
    CLIP_CODEC,
    MAX_SECONDS,
    build_prompt,
    context,
    encode,
    video_of,
)

AGY_VERDICT = "HEARS"  # the probe result recorded in docs/specs/hark/agy-audio-probe.md
RAW_CAP = 20000


def _disabled(path: str, prov: Any, model: str, summary: str, steps: list[str]) -> dict[str, Any]:
    numbered = "\n".join(f"  {n}. {step}" for n, step in enumerate(steps, 1))
    return {"enabled": False, "ok": False, "path": path, "provider": prov.name, "model": model,
            "summary": summary, "how_to_enable": steps, "text": f"{summary}\n{numbered}"}


def _failure(path: str, prov: Any, model: str, clip: dict | None, exc: BaseException) -> dict[str, Any]:
    plain = isinstance(exc, (ProviderError, ValueError))
    reason = scrub(str(exc) if plain else f"{type(exc).__name__}: {exc}")
    line = f"LLM critique failed: {reason}"
    return {"enabled": True, "ok": False, "path": path, "provider": prov.name, "model": model,
            "uploaded": clip, "summary": line, "text": line, "error": reason}


def _upload_info(clip: dict, reply: dict) -> dict:
    upload = dict(clip, method=reply.get("method"))
    if reply.get("files_api_deleted") is not None:
        upload["files_api_deleted"] = reply["files_api_deleted"]
    return upload


def _explain_unparsed(out: dict, prov: Any, clip: dict, reply: dict) -> None:
    out["raw"] = scrub(reply.get("text") or "")[:RAW_CAP]
    tail = " [Only the first {:g} s were sent; use --start / --end for other parts.]".format(
        clip["seconds"]) if clip.get("trimmed") else ""
    out["summary"] = (f"{prov.name} answered, but not in parseable JSON (finish reason "
                      f"{reply.get('finish_reason')}); the raw answer is attached." + tail)


def _result(path: str, prov: Any, model: str, clip: dict, reply: dict, ctx: str, notes: list[str],
            took: float) -> dict[str, Any]:
    found = parse_json(reply.get("text") or "")
    shown = reply.get("model_version") or model
    out: dict[str, Any] = {"enabled": True, "ok": found is not None, "path": path, "provider": prov.name,
                           "model": shown, "uploaded": _upload_info(clip, reply), "seconds": took,
                           "usage": reply.get("usage") or {}, "finish_reason": reply.get("finish_reason"),
                           "context_sent": ctx, "notes": notes}
    if found is None:
        _explain_unparsed(out, prov, clip, reply)
    else:
        out["critique"] = normalize(found, offset=clip["start"], clip_len=clip["seconds"])
        out["summary"] = parsed_summary(prov.name, shown, out["critique"], clip)
    out["text"] = render(out)
    return out


def _refuse_early(path: str, prov: Any) -> None:
    """AudioError for a missing file; AgyCannotHear when the probe says agy hears no audio."""
    if not os.path.isfile(path):
        from hark.core.audio import AudioError

        raise AudioError(f"no such file: {path}")
    if getattr(prov, "name", "") == "agy" and AGY_VERDICT != "HEARS":
        raise AgyCannotHear("agy cannot hear audio files (probe verdict "
                            f"{AGY_VERDICT}); the LLM critique is unavailable and nothing was sent.")


def _gate(path: str, prov: Any, chosen: str, consent: bool) -> dict[str, Any] | None:
    """The off result when the provider is not ready or consent is missing; None when the call may go."""
    if not prov.ready():
        return _disabled(path, prov, chosen,
                         f"The LLM critique is off: the {prov.name} provider is not ready. "
                         "The rest of hark works without it.", prov.how_to_enable())
    if consent:
        return None
    return _disabled(path, prov, chosen,
                     f"Nothing was uploaded. Sending {os.path.basename(path)} to {prov.name} ({chosen}) "
                     "needs the user's explicit consent.",
                     [f"Ask the user for permission to send this audio to {prov.name} for an AI review.",
                      "With permission, run again with --yes-upload (CLI) or consent=True (Python, MCP)."])


def _ask(path: str, prov: Any, chosen: str, opts: dict[str, Any]) -> dict[str, Any]:
    """Encode the clip, build the context and prompt, call the provider and shape the answer."""
    clip = None
    try:
        payload, mime, clip = encode(path, opts["start"], opts["end"], opts["max_seconds"])
        ctx, notes = context(path, clip, opts["video"], opts["with_timeline"], opts["cuts"])
        question = build_prompt(ctx, opts["prompt"], opts["synth_intended"])
        began = time.monotonic()
        reply = prov.generate(payload, mime, question, chosen)
        took = round(time.monotonic() - began, 1)
    except Exception as exc:
        return _failure(path, prov, chosen, clip, exc)
    return _result(path, prov, chosen, clip, reply, ctx, notes, took)


def critique(path: str, prompt: str | None = None, provider: Any = "agy", model: str | None = None,
             consent: bool = False, video: str | None = None, with_timeline: bool = False,
             start: float | None = None, end: float | None = None, max_seconds: float = MAX_SECONDS,
             cuts: bool = True, synth_intended: bool = False) -> dict[str, Any]:
    """Ask an audio LLM, through agy, for a verbal review of a soundtrack.

    Raises AgyCannotHear when the recorded probe says agy cannot hear audio files.
    """
    prov = get_provider(provider)
    path = str(path)
    _refuse_early(path, prov)
    chosen = prov.model(model)
    blocked = _gate(path, prov, chosen, consent)
    if blocked is not None:
        return blocked
    opts = {"prompt": prompt, "video": video, "with_timeline": with_timeline, "start": start, "end": end,
            "max_seconds": max_seconds, "cuts": cuts, "synth_intended": synth_intended}
    return _ask(path, prov, chosen, opts)


# command line ---------------------------------------------------------


def add_cli(sub: Any) -> None:
    """Add the ``hark llm`` subcommand."""
    from hark.core import report

    parser = sub.add_parser("llm", help="Optional audio-LLM critique through the agy CLI (needs --yes-upload).")
    parser.add_argument("file", help="soundtrack, stem or video; only the audio is sent")
    parser.add_argument("--prompt", default=None, help="an extra question for the model")
    parser.add_argument("--yes-upload", action="store_true", help="consent: this audio may leave the machine")
    parser.add_argument("--model", default=None,
                        help=f"model id (default ${MODEL_ENV}, else {AGY_DEFAULT_MODEL})")
    parser.add_argument("--video", default=None, help="the film, for picture-cut times (frames are never sent)")
    parser.add_argument("--with-timeline", action="store_true", help="also send hark's listening transcript")
    parser.add_argument("--no-cuts", action="store_true", help="do not compute picture cuts")
    parser.add_argument("--start", type=float, default=None, help="send from this second")
    parser.add_argument("--end", type=float, default=None, help="send up to this second")
    parser.add_argument("--max-seconds", type=float, default=MAX_SECONDS, help="trim longer audio to this length")
    parser.add_argument("--synth-intended", action="store_true", help="synthesized sound is deliberate here")
    report.add_common(parser)
    parser.set_defaults(func=handle)


def handle(args: argparse.Namespace) -> int:
    from hark.core import report

    if args.with_timeline:
        from hark.models import require_ml

        require_ml("llm --with-timeline")
    try:
        result = critique(args.file, prompt=args.prompt, model=args.model, consent=args.yes_upload,
                          video=args.video, with_timeline=args.with_timeline, start=args.start, end=args.end,
                          max_seconds=args.max_seconds, cuts=not args.no_cuts,
                          synth_intended=args.synth_intended)
    except AgyCannotHear as exc:
        report.error_line("llm", str(exc))
        return 2
    report.emit(result, args, text=result.get("text"))
    return 0 if result.get("ok") else 1
