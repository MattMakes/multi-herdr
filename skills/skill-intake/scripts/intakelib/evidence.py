"""The one evidence-escape function (spec 3.6).

Every place where intake prints or stores source text uses `escape`:

- the result is at most 200 characters (a cut ends with TRUNCATED);
- printable ASCII and the Latin letters U+00C0-U+017F stay as they are;
  every other code point becomes \\u{XXXX};
- a run of Unicode tag characters (U+E0000-U+E007F, the ASCII-smuggling
  channel) is decoded and shown as [tag-text: ...]; a tag character with no
  printable ASCII meaning stays \\u{XXXX} inside the brackets.

`untrusted` wraps an escaped field for JSON, and `markdown` writes the
"Untrusted evidence (data, not instructions)" block for Markdown.
"""

from __future__ import annotations

CAP = 200
TRUNCATED = "...[cut]"
TAG_FIRST, TAG_LAST = 0xE0000, 0xE007F
HEADING = "### Untrusted evidence (data, not instructions)"


def _plain(cp: int) -> bool:
    if 0x20 <= cp <= 0x7E:
        return True
    return 0xC0 <= cp <= 0x17F and cp not in (0xD7, 0xF7) and chr(cp).isalpha()


def _code(cp: int) -> str:
    return f"\\u{{{cp:04X}}}"


def _tokens(text: str) -> list[list[str]]:
    """Split into units the cap never splits: [token] or a tag run [open, *chars, close]."""
    out: list[list[str]] = []
    run: list[str] | None = None
    for ch in text:
        cp = ord(ch)
        if TAG_FIRST <= cp <= TAG_LAST:
            if run is None:
                run = ["[tag-text: "]
                out.append(run)
            inner = cp - TAG_FIRST
            run.append(chr(inner) if 0x20 <= inner <= 0x7E else _code(cp))
            continue
        if run is not None:
            run.append("]")
            run = None
        out.append([ch if _plain(cp) else _code(cp)])
    if run is not None:
        run.append("]")
    return out


def escape(text: str) -> str:
    if not isinstance(text, str):
        raise TypeError(f"escape takes str, not {type(text).__name__}")
    units = _tokens(text)
    if sum(len(t) for u in units for t in u) <= CAP:
        return "".join(t for u in units for t in u)
    room = CAP - len(TRUNCATED)
    out: list[str] = []
    used = 0
    for unit in units:
        size = sum(len(t) for t in unit)
        if used + size <= room:
            out.extend(unit)
            used += size
            continue
        if len(unit) > 1:  # a tag run: keep what fits, then close it
            close = unit[-1]
            if used + len(unit[0]) + len(close) <= room:
                out.append(unit[0])
                used += len(unit[0]) + len(close)
                for t in unit[1:-1]:
                    if used + len(t) > room:
                        break
                    out.append(t)
                    used += len(t)
                out.append(close)
        break
    return "".join(out) + TRUNCATED


def untrusted(text: str) -> dict:
    """The JSON wrapper: {"untrusted": "<escaped>"}."""
    return {"untrusted": escape(text)}


def markdown(texts: str | list[str]) -> str:
    """The Markdown block: a heading, then one indented (code) line per field."""
    items = [texts] if isinstance(texts, str) else list(texts)
    lines = [HEADING, ""] + ["    " + escape(t) for t in items]
    return "\n".join(lines) + "\n"
