"""Usage-rights policy for downloaded recordings.

classify() reads the rights text of a manifest entry and says if a commercial,
edited film may use the recording. The audit and the credits import this
module. It has no command line except --self-test.
"""
from __future__ import annotations

import json
import os
import re
import sys
from urllib.parse import urlparse

# Forbidden terms, checked in order. Each item: (test on the lower-case text, reason).
_FORBIDDEN = [
    (lambda s: "disputed" in s, "the sources disagree about the terms"),
    (lambda s: re.search(r"\bnc\b", s) or "noncommercial" in s or "non-commercial" in s,
     "non-commercial terms: a launch video is commercial use"),
    (lambda s: re.search(r"\bnd\b", s) or "noderiv" in s or "no-deriv" in s,
     "no-derivatives terms: trimming and syncing to picture make a derivative"),
    (lambda s: re.search(r"\bsa\b", s) or "sharealike" in s or "share-alike" in s,
     "share-alike terms: the whole film would have to carry the same licence"),
    (lambda s: "sampling" in s, "Sampling+ and Sampling are retired and restrictive"),
    (lambda s: "personal" in s, "personal use only"),
    (lambda s: "custom" in s or "free with credit" in s or "social media" in s,
     "a custom term that the audit cannot trust"),
    (lambda s: "editorial" in s, "editorial use only"),
]

# Fixed allowed classes: (key, test, name, url).
_FIXED = [
    ("cc0", lambda s: re.search(r"\bcc0\b", s) or "cc-0" in s or "creative commons 0" in s
     or "publicdomain/zero" in s or re.search(r"\bzero\b", s) or "public domain dedication" in s,
     "CC0 1.0 (public domain dedication)", "https://creativecommons.org/publicdomain/zero/1.0/"),
    ("pd", lambda s: "public domain" in s or "publicdomain/mark" in s,
     "Public Domain Mark 1.0", "https://creativecommons.org/publicdomain/mark/1.0/"),
    ("mixkit-music", lambda s: "mixkit" in s and "music" in s,
     "Mixkit Stock Music Free License", "https://mixkit.co/license/#musicFree"),
    ("mixkit-sfx", lambda s: "mixkit" in s,
     "Mixkit Sound Effects Free License", "https://mixkit.co/license/#sfxFree"),
    ("pixabay", lambda s: "pixabay" in s,
     "Pixabay Content License", "https://pixabay.com/service/license-summary/"),
]

_BY = re.compile(r"\b(?:by|attribution)\b[^0-9]{0,24}?([1-4])(?:\.([05]))?(?![0-9])")

SHORT_NAMES = {"cc0": "CC0", "pd": "public domain", "mixkit-sfx": "Mixkit free licence",
               "mixkit-music": "Mixkit free licence", "pixabay": "Pixabay licence"}

_SOURCES = [("freesound.org", "Freesound.org"), ("mixkit.co", "Mixkit"), ("pixabay.com", "Pixabay"),
            ("archive.org", "the Internet Archive"), ("commons.wikimedia.org", "Wikimedia Commons"),
            ("github.com", "GitHub")]


def _result(key, status, name="", url="", attribution=False, reason=""):
    return {"key": key, "status": status, "name": name, "url": url, "attribution": attribution, "reason": reason}


def _free(s: str):
    for key, test, name, url in _FIXED:
        if test(s):
            return _result(key, "allowed", name, url, False, "allowed: " + name)
    m = _BY.search(s)
    if not m:
        return None
    major, minor = m.group(1), m.group(2)
    ver = major + ("." + minor if minor else "")
    if major == "1":
        return _result("by1", "unknown", "CC BY " + ver, "https://creativecommons.org/licenses/by/%s/" % ver,
                       True, "CC BY 1.x is rare: confirm the version on the item page")
    if minor == "0" or not minor:
        ver = major + ".0"
        key = "by" + major
    else:
        key = "by" + major + minor
    return _result(key, "allowed", "CC BY " + ver, "https://creativecommons.org/licenses/by/%s/" % ver,
                   True, "allowed: attribution is the only condition")


def classify(text) -> dict:
    """Classify a rights text. Returns {key, status, name, url, attribution, reason}."""
    s = (text or "").strip().lower()
    if not s:
        return _result("none", "unknown", reason="no rights recorded")
    spaced = s.replace("_", " ")
    for test, reason in _FORBIDDEN:
        if test(spaced):
            return _result("forbidden", "forbidden", reason=reason)
    if "restricted" in s:
        return _result("restricted", "unknown", reason="restricted terms: read them for this use")
    found = _free(s)
    reserved = "all rights reserved" in s or re.search(r"\bcopyrighted\b", s)
    if reserved:
        if found is None:
            return _result("forbidden", "forbidden", reason="all rights reserved")
        if "all rights reserved" in s:
            found = dict(found, status="unknown",
                         reason="the text names a free licence and also all rights reserved: read the page")
    if found is not None:
        return found
    return _result("unknown", "unknown", reason="unknown terms: read the item page and record the exact licence")


def rel(path: str, root: str) -> str:
    """Path relative to root, with forward slashes. The root prefix matches case-insensitively."""
    p = str(path).replace("\\", "/")
    r = os.path.abspath(root).replace("\\", "/").rstrip("/") + "/"
    if p.lower().startswith(r.lower()):
        p = p[len(r):]
    return p[2:] if p.startswith("./") else p


def load_manifests(folder: str) -> tuple[list, list]:
    """Read every manifest list in folder. Skips the audit files. Returns (entries, notes)."""
    entries, notes = [], []
    for name in sorted(os.listdir(folder)):
        if not name.endswith(".json") or name == "rights.json" or name.startswith("rights."):
            continue
        path = os.path.join(folder, name)
        try:
            with open(path, encoding="utf-8") as fh:
                data = json.load(fh)
        except (OSError, ValueError) as e:
            notes.append(f"{name}: not valid JSON ({e}); skipped")
            continue
        if isinstance(data, dict):
            data = data.get("entries", data.get("items"))
        if not isinstance(data, list):
            notes.append(f"{name}: not a list of entries; skipped")
            continue
        for e in data:
            if isinstance(e, dict) and e.get("file"):
                e = dict(e)
                e["_manifest"] = path
                entries.append(e)
    return entries, notes


def source_name(entry: dict) -> str:
    """A readable name of the site an entry came from."""
    for field in ("page_url", "download_url"):
        url = entry.get(field) or ""
        host = urlparse(url if "//" in url else "//" + url).hostname if url else None
        if host:
            host = host[4:] if host.startswith("www.") else host
            for suffix, name in _SOURCES:
                if host == suffix or host.endswith("." + suffix):
                    return name
            return host
    src = (entry.get("source") or "").split(" (")[0].strip()
    return src or "unknown source"


def self_test() -> int:
    cases = [
        ("CC BY 4.0", "by4", "allowed", True), ("Attribution 4.0", "by4", "allowed", True),
        ("Creative Commons 0", "cc0", "allowed", False), ("CC BY-NC 4.0", "forbidden", "forbidden", False),
        ("CC BY-SA 3.0", "forbidden", "forbidden", False),
        ("Mixkit Sound Effects Free License", "mixkit-sfx", "allowed", False),
        ("Mixkit Stock Music Free License", "mixkit-music", "allowed", False),
        ("free with credit on social media", "forbidden", "forbidden", False),
        ("", "none", "unknown", False), ("CC BY 1.0", "by1", "unknown", True),
        ("CC BY 4.0, all rights reserved", "by4", "unknown", True),
        ("CC BY 2.5", "by25", "allowed", True), ("Public Domain Mark", "pd", "allowed", False),
        ("Copyrighted", "forbidden", "forbidden", False), ("restricted use", "restricted", "unknown", False),
        ("CC_BY_ND_4.0", "forbidden", "forbidden", False),
    ]
    ok = True
    for text, key, status, attr in cases:
        r = classify(text)
        good = r["key"] == key and r["status"] == status and r["attribution"] == attr
        ok &= good
        print(("ok   " if good else "FAIL ") + f"S1 classify {text!r} -> {r['key']} {r['status']}")
    good = classify("CC BY 2.5")["url"] == "https://creativecommons.org/licenses/by/2.5/"
    good &= classify("CC BY 4")["url"] == "https://creativecommons.org/licenses/by/4.0/"
    ok &= good
    print(("ok   " if good else "FAIL ") + "CC BY licence URLs")
    good = (source_name({"page_url": "https://www.freesound.org/people/a/sounds/1/"}) == "Freesound.org"
            and source_name({"download_url": "https://assets.mixkit.co/x.wav"}) == "Mixkit"
            and source_name({"source": "composer site (direct)"}) == "composer site"
            and source_name({}) == "unknown source")
    ok &= good
    print(("ok   " if good else "FAIL ") + "source_name")
    good = rel("/Proj/Film/audio/a.wav", "/proj/film") == "audio/a.wav" and rel("audio\\b.wav", "/x") == "audio/b.wav"
    ok &= good
    print(("ok   " if good else "FAIL ") + "rel")
    import tempfile
    with tempfile.TemporaryDirectory() as d:
        for name, body in [("ui.json", '[{"id": "fs-1", "file": "a.wav"}, {"id": "x"}]'),
                           ("rights.json", "[]"), ("rights.prev.json", "[]"),
                           ("music_choice.json", '{"winner": "x"}'), ("bad.json", "{oops"),
                           ("kb.json", '{"entries": [{"file": "k.wav"}]}')]:
            with open(os.path.join(d, name), "w") as fh:
                fh.write(body)
        entries, notes = load_manifests(d)
        good = [e["file"] for e in entries] == ["k.wav", "a.wav"] and len(notes) == 2
        ok &= good
        print(("ok   " if good else "FAIL ") + "load_manifests skips audits, decision records and bad JSON")
    return 0 if ok else 1


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    print("rights.py is a module. Run it with --self-test, or import it.", file=sys.stderr)
    sys.exit(2)
