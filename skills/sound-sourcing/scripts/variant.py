#!/usr/bin/env python3
"""Apply design operations: a variant copy for a critic, or the agreed fix.

--out DIR writes a patched copy of the design (and can render and QA it).
--apply edits the real design after a backup. Files are patched in place:
only the changed values change in the text, so a diff is the review record.
Run with the hark venv Python and -I. --self-test runs the built-in checks.
"""
from __future__ import annotations

import argparse
import copy
import difflib
import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


class OpError(Exception):
    pass


def say(*parts, file=None):
    text = " ".join(str(p) for p in parts)
    print(text.encode("ascii", "replace").decode("ascii"), file=file or sys.stdout)


# ------------------------------------------------------------------ span parser

class Node:
    """Where one JSON value sits in the text. Containers keep their children in order."""
    __slots__ = ("start", "end", "kind", "children")

    def __init__(self, start, end, kind, children=None):
        self.start, self.end, self.kind, self.children = start, end, kind, children or []


class Child:
    __slots__ = ("key", "item_start", "node")

    def __init__(self, key, item_start, node):
        self.key, self.item_start, self.node = key, item_start, node


_WS = " \t\r\n"


def _skip(s: str, i: int) -> int:
    while i < len(s) and s[i] in _WS:
        i += 1
    return i


def _string_end(s: str, i: int) -> int:
    i += 1
    while s[i] != '"':
        i += 2 if s[i] == "\\" else 1
    return i + 1


def parse_spans(s: str, i: int = 0) -> tuple[Node, int]:
    i = _skip(s, i)
    c = s[i]
    if c == "{":
        node, j = Node(i, 0, "obj"), _skip(s, i + 1)
        if s[j] == "}":
            node.end = j + 1
            return node, j + 1
        while True:
            j = _skip(s, j)
            ks = j
            ke = _string_end(s, j)
            key = json.loads(s[ks:ke])
            j = _skip(s, ke) + 1  # past ':'
            child, j = parse_spans(s, j)
            node.children.append(Child(key, ks, child))
            j = _skip(s, j)
            if s[j] == ",":
                j += 1
                continue
            node.end = j + 1
            return node, j + 1
    if c == "[":
        node, j = Node(i, 0, "list"), _skip(s, i + 1)
        if s[j] == "]":
            node.end = j + 1
            return node, j + 1
        k = 0
        while True:
            child, j = parse_spans(s, j)
            node.children.append(Child(k, child.start, child))
            k += 1
            j = _skip(s, j)
            if s[j] == ",":
                j += 1
                continue
            node.end = j + 1
            return node, j + 1
    if c == '"':
        e = _string_end(s, i)
        return Node(i, e, "str"), e
    j = i
    while j < len(s) and s[j] not in ",]}" + _WS:
        j += 1
    return Node(i, j, "lit"), j


def _dump(v) -> str:
    return json.dumps(v, ensure_ascii=False)


def _sep(text: str, node: Node) -> str:
    ch = node.children
    if len(ch) >= 2:
        return text[ch[-2].node.end:ch[-1].item_start]
    return ", "


def _identity(v):
    if isinstance(v, dict):
        ident = tuple((k, _dump(v[k])) for k in ("name", "sample", "label") if k in v)
        if ident:
            return ident
    return _dump(v)


def _delete_run(text: str, node: Node, a: int, b: int) -> tuple[int, int]:
    """Text span that removes children a..b-1 with one separator."""
    ch = node.children
    if b < len(ch):
        return ch[a].item_start, ch[b].item_start
    if a > 0:
        return ch[a - 1].node.end, ch[b - 1].node.end
    return ch[a].item_start, ch[b - 1].node.end


def diff(text: str, node: Node, old, new, edits: list):
    if old == new and type(old) is type(new):
        return
    if isinstance(old, dict) and isinstance(new, dict) and node.kind == "obj":
        ch = node.children
        keys = [c.key for c in ch]
        if not ch:
            edits.append((node.start, node.end, _dump(new)))
            return
        sep = _sep(text, node)
        i = 0
        while i < len(ch):
            if ch[i].key not in new:
                j = i
                while j < len(ch) and ch[j].key not in new:
                    j += 1
                edits.append((*_delete_run(text, node, i, j), ""))
                i = j
            else:
                diff(text, ch[i].node, old[ch[i].key], new[ch[i].key], edits)
                i += 1
        added = [k for k in new if k not in keys]
        if added:
            kept = [c for c in ch if c.key in new]
            if not kept:
                edits.append((node.start, node.end, _dump(new)))
                return
            at = kept[-1].node.end
            edits.append((at, at, "".join(sep + _dump(k) + ": " + _dump(new[k]) for k in added)))
        return
    if isinstance(old, list) and isinstance(new, list) and node.kind == "list":
        ch = node.children
        if not ch:
            edits.append((node.start, node.end, _dump(new)))
            return
        sep = _sep(text, node)
        sm = difflib.SequenceMatcher(a=[_identity(v) for v in old], b=[_identity(v) for v in new], autojunk=False)
        for op, a1, a2, b1, b2 in sm.get_opcodes():
            if op == "equal" or (op == "replace" and a2 - a1 == b2 - b1):
                for k in range(a2 - a1):
                    diff(text, ch[a1 + k].node, old[a1 + k], new[b1 + k], edits)
            elif op == "insert":
                body = sep.join(_dump(v) for v in new[b1:b2])
                if a1 > 0:
                    edits.append((ch[a1 - 1].node.end, ch[a1 - 1].node.end, sep + body))
                else:
                    edits.append((ch[0].item_start, ch[0].item_start, body + sep))
            elif op == "delete":
                edits.append((*_delete_run(text, node, a1, a2), ""))
            else:
                body = sep.join(_dump(v) for v in new[b1:b2])
                edits.append((ch[a1].item_start, ch[a2 - 1].node.end, body))
        return
    edits.append((node.start, node.end, _dump(new)))


def patch_text(text: str, new) -> tuple[str, str]:
    """Rewrite text so it parses to new, changing only what differs. Returns (text, 'patched'|'reformatted')."""
    full = json.dumps(new, indent=1, ensure_ascii=False) + "\n"
    try:
        old = json.loads(text)
        root, _ = parse_spans(text)
    except (ValueError, IndexError):
        return full, "reformatted"
    edits = []
    diff(text, root, old, new, edits)
    edits.sort(key=lambda e: (e[0], e[1]))
    for x, y in zip(edits, edits[1:]):
        if y[0] < x[1] or (y[0] == x[0] and y[1] == x[1] == x[0]):
            return full, "reformatted"
    out = text
    for s, e, rep in reversed(edits):
        out = out[:s] + rep + out[e:]
    try:
        if json.loads(out) != new:
            return full, "reformatted"
    except ValueError:
        return full, "reformatted"
    return out, "patched"


# ------------------------------------------------------------------ operations

def short(v) -> str:
    t = _dump(v)
    return t if len(t) <= 80 else t[:77] + "..."


def set_keys(obj: dict, values: dict, where: str, log: list):
    for k, v in values.items():
        if k in obj and obj[k] == v:
            log.append((False, f"{where}.{k}: unchanged ({short(v)})"))
        else:
            log.append((True, f"{where}.{k}: {short(obj.get(k))} -> {short(v)}"))
            obj[k] = v


def _find(items: list, field: str, value, what: str, frag: str) -> int:
    for i, it in enumerate(items):
        if isinstance(it, dict) and str(it.get(field)) == str(value):
            return i
    raise OpError(f"{frag}: no {what} with {field} {value!r}")


def apply_op(op: dict, frags: dict, top: dict, log: list):
    if not isinstance(op, dict):
        raise OpError(f"an operation must be an object: {short(op)}")
    if "sd" in op:
        if "value" not in op:
            raise OpError(f"sd operation without value: {short(op)}")
        parts = str(op["sd"]).split(".")
        obj = top
        for p in parts[:-1]:
            if not isinstance(obj.get(p), dict):
                obj[p] = {}
            obj = obj[p]
        set_keys(obj, {parts[-1]: op["value"]}, ".".join(parts[:-1]) or "top", log)
        return
    f = op.get("f")
    if f not in frags:
        raise OpError(f"unknown fragment {f!r} (known: {', '.join(sorted(frags))})")
    fr = frags[f]
    if "sample" in op or "slice" in op:
        name = op.get("sample", op.get("slice"))
        sec = next((s for s in ("samples", "slices") if name in (fr.get(s) or {})), None)
        if sec is None:
            raise OpError(f"{f}: no sample or slice {name!r}")
        obj = fr[sec][name]
        if "set" not in op and "eq" not in op:
            raise OpError(f"{f}: sample operation needs set or eq")
        if "set" in op:
            set_keys(obj, op["set"], f"{f}.{sec}.{name}", log)
        if "eq" in op:
            eq = obj.setdefault("eq", {})
            set_keys(eq, op["eq"], f"{f}.{sec}.{name}.eq", log)
        return
    for key, sec, field in (("rule", "rules", "name"), ("place", "place", "sample"), ("amb", "ambience", "name")):
        if key in op:
            items = fr.get(sec)
            if not isinstance(items, list):
                raise OpError(f"{f}: no {sec} list")
            i = _find(items, field, op[key], key, f)
            if op.get("delete"):
                del items[i]
                log.append((True, f"{f}.{sec}[{op[key]}]: deleted"))
            elif "set" in op:
                set_keys(items[i], op["set"], f"{f}.{sec}[{op[key]}]", log)
            else:
                raise OpError(f"{f}: {key} operation needs set or delete")
            return
    for key, sec in (("rule_add", "rules"), ("place_add", "place")):
        if key in op:
            fr.setdefault(sec, []).append(op[key])
            log.append((True, f"{f}.{sec}: added {short(op[key])}"))
            return
    if "segment" in op:
        segs = (fr.get("music") or {}).get("segments")
        i = op["segment"]
        if not isinstance(segs, list) or not isinstance(i, int) or not 0 <= i < len(segs):
            raise OpError(f"{f}: no music segment {i!r}")
        set_keys(segs[i], op.get("set") or {}, f"{f}.music.segments[{i}]", log)
        return
    if "pool" in op:
        if "value" not in op:
            raise OpError(f"{f}: pool operation needs value")
        set_keys(fr.setdefault("pools", {}), {op["pool"]: op["value"]}, f"{f}.pools", log)
        return
    raise OpError(f"unknown operation: {short(op)}")


def load_ops(arg: str):
    if os.path.isfile(arg):
        with open(arg, encoding="utf-8") as fh:
            ops = json.load(fh)
    else:
        try:
            ops = json.loads(arg)
        except ValueError as e:
            raise OpError(f"--ops is neither a file nor JSON: {e}")
    if not isinstance(ops, list):
        raise OpError("--ops must be a JSON list of operations")
    return ops


# ------------------------------------------------------------------ run

def run(a) -> int:
    root = os.path.abspath(a.root)
    dpath = a.design if os.path.isabs(a.design) else os.path.join(root, a.design)
    if not os.path.isfile(dpath):
        say(f"variant.py: design not found: {dpath}", file=sys.stderr)
        return 1
    if bool(a.out) == bool(a.apply):
        say("variant.py: give exactly one of --out DIR and --apply", file=sys.stderr)
        return 1
    texts, frags, paths = {}, {}, {}
    with open(dpath, encoding="utf-8") as fh:
        texts["_top"] = fh.read()
    top = json.loads(texts["_top"])
    for inc in top.get("include") or []:
        p = inc if os.path.isabs(inc) else os.path.join(root, inc)
        if not os.path.isfile(p):
            say(f"variant.py: include not found: {p}", file=sys.stderr)
            return 1
        name = os.path.splitext(os.path.basename(p))[0]
        with open(p, encoding="utf-8") as fh:
            texts[name] = fh.read()
        frags[name], paths[name] = json.loads(texts[name]), p
    orig_top, orig = copy.deepcopy(top), copy.deepcopy(frags)
    log = []
    try:
        for op in load_ops(a.ops):
            apply_op(op, frags, top, log)
    except OpError as e:
        say(f"nothing written: {e}")
        return 1
    changed = sum(1 for c, _ in log if c)
    same = len(log) - changed
    if a.apply:
        backup = a.backup if os.path.isabs(a.backup) else os.path.join(root, a.backup)
        if os.path.exists(backup):
            say(f"variant.py: backup folder {backup} exists: give a new --backup", file=sys.stderr)
            return 1
        say(f"{changed} change(s), {same} already as asked -> the design (backup {backup})")
        for _, m in log:
            say("  " + m)
        os.makedirs(os.path.join(backup, "design"))
        shutil.copy2(dpath, os.path.join(backup, os.path.basename(dpath)))
        for name, p in paths.items():
            shutil.copy2(p, os.path.join(backup, "design", os.path.basename(p)))
        if top != orig_top:
            txt, how = patch_text(texts["_top"], top)
            _write(dpath, txt)
            say(f"  {how} {dpath}")
        for name, p in paths.items():
            if frags[name] != orig[name]:
                txt, how = patch_text(texts[name], frags[name])
                _write(p, txt)
                say(f"  {how} {p}")
        return 0
    out = os.path.abspath(a.out)
    say(f"{changed} change(s), {same} already as asked -> {out}")
    for _, m in log:
        say("  " + m)
    os.makedirs(os.path.join(out, "design"), exist_ok=True)
    copies = {}
    for name, p in paths.items():
        dst = os.path.join(out, "design", os.path.basename(p))
        txt, how = patch_text(texts[name], frags[name])
        _write(dst, txt)
        copies[p] = dst
        say(f"  {how} {dst}")
    top_copy = copy.deepcopy(top)
    if "include" in top_copy:
        top_copy["include"] = [copies[inc if os.path.isabs(inc) else os.path.join(root, inc)]
                               for inc in top_copy["include"]]
    cues = a.cues or top.get("cues")
    if cues:
        top_copy["cues"] = cues if os.path.isabs(cues) else os.path.join(root, cues)
    dtop = os.path.join(out, "sounddesign.json")
    txt, how = patch_text(texts["_top"], top_copy)
    _write(dtop, txt)
    say(f"  {how} {dtop}")
    if a.render or a.qa:
        cmd = [sys.executable, "-I", os.path.join(HERE, "mix.py"), "--root", root, "--design", dtop,
               "--out", os.path.join(out, "soundtrack.wav"), "--stems", os.path.join(out, "stems"),
               "--report", os.path.join(out, "report.json"), "--cues-out", os.path.join(out, "cues.json"),
               "--trace", os.path.join(out, "adds.json")]
        if a.cache:
            cmd += ["--cache", a.cache]
        r = subprocess.run(cmd, capture_output=True, text=True)
        say(r.stdout[-1200:].rstrip())
        if r.stderr.strip():
            say(r.stderr[-800:].rstrip(), file=sys.stderr)
        if r.returncode != 0:
            return r.returncode
    if a.qa:
        cmd = [sys.executable, "-I", os.path.join(HERE, "qa.py"), "--root", root, "--design", dtop,
               "--report", os.path.join(out, "report.json"), "--trace", os.path.join(out, "adds.json"),
               "--stems", os.path.join(out, "stems"), "--master", os.path.join(out, "soundtrack.wav"),
               "--out", os.path.join(out, "qa.json"), "--cues-sounded", os.path.join(out, "cues_sounded.json")]
        if a.cues or top.get("cues"):
            cmd += ["--cues", top_copy["cues"]]
        if a.cache:
            cmd += ["--cache", a.cache]
        r = subprocess.run(cmd, capture_output=True, text=True)
        say(r.stdout.rstrip())
        if r.returncode == 2:
            say(r.stderr.rstrip(), file=sys.stderr)
            return 2
    return 0


def _write(path: str, text: str):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as fh:
        fh.write(text)


def parse(argv):
    p = argparse.ArgumentParser(prog="variant.py", description="Apply design operations to a copy or the design.")
    p.add_argument("--ops", help="a JSON file of operations, or the JSON list itself")
    p.add_argument("--out", help="write a variant copy to this folder")
    p.add_argument("--apply", action="store_true", help="edit the real design (after a backup)")
    p.add_argument("--backup", default="out/audio-review/backup_before_fix")
    p.add_argument("--render", action="store_true")
    p.add_argument("--qa", action="store_true")
    p.add_argument("--cues")
    p.add_argument("--cache")
    p.add_argument("--root", default=".")
    p.add_argument("--design", default="audio/sounddesign.json")
    p.add_argument("--self-test", action="store_true")
    return p.parse_args(argv)


def main(argv=None) -> int:
    a = parse(sys.argv[1:] if argv is None else argv)
    if a.self_test:
        return self_test()
    if a.ops is None:
        say("variant.py: --ops is required", file=sys.stderr)
        return 1
    return run(a)


def self_test() -> int:
    import contextlib
    import io
    import tempfile
    sys.path.insert(0, HERE)
    import fixtures as fx
    ck = fx.Checker()
    quiet = lambda: contextlib.redirect_stdout(io.StringIO())
    hand = ('{\n  "_doc": "ui fragment",\n  "rules": [\n'
            '    {"name": "ui.click", "pool": "ui.tap",  "gain_db": -6,   "_doc": "one voice"},\n'
            '    {"name": "ui.pops",  "pool": "ui.pop",  "gain_db": -1.50}\n  ],\n'
            '  "samples": {"ui.tap": {"file": "a.wav", "eq": {"hp": 120}}}\n}\n')
    with tempfile.TemporaryDirectory() as d:
        os.makedirs(os.path.join(d, "audio", "design"))
        _write(os.path.join(d, "audio", "design", "ui.json"), hand)
        top = '{"duration": 4.0,\n "include": ["audio/design/ui.json"],\n "master": {"lufs": -14}}\n'
        _write(os.path.join(d, "audio", "sounddesign.json"), top)
        with quiet():
            code = main(["--root", d, "--ops", '[{"f": "ui", "rule": "ui.click", "set": {"gain_db": -3}}]',
                         "--out", os.path.join(d, "v1")])
        new = open(os.path.join(d, "v1", "design", "ui.json")).read()
        dl = [ln for ln in difflib.unified_diff(hand.splitlines(), new.splitlines(), lineterm="", n=0)
              if ln.startswith(("+", "-")) and not ln.startswith(("+++", "---"))]
        ck("S13 a 1-line diff, only gain_db", code == 0 and len(dl) == 2 and '"gain_db": -3,' in dl[1]
           and "-1.50" in new)
        vt = json.load(open(os.path.join(d, "v1", "sounddesign.json")))
        ck("S13 the copy's include paths are absolute", all(os.path.isabs(p) for p in vt["include"]))
        ck("S13 the real design is untouched", open(os.path.join(d, "audio", "design", "ui.json")).read() == hand)
        before = sorted(os.listdir(os.path.join(d, "v1", "design")))
        with quiet() as o:
            code = main(["--root", d, "--ops", '[{"f": "ui", "rule": "nope", "set": {"gain_db": 0}}]',
                         "--out", os.path.join(d, "v1")])
        ck("S14 a missing rule writes nothing", code == 1 and "nothing written" in o.getvalue()
           and open(os.path.join(d, "v1", "design", "ui.json")).read() == new
           and sorted(os.listdir(os.path.join(d, "v1", "design"))) == before)
        ops = ('[{"f": "ui", "sample": "ui.tap", "eq": {"highshelf": [6000, -3]}},'
               ' {"f": "ui", "rule_add": {"name": "ui.new", "pool": "ui.tap"}},'
               ' {"f": "ui", "rule": "ui.pops", "delete": true}, {"sd": "buses.music.auto", "value": [[0, 0]]},'
               ' {"f": "ui", "pool": "ui.p2", "value": ["ui.tap"]}]')
        bk = os.path.join(d, "out", "bk")
        with quiet():
            code = main(["--root", d, "--ops", ops, "--apply", "--backup", bk])
        fr = json.load(open(os.path.join(d, "audio", "design", "ui.json")))
        tp = json.load(open(os.path.join(d, "audio", "sounddesign.json")))
        ck("apply: eq merge, add, delete, pool and sd", code == 0
           and fr["samples"]["ui.tap"]["eq"] == {"hp": 120, "highshelf": [6000, -3]}
           and [r["name"] for r in fr["rules"]] == ["ui.click", "ui.new"] and fr["pools"]["ui.p2"] == ["ui.tap"]
           and tp["buses"]["music"]["auto"] == [[0, 0]])
        ck("apply: the backup holds the originals",
           open(os.path.join(bk, "design", "ui.json")).read() == hand
           and open(os.path.join(bk, "sounddesign.json")).read() == top)
        with contextlib.redirect_stderr(io.StringIO()) as e, quiet():
            code = main(["--root", d, "--ops", "[]", "--apply", "--backup", bk])
        ck("S15 a second apply with the same backup refuses", code == 1 and "new --backup" in e.getvalue())
        with quiet() as o:
            main(["--root", d, "--ops", '[{"f": "ui", "rule": "ui.click", "set": {"gain_db": -6}}]',
                  "--out", os.path.join(d, "v2")])
        ck("unchanged is reported", "unchanged (-6)" in o.getvalue())
        # W4: an empty variant renders bit-identically to the base
        p = fx.project(os.path.join(d, "film"))
        sys.path.insert(0, HERE)
        import mix
        with quiet():
            mix.main(["--root", p["root"], "--trace", "out/qa/adds.json"])
            code = main(["--root", p["root"], "--ops", "[]", "--out", os.path.join(p["root"], "out", "v0"),
                         "--render"])
        base = open(os.path.join(p["root"], "out", "audio", "soundtrack.wav"), "rb").read()
        var = open(os.path.join(p["root"], "out", "v0", "soundtrack.wav"), "rb").read()
        ck("W4 an empty variant is bit-identical to the base render", code == 0 and base == var)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
