"""hark over MCP on stdio, so a coding agent can listen, review and render sounds.

The MCP SDK loads only when the server starts. Each tool imports its backing
module during the call, so a unit that is not installed gives that one tool an
error while the others keep working. The tool catalog, argument checks and
reply text live in the ``_mcp_*`` modules beside this one.
"""

from __future__ import annotations

import argparse
import contextlib
import importlib
import importlib.util
import inspect
import io
import os
import sys
import threading
import time
import types
from typing import Any, Callable

from hark.surfaces._mcp_args import (  # noqa: F401  (re-exported names)
    ToolError,
    absolute,
    choice_arg,
    cues_arg,
    file_arg,
    optional_file,
    out_arg,
    paths_arg,
    range_arg,
    video_default,
)
from hark.surfaces._mcp_calls import (  # noqa: F401  (re-exported names)
    _LOCK,
    ToolResult,
    _as_record,
    _importable,
    _run,
    backing,
    call_backing,
    filter_kwargs,
    present_tools,
)
from hark.surfaces._mcp_catalog import (  # noqa: F401  (re-exported names)
    INSTRUCTIONS,
    TOOL_BY_NAME,
    TOOLS,
    Tool,
    description,
    tool_list,
)
from hark.surfaces._mcp_text import (  # noqa: F401  (re-exported names)
    compact_json,
    format_result,
    measure_summary,
)

SERVER_NAME = "hark"
IMAGE_EDGE = 1600
IMAGE_QUALITY = 88
DETAILS = ("low", "normal", "high")
FIND_KINDS = ("auto", "event", "texture")

# one handler per tool -------------------------------------------------


def t_measure(path: str, start: float | None = None, end: float | None = None, music: bool = False,
              light: bool = False, full: bool = False) -> ToolResult:
    args = {"src": file_arg(path), "music": music, "start": start, "end": end, "light": light}
    res, ignored = call_backing("measure", backing("measure"), args)
    record = _as_record(res)
    record["summary"] = measure_summary(record)
    return ToolResult(format_result(record, full, ignored))


def t_describe(path: str, start: float | None = None, end: float | None = None, top: int = 8,
               full: bool = False) -> ToolResult:
    return _run("describe", {"path": file_arg(path), "start": start, "end": end,
                             "top": range_arg(top, "top", 1, 30)}, full)


def t_match(path: str, query: Any, alts: list[str] | None = None, start: float | None = None,
            end: float | None = None, full: bool = False) -> ToolResult:
    return _run("match", {"path": file_arg(path), "query": query, "alts": alts or None,
                          "start": start, "end": end}, full)


def t_rank(paths: Any, query: str, top: int | None = 20, full: bool = False) -> ToolResult:
    return _run("rank", {"paths": paths_arg(paths, "paths"), "query": query,
                         "top": range_arg(top, "top", 1)}, full)


def t_find(path: str, query: str, top: int = 5, kind: str = "auto", win: float | None = None,
           hop: float | None = None, start: float | None = None, end: float | None = None,
           full: bool = False) -> ToolResult:
    args = {"path": file_arg(path), "query": query, "top": range_arg(top, "top", 1, 50),
            "kind": choice_arg(kind, FIND_KINDS, "kind"), "win": win, "hop": hop, "start": start, "end": end}
    return _run("find", args, full)


def t_compare(path: str, refs: Any, k: int = 5, start: float | None = None, end: float | None = None,
              full: bool = False) -> ToolResult:
    args = {"path": file_arg(path), "refs": paths_arg(refs, "refs"), "k": range_arg(k, "k", 1, 50),
            "start": start, "end": end}
    return _run("compare", args, full)


def t_ab(a: str, b: str, query: str | None = None, refs: Any = None, a_start: float | None = None,
         a_end: float | None = None, b_start: float | None = None, b_end: float | None = None,
         full: bool = False) -> ToolResult:
    has_refs = refs not in (None, "", [])
    args = {"a": file_arg(a, "a"), "b": file_arg(b, "b"), "query": query,
            "refs": paths_arg(refs, "refs") if has_refs else None,
            "a_start": range_arg(a_start, "a_start", 0), "a_end": a_end,
            "b_start": range_arg(b_start, "b_start", 0), "b_end": b_end}
    return _run("ab", args, full)


def _realness_row(row: dict) -> str:
    chance = row.get("p_real")
    cell = f"{chance:6.2f}" if isinstance(chance, (int, float)) else f"{'-':>6}"
    name = os.path.basename(str(row.get("path") or row.get("file") or ""))
    why = row.get("why") or row.get("summary") or ""
    return f"{cell}  {str(row.get('verdict') or '-'):<24} {name}: {why}"


def _realness_table(record: dict) -> str:
    rows = [_realness_row(row) for row in record.get("files") or record.get("results") or []]
    return "\n".join([str(record.get("summary") or ""), "p_real  verdict                  file: main reason", *rows])


def _require_realness_model() -> None:
    module = sys.modules.get("hark.listen.realness") or importlib.import_module("hark.listen.realness")
    weights = getattr(module, "MODEL_PATH", None)
    if weights is not None and not os.path.exists(str(weights)):
        raise ToolError("realness failed: the realness model file is missing, so real versus synthesized was "
                        "NOT checked; tell the user so. describe (synth-type AudioSet classes) and measure are "
                        "weaker evidence.")


def _realness_batch(where: str, start: Any, end: Any, ast: bool, full: bool) -> ToolResult:
    """Judge every media file under a folder or glob in one call."""
    if not os.path.isdir(where) and not any(ch in where for ch in "*?["):
        file_arg(where)
    many = paths_arg(where, "path")
    try:
        batch_fn = backing("realness", "realness_many")
    except ToolError:
        raise ToolError("realness: this hark version judges 1 file per call; pass a file") from None
    res, ignored = call_backing("realness", batch_fn, {"paths": many, "ast": ast or None, "start": start,
                                                        "end": end})
    record = _as_record(res)
    record["text"] = _realness_table(record)
    return ToolResult(format_result(record, full, ignored))


def t_realness(path: str, start: float | None = None, end: float | None = None, ast: bool = False,
               full: bool = False) -> ToolResult:
    backing("realness")
    _require_realness_model()
    if not isinstance(path, str) or not path.strip():
        raise ToolError("path: a file path is required")
    where = absolute(path)
    if os.path.isfile(where):
        return _run("realness", {"path": where, "start": start, "end": end, "ast": ast or None}, full)
    return _realness_batch(where, start, end, ast, full)


def _soundtrack_args(path: str, video: Any, cues: Any, start: Any, end: Any, detail: str) -> dict[str, Any]:
    where = file_arg(path)
    return {"path": where, "video": video_default(where, video), "cues": cues_arg(cues), "start": start,
            "end": end, "detail": choice_arg(detail, DETAILS, "detail")}


def t_timeline(path: str, video: str | None = None, cues: Any = None, start: float | None = None,
               end: float | None = None, detail: str = "normal", full: bool = False) -> ToolResult:
    return _run("timeline", _soundtrack_args(path, video, cues, start, end, detail), full)


def _hark_home() -> str:
    override = os.environ.get("HARK_HOME")
    if override:
        return os.path.abspath(os.path.expanduser(override))
    from hark.core import config

    return str(config.HOME)


def sheet_path(path: str, start: float | None, end: float | None) -> str:
    """Where a sheet goes by default: <HARK_HOME>/sheets/<stem>_<id8>[_<from>-<to>].png."""
    from hark.core import audio

    span = ""
    if start is not None or end is not None:
        stop = "end" if end is None else f"{end:g}"
        span = f"_{(start or 0):g}-{stop}"
    stem = os.path.splitext(os.path.basename(path))[0]
    folder = os.path.join(_hark_home(), "sheets")
    os.makedirs(folder, exist_ok=True)
    return os.path.join(folder, f"{stem}_{audio.file_id(path)[:8]}{span}.png")


def jpeg_of(png: str) -> bytes:
    """Re-encode a PNG as JPEG, the long edge at most 1600 px."""
    from PIL import Image

    sink = io.BytesIO()
    with Image.open(png) as picture:
        rgb = picture.convert("RGB")
        rgb.thumbnail((IMAGE_EDGE, IMAGE_EDGE))
        rgb.save(sink, "JPEG", quality=IMAGE_QUALITY, optimize=True)
    return sink.getvalue()


def t_sheet(path: str, video: str | None = None, cues: Any = None, start: float | None = None,
            end: float | None = None, detail: str = "normal", out_png: str | None = None,
            include_image: bool = True, full: bool = False) -> ToolResult:
    args = _soundtrack_args(path, video, cues, start, end, detail)
    args["out_png"] = out_arg(out_png, "out_png") if out_png else sheet_path(args["path"], start, end)
    res, ignored = call_backing("sheet", backing("sheet"), args)
    reply = format_result(res, full, ignored)
    png = (res.get("png") if isinstance(res, dict) else None) or args["out_png"]
    if not os.path.isfile(png):
        return ToolResult(f"{reply}\n(no PNG found at {png})")
    return ToolResult(f"PNG: {png}\n\n{reply}", jpeg_of(png) if include_image else None)


def t_critique(path: str, video: str | None = None, profile: str | None = None, fast: bool = False,
               full: bool = False) -> ToolResult:
    where = file_arg(path)
    skip = False if fast else None
    return _run("critique", {"path": where, "video": video_default(where, video),
                             "profile": optional_file(profile, "profile"), "use_ast": skip,
                             "use_realness": skip}, full)


def t_doctor(load: bool = False, full: bool = False) -> ToolResult:
    return _run("doctor", {"load": load}, full)


def t_llm_critique(path: str, consent: bool = False, prompt: str | None = None, video: str | None = None,
                   with_timeline: bool = False, model: str | None = None, synth_intended: bool = False,
                   start: float | None = None, end: float | None = None, full: bool = False) -> ToolResult:
    args = {"path": file_arg(path), "consent": consent, "prompt": prompt, "video": optional_file(video, "video"),
            "with_timeline": with_timeline, "model": model, "start": start, "end": end,
            "synth_intended": synth_intended}
    return _run("llm_critique", args, full)


def _bank_line(entry: dict) -> str:
    knobs = entry.get("knobs") or {}
    pitch = knobs.get("pitch")
    note = pitch.get("note") if isinstance(pitch, dict) else None
    extras = (", apex" if knobs.get("apex") else "") + (f", pitch {note}" if note else "")
    kinds = ", ".join(entry.get("kinds") or []) or "-"
    return (f"{entry.get('id')}: {float(entry.get('dur_s') or 0):.3f} s, "
            f"peak {float(entry.get('peak_s') or 0):.3f} s; {kinds}; dur {knobs.get('dur')}{extras}")


def t_sfx_list(category: str | None = None, kind: str | None = None, full: bool = False) -> ToolResult:
    res, ignored = call_backing("sfx_list", backing("sfx_list"), {"category": category, "kind": kind})
    record = _as_record(res)
    rows = [_bank_line(entry) for entry in record.get("sounds") or []]
    record["text"] = "\n".join([str(record.get("summary") or ""), *rows])
    return ToolResult(format_result(record, full, ignored))


def t_sfx_render(sound: str, out: str, seed: int | None = None, dur: float | None = None,
                 apex: float | None = None, pitch: str | None = None, peak_db: float = -1.0,
                 full: bool = False) -> ToolResult:
    if peak_db is not None and peak_db > 0:
        raise ToolError("peak_db must be <= 0")
    args = {"sound": sound, "out": out_arg(out, "out"), "seed": seed, "dur": range_arg(dur, "dur", 1e-9),
            "apex": range_arg(apex, "apex", 1e-9), "pitch": pitch, "peak_db": peak_db}
    return _run("sfx_render", args, full)


def t_kit_render(kit: str, out: str, force: bool = False, full: bool = False) -> ToolResult:
    return _run("kit_render", {"kit": file_arg(kit, "kit"), "out": out_arg(out, "out"), "force": force}, full)


HANDLERS: dict[str, Callable[..., ToolResult]] = {
    "measure": t_measure, "describe": t_describe, "match": t_match, "rank": t_rank, "find": t_find,
    "compare": t_compare, "ab": t_ab, "realness": t_realness, "timeline": t_timeline, "sheet": t_sheet,
    "critique": t_critique, "doctor": t_doctor, "llm_critique": t_llm_critique, "sfx_list": t_sfx_list,
    "sfx_render": t_sfx_render, "kit_render": t_kit_render,
}


def call_tool(name: str, **kwargs: Any) -> ToolResult:
    """Run one tool inside this process; both the server and the tests go through here."""
    handler = HANDLERS.get(name)
    if handler is None:
        raise ToolError(f"unknown tool {name!r}")
    if name != "llm_critique":
        return handler(**kwargs)
    try:
        return handler(**kwargs)
    except ToolError as exc:
        raise ToolError(str(exc).replace("llm_critique failed: AgyCannotHear: ", "")) from None


# the stdio server -----------------------------------------------------


def hold_threads(threads: int | None = None) -> int:
    """Fix the BLAS thread count before numpy loads; a value the user set wins."""
    if threads:
        os.environ["HARK_THREADS"] = str(threads)
    count = max(1, min(4, (os.cpu_count() or 2) // 2))
    asked = os.environ.get("HARK_THREADS")
    if asked:
        with contextlib.suppress(ValueError):
            count = max(1, int(asked))
    os.environ.setdefault("OPENBLAS_NUM_THREADS", str(count))
    return count


def _sdk() -> Any:
    """The MCP SDK server module: ``mcpserver`` in mcp 2.x, ``fastmcp`` in mcp 1.x."""
    newer = importlib.util.find_spec("mcp.server.mcpserver")
    module = importlib.import_module("mcp.server.mcpserver" if newer else "mcp.server.fastmcp")
    server_cls = getattr(module, "MCPServer", None) or module.FastMCP
    return types.SimpleNamespace(Server=server_cls, Image=module.Image)


def _wrap_for_sdk(name: str) -> Callable:
    """A function the SDK can register: the handler's signature, text or [text, image] out."""
    handler = HANDLERS[name]

    def run(**kwargs: Any) -> Any:
        reply = call_tool(name, **kwargs)
        if reply.image is None:
            return reply.text
        return [reply.text, _sdk().Image(data=reply.image, format="jpeg")]

    run.__name__ = name
    run.__doc__ = description(name)
    run.__signature__ = inspect.signature(handler).replace(  # type: ignore[attr-defined]
        return_annotation=inspect.Signature.empty)
    run.__annotations__ = dict(getattr(handler, "__annotations__", {}))
    return run


def _register(server: Any, spec: Tool) -> None:
    from mcp.types import ToolAnnotations

    hints = ToolAnnotations(title=f"hark {spec.name}", readOnlyHint=spec.read_only, destructiveHint=False,
                            openWorldHint=spec.network)
    options = {"name": spec.name, "description": description(spec.name), "annotations": hints}
    fn = _wrap_for_sdk(spec.name)
    try:
        server.add_tool(fn, structured_output=False, **options)
    except TypeError:
        server.add_tool(fn, **options)


def _version() -> str:
    import hark

    return hark.__version__


def _new_server() -> Any:
    server_cls = _sdk().Server
    try:
        return server_cls(SERVER_NAME, instructions=INSTRUCTIONS, log_level="WARNING", version=_version())
    except TypeError:
        pass
    server = server_cls(SERVER_NAME, instructions=INSTRUCTIONS, log_level="WARNING")
    inner = getattr(server, "_mcp_server", None)
    if inner is not None and hasattr(inner, "version"):
        inner.version = _version()
    return server


def build_server(warm: bool = False) -> Any:
    """The SDK server with every tool registered; ImportError without the mcp extra."""
    server = _new_server()
    for spec in TOOLS:
        _register(server, spec)
    if warm:
        threading.Thread(target=warm_up, name="hark-warm", daemon=True).start()
    return server


def warm_up() -> None:
    """Load both models, measure once and load the vocabulary; say how it went on stderr."""
    began = time.monotonic()
    with _LOCK:
        try:
            import numpy as np

            from hark.core import features
            from hark.models import ast, clap

            clap.get()
            ast.get()
            features.measure(0.1 * np.random.default_rng(0).standard_normal(48000).astype(np.float32))
            if _importable("hark.listen.vocabulary"):
                importlib.import_module("hark.listen.vocabulary").get()
            print(f"models warm ({time.monotonic() - began:.1f} s)", file=sys.stderr)
        except Exception as exc:
            print(f"warm-up failed: {type(exc).__name__}: {exc}", file=sys.stderr)


def serve(warm: bool = False) -> int:
    """Serve on stdio until the client closes the pipe."""
    threads = hold_threads()
    if importlib.util.find_spec("mcp") is None:
        try:
            from hark.models import install_command

            how = install_command("mcp")
        except Exception:
            how = f'{sys.executable} -m pip install "mcp>=2"'
        print(f"hark mcp: error: MLMissing: the MCP server needs the mcp extras. Install them with: {how}",
              file=sys.stderr)
        return 3
    server = build_server(warm=warm)
    print(f"hark {_version()} MCP server: {len(TOOLS)} tools on stdio, {threads} threads", file=sys.stderr)
    server.run("stdio")
    return 0


# command line ---------------------------------------------------------


def add_cli(sub: Any) -> None:
    """Add the ``hark mcp`` subcommand."""
    from hark.core import report

    parser = sub.add_parser("mcp", help="Run hark as an MCP server over stdio.")
    parser.add_argument("--warm", action="store_true", help="load the models in the background at start")
    parser.add_argument("--tools", action="store_true", help="print the tool list and exit")
    report.add_common(parser)
    parser.set_defaults(func=handle)


def handle(args: argparse.Namespace) -> int:
    if args.tools:
        from hark.core import report

        listing = tool_list()
        report.emit(listing, args, text=listing["text"])
        return 0
    if importlib.util.find_spec("mcp") is None:
        from hark.models import require_extra

        require_extra("mcp", "MCP server")
    return serve(warm=args.warm)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="python -m hark.surfaces.mcp_server")
    parser.add_argument("--warm", action="store_true")
    return serve(warm=parser.parse_args(argv).warm)


if __name__ == "__main__":
    sys.exit(main())
