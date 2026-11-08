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


def _fixed(s: str):
    return next((_result(key, "allowed", name, url, False, "allowed: " + name)
                 for key, test, name, url in _FIXED if test(s)), None)


def _cc_by(major: str, minor) -> dict:
    ver = major + ("." + minor if minor else "")
    url = "https://creativecommons.org/licenses/by/%s/"
    if major == "1":
        return _result("by1", "unknown", "CC BY " + ver, url % ver,
                       True, "CC BY 1.x is rare: confirm the version on the item page")
    if minor in ("0", None, ""):
        ver, key = major + ".0", "by" + major
    else:
        key = "by" + major + minor
    return _result(key, "allowed", "CC BY " + ver, url % ver, True, "allowed: attribution is the only condition")


def _free(s: str):
    found = _fixed(s)
    m = _BY.search(s) if found is None else None
    return _cc_by(m.group(1), m.group(2)) if m else found


def _blocked(s: str):
    spaced = s.replace("_", " ")
    reason = next((r for test, r in _FORBIDDEN if test(spaced)), None)
    if reason:
        return _result("forbidden", "forbidden", reason=reason)
    if "restricted" in s:
        return _result("restricted", "unknown", reason="restricted terms: read them for this use")
    return None


def _reserved(s: str, found):
    """Apply "all rights reserved" and "copyrighted" to a free-licence reading."""
    arr = "all rights reserved" in s
    if found is None and (arr or re.search(r"\bcopyrighted\b", s)):
        return _result("forbidden", "forbidden", reason="all rights reserved")
    if found is not None and arr:
        return dict(found, status="unknown",
                    reason="the text names a free licence and also all rights reserved: read the page")
    return found


def classify(text) -> dict:
    """Classify a rights text. Returns {key, status, name, url, attribution, reason}."""
    s = (text or "").strip().lower()
    if not s:
        return _result("none", "unknown", reason="no rights recorded")
    verdict = _blocked(s) or _reserved(s, _free(s))
    return verdict or _result("unknown", "unknown",
                              reason="unknown terms: read the item page and record the exact licence")


def rel(path: str, root: str) -> str:
    """Path relative to root, with forward slashes. The root prefix matches case-insensitively."""
    p = str(path).replace("\\", "/")
    r = os.path.abspath(root).replace("\\", "/").rstrip("/") + "/"
    if p.lower().startswith(r.lower()):
        p = p[len(r):]
    return p[2:] if p.startswith("./") else p


def _skip_manifest(name: str) -> bool:
    return not name.endswith(".json") or name == "rights.json" or name.startswith("rights.")


def _read_entries(path: str, name: str) -> tuple[list, str | None]:
    """The entries of 1 manifest file, and a note when it was skipped."""
    try:
        with open(path, encoding="utf-8") as fh:
            data = json.load(fh)
    except (OSError, ValueError) as e:
        return [], f"{name}: not valid JSON ({e}); skipped"
    if isinstance(data, dict):
        data = data.get("entries", data.get("items"))
    if not isinstance(data, list):
        return [], f"{name}: not a list of entries; skipped"
    return [dict(e, _manifest=path) for e in data if isinstance(e, dict) and e.get("file")], None


def load_manifests(folder: str) -> tuple[list, list]:
    """Read every manifest list in folder. Skips the audit files. Returns (entries, notes)."""
    entries, notes = [], []
    for name in sorted(os.listdir(folder)):
        if _skip_manifest(name):
            continue
        found, note = _read_entries(os.path.join(folder, name), name)
        entries += found
        notes += [note] if note else []
    return entries, notes


def _host(url: str):
    if not url:
        return None
    host = urlparse(url if "//" in url else "//" + url).hostname
    return host[4:] if host and host.startswith("www.") else host


def _site(host: str) -> str:
    return next((name for suffix, name in _SOURCES if host == suffix or host.endswith("." + suffix)), host)


def source_name(entry: dict) -> str:
    """A readable name of the site an entry came from."""
    hosts = [_host(entry.get(f) or "") for f in ("page_url", "download_url")]
    host = next((h for h in hosts if h), None)
    if host:
        return _site(host)
    src = (entry.get("source") or "").split(" (")[0].strip()
    return src or "unknown source"


def report(label: str, good: bool) -> bool:
    """Print 1 self-test line and return the result."""
    print(("ok   " if good else "FAIL ") + label)
    return bool(good)


CLASSIFY_CASES = [
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


def _test_classify() -> bool:
    results = []
    for text, key, status, attr in CLASSIFY_CASES:
        r = classify(text)
        good = (r["key"], r["status"], r["attribution"]) == (key, status, attr)
        results.append(report(f"S1 classify {text!r} -> {r['key']} {r['status']}", good))
    results.append(report("CC BY licence URLs",
                          classify("CC BY 2.5")["url"] == "https://creativecommons.org/licenses/by/2.5/"
                          and classify("CC BY 4")["url"] == "https://creativecommons.org/licenses/by/4.0/"))
    return all(results)


def _test_names() -> bool:
    good = (source_name({"page_url": "https://www.freesound.org/people/a/sounds/1/"}) == "Freesound.org"
            and source_name({"download_url": "https://assets.mixkit.co/x.wav"}) == "Mixkit"
            and source_name({"source": "composer site (direct)"}) == "composer site"
            and source_name({}) == "unknown source")
    a = report("source_name", good)
    b = report("rel", rel("/Proj/Film/audio/a.wav", "/proj/film") == "audio/a.wav"
               and rel("audio\\b.wav", "/x") == "audio/b.wav")
    return a and b


def _test_manifests() -> bool:
    import tempfile
    with tempfile.TemporaryDirectory() as d:
        for name, body in [("ui.json", '[{"id": "fs-1", "file": "a.wav"}, {"id": "x"}]'),
                           ("rights.json", "[]"), ("rights.prev.json", "[]"),
                           ("music_choice.json", '{"winner": "x"}'), ("bad.json", "{oops"),
                           ("kb.json", '{"entries": [{"file": "k.wav"}]}')]:
            with open(os.path.join(d, name), "w") as fh:
                fh.write(body)
        entries, notes = load_manifests(d)
    return report("load_manifests skips audits, decision records and bad JSON",
                  [e["file"] for e in entries] == ["k.wav", "a.wav"] and len(notes) == 2)


def self_test() -> int:
    results = [_test_classify(), _test_names(), _test_manifests()]
    return 0 if all(results) else 1


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    print("rights.py is a module. Run it with --self-test, or import it.", file=sys.stderr)
    sys.exit(2)
