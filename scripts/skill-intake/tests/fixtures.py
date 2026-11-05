"""Fixture builder base (spec 12.2).

Fixtures are generated at test time in temp dirs from escaped Python
literals. No repo file holds a raw payload, an invisible character or a
binary: write a hostile character as an escape (for example "\\u200b"), and
build binary content from bytes literals. Fixture payloads are inert by
construction: every URL host ends in `.invalid`, and every secret name starts
with `INTAKE_FIXTURE_`. `build` refuses a fixture that breaks either rule.

Later tickets add one function per fixture (F-clean, F-inject, ...) that
returns a `files` dict and calls `build`.
"""

from __future__ import annotations

import re
from pathlib import Path

HOST_SUFFIX = ".invalid"
SECRET_PREFIX = "INTAKE_FIXTURE_"

_URL_HOST = re.compile(rb"[a-zA-Z][a-zA-Z0-9+.-]*://([^/\s:\"'`<>)]+)")


def host(name: str) -> str:
    """A fixture host name: `host("exfil")` is `exfil.invalid`."""
    return name if name.endswith(HOST_SUFFIX) else name + HOST_SUFFIX


def secret(name: str) -> str:
    """A fixture secret name: `secret("TOKEN")` is `INTAKE_FIXTURE_TOKEN`."""
    return name if name.startswith(SECRET_PREFIX) else SECRET_PREFIX + name


def _as_bytes(content: str | bytes) -> bytes:
    return content.encode("utf-8") if isinstance(content, str) else content


def check_inert(files: dict[str, str | bytes]) -> None:
    """Raise ValueError when a fixture names a URL host outside `.invalid`."""
    for rel, content in files.items():
        for m in _URL_HOST.finditer(_as_bytes(content)):
            h = m.group(1).decode("ascii", "replace").rsplit("@", 1)[-1]
            if not h.endswith(HOST_SUFFIX):
                raise ValueError(f"fixture {rel}: URL host {h!r} does not end in {HOST_SUFFIX}")


def build(root: Path, files: dict[str, str | bytes]) -> Path:
    """Write `files` (relative path -> text or bytes) under `root`; return `root`."""
    check_inert(files)
    root = Path(root)
    for rel, content in files.items():
        p = Path(rel)
        if p.is_absolute() or ".." in p.parts:
            raise ValueError(f"fixture path must stay under the root: {rel}")
        target = root / p
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(_as_bytes(content))
    return root
