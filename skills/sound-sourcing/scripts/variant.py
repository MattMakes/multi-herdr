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
import functools
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


def _obj_item(s: str, j: int, k: int) -> tuple[Child, int]:
    j = _skip(s, j)
    ke = _string_end(s, j)
    key = json.loads(s[j:ke])
    child, end = parse_spans(s, _skip(s, ke) + 1)  # past ':'
    return Child(key, j, child), end


def _list_item(s: str, j: int, k: int) -> tuple[Child, int]:
    child, j = parse_spans(s, j)
    return Child(k, child.start, child), j


def _parse_items(s: str, node: Node, j: int, close: str, item) -> tuple[Node, int]:
    """Parse comma-separated children up to close. item(s, j, index) returns (Child, end)."""
    j = _skip(s, j)
    more = s[j] != close
    if not more:
        j += 1
    k = 0
    while more:
        child, j = item(s, j, k)
        node.children.append(child)
        k += 1
        j = _skip(s, j)
        more = s[j] == ","
        j += 1
    node.end = j
    return node, j


def _lit_end(s: str, i: int) -> int:
    j = i
    while j < len(s) and s[j] not in ",]}" + _WS:
        j += 1
    return j


def parse_spans(s: str, i: int = 0) -> tuple[Node, int]:
    i = _skip(s, i)
    c = s[i]
    if c in _OPEN:
        kind, close, item = _OPEN[c]
        return _parse_items(s, Node(i, 0, kind), i + 1, close, item)
    e = _string_end(s, i) if c == '"' else _lit_end(s, i)
    return Node(i, e, "str" if c == '"' else "lit"), e


_OPEN = {"{": ("obj", "}", _obj_item), "[": ("list", "]", _list_item)}


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


def _run_end(ch: list, i: int, new: dict) -> int:
    """The end of the run of children from i whose keys are gone from new."""
    j = i
    while j < len(ch) and ch[j].key not in new:
        j += 1
    return j


def _diff_keys(text: str, node: Node, old: dict, new: dict, edits: list) -> None:
    ch = node.children
    i = 0
    while i < len(ch):
        if ch[i].key in new:
            diff(text, ch[i].node, old[ch[i].key], new[ch[i].key], edits)
            i += 1
            continue
        j = _run_end(ch, i, new)
        edits.append((*_delete_run(text, node, i, j), ""))
        i = j


def _added_kept(node: Node, new: dict) -> tuple[list, list]:
    """(keys new adds, children new keeps)."""
    keys = {c.key for c in node.children}
    return [k for k in new if k not in keys], [c for c in node.children if c.key in new]


def _add_keys(text: str, node: Node, new: dict, edits: list) -> None:
    added, kept = _added_kept(node, new)
    if added and not kept:
        edits.append((node.start, node.end, _dump(new)))
    elif added:
        sep, at = _sep(text, node), kept[-1].node.end
        edits.append((at, at, "".join(sep + _dump(k) + ": " + _dump(new[k]) for k in added)))


def _diff_obj(text: str, node: Node, old: dict, new: dict, edits: list) -> None:
    if not node.children:
        edits.append((node.start, node.end, _dump(new)))
        return
    _diff_keys(text, node, old, new, edits)
    _add_keys(text, node, new, edits)


def _insert_edit(ch: list, a1: int, sep: str, body: str) -> tuple:
    if a1 > 0:
        return ch[a1 - 1].node.end, ch[a1 - 1].node.end, sep + body
    return ch[0].item_start, ch[0].item_start, body + sep


def _list_op(cx: dict, code: tuple) -> None:
    """Turn 1 SequenceMatcher opcode into edits. cx: text, node, old, new, sep, edits."""
    op, a1, a2, b1, b2 = code
    ch, new, edits, sep = cx["node"].children, cx["new"], cx["edits"], cx["sep"]
    body = sep.join(_dump(v) for v in new[b1:b2])
    if op == "equal" or (op == "replace" and a2 - a1 == b2 - b1):
        for k in range(a2 - a1):
            diff(cx["text"], ch[a1 + k].node, cx["old"][a1 + k], new[b1 + k], edits)
    elif op == "insert":
        edits.append(_insert_edit(ch, a1, sep, body))
    elif op == "delete":
        edits.append((*_delete_run(cx["text"], cx["node"], a1, a2), ""))
    else:
        edits.append((ch[a1].item_start, ch[a2 - 1].node.end, body))


def _diff_list(text: str, node: Node, old: list, new: list, edits: list) -> None:
    if not node.children:
        edits.append((node.start, node.end, _dump(new)))
        return
    cx = {"text": text, "node": node, "old": old, "new": new, "sep": _sep(text, node), "edits": edits}
    sm = difflib.SequenceMatcher(a=[_identity(v) for v in old], b=[_identity(v) for v in new], autojunk=False)
    for code in sm.get_opcodes():
        _list_op(cx, code)


def diff(text: str, node: Node, old, new, edits: list):
    if old == new and type(old) is type(new):
        return
    if isinstance(old, dict) and isinstance(new, dict) and node.kind == "obj":
        _diff_obj(text, node, old, new, edits)
    elif isinstance(old, list) and isinstance(new, list) and node.kind == "list":
        _diff_list(text, node, old, new, edits)
    else:
        edits.append((node.start, node.end, _dump(new)))


def _overlap(edits: list) -> bool:
    return any(y[0] < x[1] or (y[0] == x[0] and y[1] == x[1] == x[0]) for x, y in zip(edits, edits[1:]))


def _edits(text: str, new):
    """The sorted edits that turn text into new, or None when it cannot be patched."""
    try:
        old = json.loads(text)
        root, _ = parse_spans(text)
    except (ValueError, IndexError):
        return None
    edits = []
    diff(text, root, old, new, edits)
    edits.sort(key=lambda e: (e[0], e[1]))
    return None if _overlap(edits) else edits


def _splice(text: str, edits: list) -> str:
    out = text
    for s, e, rep in reversed(edits):
        out = out[:s] + rep + out[e:]
    return out


def _parses_to(text: str, value) -> bool:
    try:
        return json.loads(text) == value
    except ValueError:
        return False


def patch_text(text: str, new) -> tuple[str, str]:
    """Rewrite text so it parses to new, changing only what differs. Returns (text, 'patched'|'reformatted')."""
    edits = _edits(text, new)
    out = _splice(text, edits) if edits is not None else None
    if out is None or not _parses_to(out, new):
        return json.dumps(new, indent=1, ensure_ascii=False) + "\n", "reformatted"
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


def _op_sd(op: dict, top: dict, log: list) -> None:
    if "value" not in op:
        raise OpError(f"sd operation without value: {short(op)}")
    parts = str(op["sd"]).split(".")
    obj = top
    for p in parts[:-1]:
        if not isinstance(obj.get(p), dict):
            obj[p] = {}
        obj = obj[p]
    set_keys(obj, {parts[-1]: op["value"]}, ".".join(parts[:-1]) or "top", log)


def _op_sample(op: dict, f: str, fr: dict, log: list) -> None:
    name = op.get("sample", op.get("slice"))
    sec = next((s for s in ("samples", "slices") if name in (fr.get(s) or {})), None)
    if sec is None:
        raise OpError(f"{f}: no sample or slice {name!r}")
    if "set" not in op and "eq" not in op:
        raise OpError(f"{f}: sample operation needs set or eq")
    obj, where = fr[sec][name], f"{f}.{sec}.{name}"
    if "set" in op:
        set_keys(obj, op["set"], where, log)
    if "eq" in op:
        set_keys(obj.setdefault("eq", {}), op["eq"], where + ".eq", log)


def _op_item(op: dict, f: str, fr: dict, log: list, key: str, sec: str, field: str) -> None:
    items = fr.get(sec)
    if not isinstance(items, list):
        raise OpError(f"{f}: no {sec} list")
    i = _find(items, field, op[key], key, f)
    where = f"{f}.{sec}[{op[key]}]"
    if op.get("delete"):
        del items[i]
        log.append((True, f"{where}: deleted"))
    elif "set" in op:
        set_keys(items[i], op["set"], where, log)
    else:
        raise OpError(f"{f}: {key} operation needs set or delete")


def _op_add(op: dict, f: str, fr: dict, log: list, key: str, sec: str) -> None:
    fr.setdefault(sec, []).append(op[key])
    log.append((True, f"{f}.{sec}: added {short(op[key])}"))


def _op_segment(op: dict, f: str, fr: dict, log: list) -> None:
    segs = (fr.get("music") or {}).get("segments")
    i = op["segment"]
    if not isinstance(segs, list) or not isinstance(i, int) or not 0 <= i < len(segs):
        raise OpError(f"{f}: no music segment {i!r}")
    set_keys(segs[i], op.get("set") or {}, f"{f}.music.segments[{i}]", log)


def _op_pool(op: dict, f: str, fr: dict, log: list) -> None:
    if "value" not in op:
        raise OpError(f"{f}: pool operation needs value")
    set_keys(fr.setdefault("pools", {}), {op["pool"]: op["value"]}, f"{f}.pools", log)


# Fragment operations by their key, checked in this order.
_HANDLERS = [
    (("sample", "slice"), _op_sample),
    (("rule",), functools.partial(_op_item, key="rule", sec="rules", field="name")),
    (("place",), functools.partial(_op_item, key="place", sec="place", field="sample")),
    (("amb",), functools.partial(_op_item, key="amb", sec="ambience", field="name")),
    (("rule_add",), functools.partial(_op_add, key="rule_add", sec="rules")),
    (("place_add",), functools.partial(_op_add, key="place_add", sec="place")),
    (("segment",), _op_segment),
    (("pool",), _op_pool),
]


def _fragment_op(op: dict, frags: dict):
    """The handler of a fragment operation, bound to its fragment."""
    f = op.get("f")
    if f not in frags:
        raise OpError(f"unknown fragment {f!r} (known: {', '.join(sorted(frags))})")
    handler = next((h for keys, h in _HANDLERS if any(k in op for k in keys)), None)
    if handler is None:
        raise OpError(f"unknown operation: {short(op)}")
    return functools.partial(handler, op, f, frags[f])


def apply_op(op: dict, frags: dict, top: dict, log: list):
    if not isinstance(op, dict):
        raise OpError(f"an operation must be an object: {short(op)}")
    handler = functools.partial(_op_sd, op, top) if "sd" in op else _fragment_op(op, frags)
    handler(log)


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

class RunError(Exception):
    """A refusal printed to stderr as 'variant.py: ...'."""


def _abs(root: str, p: str) -> str:
    return p if os.path.isabs(p) else os.path.join(root, p)


def _read(path: str) -> str:
    with open(path, encoding="utf-8") as fh:
        return fh.read()


def _includes(root: str, top: dict, texts: dict) -> tuple[dict, dict]:
    """Read every included fragment into texts. Returns (fragments, paths) by fragment name."""
    frags, paths = {}, {}
    for inc in top.get("include") or []:
        p = _abs(root, inc)
        if not os.path.isfile(p):
            raise RunError(f"include not found: {p}")
        name = os.path.splitext(os.path.basename(p))[0]
        texts[name] = _read(p)
        frags[name], paths[name] = json.loads(texts[name]), p
    return frags, paths


def _load(a, root: str):
    """The design: texts, parsed top and fragments, fragment paths and untouched copies."""
    dpath = _abs(root, a.design)
    if not os.path.isfile(dpath):
        raise RunError(f"design not found: {dpath}")
    if bool(a.out) == bool(a.apply):
        raise RunError("give exactly one of --out DIR and --apply")
    texts = {"_top": _read(dpath)}
    top = json.loads(texts["_top"])
    frags, paths = _includes(root, top, texts)
    return {"dpath": dpath, "texts": texts, "top": top, "frags": frags, "paths": paths,
            "orig_top": copy.deepcopy(top), "orig": copy.deepcopy(frags)}


def _say_log(log: list, dest: str) -> None:
    changed = sum(1 for c, _ in log if c)
    say(f"{changed} change(s), {len(log) - changed} already as asked -> {dest}")
    for _, m in log:
        say("  " + m)


def _patch_file(path: str, text: str, value) -> None:
    txt, how = patch_text(text, value)
    _write(path, txt)
    say(f"  {how} {path}")


def _apply(a, root: str, ds: dict, log: list) -> int:
    backup = _abs(root, a.backup)
    if os.path.exists(backup):
        raise RunError(f"backup folder {backup} exists: give a new --backup")
    _say_log(log, f"the design (backup {backup})")
    os.makedirs(os.path.join(backup, "design"))
    shutil.copy2(ds["dpath"], os.path.join(backup, os.path.basename(ds["dpath"])))
    for p in ds["paths"].values():
        shutil.copy2(p, os.path.join(backup, "design", os.path.basename(p)))
    if ds["top"] != ds["orig_top"]:
        _patch_file(ds["dpath"], ds["texts"]["_top"], ds["top"])
    for name, p in ds["paths"].items():
        if ds["frags"][name] != ds["orig"][name]:
            _patch_file(p, ds["texts"][name], ds["frags"][name])
    return 0


def _variant(a, root: str, ds: dict, log: list, out: str) -> tuple[str, dict]:
    """Write the patched copy to out. Returns (its top design path, its top design)."""
    _say_log(log, out)
    os.makedirs(os.path.join(out, "design"), exist_ok=True)
    copies = {}
    for name, p in ds["paths"].items():
        copies[p] = os.path.join(out, "design", os.path.basename(p))
        _patch_file(copies[p], ds["texts"][name], ds["frags"][name])
    top_copy = copy.deepcopy(ds["top"])
    if "include" in top_copy:
        top_copy["include"] = [copies[_abs(root, inc)] for inc in top_copy["include"]]
    cues = a.cues or ds["top"].get("cues")
    if cues:
        top_copy["cues"] = _abs(root, cues)
    dtop = os.path.join(out, "sounddesign.json")
    _patch_file(dtop, ds["texts"]["_top"], top_copy)
    return dtop, top_copy


def _render(a, root: str, out: str, dtop: str) -> int:
    cmd = [sys.executable, "-I", os.path.join(HERE, "mix.py"), "--root", root, "--design", dtop,
           "--out", os.path.join(out, "soundtrack.wav"), "--stems", os.path.join(out, "stems"),
           "--report", os.path.join(out, "report.json"), "--cues-out", os.path.join(out, "cues.json"),
           "--trace", os.path.join(out, "adds.json")] + (["--cache", a.cache] if a.cache else [])
    r = subprocess.run(cmd, capture_output=True, text=True)
    say(r.stdout[-1200:].rstrip())
    if r.stderr.strip():
        say(r.stderr[-800:].rstrip(), file=sys.stderr)
    return r.returncode


def _qa(a, root: str, out: str, dtop: str, cues) -> int:
    cmd = [sys.executable, "-I", os.path.join(HERE, "qa.py"), "--root", root, "--design", dtop,
           "--report", os.path.join(out, "report.json"), "--trace", os.path.join(out, "adds.json"),
           "--stems", os.path.join(out, "stems"), "--master", os.path.join(out, "soundtrack.wav"),
           "--out", os.path.join(out, "qa.json"), "--cues-sounded", os.path.join(out, "cues_sounded.json")]
    cmd += (["--cues", cues] if cues else []) + (["--cache", a.cache] if a.cache else [])
    r = subprocess.run(cmd, capture_output=True, text=True)
    say(r.stdout.rstrip())
    if r.returncode == 2:
        say(r.stderr.rstrip(), file=sys.stderr)
        return 2
    return 0


def _out_variant(a, root: str, ds: dict, log: list) -> int:
    out = os.path.abspath(a.out)
    dtop, top_copy = _variant(a, root, ds, log, out)
    code = _render(a, root, out, dtop) if a.render or a.qa else 0
    if code != 0 or not a.qa:
        return code
    return _qa(a, root, out, dtop, top_copy.get("cues") if a.cues or ds["top"].get("cues") else None)


def _ops_log(a, ds: dict) -> list:
    log = []
    for op in load_ops(a.ops):
        apply_op(op, ds["frags"], ds["top"], log)
    return log


def run(a) -> int:
    root = os.path.abspath(a.root)
    try:
        ds = _load(a, root)
        try:
            log = _ops_log(a, ds)
        except OpError as e:
            say(f"nothing written: {e}")
            return 1
        return _apply(a, root, ds, log) if a.apply else _out_variant(a, root, ds, log)
    except RunError as e:
        say(f"variant.py: {e}", file=sys.stderr)
        return 1


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


HAND = ('{\n  "_doc": "ui fragment",\n  "rules": [\n'
        '    {"name": "ui.click", "pool": "ui.tap",  "gain_db": -6,   "_doc": "one voice"},\n'
        '    {"name": "ui.pops",  "pool": "ui.pop",  "gain_db": -1.50}\n  ],\n'
        '  "samples": {"ui.tap": {"file": "a.wav", "eq": {"hp": 120}}}\n}\n')
TOP = '{"duration": 4.0,\n "include": ["audio/design/ui.json"],\n "master": {"lufs": -14}}\n'


def _quiet(argv, err=False):
    """main(argv) with stdout (and stderr when err) captured. Returns (code, stdout, stderr)."""
    import contextlib
    import io
    o, e = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(o), (contextlib.redirect_stderr(e) if err else contextlib.nullcontext()):
        code = main(argv)
    return code, o.getvalue(), e.getvalue()


def _test_variant(ck, d: str) -> None:
    code, _, _ = _quiet(["--root", d, "--ops", '[{"f": "ui", "rule": "ui.click", "set": {"gain_db": -3}}]',
                         "--out", os.path.join(d, "v1")])
    new = _read(os.path.join(d, "v1", "design", "ui.json"))
    dl = [ln for ln in difflib.unified_diff(HAND.splitlines(), new.splitlines(), lineterm="", n=0)
          if ln.startswith(("+", "-")) and not ln.startswith(("+++", "---"))]
    ck("S13 a 1-line diff, only gain_db", code == 0 and len(dl) == 2 and '"gain_db": -3,' in dl[1]
       and "-1.50" in new)
    vt = json.load(open(os.path.join(d, "v1", "sounddesign.json")))
    ck("S13 the copy's include paths are absolute", all(os.path.isabs(p) for p in vt["include"]))
    ck("S13 the real design is untouched", _read(os.path.join(d, "audio", "design", "ui.json")) == HAND)
    _test_refusal(ck, d, new)


def _test_refusal(ck, d: str, new: str) -> None:
    before = sorted(os.listdir(os.path.join(d, "v1", "design")))
    code, out, _ = _quiet(["--root", d, "--ops", '[{"f": "ui", "rule": "nope", "set": {"gain_db": 0}}]',
                           "--out", os.path.join(d, "v1")])
    ck("S14 a missing rule writes nothing", code == 1 and "nothing written" in out
       and _read(os.path.join(d, "v1", "design", "ui.json")) == new
       and sorted(os.listdir(os.path.join(d, "v1", "design"))) == before)


def _test_apply(ck, d: str) -> None:
    ops = ('[{"f": "ui", "sample": "ui.tap", "eq": {"highshelf": [6000, -3]}},'
           ' {"f": "ui", "rule_add": {"name": "ui.new", "pool": "ui.tap"}},'
           ' {"f": "ui", "rule": "ui.pops", "delete": true}, {"sd": "buses.music.auto", "value": [[0, 0]]},'
           ' {"f": "ui", "pool": "ui.p2", "value": ["ui.tap"]}]')
    bk = os.path.join(d, "out", "bk")
    code, _, _ = _quiet(["--root", d, "--ops", ops, "--apply", "--backup", bk])
    fr = json.load(open(os.path.join(d, "audio", "design", "ui.json")))
    tp = json.load(open(os.path.join(d, "audio", "sounddesign.json")))
    ck("apply: eq merge, add, delete, pool and sd", code == 0
       and fr["samples"]["ui.tap"]["eq"] == {"hp": 120, "highshelf": [6000, -3]}
       and [r["name"] for r in fr["rules"]] == ["ui.click", "ui.new"] and fr["pools"]["ui.p2"] == ["ui.tap"]
       and tp["buses"]["music"]["auto"] == [[0, 0]])
    ck("apply: the backup holds the originals",
       _read(os.path.join(bk, "design", "ui.json")) == HAND and _read(os.path.join(bk, "sounddesign.json")) == TOP)
    code, _, err = _quiet(["--root", d, "--ops", "[]", "--apply", "--backup", bk], err=True)
    ck("S15 a second apply with the same backup refuses", code == 1 and "new --backup" in err)
    _, out, _ = _quiet(["--root", d, "--ops", '[{"f": "ui", "rule": "ui.click", "set": {"gain_db": -6}}]',
                        "--out", os.path.join(d, "v2")])
    ck("unchanged is reported", "unchanged (-6)" in out)


def _test_empty_variant(ck, fx, d: str) -> None:
    """W4: an empty variant renders bit-identically to the base."""
    import mix
    p = fx.project(os.path.join(d, "film"))
    import contextlib
    import io
    with contextlib.redirect_stdout(io.StringIO()):
        mix.main(["--root", p["root"], "--trace", "out/qa/adds.json"])
    code, _, _ = _quiet(["--root", p["root"], "--ops", "[]", "--out", os.path.join(p["root"], "out", "v0"),
                         "--render"])
    base = open(os.path.join(p["root"], "out", "audio", "soundtrack.wav"), "rb").read()
    var = open(os.path.join(p["root"], "out", "v0", "soundtrack.wav"), "rb").read()
    ck("W4 an empty variant is bit-identical to the base render", code == 0 and base == var)


def self_test() -> int:
    import tempfile
    sys.path.insert(0, HERE)
    import fixtures as fx
    ck = fx.Checker()
    with tempfile.TemporaryDirectory() as d:
        _write(os.path.join(d, "audio", "design", "ui.json"), HAND)
        _write(os.path.join(d, "audio", "sounddesign.json"), TOP)
        _test_variant(ck, d)
        _test_apply(ck, d)
        _test_empty_variant(ck, fx, d)
    return ck.code()


if __name__ == "__main__":
    sys.exit(main())
