"""Opaque artifacts (spec 4.4): detection by content, and the static facts that
`intake scan` writes to sweep/opaque/<id>.json.

Nothing opaque is executed, imported, loaded or extracted: a file is read as
bytes, and an archive is never opened. The only programs that ever get a
quarantine path are the static readers `nm -gU`, `otool -L`, `objdump -T` and
`objdump -h`, and only as the file they read. They parse hostile binaries, so
each runs in the OS sandbox of `sandbox.wrap_reader`; with no usable sandbox a
reader does not run. A missing or skipped reader is recorded.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

from . import evidence, sandbox, supply

SEVERITY = {"OPQ-01": "high", "OPQ-02": "high", "OPQ-03": "high", "OPQ-04": "high", "OPQ-05": "high",
            "OPQ-06": "low"}
CAP = 500  # strings, list items and reader lines kept per record
CALL_SITES_CAP = 200
READER_TIMEOUT = 20
READER_PATH = "/usr/bin:/bin"  # readers come from the system folders only

NATIVE_EXTS = frozenset(".so .dylib .dll .node .exe .a .o".split())
BYTECODE_EXTS = frozenset(".pyc .pyo .class .jar .war .ear .wasm .beam .luac".split())
ARCHIVE_EXTS = frozenset(".zip .tar .gz .tgz .7z .xz .bz2 .zst .rar .whl .crx .vsix .skill".split())
MINIFY_EXTS = frozenset(".js .mjs .cjs .ts .tsx .jsx .css .py .pyw".split())
BUNDLE_MARKS = re.compile(
    r"__webpack_require__|sourceMappingURL|\bdefine\.amd\b|typeof exports\s*===?\s*['\"]object['\"]"
    r"|\b__toESM\b|\b__commonJS\b|\bSystem\.register\s*\("
)
ARCHIVE_MAGIC = (
    (b"PK\x03\x04", "zip archive"), (b"PK\x05\x06", "zip archive"), (b"\x1f\x8b", "gzip data"),
    (b"7z\xbc\xaf\x27\x1c", "7z archive"), (b"\xfd7zXZ\x00", "xz data"), (b"\x28\xb5\x2f\xfd", "zstd data"),
    (b"Rar!\x1a\x07", "rar archive"), (b"Cr24", "Chrome extension (crx)"),
)
MEDIA_MAGIC = (
    (b"\x89PNG\r\n\x1a\n", "PNG image"), (b"\xff\xd8\xff", "JPEG image"), (b"GIF87a", "GIF image"),
    (b"GIF89a", "GIF image"), (b"%PDF-", "PDF document"), (b"wOFF", "WOFF font"), (b"wOF2", "WOFF2 font"),
    (b"OTTO", "OpenType font"), (b"\x00\x01\x00\x00\x00", "TrueType font"), (b"ID3", "MP3 audio"),
    (b"fLaC", "FLAC audio"), (b"OggS", "Ogg media"), (b"\x1aE\xdf\xa3", "Matroska or WebM video"),
    (b"\x00\x00\x01\x00", "ICO image"),
)

STRING_RUN = re.compile(rb"[\x20-\x7e\t]{6,}")
URL_RE = re.compile(r"(?:https?|wss?|ftp)://[^\s\"'<>`\\)]+")
DOMAIN_RE = re.compile(
    r"\b(?:[a-z0-9-]+\.)+(?:com|net|org|io|dev|app|ai|co|sh|me|xyz|ru|cn|cloud|info|biz|invalid|onion)\b", re.I
)
ENV_RE = re.compile(r"\$\{?([A-Z_][A-Z0-9_]{2,})|\b([A-Z][A-Z0-9]*_[A-Z0-9_]*[A-Z0-9])\b")
PATH_RE = re.compile(r"(?:~|\.{1,2})?/(?:[\w.@+-]+/)+[\w.@+-]+|~/[\w.@+-]+")


# ---- detection ----------------------------------------------------------------


def _u32(data: bytes, at: int, order: str) -> int:
    return int.from_bytes(data[at:at + 4], order) if len(data) >= at + 4 else -1


def _native(data: bytes, ext: str) -> str | None:
    head = data[:4]
    if head == b"\x7fELF":
        return "ELF executable or library"
    if head in (b"\xfe\xed\xfa\xce", b"\xce\xfa\xed\xfe"):
        return "Mach-O 32-bit"
    if head in (b"\xfe\xed\xfa\xcf", b"\xcf\xfa\xed\xfe"):
        return "Mach-O 64-bit"
    # CA FE BA BE is a Mach-O fat header (a small arch count) or a Java class (version >= 45).
    if head == b"\xca\xfe\xba\xbf" or (head == b"\xca\xfe\xba\xbe" and 0 <= _u32(data, 4, "big") < 45):
        return "Mach-O universal binary"
    if data[:2] == b"MZ":
        pe = _u32(data, 0x3C, "little")
        if (0 < pe < len(data) - 4 and data[pe:pe + 4] == b"PE\x00\x00") or b"\x00" in data[:8192]:
            return "PE (Windows) executable"
    if data[:8] == b"!<arch>\n":
        return "ar archive (static library)"
    return f"native library or executable ({ext})" if ext in NATIVE_EXTS else None


def _bytecode(data: bytes, ext: str) -> str | None:
    magic = int.from_bytes(data[:2], "little") if len(data) >= 4 else -1
    if data[2:4] == b"\r\n" and (3000 <= magic < 4000 or magic in (62211, 62218)):
        return "Python bytecode (.pyc)"
    if data[:4] == b"\xca\xfe\xba\xbe" and _u32(data, 4, "big") >= 45:
        return "Java class"
    if data[:4] == b"\x00asm":
        return "WebAssembly module"
    if data[:4] == b"FOR1" and data[8:12] == b"BEAM":
        return "Erlang BEAM bytecode"
    if data[:4] == b"\x1bLua":
        return "Lua bytecode"
    if data[:4] == b"PK\x03\x04" and b"META-INF/" in data:
        return "Java archive (zip with META-INF/)"
    return f"bytecode ({ext})" if ext in BYTECODE_EXTS else None


def _archive(data: bytes, ext: str) -> str | None:
    for magic, kind in ARCHIVE_MAGIC:
        if data.startswith(magic):
            return kind
    if data[:3] == b"BZh" and data[3:4].isdigit() and data[4:10] == b"1AY&SY":
        return "bzip2 data"
    if data[257:262] == b"ustar":
        return "tar archive"
    return f"archive ({ext})" if ext in ARCHIVE_EXTS else None


def _media(data: bytes) -> str | None:
    for magic, kind in MEDIA_MAGIC:
        if data.startswith(magic):
            return kind
    if data[:4] == b"RIFF" and data[8:12] in (b"WEBP", b"WAVE", b"AVI "):
        return {b"WEBP": "WebP image", b"WAVE": "WAV audio", b"AVI ": "AVI video"}[data[8:12]]
    if data[4:8] == b"ftyp":
        return "MP4 or QuickTime media"
    return "binary data" if b"\x00" in data[:8192] else None


def _minified(rel: str, text: str) -> str | None:
    ext = os.path.splitext(rel)[1].lower()
    shebang = text.startswith("#!") and re.match(r"#![^\n]*\b(?:node|python[\d.]*|deno|bun)\b", text)
    if ext not in MINIFY_EXTS and not shebang:
        return None
    size = len(text.encode("utf-8", "replace"))
    if any(len(line) > 1000 for line in text.split("\n")):
        return "minified source (a line over 1,000 characters)"
    if size >= 500 and text.count("\n") * 500 < size:
        return "minified source (under 1 newline per 500 bytes)"
    m = BUNDLE_MARKS.search(text)
    return f"bundled source ({m.group(0)})" if m else None


def classify(rel: str, data: bytes, text: str | None) -> tuple[str, str] | None:
    """(OPQ rule, type by magic) for one file, or None when it is readable source.
    OPQ-05 (vendored) is decided by path, in `sweep.scan_tree`."""
    ext = os.path.splitext(rel)[1].lower()
    for rule, check in (("OPQ-01", _native), ("OPQ-02", _bytecode), ("OPQ-04", _archive)):
        kind = check(data, ext)
        if kind:
            return rule, kind
    kind = _minified(rel, text) if text is not None else None
    if kind:
        return "OPQ-03", kind
    kind = _media(data)
    return ("OPQ-06", kind) if kind else None


def opaque_hit(rule: str, kind: str, rel: str) -> dict:
    """An OPQ hit. `_path` and `_type` are for the record writer; `write_jsonl` drops them."""
    h = supply.hit(rule, SEVERITY[rule], rel, 0, kind)
    h["_path"], h["_type"] = rel, kind
    return h


# ---- facts ------------------------------------------------------------------


def _unique(items, cap: int = CAP) -> list[str]:
    out: dict[str, None] = {}
    for item in items:
        if len(out) >= cap:
            break
        out.setdefault(item, None)
    return list(out)


def string_facts(strings: list[str]) -> dict:
    """Escaped strings, URLs, domains, env-like names and file paths."""
    urls = _unique(u for s in strings for u in URL_RE.findall(s))
    hosts = (re.sub(r"^[^/]*//(?:[^@/]*@)?", "", u).split("/")[0].split(":")[0] for u in urls)
    rest = [URL_RE.sub(" ", s) for s in strings]
    domains = _unique([*hosts, *(d.lower() for s in rest for d in DOMAIN_RE.findall(s))])
    env = _unique(a or b for s in strings for a, b in ENV_RE.findall(s))
    paths = _unique(p for s in rest for p in PATH_RE.findall(s))
    esc = evidence.escape
    return {
        "strings": [esc(s) for s in strings[:CAP]],
        "urls": [esc(u) for u in urls],
        "domains": [esc(d) for d in domains],
        "env_names": [esc(e) for e in env],
        "file_paths": [esc(p) for p in paths],
    }


def printable_strings(data: bytes) -> list[str]:
    return [m.group(0).decode("ascii") for m in STRING_RUN.finditer(data)]


def call_sites(texts: dict[str, str], target: str) -> list[dict]:
    """Lines in the tree, outside the artifact, that name its file name or path."""
    name = target.rsplit("/", 1)[-1]
    pattern = re.compile(r"(?<![\w-])" + re.escape(name) + r"(?![\w-])")
    out = []
    for rel, text in texts.items():
        if rel == target or rel.startswith(target + "/"):
            continue
        for n, line in enumerate(text.split("\n"), 1):
            if pattern.search(line):
                out.append({"file": evidence.escape(rel), "line": n, "text": evidence.escape(line.strip())})
                if len(out) >= CALL_SITES_CAP:
                    return out
    return out


def _readers(kind: str) -> tuple[tuple[str, tuple[str, ...], str], ...]:
    if kind.startswith("Mach-O"):
        return ("nm", ("-gU",), "symbols"), ("otool", ("-L",), "libraries")
    return ("nm", ("-gU",), "symbols"), ("objdump", ("-T",), "symbols"), ("objdump", ("-h",), "sections")


def run_reader(tool: str, args: tuple[str, ...], path: Path) -> tuple[dict, list[str]]:
    """Run 1 static reader on `path` (its only file argument) inside the OS sandbox:
    no network, reads of system folders and `path` only, writes to 1 temp dir only.
    With no usable sandbox the reader does not run: status `skipped-no-sandbox`."""
    status = {"tool": tool, "args": list(args)}
    exe = shutil.which(tool, path=READER_PATH)
    if exe is None:
        return {**status, "status": "missing"}, []
    box = sandbox.kind()
    if box is None:
        return {**status, "status": "skipped-no-sandbox"}, []
    tmp = os.path.realpath(tempfile.mkdtemp(prefix="intake-reader-"))
    try:
        argv = sandbox.wrap_reader(box, [exe, *args, os.path.realpath(path)], os.path.realpath(path), tmp)
        r = subprocess.run(
            argv, stdin=subprocess.DEVNULL, capture_output=True, cwd=tmp, timeout=READER_TIMEOUT,
            env={"PATH": READER_PATH, "LANG": "C", "HOME": tmp, "TMPDIR": tmp}, check=False,
        )
    except subprocess.TimeoutExpired:
        return {**status, "status": "timeout"}, []
    except OSError as e:
        return {**status, "status": f"error: {e.strerror}"}, []
    finally:
        shutil.rmtree(tmp, ignore_errors=True)
    lines = r.stdout.decode("utf-8", "replace").splitlines()[:CAP]
    return {**status, "status": "ok" if r.returncode == 0 else f"exit {r.returncode}"}, lines


def _file_record(root: Path, rel: str, kind: str, rule: str) -> tuple[dict, dict]:
    data = (root / rel).read_bytes()
    strings = printable_strings(data)
    facts = string_facts(strings)
    meta = {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data), "type": kind,
            "strings_total": len(strings), "readers": []}
    if rule == "OPQ-01":
        for tool, args, field in _readers(kind):
            status, lines = run_reader(tool, args, root / rel)
            meta["readers"].append(status)
            facts.setdefault(field, []).extend(evidence.escape(x) for x in lines)
    return meta, facts


def _folder_record(root: Path, rel: str) -> tuple[dict, dict]:
    """A vendored folder (OPQ-05): 1 record, sha256 over sorted `relpath\\0sha256\\n` lines."""
    files = [(f"{rel}/{r}", p) for r, p in walk(root / rel)]
    digest, size, strings, listing = hashlib.sha256(), 0, [], []
    for frel, p in files:
        data = p.read_bytes()
        size += len(data)
        digest.update(f"{frel}\0{hashlib.sha256(data).hexdigest()}\n".encode())
        strings += printable_strings(data)
        listing.append(evidence.escape(frel))
    facts = string_facts(strings)
    facts["files"] = listing[:CAP]
    meta = {"sha256": digest.hexdigest(), "size": size, "type": f"vendored folder ({len(files)} files)",
            "strings_total": len(strings), "readers": []}
    return meta, facts


def walk(root: Path) -> list[tuple[str, Path]]:
    from .sweep import walk as sweep_walk  # one walker: symlinks are never followed

    return sweep_walk(root)


def write_records(root: Path, hits: list[dict], out_dir: Path) -> int:
    """Write sweep/opaque/<id>.json for every OPQ hit; return how many."""
    targets = [h for h in hits if h["rule"].startswith("OPQ-")]
    if not targets:
        return 0
    from .sweep import read_text

    texts = {rel: t for rel, p in walk(root) if (t := read_text(p)) is not None}
    out_dir.mkdir(parents=True, exist_ok=True)
    for h in targets:
        rel, kind = h["_path"], h["_type"]
        is_folder = (root / rel).is_dir()
        meta, facts = _folder_record(root, rel) if is_folder else _file_record(root, rel, kind, h["rule"])
        facts["call_sites"] = call_sites(texts, rel)
        record = {"id": h["id"], "rule": h["rule"], "severity": h["severity"], "path": evidence.escape(rel),
                  **meta, "untrusted": facts}
        _write_json(out_dir / f"{h['id']}.json", record)
    return len(targets)


def _write_json(path: Path, data: dict) -> None:
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tmp-", suffix=".json")
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        json.dump(data, f, ensure_ascii=True, indent=1)
        f.write("\n")
    os.replace(tmp, path)
