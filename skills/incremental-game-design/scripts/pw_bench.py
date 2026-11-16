#!/usr/bin/env python3
"""The tuning bench: install it in a Godot project, find each number of the game, and run it with no window.

references/tuning-bench.md is the text of this file. The bench is a panel in the game. This file does the part
that needs no Godot (install, inventory, init, check, corecheck) and starts Godot for the part that needs it
(selfcheck, run).

    pw_bench.py install <project> [--addon DIR]
    pw_bench.py init <project> --phases-only [--targets <targets.json>] [--length-h HOURS]
    pw_bench.py inventory <project> [--all]
    pw_bench.py confirm <project> [--file GLOB] [--class CLASS] [--rule RULE] [--id ID ...] [--set-class CLASS]
                                  [--role ROLE] [--element ELEMENT] [--phase PHASE] [--sense 1|-1]
    pw_bench.py confirm <project> --from <rules.json> [--file GLOB] [--class CLASS] [--rule RULE] [--id ID ...]
    pw_bench.py confirm <project> [--text-id ID ...] [--text-file GLOB] --ok REASON
    pw_bench.py init <project> [--refresh-controls] [--targets <targets.json>] [--length-h HOURS]
    pw_bench.py check <project> [--strict] [--all]
    pw_bench.py corecheck <project> [--core <dir or file> ...]
    pw_bench.py selfcheck <project> [--only GLOB] [--jobs N] [--godot PATH] [--out FILE] [--home DIR]
                                    [--timeout S]
    pw_bench.py run <project> [--setting FILE] [--k CONTROL=K ...] [--check FILE] [--compare RESULT_JSON]
                              [--godot PATH] [--out FILE] [--home DIR] [--timeout S]

The order of the commands: install, then the adapter, then `init --phases-only` (the phases, the players and the
targets, and no control), then the first run, then inventory, then classify and confirm, then `init` (the
controls), then check, then selfcheck. After a change of the classes, `init --refresh-controls` writes the
generated controls again from the present ledger and keeps each control that a person edited or added.

The inventory is mechanical and complete. It writes each numeric leaf of each data file (.json, .tres, .cfg, .csv),
of each scene (.tscn) and of project.godot, and each numeric literal of each .gd and .gdshader file into the ledger
`numbers` of bench/bench.json. It does not select what looks like balance. It only proposes a class
(`"proposed": true`, with the rule in `"rule"`). `confirm` removes the mark from the numbers that you select, after
you read their file. `check` fails on a proposal that is not confirmed in a file that the economy reads, and
`check --strict` fails on each proposal that is not confirmed in any file. inventory, confirm and check write the
count of the open proposals into the key `review` of the bench file.

A number can also be in a text ("+25% pot value per level"). The inventory writes each string literal of a .gd
file and each string value of a .tscn or .tres file that has a digit into the list `texts` of the bench file,
and leaves out what is clearly no text for a player (TEXT_RULES). A data file (.json, .csv, .cfg) gives each
string value with a digit that looks like a text for a player (player_text); project.godot gives none. `confirm --text-id` or `--text-file` with
`--ok <reason>` accepts a text, and `check --strict` fails on each text that nobody accepted.

2 optional keys of bench.json change the inventory. `"skip": ["docs/", "tests/*.gd"]` names folders or globs that
the inventory does not read. `"words": {"balance": ["excitement"], "presentation": ["glaze"]}` names words of this
game: a name with such a word gets that class as a proposal.

The proposal rules are in the functions propose_data and propose_gd, and in the tables above them.

corecheck is a static lint of the game core (references/game-core.md): the core holds the math as static
functions over plain state, so it uses no scene, no clock, no file, no hidden random number and no state of its
own. The comment `# corecheck: ok <reason>` at the end of a source line accepts the findings of that line.

Exit code 0: ok. 1: a gate failed (check, corecheck, selfcheck, a meter of run, other bytes with run --compare).
2: bad input, or Godot did not run or was stopped.
"""

import argparse
import bisect
import collections
import csv
import fnmatch
import hashlib
import io
import json
import math
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

FORMAT = "tuning-bench"
VERSION = 1
BENCH_JSON = "bench/bench.json"
ADAPTER = "bench/adapter.gd"
ADDON = "addons/tuning_bench"
ADDON_SOURCE = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", "godot",
                            "tuning_bench")
AUTOLOAD_NAME = "TuningBench"
AUTOLOAD_LINE = 'TuningBench="*res://addons/tuning_bench/bench.gd"'
CLI_SCRIPT = "res://addons/tuning_bench/bench_cli.gd"
DEFAULT_CORE = "core"

DATA_KINDS = (".json", ".tres", ".cfg", ".csv")         # a file of these kinds can be a source of the bench
SCENE_KINDS = (".tscn", ".godot")                       # a scene and project.godot: the text format of a .tres file
CODE_KIND = ".gd"
SHADER_KIND = ".gdshader"
CODE_KINDS = (CODE_KIND, SHADER_KIND)
VARIANT_KINDS = (".tres", ".cfg") + SCENE_KINDS
KINDS = DATA_KINDS + SCENE_KINDS + CODE_KINDS
NOT_SCANNED = (".cs",)                                  # a kind with numbers that the inventory does not read
CONTRACT_SKIPS = (".godot", "addons", "bench/settings")
CLASSES = ("balance", "structure", "presentation", "engine", "unclassified")
SELFCHECK_JSON = "bench/selfcheck.json"                # the result of the self-check; the addon reads it for M8
ROLES = ("cost_base", "cost_growth", "effect", "effect_factor", "effect_growth", "effect_exponent", "base_income",
         "active_power", "active_time", "wall", "wall_growth", "unlock_at", "reset_need", "reset_gain", "reset_bonus",
         "cap", "timer", "rate", "auto_at", "offline", "reward", "reward_growth", "end_at", "other")
SENSES = (1, -1)                                      # "sense": -1: a control with the factor k applies 1/k
IN_CODE = "in code; lift to data"
# The keys that the inventory writes again on each run. Each other key of a number is a decision of the user.
MACHINE_KEYS = ("id", "file", "pointer", "line", "index", "text", "value", "quoted", "anchor")
PROPOSAL_KEYS = ("class", "role", "proposed", "rule", "hint", "sense")
LIST_MAX = 40                                         # the lines of 1 printed list without --all


class InputError(Exception):
    """Bad input: exit code 2."""


def check(bad, message):
    """Stop with exit code 2 when bad is true."""
    if not bad:
        return
    raise InputError(message)


def ext(path):
    return os.path.splitext(path)[1].lower()


def say(text):
    sys.stdout.write(text + "\n")


def say_lines(lines):
    """Print each line. An empty list prints nothing."""
    sys.stdout.write("".join(line + "\n" for line in lines))


def one_line(value):
    return json.dumps(value, ensure_ascii=False)


def scalar(value):
    return value if isinstance(value, str) else one_line(value)


def count_by(items, key):
    return collections.Counter(map(key, items))


def counts_text(counts, order=None):
    names = [n for n in (order or sorted(counts)) if counts.get(n)]
    return ", ".join("%s %d" % (n, counts[n]) for n in names) or "none"


def class_of(number):
    return number.get("class", "unclassified")


def file_of(number):
    return str(number.get("file", "?"))


# ----------------------------------------------------------------------------- the GDScript literals
GD_TOKEN = re.compile(r'''
    (?P<comment>\#[^\n]*)
  | (?P<string>"""(?:\\.|[^\\])*?"""|\'\'\'(?:\\.|[^\\])*?\'\'\'|"(?:\\.|[^"\\\n])*"|'(?:\\.|[^'\\\n])*')
  | (?P<number>0[xX][0-9a-fA-F_]+|0[bB][01_]+|(?:\d[\d_]*(?:\.(?!\.)[\d_]*)?|\.\d[\d_]*)(?:[eE][+-]?\d[\d_]*)?)
  | (?P<name>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<cont>\\\r?\n)
  | (?P<newline>\n)
  | (?P<space>[ \t\r]+)
  | (?P<op>\*\*=?|<<=?|>>=?|[-+*/%&|^<>=!]=|:=|->|&&|\|\||.)
''', re.X | re.S)
# A shader has the comments // and /* */, and a number can end with f or u.
SHADER_TOKEN = re.compile(r'''
    (?P<comment>//[^\n]*|/\*.*?\*/)
  | (?P<string>"(?:\\.|[^"\\\n])*")
  | (?P<number>(?:0[xX][0-9a-fA-F]+|(?:\d+(?:\.(?!\.)\d*)?|\.\d+)(?:[eE][+-]?\d+)?)[fFuU]?)
  | (?P<name>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<cont>\\\r?\n)
  | (?P<newline>\n)
  | (?P<space>[ \t\r]+)
  | (?P<op>[-+*/%&|^<>=!]=|&&|\|\||.)
''', re.X | re.S)
ID_FORMAT = re.compile(r"[\w/.:-]*%0?\d*d[\w/.:-]*")       # a text that makes an id: "town%d", "glaze%02d"
# After these words a minus sign is a sign, not a subtraction.
GD_NO_OPERAND = frozenset("return in and or not if elif else while for match when await assert yield is as".split())
GD_ASSIGN = frozenset(("=", ":=", "+=", "-=", "*=", "/="))
GD_BRACKETS = {"(": "open_bracket", "[": "open_bracket", "{": "open_bracket", ")": "close_bracket",
               "]": "close_bracket", "}": "close_bracket"}
NUMBER_SHAPE = re.compile(r"(?<![\w.])-?(?:0[xX][0-9a-fA-F_]+|\d[\d_]*\.?[\d_]*(?:[eE][+-]?\d+)?|\.\d+)")
NO_TOKEN = (None, None)


def line_starts(text):
    """The position of each line end. bisect_left of a position in this list, plus 1, is its line number."""
    return [match.start() for match in re.finditer("\n", text)]


def gd_value(text):
    """The value of a numeric literal of GDScript (1_000, 1e6, 0xFF, 0b101, .5, 1.) or of a shader (1.0f, 2u)."""
    low = text.replace("_", "").lower().rstrip("u")
    if low.startswith(("0x", "0b")):
        return int(low[2:], 16 if low[1] == "x" else 2)
    low = low.rstrip("f")
    return float(low) if "." in low or "e" in low else int(low)


def unquote(text):
    """The content of a string literal of GDScript: the text with no quotes."""
    edge = 3 if len(text) >= 6 and text[:3] in ('"""', "'''") else 1
    return text[edge:-edge]


def gd_operand(token):
    """True when a value can end with this token. A minus sign after it is a subtraction."""
    kind, text = token or NO_TOKEN
    if kind == "name":
        return text not in GD_NO_OPERAND
    return kind in ("number", "string") or text in (")", "]", "}")


class GdScan:
    """Read GDScript source and collect each numeric literal that is not in a string or a comment.

    A minus sign is part of the literal only when it is unary: the token before it cannot end a value.
    Each literal has its line, its index in the line (from 1), the text of the line, and the context that the
    proposal rules read: the token before, the token after, the call and the names around it.
    """

    def __init__(self, text, pattern=GD_TOKEN):
        self.lines = text.split("\n")
        self.starts = line_starts(text)
        self.fmt = False                # the statement has a text that makes an id ("town%d")
        self.line = 1
        self.count = {}                 # line -> count of literals so far
        self.seen = {}                  # line -> count of string literals so far
        self.hist = []                  # the last tokens of the statement that are not space
        self.stack = []                 # open brackets: [bracket, name of the call, is a subscript, first string]
        self.decl = self.func = None    # the name of the var or const statement, and of the function
        self.const = False              # the statement is a const statement
        self.bol = True                 # at the first column of a statement line
        self.wait = None                # the context of the literal that waits for its next token
        self.tail = None                # the string literal that waits for its next token
        self.strings = []               # each string literal: line, index in the line, text, and its context
        self.out = []
        for match in pattern.finditer(text):
            self.token(match)

    def token(self, match):
        self.line = bisect.bisect_left(self.starts, match.start()) + 1
        getattr(self, "on_" + match.lastgroup)(match.group())

    def on_comment(self, text):
        pass

    def on_cont(self, text):
        pass

    def on_space(self, text):
        self.bol = False

    def on_newline(self, text):
        if self.stack:
            return
        self.push(("newline", "\n"))
        self.hist, self.decl, self.const, self.bol, self.fmt = [], None, False, True, False

    def feed_wait(self, text):
        if self.wait is not None:
            self.wait["next"] = text
        if self.tail is not None:
            self.tail["next"] = text
        self.wait = self.tail = None

    def push(self, token):
        """Add a token to the history of the statement, and give it to the literal that waits for it."""
        self.feed_wait(token[1])
        blank = token[0] == "newline"
        self.func = None if self.bol and not blank else self.func
        self.bol = self.bol and blank
        self.hist = self.hist[-5:] + [token]

    def last(self, back=1):
        return self.hist[-back] if len(self.hist) >= back else None

    def top(self):
        return self.stack[-1] if self.stack else [None, None, False, None]

    def on_string(self, text):
        plain = text.strip("\"'")
        top = self.top()
        top[3] = plain if top[3] is None else top[3]
        self.fmt = self.fmt or bool(ID_FORMAT.fullmatch(plain))
        found = self.string_entry(text)
        self.push(("string", plain))
        self.tail = found

    def string_entry(self, text):
        """Record 1 string literal with what the text rules read: the token before it, the token 2 before it,
        the call around it, and (later) the token after it."""
        index = self.seen[self.line] = self.seen.get(self.line, 0) + 1
        call = next((frame[1] for frame in reversed(self.stack) if frame[0] == "("), None)
        entry = dict(line=self.line, index=index, text=unquote(text), prev=(self.last() or NO_TOKEN)[1],
                     prev2=self.last(2), call=call, next="\n", sub=self.top()[2])
        self.strings.append(entry)
        return entry

    def on_name(self, text):
        before = self.last() or NO_TOKEN
        if before[0] == "name" and before[1] in ("var", "const"):
            self.decl, self.const = text, before[1] == "const"
        self.func = text if before == ("name", "func") else self.func
        self.push(("name", text))

    def on_op(self, text):
        before = self.last()
        getattr(self, GD_BRACKETS.get(text, "no_bracket"))(text, before)
        self.push(("op", text))

    def no_bracket(self, text, before):
        pass

    def open_bracket(self, text, before):
        call = before[1] if text == "(" and before and before[0] == "name" else None
        call = "enum" if text == "{" and ("name", "enum") in self.hist[-2:] else call
        self.stack.append([text, call, text == "[" and gd_operand(before), None])

    def close_bracket(self, text, before):
        del self.stack[-1:]

    def negative(self):
        return self.last() == ("op", "-") and not gd_operand(self.last(2))

    def on_number(self, text):
        negative = self.negative()
        value = -gd_value(text) if negative else gd_value(text)
        index = self.count[self.line] = self.count.get(self.line, 0) + 1
        ctx = self.context(self.hist[:-1] if negative else self.hist)
        self.push(("number", text))
        self.wait = ctx
        self.out.append(dict(line=self.line, index=index, text=self.lines[self.line - 1].strip(), value=value,
                             ctx=ctx))

    def context(self, hist):
        top = self.top()
        call = next((frame[1] for frame in reversed(self.stack) if frame[0] == "("), None)
        return dict(prev=hist[-1][1] if hist else None, next=None, key=self.key(hist, top), decl=self.decl,
                    func=self.func, call=call, sub=top[2], arg=top[0] == "(", dict=top[0] == "{",
                    enum=top[1] == "enum", const=self.const, fmt=self.fmt)

    def key(self, hist, top):
        """The name that the literal belongs to: a dictionary key, the first argument of get, or the name left
        of the assignment."""
        t4, t3, t2, t1 = ([NO_TOKEN] * 4 + hist)[-4:]
        if t1[1] == ":" and top[0] == "{":
            return t2[1] if t2[0] in ("name", "string") else None
        if t1[1] == "," and top[1] == "get":
            return top[3]
        return self.assigned(t4, t3, t2) if t1[1] in GD_ASSIGN else None

    def assigned(self, t4, t3, t2):
        if self.decl and not self.stack:
            return self.decl
        if t3[1] == ":" and t4[0] == "name" and t2[0] == "name":      # name: type = literal
            return t4[1]
        subscript = t3[1] if t2[1] == "]" and t3[0] == "string" else None
        return t2[1] if t2[0] in ("name", "string") else subscript


def gd_literals(text):
    """Return the list of the numeric literals of GDScript source, in the order of the file."""
    return GdScan(text).out


def shader_literals(text):
    """Return the list of the numeric literals of a .gdshader source, in the order of the file."""
    return GdScan(text, SHADER_TOKEN).out


def gd_tokens(text):
    """The tokens of GDScript source as (kind, text, line), with no space and no comment. A string is 1 token."""
    starts = line_starts(text)
    found = [m for m in GD_TOKEN.finditer(text) if m.lastgroup not in ("space", "comment", "cont")]
    return [(m.lastgroup, m.group(), bisect.bisect_left(starts, m.start()) + 1) for m in found]


def shape(text):
    """The text of a line with each number replaced: 2 lines with the same shape differ only in their values."""
    return NUMBER_SHAPE.sub("#", text)


# ----------------------------------------------------------------------------- the data files
NUMERIC_TEXT = re.compile(r"[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?")
ID_KEYS = ("id", "key", "name")


def escape(token):
    """Escape 1 reference token of a JSON pointer (RFC 6901)."""
    return str(token).replace("~", "~0").replace("/", "~1")


def pointer(tokens):
    return "".join("/" + escape(t) for t in tokens)


def text_number(text):
    """The value of a text that is a number and nothing else, or None."""
    text = str(text).strip()
    if not NUMERIC_TEXT.fullmatch(text):
        return None
    return float(text) if re.search(r"[.eE]", text) else int(text)


def is_id(value):
    return (isinstance(value, str) and value != "") or (isinstance(value, int) and not isinstance(value, bool))


def element_id(obj):
    """The id of a JSON object: the value of its key id, key or name, when it is text or an integer."""
    values = [obj.get(key) for key in ID_KEYS] if isinstance(obj, dict) else []
    return next((str(value) for value in values if is_id(value)), None)


def json_leaf(node):
    """Return (value, quoted) for a leaf of JSON. The value is None when the leaf is not a number. A text that
    is a number and nothing else is a quoted number."""
    if isinstance(node, str):
        return text_number(node), True
    plain = isinstance(node, (int, float)) and not isinstance(node, bool) and math.isfinite(node)
    return (node if plain else None), False


def list_child(index, child, tokens, anchor, element):
    cid = element_id(child)
    return child, tokens + [index], anchor + ["[%s]" % cid if cid else index], cid or element, None


def json_children(node, tokens, anchor, element):
    """The children of a JSON container as (child, tokens, anchor tokens, element, the object around the child
    or None)."""
    if isinstance(node, dict):
        return [(value, tokens + [key], anchor + [key], element, node) for key, value in node.items()]
    return [list_child(i, child, tokens, anchor, element) for i, child in enumerate(node)]


def json_entry(node, tokens, anchor, element, parent):
    value, quoted = json_leaf(node)
    names = [token for token in reversed(tokens) if isinstance(token, str)]
    entry = dict(pointer=pointer(tokens), value=value, names=names, element=element, beside=parent or {})
    entry.update({"quoted": True} if quoted else {})
    entry.update({"anchor": pointer(anchor)} if pointer(anchor) != entry["pointer"] else {})
    return entry


def json_walk(node, tokens, anchor, element, parent=None):
    if not isinstance(node, (dict, list)):
        return [json_entry(node, tokens, anchor, element, parent)]
    own = element_id(node) or element
    return [entry for child in json_children(node, tokens, anchor, own) for entry in json_walk(*child)]


def json_numbers(root):
    """Each numeric leaf of parsed JSON, with its pointer.

    The anchor is the pointer with the id of the object in place of each array index. An insert in an array moves
    each pointer after it, and the anchor stays, so the update can follow the number. The names are the object
    keys from the leaf to the root, and `beside` is the object around the leaf: the proposal rules read them.
    """
    return [entry for entry in json_walk(root, [], [], None) if entry["value"] is not None]


def parse_json(text):
    error = None
    try:
        return json.loads(text)
    except ValueError as exc:
        error = str(exc)
    raise InputError("not valid JSON: %s" % error)


def number_keys(node):
    """The count of object keys of parsed JSON that are numbers. A key is not a leaf, so it is not in the ledger."""
    if isinstance(node, dict):
        return sum(text_number(key) is not None for key in node) + sum(map(number_keys, node.values()))
    return sum(map(number_keys, node)) if isinstance(node, list) else 0


VT_TOKEN = re.compile(r'''
    (?P<string>"(?:\\.|[^"\\])*")
  | (?P<comment>[;\#][^\n]*)
  | (?P<number>-?(?:0[xX][0-9a-fA-F]+|(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?)(?![\w.]))
  | (?P<name>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<newline>\n)
  | (?P<space>[ \t\r]+)
  | (?P<op>.)
''', re.X | re.S)
VT_HEADER = re.compile(r'\[\s*([A-Za-z_]\w*)((?:\s+[\w/]+\s*=\s*(?:"[^"\n]*"|[^\s\]]+))*)\s*\][ \t\r]*(?=\n|$)')
VT_ATTR = re.compile(r'([\w/]+)\s*=\s*("[^"\n]*"|[^\s\]]+)')
VT_PLAIN_HEADER = re.compile(r"\[([^\]\n]*)\][ \t\r]*(?=\n|$)")
VT_KEY = re.compile(r'"?([^\s="\[\]{}(),;#]+)"?[ \t]*=')
VT_OPS = {"[": "op_square", "(": "op_round", "{": "op_curly", ")": "op_close", "]": "op_close", "}": "op_close",
          ",": "op_comma", ":": "op_colon"}
VT_NO_FRAME = ("", 0, None, False)


def section_name(word, attrs):
    """The name of a section: <word>:<id> with an id, <word>:<path> for a node of a scene, <word> if not."""
    clean = {key: value.strip('"') for key, value in attrs.items()}
    parent, name = clean.get("parent", ""), clean.get("name", "")
    path = name if parent in ("", ".") else parent + "/" + name
    ident = clean.get("id") or (path if word == "node" else "")
    return word + (":" + ident if ident else "")


def resource_type(word, attrs):
    """The type of a .tres file from its first header: the script class, or the type."""
    return attrs.get("script_class", attrs.get("type", "")).strip('"') if word == "gd_resource" else None


class VariantScan:
    """Read the text format of Godot (.tres, .cfg) and collect each number that is not in a string or a comment.

    The pointer of a number is /<section>/<key>, and then 1 token for each container around the number: the index
    in an array or in the arguments of a constructor (Vector2(1, 2)), or the key in a dictionary. A section of a
    .tres file with an id has the name <word>:<id>, and a node of a scene has the name node:<path>. A number in a
    section header has the pointer /<section>/@<attribute>.
    """

    def __init__(self, text):
        self.text = text
        self.section, self.key, self.stype = "", None, ""
        self.stack = []                 # frames: [kind, index, dictionary key, a key comes next]
        self.before = self.closed = None
        self.skip = 0                   # the tokens before this position are read already
        self.sol = True                 # at the start of a line and outside each container
        self.res_type = None            # the type of the resource of a .tres file
        self.pos = 0                    # the position of the present token
        self.strings = []               # each string value of a key line: position, key, text
        self.out = []
        for match in VT_TOKEN.finditer(text):
            self.token(match)

    def token(self, match):
        kind = match.lastgroup
        self.pos = match.start()
        starts = self.sol and kind not in ("newline", "comment", "space")
        read = match.start() < self.skip or (starts and self.line_start(match.start()))
        if not read:
            getattr(self, "on_" + kind)(match.group())

    def line_start(self, pos):
        """Read a section header or the key of a line. Return True when the line start was one of them."""
        self.sol = False
        head = VT_HEADER.match(self.text, pos) or VT_PLAIN_HEADER.match(self.text, pos)
        key = None if head else VT_KEY.match(self.text, pos)
        (self.header if head else self.key_line if key else self.no_start)(head or key)
        return bool(head or key)

    def no_start(self, found):
        pass

    def key_line(self, key):
        self.key, self.skip, self.stack = key.group(1), key.end(), []

    def header(self, head):
        attrs = dict(VT_ATTR.findall(head.group(2))) if head.re is VT_HEADER else {}
        word = head.group(1).strip()
        self.section, self.key = section_name(word, attrs), None
        self.stype = attrs.get("type", "").strip('"') if word == "sub_resource" else ""
        self.skip, self.sol = head.end(), True
        self.res_type = self.res_type or resource_type(word, attrs)
        self.out += [dict(pointer=pointer([self.section, "@" + name]), value=text_number(raw), names=[], attr=True)
                     for name, raw in attrs.items() if text_number(raw) is not None]

    def note(self, kind):
        self.before, self.closed = kind, None

    def on_comment(self, text):
        pass

    def on_space(self, text):
        pass

    def on_newline(self, text):
        self.sol = not self.stack

    def dict_key(self, text):
        """Take the token as a dictionary key when a key comes next. Return True when it was a key."""
        top = self.stack[-1] if self.stack else VT_NO_FRAME
        is_key = top[0] == "{" and top[3]
        if is_key:
            top[2], top[3] = text, False
        return is_key

    def on_string(self, text):
        value = not self.dict_key(text[1:-1]) and self.key is not None
        calls = [frame for frame in self.stack if frame[0] in ("(", "x")]
        if value and not calls:
            self.strings.append(dict(pos=self.pos, key=self.key, text=text[1:-1], prev=self.text[self.pos - 1:self.pos]))
        self.note("string")

    def on_name(self, text):
        self.dict_key(text)
        self.note("name")

    def on_number(self, text):
        if not self.dict_key(text):
            self.out.append(self.number(text))
        self.note("number")

    def number(self, text):
        frames = [frame for frame in self.stack if frame[0] in ("{", "[", "(")]
        path = [frame[2] if frame[0] == "{" else frame[1] for frame in frames]
        value = int(text, 16) if "x" in text.lower() else text_number(text)
        tokens = [self.section, "?" if self.key is None else self.key] + path
        return dict(pointer=pointer(tokens), value=value, names=self.names(frames), key=self.key,
                    section=self.section, stype=self.stype)

    def names(self, frames):
        """The names around a number, from the nearest: the dictionary keys, the key of the line, the section."""
        found = [frame[2] for frame in reversed(frames) if frame[0] == "{"] + [self.key, self.section]
        return [name for name in found if name]

    def on_op(self, text):
        closed = getattr(self, VT_OPS.get(text, "op_other"))()
        self.note("op")
        self.closed = closed

    def op_other(self):
        return None

    def op_square(self):
        self.stack.append(["t" if self.before == "name" else "[", 0, None, False])

    def op_round(self):
        self.stack.append(["x" if self.closed == "t" else "(", 0, None, False])

    def op_curly(self):
        self.stack.append(["{", 0, "?", True])

    def op_close(self):
        return self.stack.pop()[0] if self.stack else None

    def op_comma(self):
        top = self.stack[-1] if self.stack else list(VT_NO_FRAME)
        top[1], top[3] = top[1] + 1, True

    def op_colon(self):
        top = self.stack[-1] if self.stack else list(VT_NO_FRAME)
        top[3] = False


def variant_numbers(text):
    """Each number of a .tres or .cfg text, and the type of the resource."""
    scan = VariantScan(text)
    return scan.out, scan.res_type


def csv_cell(row, column, cell, head):
    name = head[column] if column < len(head) else column
    names = [name] if isinstance(name, str) else []
    return dict(pointer=pointer([row, name]), value=text_number(cell), names=names)


def csv_numbers(text):
    """Each numeric cell of a CSV text. The pointer is /<row>/<column>: the row from 0, the column name or index.

    The first row gives the column names when it has no numeric cell and no empty cell, and no 2 cells are the
    same.
    """
    rows = list(csv.reader(io.StringIO(text)))
    first = rows[0] if rows else []
    named = all(first) and len(set(first)) == len(first) and all(text_number(c) is None for c in first)
    head = first if named else []
    cells = [csv_cell(r, c, cell, head) for r, row in enumerate(rows) for c, cell in enumerate(row)]
    return [cell for cell in cells if cell["value"] is not None]


# ----------------------------------------------------------------------------- the proposal rules
# A proposal is a help, not a decision. Each rule is simple, and the entry names the rule that made it.
# The words of a name are its parts in lower case (max_level -> max, level; baseCost -> base, cost). A word
# matches with or without a last "s".
PRES_STRONG = frozenset("""color colour font alpha opacity margin padding anim animation tween fade flash shake blink
    volume pitch pixel px db hue saturation brightness modulate shadow outline glow particle sfx vfx sound audio z zoom
    icon sprite texture frame fps popup toast tooltip label panel button screen viewport""".split())
PRES_WEAK = frozenset("""size width height pos position offset scale x y w h radius rotation angle spacing separation
    thickness border corner indent layer align""".split())
STRUCTURE = frozenset("id index idx order sort version format slot tier stage row col column page".split())
PRES_FOLDERS = frozenset("""ui gui hud menu menus theme themes font fonts audio sound sounds music sfx vfx fx shader
    shaders art juice particles anim animation animations""".split())
# The keys of a scene that are layout or look. Each other key of a scene gets no proposal: a property of a script
# that a scene sets is a number of the game that hides.
SCENE_LAYOUT = re.compile(r"(?:offset|anchor|anchors|grow|size_flags|theme_override|custom_minimum|pivot|layout|z)_\w+"
                          r"(?:/\w+)*|position|size|scale|rotation|skew|color|modulate|self_modulate|\w*alignment"
                          r"|\w+_mode|mouse_filter|texture_filter|texture_repeat")
PRES_TYPES = re.compile(r"Theme|StyleBox|Font|LabelSettings|Gradient|Material|Animation|SpriteFrames|AudioBus"
                        r"|Environment|Texture|TileSet|Mesh")
PRES_CALLS = re.compile(r"Color8?|Vector[234]i?|Rect2i?|tween_\w+|add_theme_\w+|set_anchor\w*|set_offset\w*")
ENGINE_FILES = ("export_presets.cfg",)

COST = frozenset(("cost", "price"))
GROWTH = frozenset("growth grow ratio scale scaling exponent factor increase".split())
COST_GROWTH = GROWTH | frozenset(("mult", "multiplier", "rate"))
WALL = frozenset("wall gate goal requirement require required req".split())
UNLOCK = frozenset(("unlock", "reveal"))
RESET = frozenset("prestige reset rebirth ascend ascension".split())
NEED = frozenset("need min threshold require requirement required req at".split())
GAIN = frozenset("gain divisor div exponent".split())
BONUS = frozenset("bonus mult multiplier boost effect".split())
RESET_REWARD = frozenset(("reward", "bonus", "boost", "effect"))      # what the reset currency gives
OFFLINE = frozenset(("offline", "idle", "away"))
AUTO = frozenset(("auto", "automation", "autobuy"))
START = frozenset(("at", "start", "threshold"))
MANUAL = frozenset(("click", "tap", "manual", "active"))
TIME = frozenset("cooldown window duration time interval second sec".split())
POWER = frozenset("power value gain amount mult multiplier bonus boost strength".split())
CAP = frozenset(("cap", "capacity"))
MAX = frozenset(("max", "maximum", "limit"))
CAP_OF = frozenset("level count stack amount owned rank storage".split())
REWARD = frozenset(("reward", "payout", "bounty", "prize"))
BASE = frozenset(("base", "start", "starting", "initial"))
INCOME = frozenset(("income", "rate", "production"))
PERSEC = frozenset(("persec",))                       # the word that per_time makes from `per` and a time unit
BASE_OF = INCOME | PERSEC | frozenset(("power", "yield", "output", "dps"))
FINAL = frozenset(("final", "last", "end"))
STEP = frozenset("stage level zone wave floor world era".split())
EXPONENT = frozenset(("exponent",))
DIVISOR = frozenset(("divisor", "div", "denominator", "denom"))
MULTIPLY = frozenset("mult multiplier multiply mul factor x times".split())
KIND_KEYS = ("type", "op", "kind", "operation", "mode")       # the field of an effect that says what it does
FACTOR_RANGE = (1.0, 3.0)                             # a factor of income for 1 level or 1 purchase
PER_UNIT = frozenset("s sec second min minute hour tick".split())
UNIT_NAMES = {"s": "second", "sec": "second", "second": "second", "ms": "ms", "msec": "ms", "millisecond": "ms",
              "min": "minute", "minute": "minute", "hr": "hour", "hour": "hour", "day": "day", "week": "week"}
UNIT_VALUES = frozenset((60, 3600, 86400, 1000, 0.001, 24, 1440, 7))
DURATION = frozenset("s sec second ms msec duration delay time timeout".split())
EFFECT = frozenset("effect mult multiplier bonus boost income production yield output dps power".split())
TIMER = frozenset("cooldown interval period duration timer tick second sec minute hour".split())
LOOSE = frozenset("growth grow rate speed chance probability odd crit".split())
# (a word of this set, and a word of this set or None, the role or None). The first rule that matches gives the
# role of a balance number. The role None means "balance, and the name does not tell the role".
# Some rules come first for a reason. An unlock point of automation (auto_unlock_at) is auto_at, not unlock_at.
# The strength of what a reset gives (prestige_reward_exponent) is reset_bonus, though `exponent` is a gain word.
# A base with a production word (base_spores_per_sec, base_power) is base_income, not a timer and not a growth.
ROLE_RULES = (
    (COST, COST_GROWTH, "cost_growth"), (COST, None, "cost_base"),
    (WALL, GROWTH, "wall_growth"), (WALL, None, "wall"),
    (AUTO, START | UNLOCK, "auto_at"),
    (UNLOCK, None, "unlock_at"),
    (RESET, NEED, "reset_need"), (RESET, RESET_REWARD, "reset_bonus"), (RESET, GAIN, "reset_gain"),
    (RESET, BONUS, "reset_bonus"), (RESET, None, None),
    (OFFLINE, None, "offline"),
    (AUTO, None, None),
    (MANUAL, TIME, "active_time"), (MANUAL, POWER, "active_power"),
    (CAP, None, "cap"), (MAX, CAP_OF, "cap"),
    (REWARD, GROWTH, "reward_growth"), (REWARD, None, "reward"),
    (FINAL, STEP, "end_at"),
    (BASE, BASE_OF, "base_income"),
    (EFFECT, EXPONENT, "effect_exponent"), (EFFECT, GROWTH, "effect_growth"), (EFFECT, None, "effect"),
    (PERSEC, None, "rate"),
    (TIMER, None, "timer"),
    (LOOSE, None, None),
)
# A number of a data file under a key with 1 of these words is a number of a tool or a design note, not of the game.
TOOL_WORDS = frozenset("autosave tolerance target debug version".split())
TOOL_RULE = "tool or note"
FALLBACK_RULE = "fallback copy"
FALLBACK_REASON = "fallback copy of "
COMPARE = frozenset(("==", "!=", "<", ">", "<=", ">="))
UNITS = frozenset((60, 3600, 86400, 1000, 0.001))
WORD = re.compile(r"[A-Za-z][a-z]*")
# GDScript literals with no game meaning: (class, rule, test). The test reads the value v, the context c of the
# literal and its place p. c["prev"] is the token before the literal. The place "start" is after a comparison,
# after `return` or `else`, as an argument of a call, or, outside a const statement, after an assignment or as a
# value of a dictionary. A const with a name is for the name rules.
#   an enum value (enum State { A = 0 }) and a match label (1: at the start of a line): structure
#   an index: 0, 1, 2 or -1 in a subscript (list[0])
#   an increment: 1 after +=, -=, + or - (count += 1, size - 1)
#   a zero: 0 in a place "start" (count > 0, var total := 0, maxf(x, 0.0))
#   a sentinel: -1 in a place "start" (return -1)
#   a fallback: 0 or 1 as the second argument of get (data.get("growth", 1.0))
#   a range start: 0 or 1 as an argument of range; each other literal in the arguments of range: structure
#   a start value: 1 after an assignment in a function (var multiplier := 1.0)
#   a unit conversion: 60, 3600, 86400, 1000 or 0.001 after * or / (seconds / 60.0)
#   an id in a format text: a literal in a statement with a text that makes an id ("town%d" % (n + 2)): structure
ENGINE_RULES = (
    ("structure", "an enum value", lambda v, c, p: c["enum"]),
    ("structure", "a match label", lambda v, c, p: c["prev"] is None and c["next"] == ":"),
    ("engine", "an index", lambda v, c, p: c["sub"] and isinstance(v, int) and v in (0, 1, 2, -1)),
    ("engine", "an increment", lambda v, c, p: v == 1 and c["prev"] in ("+=", "-=", "+", "-")),
    ("engine", "a zero", lambda v, c, p: v == 0 and p["start"]),
    ("engine", "a sentinel -1", lambda v, c, p: v == -1 and p["start"]),
    ("engine", "a fallback of get", lambda v, c, p: v in (0, 1) and c["call"] == "get" and c["prev"] == ","),
    ("engine", "a range start", lambda v, c, p: v in (0, 1) and c["call"] == "range" and c["arg"]),
    ("structure", "a range end", lambda v, c, p: c["call"] == "range"),
    ("engine", "a start value 1 in a function", lambda v, c, p: v == 1 and p["assign"] and bool(c["func"])),
    ("engine", "a unit conversion", lambda v, c, p: v in UNITS and c["prev"] in ("*", "/")),
    ("structure", "an id in a format text", lambda v, c, p: c["fmt"]),
)


def words(name):
    """The words of a name in lower case, each also without a last "s"."""
    parts = [part for part in re.split(r"[^A-Za-z]+", str(name)) if part]
    found = [word.lower() for part in parts for word in ([part] if part.isupper() else WORD.findall(part))]
    return set(found) | {word[:-1] for word in found if word.endswith("s") and len(word) > 3}


def per_time(found):
    """The word `per` with a time unit (spores_per_sec) says a rate, not a time: the unit words go, and the
    word `persec` comes in their place."""
    rate = "per" in found and found & PER_UNIT
    return (found - PER_UNIT) | PERSEC if rate else found


def role_of(found):
    """Return (True, role) for the first role rule that the words match, or (False, None)."""
    found = per_time(found)
    hits = ((True, role) for first, second, role in ROLE_RULES
            if found & first and (second is None or found & second))
    return next(hits, (False, None))


def game_words(data):
    """The words of this game from the key `words` of the bench file: class -> set of words."""
    given = data.get("words") or {}
    fine = isinstance(given, dict) and all(k in CLASSES and isinstance(v, list) for k, v in given.items())
    check(not fine, '"words" of the bench file must map a class (%s) to a list of words' % ", ".join(CLASSES))
    return {cls: {str(word).lower() for word in listed} for cls, listed in given.items()}


def class_of_name(name, extra=None):
    """Propose (class, role) from 1 name, or None. The order: a word of this game (the key `words` of the bench
    file), strong presentation, balance, structure, layout."""
    found = words(name)
    mine = next((cls for cls, listed in (extra or {}).items() if found & listed), None)
    if mine or found & PRES_STRONG:
        return mine or "presentation", None
    balance, role = role_of(found)
    if balance:
        return "balance", role
    if found & STRUCTURE:
        return "structure", None
    return ("presentation", None) if found & PRES_WEAK else None


def class_of_names(names, label, extra=None):
    """Propose from the first name that gives a class. Return (class, role, rule) or None."""
    found = ((name, class_of_name(name, extra)) for name in names if name)
    return next(((hit[0], hit[1], "%s %s" % (label, name)) for name, hit in found if hit), None)


def unit_constant(names, value):
    """A name with `per` and 2 units of time (SECONDS_PER_HOUR) and a value of a conversion is engine."""
    found = words(names[0]) if names else set()
    units = {UNIT_NAMES[word] for word in found if word in UNIT_NAMES}
    hit = "per" in found and len(units) >= 2 and value in UNIT_VALUES
    return ("engine", None, "a unit conversion %s" % names[0]) if hit else None


def effect_names(names):
    """The names to class. A key that is only a multiply sign (x, times) under an effect key gives no class by
    itself: the effect key gives it."""
    sign = len(names) > 1 and words(names[0]) <= {"x", "times"} and words(names[1]) & {"effect"}
    return names[1:] if sign else names


def kind_words(beside):
    """The words of the field beside an effect value that says what the effect does ("kind": "mult")."""
    texts = [value for key, value in beside.items() if key in KIND_KEYS and isinstance(value, str)]
    return set().union(*map(words, texts))


def refine_auto(found, names, value, beside):
    """An unlock point under a key of automation (auto_buy/lamp/unlock_active_minutes) is auto_at."""
    under = next((name for name in names[1:] if words(name) & AUTO), None)
    hit = found[1] == "unlock_at" and under
    return ("balance", "auto_at", "%s under %s" % (found[2], under)) if hit else found


def refine_growth(found, names, value, beside):
    """A growth key with no role beside a cost key is cost_growth."""
    loose = found[:2] == ("balance", None) and words(names[0]) & GROWTH
    cost = next((key for key in beside if key != names[0] and words(key) & COST), None)
    return ("balance", "cost_growth", "%s beside %s" % (found[2], cost)) if loose and cost else found


def refine_effect(found, names, value, beside):
    """An effect whose key or type field says multiply, with a value from 1 to 3, is effect_factor: a control
    changes the part above 1. An effect whose type field says cap is a cap. Each other effect is an amount."""
    if found[1] != "effect":
        return found
    kinds = kind_words(beside)
    if kinds & CAP:
        return "balance", "cap", found[2] + ", an effect of the kind cap"
    factor = (words(names[0]) | kinds) & MULTIPLY and FACTOR_RANGE[0] <= value <= FACTOR_RANGE[1]
    return ("balance", "effect_factor", found[2] + ", a factor from 1 to 3") if factor else found


REFINE = (refine_auto, refine_growth, refine_effect)


def refined(found, names, value, beside):
    """Give the proposal of a name to each rule that reads more than the name: the keys above the number, the
    keys beside it and its value."""
    for rule in REFINE:
        found = rule(found, names, value, beside)
    return found


def sense_of(found, names):
    """-1 for a balance number with a divisor name: a control with the factor k applies 1/k. None if not."""
    hit = found and found[0] == "balance" and names and names[0] and words(names[0]) & DIVISOR
    return -1 if hit else None


def folder_rule(rel, table):
    """The first folder of the path that is in the table, or None."""
    return next((part for part in rel.split("/")[:-1] if part.lower() in table), None)


def presentation_place(rel):
    """A presentation folder, or a word of the file name that is one (hud_panel.gd)."""
    folder = folder_rule(rel, PRES_FOLDERS)
    if folder:
        return "presentation", None, "folder %s/" % folder
    hit = sorted(words(os.path.splitext(os.path.basename(rel))[0]) & (PRES_FOLDERS | PRES_STRONG))
    return ("presentation", None, "file name word %s" % hit[0]) if hit else None


def display_time(rel, names, extra):
    """A time in a presentation place (PURCHASE_PRICE_SEC in ui/, a duration in sfx.gd) is how long the game
    shows or plays a thing: presentation. A word of this game (the key `words`) comes before this rule."""
    where = presentation_place(rel)
    found = words(names[0]) if names else set()
    mine = any(found & listed for listed in (extra or {}).values())
    hit = where and found & DURATION and not mine
    return ("presentation", None, "a time (%s) in %s" % (names[0], where[2])) if hit else None


def name_proposal(rel, names, value, beside, label, extra):
    """Propose (class, role, rule) from the names of 1 number, or None. The order: a unit conversion, a time in
    a presentation place, the first name that gives a class (class_of_name), then the rules of REFINE."""
    names = [name for name in names if name]
    first = unit_constant(names, value) or display_time(rel, names, extra)
    found = first or class_of_names(effect_names(names), label, extra)
    return refined(found, names, value, beside) if found and not first else found


def propose_scene(raw):
    """Propose for a number of a scene (.tscn). Only what is clear gets a proposal:

    1. A section header attribute (load_steps, format): engine.
    2. A sub-resource of a presentation type (a style box, a gradient, an animation): presentation.
    3. A layout key (offset_left, anchors_preset, theme_override_font_sizes/font_size): presentation.
    """
    if raw.get("attr"):
        return "engine", None, "file format"
    if PRES_TYPES.search(raw["stype"]):
        return "presentation", None, "resource type %s" % raw["stype"]
    layout = raw["key"] and SCENE_LAYOUT.fullmatch(raw["key"])
    return ("presentation", None, "layout key %s" % raw["key"]) if layout else None


def propose_project(raw, extra):
    """Propose for a number of project.godot: the input map is engine, and each other number goes by its name."""
    if raw.get("attr") or raw["section"] == "input":
        return "engine", None, "file format" if raw.get("attr") else "input map"
    return class_of_names(raw["names"], "key", extra)


def tool_or_note(names, extra):
    """A number of a data file under a key of a tool or of a design note (autosave, tolerance, target, debug,
    version) is engine. A word of this game (the key `words` of the bench file) comes before this rule."""
    mine = set().union(*(extra or {}).values())
    hit = next((name for name in names if name and words(name) & TOOL_WORDS and not words(name) & mine), None)
    return ("engine", None, TOOL_RULE) if hit else None


def propose_data(rel, raw, res_type, extra=None):
    """Propose (class, role, rule) for a number of a data file, or None. The rules, in order:

    1. A scene and project.godot have their own rules: propose_scene, propose_project.
    2. A section header attribute (load_steps, format) and the file export_presets.cfg: engine.
    3. A .tres file of a presentation type (a theme, a style box, a font, a material): presentation.
    4. A key of a tool or of a design note (autosave, tolerance, target, debug, version): engine (tool_or_note).
    5. The key names, from the leaf to the root: name_proposal.
    6. A presentation folder (ui/, audio/) or file name: presentation.

    No rule reads the name of a folder to say "this is not the game": the key `skip` of the bench file does that.
    """
    if ext(rel) in SCENE_KINDS:
        return propose_scene(raw) if ext(rel) == ".tscn" else propose_project(raw, extra)
    if raw.get("attr") or os.path.basename(rel) in ENGINE_FILES:
        return "engine", None, "file format"
    if res_type and PRES_TYPES.search(res_type):
        return "presentation", None, "resource type %s" % res_type
    found = tool_or_note(raw["names"], extra) or name_proposal(rel, raw["names"], raw["value"],
                                                               raw.get("beside", {}), "key", extra)
    return found or presentation_place(rel)


def place(ctx):
    """The place of a GDScript literal for the rules of ENGINE_RULES."""
    prev = ctx["prev"]
    assign = prev in GD_ASSIGN and not ctx["const"]
    value_of_key = ctx["dict"] and prev == ":" and not ctx["const"]
    argument = ctx["arg"] and prev in ("(", ",")
    start = assign or value_of_key or argument or prev in COMPARE or prev in ("return", "else")
    return dict(assign=assign, start=start)


def engine_literal(value, ctx):
    """Propose (class, role, rule) for a GDScript literal with no game meaning, or None (ENGINE_RULES)."""
    where = place(ctx)
    return next(((cls, None, rule) for cls, rule, test in ENGINE_RULES if test(value, ctx, where)), None)


def compared(ctx):
    """True when the literal is 1 side of a comparison: a threshold."""
    return ctx["prev"] in COMPARE or ctx["next"] in COMPARE


def function_rule(ctx, extra=None):
    """A literal in a function with a balance name (func upgrade_cost) is balance. A threshold in such a function
    gets no proposal: a person decides (the entry gets the hint "threshold")."""
    found = class_of_names([ctx["func"]], "function", extra)
    return found if found and found[0] == "balance" and not compared(ctx) else None


def call_rule(ctx):
    call = ctx["call"] if ctx["call"] and PRES_CALLS.fullmatch(ctx["call"]) else None
    return ("presentation", None, "call %s" % call) if call else None


def fallback_copy(ctx, keys):
    """A literal that is the default of `.get("<key>", <literal>)`, where <key> is a key of a number of a data
    file of `sources`, is a copy of that number. It gets the class of the number. A copy of a balance number is
    balance and not tunable, with the rule `fallback copy`; its role comes from the name of the key, or it is
    `other`. Return (class, role, rule, reason), or None when there is no such key or its number has no class."""
    name = ctx["key"]
    if not (ctx["call"] == "get" and ctx["prev"] == "," and name in (keys or {})):
        return None
    rel, cls = keys[name]
    if cls != "balance":
        return (cls, None, "%s of a number with the class %s" % (FALLBACK_RULE, cls)) if cls != "unclassified" else None
    named = class_of_name(name) or (None, None)
    role = named[1] if named[0] == "balance" and named[1] else "other"
    return "balance", role, FALLBACK_RULE, "%s%s in %s" % (FALLBACK_REASON, name, rel)


def propose_gd(rel, raw, extra=None, keys=None):
    """Propose (class, role, rule) for a literal of a .gd file, or None. The rules, in order:

    1. A literal with no game meaning: engine_literal. It comes before the names, because `count += 1` and
       `var cost_sum := 0.0` have a name and no game meaning.
    1b. A default of `.get("<key>", <literal>)` whose key is a key of a number of a data file of `sources`: the
       class of that number; for a balance number, balance and not tunable, the rule `fallback copy`
       (fallback_copy). A default 0 or 1 is engine by rule 1.
    2. The name of the literal (the dictionary key, the name left of the assignment, the first argument of get,
       the var or const of the statement): name_proposal. A balance name wins against a presentation folder,
       but a time in a presentation place is presentation.
    3. A call that builds a colour or a position (Color, Vector2, tween_property): presentation.
    4. A presentation folder (ui/, audio/) or file name: presentation.
    5. The name of the function, when it is a balance name: balance.
    """
    ctx = raw["ctx"]
    named = name_proposal(rel, [ctx["key"], ctx["decl"]], raw["value"], {}, "name", extra)
    return (engine_literal(raw["value"], ctx) or fallback_copy(ctx, keys) or named or call_rule(ctx)
            or presentation_place(rel) or function_rule(ctx, extra))


def propose_code(rel, raw, extra=None, keys=None):
    """Propose for a literal of code. Each literal of a shader is presentation."""
    return ("presentation", None, "a shader") if ext(rel) == SHADER_KIND else propose_gd(rel, raw, extra, keys)


def proposal(found, in_code, sense=None):
    """The proposal keys of a ledger entry. A balance number in code is not tunable until it is in a data file.
    A proposed sense is in the proposal: the rule names it, and the mark `proposed` covers it."""
    if not found:
        return {"class": "unclassified"}
    role = {"role": found[1]} if found[1] else {}
    turn = {"sense": sense} if sense else {}
    rule = found[2] + ("; sense -1: a divisor name" if sense else "")
    reason = found[3] if len(found) > 3 else IN_CODE
    fixed = {"tunable": False, "reason": reason} if in_code and found[0] == "balance" else {}
    return dict({"class": found[0]}, **role, **turn, proposed=True, rule=rule, **fixed)


def tool_reason(reason):
    """True for a reason that the tool wrote: the number is in code, or it is a fallback copy."""
    return reason == IN_CODE or str(reason or "").startswith(FALLBACK_REASON)


# ----------------------------------------------------------------------------- the texts with a number
# A string literal can hold a copy of a balance number ("+25% pot value per level", "Unlocks at Shop Level 4").
# The ledger of the numbers does not see it, and the view shows the old value after a change on the bench. So
# the inventory lists each string literal of a .gd file and each string value of a property of a .tscn or
# .tres file that has a digit, in the list `texts` of the bench file. A person reads each one and accepts it
# with a reason (the key `ok`), or builds the text from the data.
#
# The inventory leaves out a string that is clearly not a text for a player (TEXT_RULES). The digits of a format
# spec do not count: "%.2f", "%d / %d", "%02d:%02d" and "{0} of {1}" have no number of their own. The digits of
# a code do not count: 1 to 4 capital letters and a number with no space ("FM-1 gap", "C12 Tend boost", "F2").
# "X2" is a factor, not a code.
TEXT_KINDS = (CODE_KIND, ".tscn", ".tres", ".cfg")      # and .json and .csv, by pointer (data_texts)
TEXT_MAX = 200                                         # the ledger keeps the first characters of a long text
FORMAT_SPEC = re.compile(r"%%|%[-+ #0]*(?:\d+|\*)?(?:\.(?:\d+|\*)?)?[a-zA-Z]|\{[^{}\s]*\}")
CODE_TOKEN = re.compile(r"(?<![A-Za-z0-9])(?![xX]\d)[A-Z]{1,4}-?\d+(?![\w.])")
DATA_AMOUNT = re.compile(r"[%+\u00d7]|(?<![A-Za-z])[xX]\d")
AMOUNT_SIGN = re.compile(r"[%+×$€£]")
URL = re.compile(r"\s*(?:res|user|uid|file|https?)://")
FILE_NAME = re.compile(r"[\w./-]+\.[a-z][a-z0-9]{1,7}")
HEX_COLOUR = re.compile(r"#[0-9a-fA-F]{3,8}|(?=[0-9]*[a-fA-F])(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{8})")
IDENT = r"(?:[a-z_][a-z0-9_]*|[A-Z_][A-Z0-9_]*)"
IDENTIFIER = re.compile(r"[%%$^&@]?%s(?:[.:]%s)*|[%%$^&@][A-Za-z_]\w*" % (IDENT, IDENT))
MULTIPLIER = re.compile(r"[xX]\d+")
# A call whose text argument is a name for the engine or a message for the developer.
NOT_TEXT_CALL = re.compile(
    r"get_node(?:_or_null)?|has_node|find_child(?:ren)?|(?:dis)?connect|is_connected|emit_signal|has_signal"
    r"|has_method|call(?:v|_deferred)?|set_deferred|rpc(?:_id)?|is_action_\w+|get_action_\w+|action_(?:press|release)"
    r"|add_to_group|remove_from_group|is_in_group|get_nodes_in_group|(?:pre)?load|print\w*|push_(?:error|warning)"
    r"|assert|(?:get|set|has|remove)_meta|StringName|NodePath|Color|(?:add|get|has|remove)_theme_\w+|export_\w+"
    r"|warning_ignore|is_class|get_singleton|has_singleton")
# The keys of a scene or a resource whose string is a name for the engine or for the editor.
NOT_TEXT_KEY = re.compile(r"script|resource_name|resource_path|editor_description|bus|animation|autoplay"
                          r"|\w*_animation|theme_type_variation|metadata/.*|_.*")


def spec_text(found):
    return "%" if found.group() == "%%" else ""


def plain_text(text):
    """The text with no format spec and no code: "%d", "%.2f", "%02d", "{0}" and "FM-1" go, and "%%" becomes
    "%"."""
    return CODE_TOKEN.sub("", FORMAT_SPEC.sub(spec_text, text))


def path_like(text):
    """True for a node path or a file path: no space, 2 parts or more, and each part is `.`, `..` or has 2
    letters or more. "HBox/Label2" is a path. "x1.5/s" and "10/s" are not."""
    parts = text.lstrip("%$^").split("/")
    wordy = all(part in (".", "..") or len(re.findall(r"[A-Za-z]", part)) >= 2 for part in parts)
    return len(parts) > 1 and wordy and not re.search(r"\s", text)


def unique_node(raw):
    """True for the text of %"Name": the percent sign is not the format operator."""
    return raw.get("prev") == "%" and not gd_operand(raw.get("prev2"))


# The reasons to leave a string out of the list of the texts: (reason, test). The test reads the text t, the
# text p with no format spec, and the context c of the string. The first tests read only the text; the last
# tests read the place of the string in the code.
TEXT_RULES = (
    ("no number of its own", lambda t, p, c: not re.search(r"\d", p)),
    ("no letter and no sign of an amount", lambda t, p, c: not re.search(r"[^\W\d_]", p) and not AMOUNT_SIGN.search(p)),
    ("a path", lambda t, p, c: bool(URL.match(t)) or path_like(t)),
    ("a file name", lambda t, p, c: bool(FILE_NAME.fullmatch(t))),
    ("a hex colour", lambda t, p, c: bool(HEX_COLOUR.fullmatch(t))),
    ("a name", lambda t, p, c: bool(IDENTIFIER.fullmatch(t)) and not MULTIPLIER.fullmatch(t)),
    ("a string name or a node path", lambda t, p, c: c.get("prev") in ("&", "^", "$") or unique_node(c)),
    ("a name or a message for the engine", lambda t, p, c: bool(NOT_TEXT_CALL.fullmatch(c.get("call") or ""))),
    ("a string that is a statement", lambda t, p, c: c.get("prev") is None and c.get("next") == "\n"),
    ("a subscript", lambda t, p, c: bool(c.get("sub"))),
    ("a key for the engine", lambda t, p, c: bool(NOT_TEXT_KEY.fullmatch(c.get("key") or ""))),
)


def left_out(text, ctx):
    """The reason to leave a string out of the list of the texts, or None for a text that the list takes."""
    plain = plain_text(text)
    return next((reason for reason, test in TEXT_RULES if test(text, plain, ctx)), None)


def text_entry(rel, line, index, text):
    cut = text if len(text) <= TEXT_MAX else text[:TEXT_MAX] + "..."
    return dict(id="%s:%d:%d" % (rel, line, index), file="res://" + rel, line=line, text=cut)


def gd_texts(rel, strings):
    """The texts with a number of a script: each string literal that no rule of TEXT_RULES leaves out. The index
    of a text is the place of its string among the strings of the line, from 1."""
    return [text_entry(rel, raw["line"], raw["index"], raw["text"]) for raw in strings
            if not left_out(raw["text"], raw)]


def variant_texts(rel, text, strings):
    """The texts with a number of a scene or a resource: each string value of a property, outside the arguments
    of a constructor (ExtResource("1_abc"), NodePath("A/B2")), that no rule of TEXT_RULES leaves out."""
    starts = line_starts(text)
    lines = [bisect.bisect_left(starts, raw["pos"]) + 1 for raw in strings]
    places = [(line, lines[:i + 1].count(line)) for i, line in enumerate(lines)]
    wanted = player_text if ext(rel) == ".cfg" else str
    return [text_entry(rel, line, index, raw["text"]) for raw, (line, index) in zip(strings, places)
            if not left_out(raw["text"], raw) and wanted(raw["text"])]


def json_strings(node, tokens=()):
    """Each string leaf of parsed JSON as (pointer, text)."""
    if isinstance(node, str):
        return [(pointer(tokens), node)]
    if isinstance(node, dict):
        return [found for key, child in node.items() for found in json_strings(child, tokens + (key,))]
    children = enumerate(node) if isinstance(node, list) else ()
    return [found for index, child in children for found in json_strings(child, tokens + (index,))]


def csv_strings(text):
    """Each cell of a CSV text as (pointer, text), with the pointers of csv_numbers."""
    rows = list(csv.reader(io.StringIO(text)))
    return [(cell["pointer"], raw) for r, row in enumerate(rows) for cell, raw in csv_cells(rows, r, row)]


def csv_cells(rows, r, row):
    first = rows[0]
    named = all(first) and len(set(first)) == len(first) and all(text_number(c) is None for c in first)
    return [(csv_cell(r, c, raw, first if named else []), raw) for c, raw in enumerate(row)]


def player_text(text):
    """True for a string of a data file that looks like a text for a player: it has a digit, no rule of
    TEXT_RULES leaves it out, and it has a letter and a space, or the sign of an amount (x2, +5, 25%)."""
    plain = plain_text(text)
    words_ = re.search(r"[^\W\d_]", plain) and re.search(r"\s", plain.strip())
    return not left_out(text, {}) and bool(words_ or DATA_AMOUNT.search(plain))


def data_text_entry(rel, place, text):
    cut = text if len(text) <= TEXT_MAX else text[:TEXT_MAX] + "..."
    return dict(id="%s#%s" % (rel, place), file="res://" + rel, pointer=place, text=cut)


def data_texts(rel, strings):
    """The texts with a number of a .json or .csv file: each string value that looks like a text for a player.
    The id is <file>#<pointer>, as for a number. A second text with the same id gets ~2."""
    return unique_ids([data_text_entry(rel, place, text) for place, text in strings if player_text(text)])


def accepted(text):
    """True for a text that a person accepted: the key `ok` has a reason."""
    return bool(str(text.get("ok") or "").strip())


def text_exact(text):
    return text.get("id"), text.get("text")


def text_moved(text):
    """A text that moved keeps its file and its content."""
    return text.get("file"), text.get("text")


def text_place(text):
    """A text whose numbers changed keeps its place and its shape ("+25% pot value" -> "+30% pot value")."""
    return text.get("id"), shape(str(text.get("text")))


TEXT_MACHINE_KEYS = ("id", "file", "line", "pointer", "text")


def update_texts(olds, scanned):
    """Merge the scan into the list `texts`. Return (texts, new texts, texts that went). A text that is still
    there keeps its place in the list and each key that a person wrote (`ok`). A text is followed as a number
    is: by its place and its content, then by its file and its content, then by its place and its shape."""
    pairs = []
    rest, news = pair(olds, scanned, text_exact, pairs)
    rest, news = pair(rest, news, text_moved, pairs)
    gone, news = pair(rest, news, text_place, pairs)
    kept = {id(old): dict(new, **{k: v for k, v in old.items() if k not in TEXT_MACHINE_KEYS}) for old, new in pairs}
    return [kept[id(old)] for old in olds if id(old) in kept] + news, news, gone


def text_list(data):
    """The list `texts` of the bench file."""
    texts = data.get("texts") or []
    fine = isinstance(texts, list) and all(isinstance(text, dict) for text in texts)
    check(not fine, 'the key "texts" of the bench file is not a list of objects')
    return texts


def texts_line(texts):
    return "texts with a number: %d (%d accepted)" % (len(texts), sum(map(accepted, texts)))


def text_line(text):
    return "%s | %s" % (text.get("id"), one_line(text.get("text")))


# ----------------------------------------------------------------------------- the scan of the project
def read_text(path):
    return pathlib.Path(path).read_text(encoding="utf-8-sig", errors="replace")


def skip_pattern(rel, patterns):
    """The first entry of the key `skip` of the bench file that names this path, or None. An entry is a folder
    (docs/) or a glob (tests/*.gd)."""
    return next((p for p in patterns if fnmatch.fnmatch(rel, p) or rel.startswith(p.rstrip("/") + "/")), None)


def skip_list(data):
    given = data.get("skip") or []
    fine = isinstance(given, list) and all(isinstance(entry, str) and entry for entry in given)
    check(not fine, '"skip" of the bench file must be a list of folders or globs')
    return given


def skip_folder(root, rel):
    """True for a folder that the inventory does not read: addons/, bench/settings/, a folder whose name starts
    with a point (.godot/, .git/), and a folder with a .gdignore file (Godot does not read it). The inventory
    also does not read the bench files of bench/ (bench_file)."""
    if rel in ("addons", "bench/settings") or os.path.basename(rel).startswith("."):
        return True
    return os.path.exists(os.path.join(root, rel, ".gdignore"))


def walk_folder(root, folder, dirs, names, found, skip):
    """Sort the content of 1 folder into the lists of `found`, and take the skipped folders out of the walk."""
    base = os.path.relpath(folder, root).replace(os.sep, "/")
    prefix = "" if base == "." else base + "/"
    skipped = [prefix + name for name in dirs if skip_folder(root, prefix + name)]
    dirs[:] = sorted(name for name in dirs if prefix + name not in skipped)
    found["skipped"] += [rel for rel in skipped if rel not in CONTRACT_SKIPS]
    sort_files(root, [prefix + name for name in sorted(names)], found, skip)


def loads(text):
    """The parsed JSON of a text, or None for a text that is not JSON."""
    data = None
    try:
        data = json.loads(text)
    except ValueError:
        pass
    return data


def bench_format(text):
    """True for the text of a JSON object whose key "format" starts with tuning-bench. The key can be at any
    place of the object: a result file has its keys in the order of the alphabet."""
    data = loads(text)
    return isinstance(data, dict) and str(data.get("format", "")).startswith(FORMAT)


def bench_file(root, rel):
    """True for a JSON file of the folder bench/ with a bench format: bench.json, a second bench file, a result.
    Each other file of bench/ is data of the game, of any size."""
    if os.path.dirname(rel) != "bench" or ext(rel) != ".json":
        return False
    return rel == BENCH_JSON or bench_format(read_text(os.path.join(root, rel)))


def not_skipped(rels, found, skip):
    """The files that the key `skip` does not name. Count each file that it names under its entry."""
    rels = [rel for rel in rels if ext(rel) in KINDS + NOT_SCANNED]
    found["skip"].update(pattern for pattern in (skip_pattern(rel, skip) for rel in rels) if pattern)
    return [rel for rel in rels if not skip_pattern(rel, skip)]


def sort_files(root, rels, found, skip):
    rels = not_skipped(rels, found, skip)
    kinds = [rel for rel in rels if ext(rel) in KINDS]
    bench = {rel for rel in kinds if bench_file(root, rel)}
    found["bench"] += [rel for rel in kinds if rel in bench and rel != BENCH_JSON]
    found["files"] += [rel for rel in kinds if rel not in bench]
    found["others"] += [rel for rel in rels if ext(rel) in NOT_SCANNED]


def project_files(root, skip=()):
    """The files to read, the other files that can hold numbers, the folders skipped beyond the contract, and
    the count of files that each entry of `skip` took out."""
    found = dict(files=[], others=[], skipped=[], bench=[], skip=collections.Counter())
    for folder, dirs, names in os.walk(root):
        walk_folder(root, folder, dirs, names, found, skip)
    found["files"].sort()
    found["skipped"].sort()
    return found


def gd_entry(rel, raw, extra=None, keys=None):
    entry = dict(id="%s:%d:%d" % (rel, raw["line"], raw["index"]), file="res://" + rel, line=raw["line"],
                 index=raw["index"], text=raw["text"], value=raw["value"])
    found = propose_code(rel, raw, extra, keys)
    hint = {"hint": "threshold"} if not found and compared(raw["ctx"]) else {}
    names = [name for name in (raw["ctx"]["key"], raw["ctx"]["decl"]) if name]
    entry["_new"] = dict(proposal(found, True, sense_of(found, names)), **hint)
    return entry


def scan_code(rel, text, extra=None, keys=None):
    """The numbers and the texts with a number of a script or a shader. A shader has no text."""
    scan = GdScan(text, SHADER_TOKEN if ext(rel) == SHADER_KIND else GD_TOKEN)
    texts = [] if ext(rel) == SHADER_KIND else gd_texts(rel, scan.strings)
    return [gd_entry(rel, raw, extra, keys) for raw in scan.out], texts


def data_raws(rel, text):
    """The numbers of a data file, the type of the resource for a .tres file, and the texts with a number of a
    scene or a resource."""
    kind = ext(rel)
    if kind in VARIANT_KINDS:
        scan = VariantScan(text)
        return scan.out, scan.res_type, variant_texts(rel, text, scan.strings) if kind in TEXT_KINDS else []
    if kind == ".csv":
        return csv_numbers(text), None, data_texts(rel, csv_strings(text))
    root = parse_json(text)
    return json_numbers(root), None, data_texts(rel, json_strings(root))


def owner_of(anchor):
    """The id of the object that owns a number: the item of a top-level list of the file that the number is in.
    A top-level list is the root of the file or a value of the root object. In the anchor
    /purchases/[training]/requires/[helper]/level the owner is training. None when there is no such item or it
    has no id."""
    tokens = str(anchor or "").split("/")[1:3]
    found = next((token[1:-1] for token in tokens if re.fullmatch(r"\[.+\]", token)), None)
    return found.replace("~1", "/").replace("~0", "~") if found else None


def element_keys(raw):
    """Propose the element and the label of a number: the id of its owner (owner_of), or with no owner the id of
    the nearest JSON object around it. A requirement belongs to the thing that has it, not to what it needs."""
    element = owner_of(raw.get("anchor")) or raw.get("element")
    if not element:
        return {}
    name = raw["names"][0] if raw["names"] else raw["pointer"]
    return {"element": element, "label": "%s: %s" % (element, name)}


def data_entry(rel, raw, res_type, extra=None):
    entry = dict(id="%s#%s" % (rel, raw["pointer"]), file="res://" + rel, pointer=raw["pointer"],
                 value=raw["value"])
    entry.update({key: raw[key] for key in ("quoted", "anchor") if key in raw})
    found = propose_data(rel, raw, res_type, extra)
    sense = sense_of(found, raw["names"])
    entry["_new"] = dict(proposal(found, ext(rel) not in DATA_KINDS, sense), **element_keys(raw))
    return entry


def scan_data(rel, text, extra=None, keys=None):
    raws, res_type, texts = data_raws(rel, text)
    return [data_entry(rel, raw, res_type, extra) for raw in raws], texts


def renumber(entry, seen):
    count = seen[entry["id"]] = seen.get(entry["id"], 0) + 1
    tail = "~%d" % count if count > 1 else ""
    entry["id"] += tail
    entry.update({"pointer": entry["pointer"] + tail} if "pointer" in entry else {})


def unique_ids(entries):
    """Make each id unique: a second number with the same id (a key that is 2 times in a file) gets ~2, ~3."""
    seen = {}
    for entry in entries:
        renumber(entry, seen)
    return entries


def scan_file(root, rel, extra=None, keys=None):
    """Return (the numbers of 1 file, its texts with a number)."""
    text = read_text(os.path.join(root, rel))
    scan = scan_code if ext(rel) in CODE_KINDS else scan_data
    error = None
    try:
        numbers, texts = scan(rel, text, extra, keys)
        return unique_ids(numbers), texts
    except InputError as exc:
        error = "%s: %s" % (rel, exc)
    raise InputError(error)


def key_class(entries, decided):
    """The class of the numbers of 1 key of a data file: the class that a person decided, or the proposal.
    balance when 1 of them is balance, and if not the class of the first one."""
    classes = [decided.get(entry["id"]) or entry["_new"].get("class") for entry in entries]
    return "balance" if "balance" in classes else classes[0]


def file_keys(rel, entries, decided):
    """key -> (file, class) for the numbers of 1 data file."""
    names = dict.fromkeys(filter(None, map(key_name, entries)))
    return {name: (rel, key_class([e for e in entries if key_name(e) == name], decided)) for name in names}


def source_keys(scans, data):
    """The keys of the numbers of each data file that is a source or that holds a balance proposal:
    key -> (file, class of its number). The rule `fallback copy` reads this table."""
    sources = [str(source) for source in data.get("sources") or []]
    fixed = {n.get("id"): class_of(n) for n in data.get("numbers") or [] if isinstance(n, dict) and decided(n)}
    used = [rel for rel in scans if ext(rel) in DATA_KINDS and ("res://" + rel in sources or any(
        entry["_new"].get("class") == "balance" for entry in scans[rel][0]))]
    found = {}
    for rel in reversed(used):
        found.update(file_keys(rel, scans[rel][0], fixed))
    return found


def key_count(root, rel):
    return number_keys(parse_json(read_text(os.path.join(root, rel)))) if ext(rel) == ".json" else 0


def scan_project(root, data=None):
    """Scan each file of the project. Return (entries, facts for the report). The bench file gives the keys
    `skip` and `words`."""
    extra = game_words(data or {})
    facts = project_files(root, skip_list(data or {}))
    code = [rel for rel in facts["files"] if ext(rel) in CODE_KINDS]
    scans = {rel: scan_file(root, rel, extra) for rel in facts["files"] if rel not in code}
    keys = source_keys(scans, data or {})
    scans.update({rel: scan_file(root, rel, extra, keys) for rel in code})
    entries = [entry for rel in facts["files"] for entry in scans[rel][0]]
    facts["texts"] = [text for rel in facts["files"] for text in scans[rel][1]]
    facts["number_keys"] = sum(key_count(root, rel) for rel in facts["files"])
    return entries, facts


# ----------------------------------------------------------------------------- the update of the ledger
def kind_of(entry):
    return ext(str(entry.get("file", "")))


def exact_key(e):
    if "line" in e:
        return e.get("file"), e["line"], e.get("index"), e.get("text")
    return e.get("id"), e.get("anchor")


def moved_key(e):
    """A literal of code that moved keeps its file, its line text and its index. A data number keeps its anchor."""
    if "line" in e:
        return e.get("file"), e.get("text"), e.get("index")
    return (e.get("file"), e["anchor"]) if e.get("anchor") else None


def place_key(e):
    """A literal of code whose value changed keeps its place and the shape of its line."""
    return (e.get("file"), e["line"], e.get("index"), shape(str(e.get("text")))) if "line" in e else None


def id_key(e):
    """A data number of a ledger that a person wrote has no anchor: it keeps its id."""
    return None if "line" in e else e.get("id")


def key_table(olds, key):
    table = {}
    for old in olds:
        table.setdefault(key(old), []).append(old)
    table.pop(None, None)
    return table


def take(table, key):
    """Take the first old entry with this key out of the table, or return None."""
    return (table.get(key) or [None]).pop(0)


def pair(olds, news, key, pairs):
    """Pair each new entry with the first old entry that has the same key. Return what has no partner."""
    table = key_table(olds, key)
    matched = [(take(table, key(new)), new) for new in news]
    pairs += [(old, new) for old, new in matched if old is not None]
    used = {id(old) for old, _ in pairs}
    return [old for old in olds if id(old) not in used], [new for old, new in matched if old is None]


def decided(old):
    """True when a person decided the class: it is not a proposal and it is not unclassified."""
    return class_of(old) != "unclassified" and not old.get("proposed")


def merged(old, new):
    """The ledger entry of a number that is still there. A decision of the user stays. A proposal is made again."""
    fresh = new.pop("_new")
    user = {k: v for k, v in old.items() if k not in MACHINE_KEYS}
    if decided(old):
        return dict(new, **user)
    keep = {k: v for k, v in user.items() if k not in PROPOSAL_KEYS}
    stale = tool_reason(keep.get("reason"))             # the new proposal writes the reason of the tool again
    keep = {k: v for k, v in keep.items() if not (stale and k in ("reason", "tunable"))}
    return dict(new, **dict(fresh, **keep))


def match_ledger(olds, scanned):
    """Return (pairs, pairs that moved, old entries with no partner, new entries with no partner)."""
    pairs, moved = [], []
    olds, news = pair(olds, scanned, exact_key, pairs)
    olds, news = pair(olds, news, moved_key, moved)
    olds, news = pair(olds, news, place_key, pairs)
    bare, news = pair([old for old in olds if not old.get("anchor")], news, id_key, pairs)
    return pairs, moved, [old for old in olds if old.get("anchor")] + bare, news


def ledger_report(pairs, moved, gone, by_hand):
    both = pairs + moved
    return dict(gone=gone, by_hand=by_hand, moved=[(o["id"], n["id"]) for o, n in moved if o["id"] != n["id"]],
                changed=[(n["id"], o.get("value"), n["value"]) for o, n in both if o.get("value") != n["value"]])


def update_ledger(old_numbers, scanned):
    """Merge the scan into the old ledger. Return (numbers, report) with the lists new, gone, moved and changed.

    Each number that is still there keeps its place in the ledger, so a ledger that a person put in order stays
    in order. The new numbers go to the end. A number of a file kind that the inventory does not read was
    written by hand: it stays.
    """
    by_hand = [old for old in old_numbers if kind_of(old) not in KINDS]
    pairs, moved, gone, news = match_ledger([old for old in old_numbers if kind_of(old) in KINDS], scanned)
    report = ledger_report(pairs, moved, gone, by_hand)
    done = {id(old): merged(old, new) for old, new in pairs + moved}
    done.update({id(old): old for old in by_hand})
    report["new"] = [dict(new, **new.pop("_new")) for new in news]
    return [done[id(old)] for old in old_numbers if id(old) in done] + report["new"], report


def sources_of(numbers, old_sources, skip=()):
    """The data files that hold 1 or more balance numbers. A source that the user added stays. A file that the
    key `skip` names is not a source."""
    found = sorted({n["file"] for n in numbers if n.get("class") == "balance" and kind_of(n) in DATA_KINDS})
    names = list(old_sources) + [name for name in found if name not in old_sources]
    return [name for name in names if not skip_pattern(str(name).replace("res://", "", 1), skip)]


# ----------------------------------------------------------------------------- bench.json
def dump_value(value):
    """A list has 1 item on each line, and a dictionary of dictionaries has 1 key on each line."""
    if isinstance(value, list) and value:
        return "[\n" + ",\n".join("    " + one_line(v) for v in value) + "\n  ]"
    if isinstance(value, dict) and value and all(isinstance(v, dict) for v in value.values()):
        return "{\n" + ",\n".join("    %s: %s" % (one_line(k), one_line(v)) for k, v in value.items()) + "\n  }"
    return one_line(value)


def dump_bench(data):
    """The text of bench.json: 1 number on each line, so that a person can read, edit and compare it."""
    return "{\n" + ",\n".join("  %s: %s" % (one_line(k), dump_value(v)) for k, v in data.items()) + "\n}\n"


def project_root(path):
    check(not os.path.isfile(os.path.join(path, "project.godot")),
          "%s is not a Godot project: no project.godot" % path)
    return os.path.abspath(path)


def game_name(root):
    match = re.search(r'^config/name="((?:\\.|[^"\\])*)"', read_text(os.path.join(root, "project.godot")), re.M)
    return match.group(1) if match else os.path.basename(root)


def static_functions(tokens):
    """The names of the static functions of a script."""
    triples = zip(tokens, tokens[1:], tokens[2:])
    return {name[1] for first, second, name in triples if (first[1], second[1]) == ("static", "func")}


def core_script(root):
    """The first script of core/ that defines the static function new_state, as a res:// path, or None."""
    folder = os.path.join(root, DEFAULT_CORE)
    names = sorted(n for n in os.listdir(folder) if n.endswith(CODE_KIND)) if os.path.isdir(folder) else []
    cores = (n for n in names if "new_state" in static_functions(gd_tokens(read_text(os.path.join(folder, n)))))
    return next(("res://%s/%s" % (DEFAULT_CORE, name) for name in cores), None)


# The keys that the bench can take for its panel, with the key code that a scene or the input map writes.
HOTKEYS = (("F2", 4194333), ("F9", 4194340), ("F10", 4194341), ("F11", 4194342))
HOTKEY_FILES = (CODE_KIND, ".tscn", ".tres", ".godot")


def hotkey_texts(root, skip):
    """The text of each file of the game that can handle a key: the scripts, the scenes, the resources and
    project.godot. The files of the bench and the files of the key `skip` are not the game."""
    files = [rel for rel in project_files(root, skip)["files"] if ext(rel) in HOTKEY_FILES]
    return [(rel, read_text(os.path.join(root, rel))) for rel in files if not rel.startswith("bench/")]


def key_handler(texts, key, code):
    """The first file that names this key (KEY_F2, "F2", or its key code in a scene or in the input map)."""
    pattern = re.compile(r'\bKEY_%s\b|"%s"|keycode"?\s*[:=]\s*%d\b' % (key, key, code))
    return next((rel for rel, text in texts if pattern.search(text)), None)


def free_hotkey(root, skip=()):
    """Return (the first key of F2, F9, F10, F11 that no file of the game handles, or F2 when each one has a
    handler; the file of the handler of each key that has one)."""
    texts = hotkey_texts(root, skip)
    found = [(key, key_handler(texts, key, code)) for key, code in HOTKEYS]
    free = [key for key, rel in found if not rel]
    return (free or [HOTKEYS[0][0]])[0], {key: rel for key, rel in found if rel}


def hotkey_line(root):
    """The line of install about the hotkey of the bench file. A bench file that exists keeps its key."""
    data = read_bench(root)
    key = str(data.get("hotkey", HOTKEYS[0][0]))
    free, used = free_hotkey(root, skip_list(data))
    taken = "; ".join("%s has a handler in %s" % item for item in used.items())
    if key not in used:
        return "hotkey: %s%s" % (key, " (%s)" % taken if taken else "")
    advice = "no key of F2, F9, F10, F11 is free" if free in used else "%s is free" % free
    return 'hotkey: %s, but %s: write a free key into "hotkey" of %s (%s)' % (key, taken, BENCH_JSON, advice)


def skeleton(root):
    """A new bench file. A game with a core names the core, and it needs no adapter. The hotkey is the first
    key that the game does not handle."""
    core = core_script(root)
    link = {"core": core} if core else {"adapter": "res://" + ADAPTER}
    return dict({"format": FORMAT, "version": VERSION, "game": game_name(root)}, **link,
                **{"hotkey": free_hotkey(root)[0], "skip": [], "sources": [], "numbers": [], "formulas": []})


def read_json(path, what):
    error = None
    try:
        return json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    except (OSError, ValueError) as exc:
        error = "cannot read %s %s: %s" % (what, path, exc)
    raise InputError(error)


def read_bench(root, make=False):
    path = os.path.join(root, BENCH_JSON)
    if not os.path.exists(path):
        check(not make, "%s is absent: run `pw_bench.py install` or `pw_bench.py inventory` first" % BENCH_JSON)
        return skeleton(root)
    data = read_json(path, "the bench file")
    check(not isinstance(data, dict) or data.get("format") != FORMAT,
          '%s is not a bench file: the key "format" is not "%s"' % (path, FORMAT))
    check(not isinstance(data.get("numbers", []), list), '%s: the key "numbers" is not a list' % path)
    return data


def write_file(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    pathlib.Path(path).write_bytes(text.encode("utf-8"))


def write_bench(root, data):
    write_file(os.path.join(root, BENCH_JSON), dump_bench(data))


def short(lines, show_all):
    """The lines of a printed list. Without --all, the first lines only, and how many more there are."""
    shown = lines if show_all else lines[:LIST_MAX]
    more = len(lines) - len(shown)
    tail = ["  ... and %d more (--all prints each one)" % more] if more else []
    return ["  " + line for line in shown] + tail


def say_list(title, lines, show_all):
    say_lines(["%s: %d" % (title, len(lines))] + short(lines, show_all))


# ----------------------------------------------------------------------------- install
STUB = '''extends "res://addons/tuning_bench/bench_adapter.gd"
# The adapter of this game for the tuning bench. It is the only bench code that knows the game.
# The contract is in references/tuning-bench.md, section 6, of the skill incremental-game-design.
# Write each method below. The self-check (`pw_bench.py selfcheck`) fails until each method is written.
# All times are game seconds.
# Connect the purchase signal of the game: a purchase that the bot did not ask for (an auto-buy) is an event
# {"kind": "bought", "id": ..., "count": n} that events() returns.
#
# Before you write setup(): list each autoload of project.godot. For each one that writes a file, reads a setting
# or reacts to an economy signal, switch it off in setup() (stop its autosave, disconnect its handlers, stop its
# _process). An adapter that drives autoloads runs the whole game: a save on a signal overwrites the real save
# file of the designer. The self-check row "No file written" names each file of user:// that the simulation made.


# Start a new game from the start. `data` maps each file of "sources" in bench.json to its parsed content, with
# the controls applied. Give this data to the economy in place of the files. Read no save file. Write no save file.
func setup(data: Dictionary) -> void:
\tpush_error("not written")


# Advance the economy by `dt` seconds. Use no frame and no real clock.
func step(dt: float) -> void:
\tpush_error("not written")


# Return each thing the player can do or see now. Each item is a dictionary with the keys id, kind ("buy",
# "once", "advance", "reset" or "other"), label, cost (0 when free) and affordable.
func actions() -> Array:
\tpush_error("not written")
\treturn []


# Do 1 action. Return false when the game refuses it.
func do_action(id: String) -> bool:
\tpush_error("not written")
\treturn false


# Return the main income for each second now, with no manual action.
func rate() -> float:
\tpush_error("not written")
\treturn 0.0


# Return 0.0 at the start and 1.0 when the game is complete.
func progress() -> float:
\tpush_error("not written")
\treturn 0.0
'''


def copy_addon(source, root):
    check(not os.path.isdir(source), "the addon source %s is absent" % source)
    target = os.path.join(root, ADDON)
    shutil.copytree(source, target, dirs_exist_ok=True, ignore=shutil.ignore_patterns("__pycache__", ".DS_Store"))
    return sum(len(names) for _, _, names in os.walk(target))


def autoload_section(lines):
    """The index of the line [autoload] and the index after the last line of its section, or None."""
    head = next((i for i, line in enumerate(lines) if line.strip() == "[autoload]"), None)
    if head is None:
        return None
    end = next((i for i in range(head + 1, len(lines)) if lines[i].startswith("[")), len(lines))
    return head, end


def new_section(text, eol):
    gap = eol if text.endswith(eol) else eol + eol
    gap = "" if text.endswith(eol + eol) or not text else gap
    return text + gap + "[autoload]" + eol + eol + AUTOLOAD_LINE + eol


def edit_section(lines, head, end):
    """Put the autoload line into the section, at its end. Return what was done."""
    own = [i for i in range(head + 1, end) if re.match(r"%s\s*=" % AUTOLOAD_NAME, lines[i])]
    if own:
        same = lines[own[0]] == AUTOLOAD_LINE
        lines[own[0]] = AUTOLOAD_LINE
        return "the autoload is there" if same else "corrected the autoload line"
    last = max(i for i in range(head, end) if lines[i].strip())
    lines.insert(last + 1, AUTOLOAD_LINE)
    return "added the autoload"


def with_autoload(text):
    """The text of project.godot with the autoload of the bench. Each other byte of the text stays.

    The line goes to the end of the section [autoload], so the bench loads after the autoloads of the game.
    Return (text, what was done).
    """
    eol = "\r\n" if "\r\n" in text else "\n"
    lines = text.split(eol)
    section = autoload_section(lines)
    if section is None:
        return new_section(text, eol), "added the section [autoload]"
    done = edit_section(lines, *section)
    return eol.join(lines), done


def install_autoload(path):
    text = pathlib.Path(path).read_bytes().decode("utf-8")
    new, done = with_autoload(text)
    if new != text:
        pathlib.Path(path).write_bytes(new.encode("utf-8"))
    return done


def write_if_absent(path, text):
    if os.path.exists(path):
        return "kept"
    write_file(path, text)
    return "written"


def install_adapter(root):
    """Write the adapter stub when it is absent. A game with a core needs no adapter."""
    core = read_bench(root).get("core") or core_script(root)
    if core:
        return "not written, the game has the core %s" % core
    return write_if_absent(os.path.join(root, ADAPTER), STUB)


def install_next(root, stub, project):
    """The next command after install. A bench file with phases is past the first steps."""
    if not absent(read_bench(root), "phases"):
        return "next: pw_bench.py inventory %s" % project
    first = "write %s (the adapter), then " % ADAPTER if stub else ""
    return ("next: %spw_bench.py init %s --phases-only, then the first run (pw_bench.py run %s), then "
            "pw_bench.py inventory %s" % (first, project, project, project))


def run_install(a):
    root = project_root(a.project)
    count = copy_addon(a.addon or ADDON_SOURCE, root)
    done = install_autoload(os.path.join(root, "project.godot"))
    bench = write_if_absent(os.path.join(root, BENCH_JSON), dump_bench(skeleton(root)))
    adapter = install_adapter(root)
    say_lines(["pw_bench install: %s" % root, "addon: %d files in %s/" % (count, ADDON), "project.godot: %s" % done,
               "%s: %s" % (BENCH_JSON, bench), "%s: %s" % (ADAPTER, adapter), hotkey_line(root),
               install_next(root, adapter == "written", a.project)])
    return 0


# ----------------------------------------------------------------------------- inventory
NOT_GAME = ("tests", "test", "tools", "addons", "docs")    # folders that often hold no number of the game
NOT_GAME_SHARE = 0.25


def set_review(data):
    """Write the key `review` of the bench file: the count of the numbers that are a proposal that nobody
    confirmed, the count of the files that hold them, and the count of the texts with a number that nobody
    accepted. Return True when the key changed."""
    still = [n for n in data.get("numbers", []) if isinstance(n, dict) and n.get("proposed")]
    texts = [text for text in data.get("texts") or [] if isinstance(text, dict) and not accepted(text)]
    review = {"not_reviewed": len(still), "files": len(set(map(file_of, still))), "texts": len(texts)}
    changed = data.get("review") != review
    data["review"] = review
    return changed


def review_line(data):
    return "not reviewed: %(not_reviewed)d numbers in %(files)d files" % data["review"]


def not_game_lines(numbers):
    """A hint when more than 25 percent of the numbers are in folders that often are not the game. The inventory
    does not skip them by itself."""
    folders = [folder_rule(file_of(n).replace("res://", "", 1), NOT_GAME) for n in numbers]
    counts = count_by([folder for folder in folders if folder], str)
    total = sum(counts.values())
    if total <= NOT_GAME_SHARE * len(numbers):
        return []
    where = ", ".join("%s/ %d" % item for item in counts.most_common())
    return ["hint: %d of %d numbers (%d percent) are in %s: add these to `skip` of %s if they are not the game"
            % (total, len(numbers), round(100.0 * total / len(numbers)), where, BENCH_JSON)]


def sense_lines(numbers, project):
    """A line for each balance number that a person classed, with a divisor name and no sense. The inventory
    does not write a sense into a number that a person classed."""
    loose = [n for n in numbers if decided(n) and class_of(n) == "balance" and part_of(n) == "div"]
    return ['sense: %s has a divisor name and no "sense". For 1/k in a control: pw_bench.py confirm %s --id %s '
            '--sense -1' % (n["id"], project, n["id"]) for n in loose]


def number_line(n):
    place_text = " | " + str(n["text"]) if "text" in n else ""
    return "%s = %s [%s]%s" % (n.get("id"), one_line(n.get("value")), class_of(n), place_text)


def count_lines(numbers):
    proposed = [n for n in numbers if n.get("proposed")]
    turned = sum(1 for n in proposed if n.get("sense") == -1)
    copies = sum(1 for n in proposed if n.get("rule") == FALLBACK_RULE)
    return ["numbers: %d" % len(numbers),
            "numbers by file kind: %s" % counts_text(count_by(numbers, kind_of), KINDS),
            "numbers by class: %s" % counts_text(count_by(numbers, class_of), CLASSES),
            "proposals to confirm: %d (%s)" % (len(proposed), counts_text(count_by(proposed, class_of), CLASSES))
            ] + (['proposals with "sense": -1 (a divisor name): %d' % turned] if turned else []) + [
            'proposals of the rule "fallback copy" (a default of get that copies a key of a data file; balance, '
            "not tunable): %d" % copies]


def scan_lines(facts, numbers):
    others = count_by(facts["others"], ext)
    lines = ["files read: %s" % counts_text(count_by(facts["files"], ext), KINDS)] + count_lines(numbers)
    lines += ["folder not read: %s/" % folder for folder in facts["skipped"]]
    lines += ["bench file not read: %s" % rel for rel in facts["bench"]]
    lines += ["not read, `skip` %s: %d files" % item for item in sorted(facts["skip"].items())]
    lines += ["files not read, and they can hold numbers: %s" % counts_text(others)] if others else []
    keys = facts["number_keys"]
    return lines + (["object keys that are numbers, not in the ledger: %d" % keys] if keys else [])


def say_report(report, show_all):
    say_list("new numbers", [number_line(n) for n in report["new"]], show_all)
    say_list("numbers that went", [number_line(n) for n in report["gone"]], show_all)
    say_list("numbers that moved", ["%s -> %s" % move for move in report["moved"]], show_all)
    changed = ["%s: %s -> %s" % (i, one_line(old), one_line(new)) for i, old, new in report["changed"]]
    say_list("numbers with a changed value", changed, show_all)


def next_init(data, project):
    """The init command that comes after the classification."""
    have = bool(data.get("controls"))
    return "pw_bench.py init %s%s" % (project, " --refresh-controls" if have else "")


def say_texts(data, news, gone, project, show_all):
    """The lines of the inventory about the texts with a number."""
    say(texts_line(data["texts"]))
    say_list("new texts with a number", [text_line(text) for text in news], show_all)
    say_list("texts with a number that went", [text_line(text) for text in gone], show_all)
    say_lines(text_hint(data, project))


def text_hint(data, project):
    """What to do with the texts that nobody accepted, or no line when there is none."""
    count = data["review"]["texts"]
    return ["texts: %d texts with a number are not accepted. Read each one. A text with a balance number must "
            "take the number from the data. Accept each other text: pw_bench.py confirm %s --text-file <glob> "
            '--ok "<reason>" (or --text-id <id>)' % (count, project)] if count else []


def end_lines(data, report, project):
    by_hand = len(report["by_hand"])
    kept = ["numbers written by hand that stay (a file kind that the inventory does not read): %d" % by_hand]
    return (kept if by_hand else []) + sense_lines(data["numbers"], project) + [
        "sources: %s" % (", ".join(data["sources"]) or "none"), review_line(data),
        "next: class each number in %s (pw_bench.py confirm), then %s, then pw_bench.py check %s"
        % (BENCH_JSON, next_init(data, project), project)]


def run_inventory(a):
    root = project_root(a.project)
    data = read_bench(root, make=True)
    first = not data.get("numbers")
    scanned, facts = scan_project(root, data)
    data["numbers"], report = update_ledger(data.get("numbers", []), scanned)
    data["texts"], new_texts, gone_texts = update_texts(text_list(data), facts["texts"])
    data["sources"] = sources_of(data["numbers"], data.get("sources", []), skip_list(data))
    data.setdefault("formulas", [])
    set_review(data)
    write_bench(root, data)
    say_lines(["pw_bench inventory: %s" % root] + scan_lines(facts, data["numbers"]) + not_game_lines(data["numbers"]))
    say_lines(["first inventory: each number is new"] if first else [])
    say_report(report, a.all)
    say_texts(data, new_texts, gone_texts, a.project, a.all)
    say_lines(end_lines(data, report, a.project))
    return 0


# ----------------------------------------------------------------------------- init
DEFAULT_PHASES = [
    {"id": "p1", "name": "Opening", "until": {"progress": 0.15}, "share": 0.1},
    {"id": "p2", "name": "Middle", "until": {"progress": 0.7}, "share": 0.5},
    {"id": "p3", "name": "End", "until": {"progress": 1.0}, "share": 0.4},
]
DEFAULT_PLAYERS = [
    {"id": "active", "name": "Active player", "decision_s": 2, "active_share": 0.8, "reset_gain": 1.0,
     "stuck_s": 600},
    {"id": "casual", "name": "Casual player", "decision_s": 30, "active_share": 0.2, "reset_gain": 2.0,
     "stuck_s": 1800},
    {"id": "idle", "name": "Idle player", "decision_s": 300, "active_share": 0.0, "reset_gain": 2.0,
     "stuck_s": 3600},
]
NO_SOURCE = 0.2                                       # the tolerance of a target with no source
DEFAULT_LENGTH_H = 6
DEFAULT_TARGETS = {
    "length_h": {"about": DEFAULT_LENGTH_H, "tolerance": NO_SOURCE},
    "upgrade_gap_min": {"median_about": 0.5, "longest_max": 2, "tolerance": NO_SOURCE},
    "content_gap_min": {"longest_max": 10, "first_hour_min_count": 11, "tolerance": NO_SOURCE},
    "impact": {"low_below": 0.03, "high_from": 0.25, "low_share_max": 0.4, "high_share_min": 0.1,
               "tolerance": NO_SOURCE},
    "idle_ratio": {"min": 1.5, "max": 6.0, "tolerance": NO_SOURCE},
    "wait_share_max": {"about": 0.15, "tolerance": NO_SOURCE},
}
# The targets file of the solver: (its key, its field) -> (the target of the bench, its field).
SOLVER_MAP = (
    (("length_h", "value"), ("length_h", "about")), (("length_h", "tolerance"), ("length_h", "tolerance")),
    (("purchase_gap", "largest_max_min"), ("upgrade_gap_min", "longest_max")),
    (("purchase_gap", "unlock_max_min"), ("content_gap_min", "longest_max")),
    (("unlocks", "first_hour"), ("content_gap_min", "first_hour_min_count")),
)
MAX_HOURS_FACTOR = 3                                  # sim.max_hours is this times the length target
MUL = dict(op="mul", min=0.25, max=4.0)
GROWTH_OP = dict(op="growth", min=0.5, max=2.0)
ADD_OP = dict(op="add", min=-0.1, max=0.1)            # for an exponent: k is added, and the range is small
ROLE_OP = dict(op="role", min=0.25, max=4.0)          # for each number, the op of its role: role_op
END_OP = dict(op="mul", min=0.5, max=2.0)
# The controls of the group "Whole game": (role, id, name, op). The role `other` has no control.
WHOLE = (
    ("cost_base", "all_cost", "All prices", MUL), ("cost_growth", "all_cost_growth", "Price growth", GROWTH_OP),
    ("effect", "all_effect", "Purchase punch", MUL),
    ("effect_factor", "all_effect_factor", "Purchase punch: factors", GROWTH_OP),
    ("effect_growth", "all_effect_growth", "Effect growth", GROWTH_OP),
    ("effect_exponent", "all_effect_exponent", "Effect exponent", ADD_OP),
    ("base_income", "all_base_income", "Base income", MUL),
    ("active_power", "all_active_power", "Manual action power", MUL),
    ("active_time", "all_active_time", "Manual action time", MUL),
    ("wall", "all_wall", "Walls", MUL), ("wall_growth", "all_wall_growth", "Wall growth", GROWTH_OP),
    ("unlock_at", "all_unlock", "Unlock points", MUL), ("reset_need", "all_reset_need", "Reset need", MUL),
    ("reset_gain", "all_reset_gain", "Reset gain", MUL), ("reset_bonus", "all_reset_bonus", "Reset bonus", MUL),
    ("cap", "all_cap", "Caps", MUL), ("timer", "all_timer", "Timers", MUL), ("rate", "all_rate", "Side rates", MUL),
    ("auto_at", "all_auto", "Automation start", MUL), ("offline", "all_offline", "Offline", MUL),
    ("reward", "all_reward", "Rewards", MUL), ("reward_growth", "all_reward_growth", "Reward growth", GROWTH_OP),
    ("end_at", "all_end", "Game end", END_OP),
)
# The controls of each phase: (id suffix, name, roles, op).
PHASE = (("cost", "prices", ["cost_base"], MUL), ("effect", "purchase punch", ["effect", "effect_factor"], ROLE_OP),
         ("wall", "walls and unlock points", ["wall", "unlock_at"], MUL))
# What init wrote before it marked its controls (the key `by`). A control of the file with no mark that equals
# 1 of these is a generated control that nobody edited: `init --refresh-controls` writes it again.
LEGACY_ROLES = ("cost_base", "cost_growth", "effect", "effect_growth", "base_income", "active_power", "active_time",
                "wall", "wall_growth", "unlock_at", "reset_need", "reset_gain", "reset_bonus", "cap", "timer", "rate",
                "auto_at", "offline", "reward")
LEGACY_PHASE = (("cost", "prices", ["cost_base"], MUL), ("effect", "purchase punch", ["effect"], MUL),
                ("wall", "walls and unlock points", ["wall", "unlock_at"], MUL))
# A number that leaves the control of its role: (part, id suffix, name suffix, op or None for the op of the role).
VARIANTS = (("exp", "_exp", ": exponent (k is added)", ADD_OP),
            ("div", "_div", ": divisor (a larger k gives less)", None))
PARTS = ("main", "exp", "div")
# The times away of a player, when the ledger has a number with the role offline: after `away_every_min` minutes
# of play the player closes the game for `away_min` minutes. 0 or no key: the player is never away.
AWAY = {"casual": {"away_every_min": 30, "away_min": 60}, "idle": {"away_every_min": 15, "away_min": 240}}
AWAY_KEYS = ("away_every_min", "away_min")
CONTROL_RULE = [
    "rule of the generated controls: 1 control moves its numbers in 1 direction. A larger k gives a larger number, "
    'and a number with "sense": -1 gets 1/k.',
    "  an exponent (`exponent` in the key name, or the role effect_exponent) is in no mul or growth control: it "
    "gets <control>_exp with the op add, -0.1 to 0.1",
    '  a divisor (`divisor`, `div` or `denominator` in the key name) with no "sense" gets <control>_div; with '
    '"sense": -1 it is in <control> and gets 1/k',
    "  a control with such a split selects its numbers by id: run `init --refresh-controls` after the ledger changes",
    '  a generated control has "by": "init:<stamp>". The stamp is a hash of the control, so `--refresh-controls` '
    "writes a control again only when nobody edited it",
]
# The controls of the group "Player" change the bot, not the game. The key `player` names the field of each
# player that the control multiplies: {"field": <name>}. The bench keeps a share between 0 and 1. This is the
# shape that the addon reads (bench_knobs.gd) and that the sample game has.
PLAYER = (("player_decision", "Decision time", "decision_s", 0.25, 4.0),
          ("player_active_share", "Share of time with the manual action", "active_share", 0.0, 1.25),
          ("player_reset_gain", "Reset gain that starts a reset", "reset_gain", 0.25, 4.0))
# The fields of a player that a control can change (the addon has the same list).
PLAYER_FIELDS = tuple(p[2] for p in PLAYER) + ("stuck_s", "away_every_min", "away_min")
CONTROL_KEYS = ("id", "group", "name", "op", "min", "max", "select")
ALL_NUMBERS_NOTE = ('The panel makes the controls of the group "All numbers" from the ledger when it starts: 1 for '
                    "each tunable balance number. They are not in the file.")


def tunable(numbers):
    """The numbers that a control can change: the class balance, and not `tunable: false`."""
    return [n for n in numbers if n.get("class") == "balance" and n.get("tunable", True) is not False]


def selected(control, numbers):
    """The tunable balance numbers that the `select` of a control names. A number must match each key."""
    select = control.get("select") or {}
    return [n for n in tunable(numbers) if all(n.get(key) in values for key, values in select.items())]


def role_op(role):
    """What a control with the op `role` does to a number with this role: growth for a factor (effect_factor)
    and for each role that ends in _growth, mul for each other role. The addon has the same rule."""
    return "growth" if role == "effect_factor" or str(role).endswith("_growth") else "mul"


def stamp(control):
    """The mark of a generated control: a hash of each key of the control but `by`."""
    plain = {key: value for key, value in control.items() if key != "by"}
    return "init:" + hashlib.sha1(json.dumps(plain, sort_keys=True).encode("utf-8")).hexdigest()[:8]


def marked(control):
    return dict(control, by=stamp(control))


def untouched(control, legacy):
    """True for a generated control that nobody edited: its mark is the hash of its content, or, with no mark,
    it equals a control that init wrote before it marked them."""
    if "by" in control:
        return control["by"] == stamp(control)
    return legacy.get(control.get("id")) == control


def key_name(number):
    """The key of a data number: the last token of its pointer that is not an index."""
    tokens = [token for token in str(number.get("pointer", "")).split("/") if token and text_number(token) is None]
    return tokens[-1] if tokens else ""


def part_of(number):
    """The part of a role control that a number goes to: `exp` for an exponent, `div` for a divisor with no
    sense, `main` for each other number."""
    found = words(key_name(number))
    if number.get("role") == "effect_exponent" or found & EXPONENT:
        return "exp"
    return "div" if found & DIVISOR and "sense" not in number else "main"


def with_ids(control, part):
    return dict(control, select=dict(control["select"], id=[n["id"] for n in part]))


def variant(control, part, suffix, label, op):
    made = dict(control, id=control["id"] + suffix, name=control["name"] + label)
    return with_ids(dict(made, **(op or {})), part)


def split_control(control, numbers, whole):
    """The controls that 1 default control becomes: none when it selects no number, the control itself when no
    exponent and no divisor is in it, and if not the control with its other numbers by id. A control of the
    whole game also gets 1 control for its exponents and 1 for its divisors."""
    hits = selected(control, numbers)
    parts = {kind: [n for n in hits if part_of(n) == kind] for kind in PARTS}
    if control["op"] == "add" or len(parts["main"]) == len(hits):
        return [control] if hits else []
    main = [with_ids(control, parts["main"])] if parts["main"] else []
    return main + [variant(control, parts[kind], suffix, label, op)
                   for kind, suffix, label, op in VARIANTS if whole and parts[kind]]


def phase_controls(phase, table=PHASE):
    name = phase.get("name", phase["id"])
    return [dict(id="%s_%s" % (phase["id"], suffix), group="Phase: %s" % name, name="%s: %s" % (name, what),
                 select={"role": roles, "phase": [phase["id"]]}, **op) for suffix, what, roles, op in table]


def player_controls():
    return [dict(id=cid, group="Player", name=name, op="mul", min=low, max=high, player={"field": field})
            for cid, name, field, low, high in PLAYER]


def game_controls(phases, roles, table):
    """The controls of the whole game as (control, True) and of each phase as (control, False)."""
    game = [(dict(id=cid, group="Whole game", name=name, select={"role": [role]}, **op), True)
            for role, cid, name, op in WHOLE if role in roles]
    return game + [(control, False) for phase in phases for control in phase_controls(phase, table)]


def phase_list(data):
    """The phases of the bench file that have an id."""
    phases = data.get("phases")
    return [p for p in phases if isinstance(p, dict) and p.get("id")] if isinstance(phases, list) else []


def default_controls(numbers, phases):
    """The controls of section 8 for the roles and the phases that the tunable balance numbers have, each with
    the mark of a generated control."""
    game = game_controls(phases, ROLES, PHASE)
    used = [part for control, whole in game for part in split_control(control, numbers, whole)]
    ordered = [{key: c[key] for key in CONTROL_KEYS} for c in used]
    return [marked(control) for control in ordered + player_controls()]


def legacy_controls(phases):
    """The controls that init wrote before it marked them, by id."""
    game = [control for control, _ in game_controls(phases, LEGACY_ROLES, LEGACY_PHASE)]
    return {control["id"]: control for control in game + player_controls()}


def stale_controls(data):
    """The ids of the generated controls of the file that nobody edited and that init would not write now."""
    fresh = {c["id"]: c for c in default_controls(data.get("numbers", []), phase_list(data))}
    mine = [c for c in data.get("controls") or [] if isinstance(c, dict) and c.get("by")]
    return [str(c.get("id")) for c in mine if c["by"] == stamp(c) and fresh.get(c.get("id")) != c]


def solver_targets(path):
    """Convert the targets file of the solver: its length, its purchase gap and its unlock targets.

    Return a list of (target, field, value).
    """
    data = read_json(path, "the targets file")
    tg = data.get("targets", data) if isinstance(data, dict) else None
    check(not isinstance(tg, dict), "%s is not a targets file of the solver" % path)
    found = [(to, tg[src[0]].get(src[1])) for src, to in SOLVER_MAP if isinstance(tg.get(src[0]), dict)]
    given = [(name, field, value) for (name, field), value in found if value is not None]
    check(not given, "%s has no length_h, purchase_gap or unlocks target" % path)
    return given


def set_target(targets, name, field, value, source):
    entry = targets.setdefault(name, {})
    old = entry.get(field)
    entry[field] = value
    note = "target %s.%s: %s -> %s (%s)" % (name, field, one_line(old), one_line(value), source)
    return [] if old == value else [note]


def set_targets(targets, given, source):
    """Write the target values that the command line gave. Each value that changes gives 1 note."""
    return [note for name, field, value in given for note in set_target(targets, name, field, value, source)]


def whole(value):
    return int(value) if value == int(value) else value


def length_target(hours):
    check(hours is not None and not hours > 0, "--length-h must be above 0")
    return [] if hours is None else [("length_h", "about", whole(hours))]


def init_sim(data, targets):
    if "sim" in data:
        return []
    about = targets["length_h"].get("about", DEFAULT_LENGTH_H)
    data["sim"] = {"max_hours": whole(about * MAX_HOURS_FACTOR), "seed": 1}
    return ["sim: added (max_hours %s)" % one_line(data["sim"]["max_hours"])]


def init_targets(data, a):
    """Add each target that is absent, then write the values of --targets and --length-h. Return the notes."""
    given = solver_targets(a.targets) if a.targets else []
    targets = data.setdefault("targets", {})
    absent = [name for name in DEFAULT_TARGETS if name not in targets]
    targets.update({name: dict(DEFAULT_TARGETS[name]) for name in absent})
    notes = ["target %s: added" % name for name in absent]
    notes += set_targets(targets, given, "--targets")
    notes += set_targets(targets, length_target(a.length_h), "--length-h")
    return notes + init_sim(data, targets)


def init_lists(data):
    """Add the phases and the players when the file has none. Return the notes."""
    defaults = (("phases", DEFAULT_PHASES), ("players", DEFAULT_PLAYERS))
    absent = [(key, default) for key, default in defaults if not data.get(key)]
    data.update({key: [dict(item) for item in default] for key, default in absent})
    return ["%s: added %s" % (key, ", ".join(item["id"] for item in default)) for key, default in absent]


def never_away(player):
    """True for a casual or idle player of the file with no away key."""
    return isinstance(player, dict) and player.get("id") in AWAY and not set(AWAY_KEYS) & set(player)


def init_away(data):
    """Give the casual and the idle player their times away when the ledger has a number with the role offline
    and the player has no away key. Without them no player closes the game, and no offline number has an effect."""
    offline = [n for n in data.get("numbers", []) if n.get("class") == "balance" and n.get("role") == "offline"]
    players = [p for p in data.get("players") or [] if never_away(p)] if offline else []
    for player in players:
        player.update(AWAY[player["id"]])
    who = ", ".join(str(p["id"]) for p in players)
    return ["players: added away_every_min and away_min to %s: the ledger has %d numbers with the role offline "
            "(write 0 for a player that is never away)" % (who, len(offline))] if players else []


def ids_text(ids):
    return ": " + ", ".join(map(str, ids)) if ids else ""


def add_controls(old, fresh):
    """Add each default control whose id is absent. A control of the file stays as it is."""
    have = {c.get("id") for c in old if isinstance(c, dict)}
    added = [c for c in fresh if c["id"] not in have]
    note = "controls: %d kept, %d added%s" % (len(have), len(added), ids_text(c["id"] for c in added))
    return old + added, [note]


def is_control(item):
    return isinstance(item, dict)


def refresh_notes(old, now, by_id):
    """The notes of a refresh: how many controls it wrote, which stayed, which are new, which went."""
    was = [c.get("id") for c in filter(is_control, old)]
    groups = (("kept as a person left them", list(by_id)), ("new", [i for i in now if i not in was]),
              ("removed", [i for i in was if i not in now and i not in by_id]))
    written = sum(1 for i in now if i not in by_id)
    return (["controls: %d generated controls written from the present ledger" % written]
            + ["controls: %d %s%s" % (len(ids), what, ids_text(ids)) for what, ids in groups])


def refresh_controls(old, fresh, legacy):
    """Write the generated controls again from the present ledger. A control that a person edited or added
    stays as it is; with the id of a default control it stays in the place of that control."""
    mine = [c for c in old if not (is_control(c) and untouched(c, legacy))]
    by_id = {c.get("id"): c for c in filter(is_control, mine)}
    now = [c["id"] for c in fresh]
    tail = [c for c in mine if not is_control(c) or c.get("id") not in now]
    return [by_id.get(c["id"], c) for c in fresh] + tail, refresh_notes(old, now, by_id)


def mixed_line(control, numbers):
    """A line for a control of the whole game that moves more than 1 key name, or None."""
    names = count_by(selected(control, numbers), key_name)
    text = ", ".join("%s %d" % (name or "(no key)", count) for name, count in names.most_common())
    line = "  %s moves %d key names, each to a larger number with a larger k: %s" % (control["id"], len(names), text)
    return line if len(names) > 1 else None


def split_line(control, numbers, project):
    """A line for a control that holds the exponents or the divisors of a role, or None."""
    cid = str(control["id"])
    ids = ", ".join(n["id"] for n in selected(control, numbers))
    if cid.endswith("_exp"):
        return "  %s (op add) holds: %s" % (cid, ids)
    turn = ('  %s holds: %s. To put a divisor into %s with 1/k: pw_bench.py confirm %s --id <id> --sense -1, then '
            'init --refresh-controls. --sense 1 puts it there as a plain factor' % (cid, ids, cid[:-4], project))
    return turn if cid.endswith("_div") else None


def control_lines(data, project):
    """The rule of the generated controls, and what it did in this ledger."""
    numbers = data.get("numbers", [])
    made = [c for c in data["controls"] if isinstance(c, dict) and c.get("by") and c.get("group") == "Whole game"]
    found = [split_line(c, numbers, project) or mixed_line(c, numbers) for c in made]
    open_count = sum(1 for n in tunable(numbers) if n.get("proposed"))
    warn = ["warning: %d tunable balance numbers are proposals that nobody confirmed, and the controls come from "
            "them. Confirm them, then: pw_bench.py init %s --refresh-controls" % (open_count, project)]
    return CONTROL_RULE + [line for line in found if line] + (warn if open_count else [])


def init_controls(data, a):
    """Write the controls: each default control whose id is absent, or with --refresh-controls each generated
    control again. Return the notes."""
    old = data.get("controls") if isinstance(data.get("controls"), list) else []
    phases = phase_list(data)
    fresh = default_controls(data.get("numbers", []), phases)
    write = refresh_controls(old, fresh, legacy_controls(phases)) if a.refresh_controls else add_controls(old, fresh)
    data["controls"] = write[0]
    return write[1] + control_lines(data, a.project)


def init_sources(root, data):
    """--phases-only comes before the inventory, and the first run needs the data files of the game. When
    `sources` is empty, write each data file that holds a balance proposal. The ledger stays as it is."""
    if data.get("sources"):
        return []
    scanned, _ = scan_project(root, data)
    data["sources"] = sources_of([dict(new, **new["_new"]) for new in scanned], [], skip_list(data))
    names = ", ".join(data["sources"]) or "none"
    return ["sources: added %d data files that hold a balance proposal, for the first run: %s"
            % (len(data["sources"]), names)]


def init_end(data, a):
    """The last lines of init: the roles, and the next command."""
    roles = count_by(tunable(data.get("numbers", [])), lambda n: n.get("role") or "no role")
    first = ["no control written (--phases-only)",
             "next: the first run (pw_bench.py run %s), then pw_bench.py inventory %s, then class and confirm "
             "each number, then pw_bench.py init %s" % ((a.project,) * 3)]
    later = ["tunable balance numbers by role: %s" % counts_text(roles), ALL_NUMBERS_NOTE,
             "next: pw_bench.py check %s" % a.project]
    return first if a.phases_only else later


def run_init(a):
    root = project_root(a.project)
    check(a.phases_only and a.refresh_controls,
          "--phases-only writes no control: do not give --refresh-controls with it")
    data = read_bench(root)
    notes = init_lists(data) + init_away(data) + init_targets(data, a)
    notes += init_sources(root, data) if a.phases_only else init_controls(data, a)
    write_bench(root, data)
    say_lines(["pw_bench init: %s" % root] + notes + init_end(data, a))
    return 0


# ----------------------------------------------------------------------------- confirm
# The selectors of `confirm`: (the option, the test of a number against the value of the option).
SELECTORS = (
    ("file", lambda n, glob: fnmatch.fnmatch(file_of(n).replace("res://", "", 1), glob)),
    ("cls", lambda n, cls: class_of(n) == cls),
    ("rule", lambda n, glob: fnmatch.fnmatch(str(n.get("rule", "")), glob)),
    ("id", lambda n, ids: n.get("id") in ids),
)


def selected_by(a, number):
    """True when the number matches each selector that the command line has."""
    return all(test(number, getattr(a, name)) for name, test in SELECTORS if getattr(a, name))


def confirm_changes(a):
    """The keys that `confirm` writes into each selected number."""
    check(a.set_class and a.set_class not in CLASSES, "--set-class must be 1 of %s" % ", ".join(CLASSES))
    check(a.role and a.role not in ROLES, "--role must be 1 of %s" % ", ".join(ROLES))
    pairs = (("class", a.set_class), ("role", a.role), ("element", a.element), ("phase", a.phase),
             ("sense", a.sense))
    return {key: value for key, value in pairs if value}


def confirmed(number, changes):
    """The number with no proposal mark and with the changes. A balance number in code or in a scene is not
    tunable; a number that is not balance has no role and no sense. A reason that a person wrote stays: the
    tool writes its reason only where there is none."""
    new = {k: v for k, v in number.items() if k not in ("proposed", "rule", "hint")}
    new.update(changes)
    balance = new.get("class") == "balance"
    by_tool = tool_reason(new.get("reason"))
    drop = () if balance else ("role", "sense") + (("tunable", "reason") if by_tool else ())
    reason = new["reason"] if str(new.get("reason") or "").strip() else IN_CODE
    fixed = {"tunable": False, "reason": reason} if balance and kind_of(new) not in DATA_KINDS else {}
    return dict({k: v for k, v in new.items() if k not in drop}, **fixed)


def confirm_line(n):
    role = " " + n["role"] if n.get("role") else ""
    place_text = " | " + str(n["text"]) if "text" in n else ""
    return "  %s = %s [%s%s]%s" % (n.get("id"), one_line(n.get("value")), class_of(n), role, place_text)


def confirm_lines(done):
    """The confirmed numbers as lines, with 1 group for each file."""
    files = list(dict.fromkeys(file_of(n) for n in done))
    groups = [(name, [n for n in done if file_of(n) == name]) for name in files]
    return [line for name, group in groups
            for line in ["%s: %d" % (name, len(group))] + [confirm_line(n) for n in group]]


def stale_lines(data, project):
    """A note when the generated controls of the file are not those that init writes from the present ledger."""
    stale = stale_controls(data)
    return ["note: %d generated controls are not those of the present ledger (%s): pw_bench.py init %s "
            "--refresh-controls" % (len(stale), ", ".join(stale), project)] if stale else []


def confirm_numbers(data, a):
    """Confirm each number that the selectors name, in the ledger. Return the confirmed numbers."""
    changes = confirm_changes(a)
    hit = [selected_by(a, n) and bool(changes or n.get("proposed")) for n in data["numbers"]]
    data["numbers"] = [confirmed(n, changes) if yes else n for n, yes in zip(data["numbers"], hit)]
    return [n for n, yes in zip(data["numbers"], hit) if yes]


# The rules file of `confirm --from`: a JSON list of rules, or an object with the key "rules". 1 rule:
#   {"match": {"file": "data/purchases.json", "pointer": "/purchases/*/requires/*/level", "key": "level",
#              "class": "balance", "role": "unlock_at"},
#    "set": {"class": "balance", "role": "unlock_at", "element": "{owner}", "phase": "p1", "label": "{owner}: needs {2}",
#            "sense": 1, "tunable": false, "reason": "...", "dead_ok": "..."}}
# Each key of "match" is optional, and a number must pass each key that is there. For each number the first rule
# that matches is the only rule that writes. See rule_tests for the keys of "match" and captures for the names
# that a text of "set" can use.
MATCH_KEYS = ("file", "pointer", "key", "class", "role")
SET_KEYS = ("class", "role", "element", "phase", "label", "sense", "tunable", "reason", "dead_ok")
SET_VALUES = (("class", CLASSES), ("role", ROLES), ("sense", SENSES), ("tunable", (True, False)))
CAPTURE = re.compile(r"\{(\w+)\}")


def pointer_pattern(glob):
    """The pattern of a pointer glob. `*` is 1 part of the pointer (no `/`), `**` is any count of parts, and each
    other character is itself, so `[` and `]` need no escape."""
    parts = re.split(r"(\*\*|\*)", glob)
    return re.compile("".join({"**": "(.*)", "*": "([^/]*)"}.get(part) or re.escape(part) for part in parts))


def pointer_match(glob, number):
    """Match the glob against the anchor of a number (the pointer with [id] in place of each index), and then
    against its pointer. Return the match or None."""
    pattern = pointer_pattern(glob)
    texts = [number.get("anchor"), number.get("pointer")]
    return next((found for found in (pattern.fullmatch(str(t)) for t in texts if t) if found), None)


# The keys of "match": (key, test of a number against the value of the key).
RULE_TESTS = (
    ("file", lambda n, glob: fnmatch.fnmatch(file_of(n).replace("res://", "", 1), glob)),
    ("pointer", lambda n, glob: pointer_match(glob, n) is not None),
    ("key", lambda n, glob: fnmatch.fnmatchcase(key_name(n), glob)),
    ("class", lambda n, cls: class_of(n) == cls),
    ("role", lambda n, role: n.get("role") == role),
)


def one_of(value, allowed):
    """True when the value is 1 of the allowed values, with the same type (true is not 1)."""
    return any(value == item and type(value) is type(item) for item in allowed)


def rule_keys_fault(rule):
    """The fault of the keys of "match" and "set", or None."""
    unknown = [key for key in rule["match"] if key not in MATCH_KEYS]
    unknown += [key for key in rule["set"] if key not in SET_KEYS]
    texts = all(isinstance(value, str) for value in rule["match"].values())
    what = ", ".join(unknown) or "a value of \"match\" that is not a text"
    return None if texts and not unknown else '"match" takes the texts %s, and "set" takes %s; not %s' % (
        ", ".join(MATCH_KEYS), ", ".join(SET_KEYS), what)


def rule_values_fault(rule):
    given = rule["set"]
    bad = [key for key, allowed in SET_VALUES if key in given and not one_of(given[key], allowed)]
    return '"set" has a wrong value for %s' % ", ".join(bad) if bad else None


def rule_fault(rule):
    """The fault of 1 rule of the rules file, or None."""
    parts = [rule.get("match"), rule.get("set")] if isinstance(rule, dict) else [None]
    if not all(isinstance(part, dict) for part in parts):
        return 'it needs the objects "match" and "set"'
    return rule_keys_fault(rule) or rule_values_fault(rule)


def read_rules(path):
    """Read the rules file of `confirm --from`. Each fault of a rule stops the command before it writes."""
    data = read_json(path, "the rules file")
    rules = data.get("rules") if isinstance(data, dict) else data
    check(not isinstance(rules, list) or not rules, '%s must be a list of rules, or an object with the list '
          '"rules"' % path)
    faults = ["rule %d: %s" % (i + 1, fault) for i, fault in enumerate(map(rule_fault, rules)) if fault]
    check(faults, "%s: %s" % (path, "; ".join(faults)))
    return rules


def rule_matches(rule, number):
    return all(test(number, rule["match"][key]) for key, test in RULE_TESTS if key in rule["match"])


def captures(rule, number):
    """The names that a text of "set" can use in braces: {owner} is the id of the object of a top-level list
    that owns the number, {key} is the key of the number, {element} is its present element, and {1}, {2} are
    the parts of the pointer that the first and the second `*` of the pointer glob matched, with no brackets."""
    found = pointer_match(rule["match"]["pointer"], number) if "pointer" in rule["match"] else None
    stars = [re.sub(r"^\[(.+)\]$", r"\1", part) for part in (found.groups() if found else ())]
    named = {"owner": owner_of(number.get("anchor")), "key": key_name(number), "element": number.get("element")}
    return dict(named, **{str(i + 1): part for i, part in enumerate(stars)})


def filled(text, names, where):
    """The text with each {name} replaced. A name with no value for this number stops the command."""
    absent = [name for name in CAPTURE.findall(text) if not names.get(name)]
    check(absent, "%s: {%s} has no value" % (where, "}, {".join(absent)))
    return CAPTURE.sub(lambda found: str(names[found.group(1)]), text)


def rule_changes(rule, index, number):
    """The keys that 1 rule writes into 1 number."""
    names = captures(rule, number)
    where = "rule %d, number %s" % (index + 1, number.get("id"))
    return {key: filled(value, names, where) if isinstance(value, str) else value
            for key, value in rule["set"].items()}


def first_rule(rules, number):
    """The index of the first rule that matches the number, or None."""
    return next((i for i, rule in enumerate(rules) if rule_matches(rule, number)), None)


def rule_lines(rules, hits, loose, show_all):
    """The count of each rule, and each number of the selection that no rule matched and that is still open (a
    proposal, or unclassified)."""
    counts = count_by([hit for hit in hits if hit not in (None, -1)], int)
    lines = ["rule %d %s: %d numbers" % (i + 1, one_line(rule["match"]), counts.get(i, 0))
             for i, rule in enumerate(rules)]
    still = [n for n in loose if n.get("proposed") or class_of(n) == "unclassified"]
    head = "no rule matched: %d numbers; %d of them are not confirmed" % (len(loose), len(still))
    return lines + [head] + short([number_line(n) for n in still], show_all)


def rule_hits(data, a, rules):
    """For each number of the ledger: the index of the first rule that matches it, None when no rule matches,
    and -1 for a number that the selectors do not name."""
    chosen = [selected_by(a, n) for n in data["numbers"]]
    return [first_rule(rules, n) if yes else -1 for n, yes in zip(data["numbers"], chosen)]


def by_rule(rules, hit, number):
    """The number after the rule `hit` wrote its keys, or the number as it is when no rule is for it."""
    return confirmed(number, rule_changes(rules[hit], hit, number)) if hit not in (None, -1) else number


def confirm_from(data, a):
    """Confirm with the rules file: for each number that the selectors name (each number of the ledger with no
    selector), the first rule that matches writes its keys. Return (the confirmed numbers, the report lines)."""
    check(confirm_changes(a), "--from takes the keys from the rules file: do not give --set-class, --role, "
                              "--element, --phase or --sense with it")
    rules = read_rules(a.rules)
    hits = rule_hits(data, a, rules)
    data["numbers"] = [by_rule(rules, hit, n) for n, hit in zip(data["numbers"], hits)]
    pairs = list(zip(data["numbers"], hits))
    loose = [n for n, hit in pairs if hit is None]
    return [n for n, hit in pairs if hit not in (None, -1)], rule_lines(rules, hits, loose, a.all)


def text_selected(a, text):
    """True for a text that --text-id names (its id, or a glob on its id), or a text with no `ok` in a file
    that --text-file names."""
    rel = file_of(text).replace("res://", "", 1)
    by_file = bool(a.text_file) and fnmatch.fnmatch(rel, a.text_file) and not accepted(text)
    return id_named(text.get("id"), a.text_id) or by_file


def id_named(tid, wanted):
    return tid in wanted or any(fnmatch.fnmatchcase(str(tid), glob) for glob in wanted)


def text_reason(a):
    """The reason of --ok. --text-id and --text-file need it, and it needs 1 of them."""
    wanted = bool(a.text_id or a.text_file)
    reason = str(a.ok or "").strip()
    check(wanted != bool(reason), '--text-id and --text-file need --ok "<reason>", and --ok needs 1 of them')
    return reason


def confirm_texts(data, a):
    """Write the reason of --ok into each text that --text-id or --text-file names. Return the texts."""
    reason = text_reason(a)
    texts = text_list(data)
    known = [text.get("id") for text in texts]
    absent = [tid for tid in a.text_id if not any(id_named(have, [tid]) for have in known)]
    check(absent, "no text of the list `texts` has the id %s: run `pw_bench.py inventory` first" % ", ".join(absent))
    hits = [bool(reason) and text_selected(a, text) for text in texts]
    data["texts"] = [dict(text, ok=reason) if yes else text for text, yes in zip(texts, hits)]
    return [text for text, yes in zip(data["texts"], hits) if yes]


def confirm_selectors(a):
    numbers = bool(a.file or a.cls or a.rule or a.id or a.rules)
    check(not (numbers or a.text_id or a.text_file or a.ok),
          "confirm needs 1 selector or more: --file, --class, --rule, --id, --from, --text-id, --text-file")
    return numbers


def run_confirm(a):
    root = project_root(a.project)
    data = read_bench(root)
    numbers = confirm_selectors(a)
    texts = confirm_texts(data, a)
    plain = ([], []) if a.rules or not numbers else (confirm_numbers(data, a), [])
    done, report = confirm_from(data, a) if a.rules else plain
    set_review(data)
    write_bench(root, data)
    say_lines(["pw_bench confirm: %s" % root] + confirm_lines(done) + report)
    say_lines(["text accepted: " + text_line(text) for text in texts])
    say_lines([review_line(data)] + ([texts_line(data["texts"])] if a.ok else []) + stale_lines(data, a.project))
    say("pw_bench confirm: %d numbers confirmed%s" % (len(done), ", %d texts accepted" % len(texts) if a.ok else ""))
    return 0 if done or texts else 1


# ----------------------------------------------------------------------------- check
def in_code_and_tunable(n):
    return kind_of(n) not in DATA_KINDS and n.get("tunable", True) is not False


# The rules for the numbers: (message, True for the balance numbers only, test). phase_ids is None when the
# phases have a fault.
NUMBER_RULES = (
    ("unclassified: %d numbers have the class unclassified", False,
     lambda n, phase_ids: class_of(n) == "unclassified"),
    ("class: %d numbers have a class that is not balance, structure, presentation, engine or unclassified", False,
     lambda n, phase_ids: class_of(n) not in CLASSES),
    ("role: %d balance numbers have no role", True, lambda n, phase_ids: not n.get("role")),
    ("role: %d balance numbers have a role that is not in the list of section 5", True,
     lambda n, phase_ids: bool(n.get("role")) and n["role"] not in ROLES),
    ('phase: %d balance numbers have a phase that is not a phase id and not "all"', True,
     lambda n, phase_ids: phase_ids is not None and n.get("phase") is not None and n["phase"] not in phase_ids),
    ('in code: %d balance numbers are in code or in a scene and are tunable: move each one to a data file, or '
     'write "tunable": false with a reason', True, lambda n, phase_ids: in_code_and_tunable(n)),
    ("reason: %d numbers are not tunable and have no reason", False,
     lambda n, phase_ids: n.get("tunable") is False and not str(n.get("reason") or "").strip()),
    ('sense: %d numbers have a "sense" that is not 1 or -1', False,
     lambda n, phase_ids: "sense" in n and (isinstance(n["sense"], bool) or n["sense"] not in SENSES)),
)


def number_faults(numbers, phase_ids):
    """The failures of the numbers as (message, numbers)."""
    balance = [n for n in numbers if n.get("class") == "balance"]
    found = [(text, [n for n in (balance if only_balance else numbers) if test(n, phase_ids)])
             for text, only_balance, test in NUMBER_RULES]
    empty = [("numbers: the ledger is empty: run `pw_bench.py inventory`", [])] if not numbers else []
    return empty + [(text % len(hits), hits) for text, hits in found if hits]


CONFIRM_HINT = ["  read each file, then confirm its proposals: pw_bench.py confirm <project> --file <glob> "
                "[--class <class>] [--rule <rule>] [--id <id> ...]"]


def core_folder(data):
    """The folder of the core: the folder of the key "core" of the bench file, or core/."""
    return os.path.dirname(str(data.get("core") or "").replace("res://", "", 1)) or DEFAULT_CORE


def economy_files(root, data):
    """The files that the economy reads for sure: each source, the adapter, each file of the autoload list."""
    listed = [str(source) for source in data.get("sources", [])] + [str(data.get("adapter") or "")]
    return set(listed) | autoload_files(root)


def proposal_faults(root, data, strict=False):
    """A proposal that is not confirmed fails the gate when its file is one that the economy reads: a source,
    a .gd file of the core, the adapter, a file of the autoload list. A wrong class there hides a number of the
    game. In each other file a proposal is a note. With --strict each proposal fails, in any file."""
    files, core = economy_files(root, data), dict(paths=[core_folder(data)])
    proposed = [n for n in data.get("numbers", []) if n.get("proposed")]
    hits = [n for n in proposed if file_of(n) in files or (kind_of(n) == CODE_KIND and in_core(file_of(n), core))]
    text = ("proposal: %d proposals are not confirmed in a file that the economy reads (a source, the core, the "
            "adapter, an autoload)" % len(hits))
    every = "strict: %d proposals are not confirmed (--strict fails on each proposal, in any file)" % len(proposed)
    found = (every, proposed) if strict else (text, hits)
    return [found] if found[1] else []


def ids_fine(items):
    ids = [item.get("id") for item in items if isinstance(item, dict)]
    return len(ids) == len(items) and all(ids) and len(set(map(str, ids))) == len(ids)


def progress_of(phase):
    until = phase.get("until")
    return until.get("progress") if isinstance(until, dict) else None


def phase_step_fault(phase, last):
    """The fault of 1 phase, or None. `last` is the largest progress of the phases before it."""
    until = phase.get("until")
    if not isinstance(until, dict) or not set(until) & {"progress", "event", "resets"}:
        return "the phase %s has no `until` with progress, event or resets" % phase["id"]
    at = until.get("progress")
    fine = at is None or (isinstance(at, (int, float)) and last < at <= 1.0)
    return None if fine else ("the phase %s ends at progress %s: each phase must end after the phase before it, "
                              "and at 1 or below" % (phase["id"], one_line(at)))


def phase_fault(phases):
    """The first fault of the phase list, or None. Phases are in order and the last phase ends at progress 1."""
    if not ids_fine(phases):
        return "each phase needs an id, and no 2 phases can have the same id"
    marks = [at if isinstance(at, (int, float)) else 0.0 for at in map(progress_of, phases)]
    faults = [phase_step_fault(phase, max([0.0] + marks[:i])) for i, phase in enumerate(phases)]
    end = None if progress_of(phases[-1]) == 1 else "the last phase %s does not end at progress 1" % phases[-1]["id"]
    return next((fault for fault in faults + [end] if fault), None)


def op_fault(control, numbers, phase_ids):
    fine = control.get("op") in ("mul", "growth", "add", "set", "role")
    return None if fine else "the op %s is not mul, growth, add, set or role" % one_line(control.get("op"))


def select_fault(control, numbers, phase_ids):
    select = control.get("select")
    fine = isinstance(select, dict) and all(isinstance(values, list) for values in select.values())
    return None if fine else "`select` must map each key to a list"


def phase_ref_fault(control, numbers, phase_ids):
    names = [] if phase_ids is None else control["select"].get("phase", [])
    unknown = [str(name) for name in names if name not in phase_ids]
    return "the phase %s is not a phase id" % ", ".join(unknown) if unknown else None


def count_fault(control, numbers, phase_ids):
    count = len(selected(control, numbers))
    advice = "; it is a generated control: run `pw_bench.py init <project> --refresh-controls`"
    again = advice if control.get("by") else ""
    if count == 0:
        return "it selects 0 numbers; each control needs 1 tunable balance number or more" + again
    return "the op set needs 1 number, and it selects %d" % count if control["op"] == "set" and count != 1 else None


def player_fault(control):
    spec = control["player"]
    field = spec.get("field") if isinstance(spec, dict) else spec
    fine = isinstance(spec, dict) and field in PLAYER_FIELDS
    return None if fine else "`player` must be {\"field\": <name>} with 1 of %s, not %s" % (
        ", ".join(PLAYER_FIELDS), one_line(spec))


def control_fault(control, numbers, phase_ids):
    """The fault of 1 control, or None. A control of the group "Player" has the key `player` and no `select`."""
    if "player" in control:
        return player_fault(control)
    checks = (op_fault, select_fault, phase_ref_fault, count_fault)
    return next((fault for fault in (test(control, numbers, phase_ids) for test in checks) if fault), None)


def minutes_fine(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and value >= 0


def check_players(data):
    """The away keys of each player are minutes, 0 or above. 0 or no key: the player is never away."""
    players = [p for p in data.get("players") or [] if isinstance(p, dict)]
    bad = [(p.get("id"), key) for p in players for key in AWAY_KEYS if key in p and not minutes_fine(p[key])]
    return ["player %s: %s must be a number of minutes, 0 or above" % item for item in bad]


def absent(data, key):
    return not isinstance(data.get(key), list) or not data[key]


def next_steps(data, project):
    """What the bench file does not have yet, with the command that writes it. These lines are not a fault of
    the ledger: the classification needs no phase and no control."""
    phases = ["phases: there is no phase yet. The classes need none: class and confirm the numbers now. For the "
              "phases and the first run: pw_bench.py init %s --phases-only" % project]
    controls = ["controls: there is no control yet. After the classification: pw_bench.py init %s" % project]
    return (phases if absent(data, "phases") else []) + (controls if absent(data, "controls") else [])


def check_controls(data, phase_ids):
    controls = data.get("controls")
    if absent(data, "controls"):
        return []
    if not ids_fine(controls):
        return ["controls: each control needs an id, and no 2 controls can have the same id"]
    faults = [(c["id"], control_fault(c, data.get("numbers", []), phase_ids)) for c in controls]
    return ["control %s: %s" % (cid, fault) for cid, fault in faults if fault]


def check_sources(root, data):
    known = [str(source) for source in data.get("sources", [])]
    absent = [s for s in known if not os.path.isfile(os.path.join(root, s.replace("res://", "", 1)))]
    files = {n.get("file", "?") for n in tunable(data.get("numbers", [])) if kind_of(n) in DATA_KINDS}
    lost = sorted(name for name in files if name not in known)
    return (["sources: the file %s does not exist" % name for name in absent]
            + ["sources: the file %s has tunable numbers and is not in `sources`" % name for name in lost])


def check_link(data):
    linked = data.get("core") or data.get("adapter")
    return [] if linked else ['link: the bench file has no "core" and no "adapter": it needs 1 of them']


def check_result(failures, steps):
    """The last line of check, and its exit code."""
    todo = "%d steps to do (the lines NEXT)" % steps
    if failures:
        return "%d failures%s" % (failures, ", " + todo if steps else ""), 1
    return ("no failure in the ledger, " + todo, 1) if steps else ("pass", 0)


def note_lines(numbers):
    balance = [n for n in numbers if n.get("class") == "balance"]
    proposed = [n for n in numbers if n.get("proposed")]
    fixed = len(balance) - len(tunable(balance))
    bare = [sum(1 for n in balance if not n.get(key)) for key in ("element", "phase")]
    return ["numbers by class: %s" % counts_text(count_by(numbers, class_of), CLASSES),
            "note: %d proposals are not confirmed (%s)" % (len(proposed),
                                                           counts_text(count_by(proposed, class_of), CLASSES)),
            "note: %d balance numbers, %d not tunable, %d with no element, %d with no phase" % (
                len(balance), fixed, bare[0], bare[1])]


def fault_lines(text, hits, show_all):
    by_file = ["  by file: %s" % counts_text(count_by(hits, file_of))] if hits else []
    return ["FAIL " + text] + by_file + short([number_line(n) for n in hits], show_all)


def known_phases(data):
    """Return (the fault of the phase list or None, the phase ids or None when there is no good phase list)."""
    if absent(data, "phases"):
        return None, None
    fault = phase_fault(data["phases"])
    return fault, None if fault else {"all"} | {phase["id"] for phase in data["phases"]}


def plain_faults(root, data, fault, phase_ids):
    """The failures that list no number: the phases, the controls, the players, the sources, the link."""
    first = ["phases: " + fault] if fault else []
    return (first + check_controls(data, phase_ids) + check_players(data) + check_sources(root, data)
            + check_link(data))


def check_head(root, data, project):
    """The first lines of check: the counts, the review line, the texts and the note about stale controls."""
    return (["pw_bench check: %s" % root] + note_lines(data.get("numbers", [])) + [review_line(data)]
            + [texts_line(text_list(data))] + stale_lines(data, project))


def text_faults(data, strict):
    """With --strict, each text with a number that nobody accepted fails. Return (message, texts) or None."""
    still = [text for text in text_list(data) if not accepted(text)]
    found = ('strict: %d texts with a number have no "ok" (--strict fails on each text that nobody accepted)'
             % len(still), still)
    return found if strict and still else None


def text_fault_lines(found, show_all):
    text, hits = found
    by_file = ["  by file: %s" % counts_text(count_by(hits, file_of))]
    return ["FAIL " + text] + by_file + short([text_line(hit) for hit in hits], show_all)


def say_text_faults(data, a):
    """Print the failure of the texts (--strict), or the hint about the texts that nobody accepted. Return the
    count of failures: 1 or 0."""
    found = text_faults(data, a.strict)
    say_lines(text_fault_lines(found, a.all) if found else text_hint(data, a.project))
    return 1 if found else 0


def run_check(a):
    root = project_root(a.project)
    data = read_bench(root)
    fault, phase_ids = known_phases(data)
    proposals = proposal_faults(root, data, a.strict)
    listed = number_faults(data.get("numbers", []), phase_ids) + proposals
    plain = plain_faults(root, data, fault, phase_ids)
    steps = next_steps(data, a.project)
    if set_review(data):
        write_bench(root, data)
    say_lines(check_head(root, data, a.project))
    say_lines([line for text, hits in listed for line in fault_lines(text, hits, a.all)])
    say_lines(CONFIRM_HINT if proposals else [])
    texts = say_text_faults(data, a)
    say_lines(["FAIL " + text for text in plain] + ["NEXT " + s for s in steps])
    text, code = check_result(len(listed) + len(plain) + texts, len(steps))
    say("pw_bench check: " + text)
    return code


# ----------------------------------------------------------------------------- corecheck
CORE_BASES = ("RefCounted", "Resource", "Object")
CORE_FUNCTIONS = ("new_state", "step", "actions", "apply", "rate", "progress")
CORE_OPTIONAL = ("active", "wealth", "max_step")
ACCEPT = re.compile(r"#\s*corecheck:\s*ok\s+(\S.*)$")
SCRIPT_VAR = re.compile(r"(?:@\w+(?:\([^)]*\))?\s+)*(?:static\s+)?var\b")
SCENE, FILES, RANDOM, ASYNC = "scene or clock", "file or network", "hidden randomness", "async or signal"
CORE_NAMES = dict(
    [(n, SCENE) for n in "get_tree get_node add_child _process _physics_process _ready _input get_viewport".split()]
    + [(n, FILES) for n in "FileAccess DirAccess ResourceLoader HTTPRequest ProjectSettings".split()]
    + [(n, RANDOM) for n in "randf randi randf_range randi_range randomize".split()]
    + [(n, ASYNC) for n in "await signal Thread Mutex".split()])
CORE_SINGLETONS = ("Engine", "Time", "OS", "Input", "DisplayServer")
CORE_METHODS = ("emit", "connect")
EDGE = (None, None, 0)                                # the token before the first token and after the last token


def in_core(path, core):
    """True when a res:// path is in 1 of the core paths."""
    rel = path.replace("res://", "", 1)
    return any(rel == c or rel.startswith(c + "/") for c in core["paths"])


# Each rule reads 1 token, the token before it, the 2 tokens after it, and the facts of the core. It returns
# (rule, detail) for a finding, or None.
def rule_name(tok, before, after, after2, core):
    hit = tok[0] == "name" and tok[1] in CORE_NAMES
    return (CORE_NAMES[tok[1]], tok[1]) if hit else None


def rule_singleton(tok, before, after, after2, core):
    if tok[0] == "name" and tok[1] in CORE_SINGLETONS and after[1] == "." and before[1] != ".":
        return SCENE, tok[1] + "."
    new = tok[1] == "RandomNumberGenerator" and (after[1], after2[1]) == (".", "new")
    return (RANDOM, "RandomNumberGenerator.new: keep a seed in the state") if new else None


def rule_method(tok, before, after, after2, core):
    hit = tok[0] == "name" and tok[1] in CORE_METHODS and before[1] == "." and after[1] == "("
    return (ASYNC, ".%s(" % tok[1]) if hit else None


def rule_load(tok, before, after, after2, core):
    if tok[0] != "name" or tok[1] not in ("load", "preload") or after[1] != "(" or before[1] == ".":
        return None
    path = after2[1].strip("\"'") if after2[0] == "string" else None
    inside = bool(path) and in_core(path, core)
    return None if inside else (FILES, "%s(%s) of a file outside the core" % (tok[1], path or "..."))


def rule_autoload(tok, before, after, after2, core):
    hit = tok[0] == "name" and tok[1] in core["autoloads"] and before[1] != "."
    return ("autoload", tok[1]) if hit else None


def rule_node(tok, before, after, after2, core):
    if tok[0] != "op" or after[0] not in ("name", "string"):
        return None
    unique = tok[1] == "%" and after[0] == "name" and not gd_operand(before[:2])
    return (SCENE, "node path %s%s" % (tok[1], after[1])) if tok[1] == "$" or unique else None


def rule_var(tok, before, after, after2, core):
    hit = tok[:2] == ("name", "var") and SCRIPT_VAR.match(core["lines"][tok[2] - 1])
    return ("script-level var", "%s: the state must come in as an argument" % after[1]) if hit else None


def rule_extends(tok, before, after, after2, core):
    if tok[:2] != ("name", "extends"):
        return None
    base = str(after[1]).strip("\"'")
    fine = in_core(base, core) if after[0] == "string" else base in CORE_BASES or base in core["classes"]
    return None if fine else ("extends", "%s: a core script extends %s or nothing" % (base, ", ".join(CORE_BASES)))


CORE_RULES = (rule_extends, rule_name, rule_singleton, rule_method, rule_load, rule_autoload, rule_node, rule_var)


def token_findings(tokens, i, core):
    hits = [rule(tokens[i], tokens[i - 1], tokens[i + 1], tokens[i + 2], core) for rule in CORE_RULES]
    line = tokens[i][2]
    accept = ACCEPT.search(core["lines"][line - 1])
    reason = accept.group(1).strip() if accept else None
    return [(line, hit[0], hit[1], reason) for hit in hits if hit]


def core_findings(text, core):
    """The findings of 1 core script as (line, rule, detail, the reason of the accept comment or None)."""
    core = dict(core, lines=text.split("\n"))
    tokens = [EDGE] + gd_tokens(text) + [EDGE, EDGE]
    return [finding for i in range(1, len(tokens) - 2) for finding in token_findings(tokens, i, core)]


def autoload_text(root):
    """The text of the section [autoload] of project.godot."""
    text = read_text(os.path.join(root, "project.godot"))
    section = re.search(r"^\[autoload\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    return section.group(1) if section else ""


def autoload_names(root):
    return set(re.findall(r"^(\w+)\s*=", autoload_text(root), re.M))


def autoload_files(root):
    """The res:// path of each file of the autoload list."""
    return set(re.findall(r'^\w+\s*=\s*"\*?(res://[^"\n]+)"', autoload_text(root), re.M))


def core_paths(root, given, data):
    """The core paths from the project: --core, or the folder of the key "core" of the bench file, or core/."""
    paths = given or [core_folder(data)]
    paths = [path.replace("res://", "", 1).replace(os.sep, "/").strip("/") for path in paths]
    absent = [path for path in paths if not os.path.exists(os.path.join(root, path))]
    check(absent, "the core path %s is absent%s" % (", ".join(absent), "" if given else other_core(root)))
    return paths


CORE_FILE = "game_core.gd"
CORE_CLASS = re.compile(r"^class_name\s+GameCore\b", re.M)


def core_like(root, rel):
    """True for a script with the file name or the class name of a core."""
    return os.path.basename(rel) == CORE_FILE or bool(CORE_CLASS.search(read_text(os.path.join(root, rel))))


def core_folders(root):
    """Each folder of the game with a file game_core.gd or a script of the class GameCore."""
    scripts = [rel for rel in project_files(root)["files"] if ext(rel) == CODE_KIND]
    return list(dict.fromkeys(os.path.dirname(rel) or "." for rel in scripts if core_like(root, rel)))


def other_core(root):
    """The end of the message for a default core path that is absent: the folders that can be the core."""
    found = core_folders(root)
    if not found:
        return ": name the folder or the file of the core with --core <path>"
    return ("; the folder %s has %s or a script with the class GameCore: if it is the core, give --core %s"
            % (", ".join(f + "/" for f in found), CORE_FILE, found[0]))


def gd_files_of(root, path):
    full = os.path.join(root, path)
    walked = [os.path.join(folder, name) for folder, _, names in os.walk(full) for name in names]
    found = walked if os.path.isdir(full) else [full]
    return [os.path.relpath(name, root).replace(os.sep, "/") for name in found if name.endswith(CODE_KIND)]


def core_setup(root, given, data):
    """Return (the facts of the core, the text of each core script)."""
    paths = core_paths(root, given, data)
    files = sorted({name for path in paths for name in gd_files_of(root, path)})
    check(not files, "the core paths %s hold no .gd file" % ", ".join(paths))
    texts = {rel: read_text(os.path.join(root, rel)) for rel in files}
    pairs = [p for text in texts.values() for p in zip(gd_tokens(text), gd_tokens(text)[1:])]
    classes = {name[1] for word, name in pairs if word[1] == "class_name"}
    return dict(paths=paths, autoloads=autoload_names(root), classes=classes), texts


def core_functions(root, data):
    """The static functions of the core script of the bench file, or None when the file names no core."""
    script = str(data.get("core") or "").replace("res://", "", 1)
    if not script:
        return None
    check(not os.path.isfile(os.path.join(root, script)), 'the core script %s of the key "core" is absent' % script)
    return static_functions(gd_tokens(read_text(os.path.join(root, script))))


def missing_functions(root, data):
    """Return (a line for each required function that the core script does not define, a note for each optional
    function that it does not define). The functions are those of references/game-core.md, section 3."""
    have = core_functions(root, data)
    script = str(data.get("core") or "").replace("res://", "", 1)
    absent = [name for name in CORE_FUNCTIONS + CORE_OPTIONAL if have is not None and name not in have]
    fails = ["%s:1 contract: the core script has no static function %s" % (script, name)
             for name in absent if name in CORE_FUNCTIONS]
    notes = ["note: %s has no static function %s (optional)" % (script, name)
             for name in absent if name in CORE_OPTIONAL]
    return fails, notes


def finding_lines(texts, core):
    """Return (the findings that fail, the findings that a comment accepts), each as printed lines."""
    found = [(rel, finding) for rel in texts for finding in core_findings(texts[rel], core)]
    fails = ["%s:%d %s: %s" % ((rel,) + f[:3]) for rel, f in found if not f[3]]
    accepted = ["%s:%d %s: %s (accepted: %s)" % ((rel,) + f) for rel, f in found if f[3]]
    return fails, accepted


def outside_lines(data, core):
    """The count of balance numbers of the ledger that are in code outside the core."""
    numbers = [n for n in data.get("numbers", []) if n.get("class") == "balance" and kind_of(n) == CODE_KIND]
    outside = [n for n in numbers if not in_core(n["file"], core)]
    return ["balance numbers in code outside the core: %d" % len(outside)] if data else []


def run_corecheck(a):
    root = project_root(a.project)
    data = read_bench(root) if os.path.exists(os.path.join(root, BENCH_JSON)) else {}
    core, texts = core_setup(root, a.core, data)
    fails, accepted = finding_lines(texts, core)
    missing, notes = missing_functions(root, data)
    fails = missing + fails
    result = "%d findings" % len(fails) if fails else "pass"
    say_lines(["pw_bench corecheck: %s" % root, "core: %s (%d .gd files)" % (", ".join(core["paths"]), len(texts))])
    say_lines(["FAIL " + line for line in fails] + ["ok   " + line for line in accepted] + notes)
    say_lines(outside_lines(data, core) + ["pw_bench corecheck: %s, %d accepted" % (result, len(accepted))])
    return 1 if fails else 0


# ----------------------------------------------------------------------------- selfcheck and run
METER_HEAD = ["Id", "Meter", "Value", "Target", "Status", "Controls that move it"]


def godot_program(given):
    program = given or shutil.which("godot")
    found = bool(program) and (shutil.which(program) or os.path.isfile(program))
    check(not found, "the program godot is not found: put it on the path or give --godot <path>")
    return program


def echo_line(line):
    """Print 1 line of the output of Godot now, so that a long self-check shows its progress. Return the line."""
    sys.stdout.write("  | " + line.rstrip("\r\n") + "\n")
    sys.stdout.flush()
    return line


def no_echo(line):
    return line


PROGRESS_LINE = re.compile(r"\s*\S.*: (\d+) of (\d+)\s*")      # "  dead-number check: 40 of 297"
PROGRESS_EVERY = 10                                   # print each 10th progress line
PROGRESS_QUIET_S = 60.0                               # and each progress line after this many seconds with none
IDLE_POLL_S = 1.0
HOME_FOLDERS = (("XDG_DATA_HOME", ".local/share"), ("XDG_CONFIG_HOME", ".config"), ("XDG_CACHE_HOME", ".cache"))


def duration(seconds):
    """A time for a person: "45 s", "12 min 5 s", "1 h 9 min"."""
    hours, rest = divmod(int(round(seconds)), 3600)
    parts = [(hours, "h"), (rest // 60, "min"), (rest % 60, "s")]
    shown = [part for part in parts if part[0]][:2] or [(0, "s")]
    return " ".join("%d %s" % part for part in shown)


class Progress:
    """The echo of the self-check. A progress line of Godot ("<what>: <done> of <total>") is printed when it is
    the first one, each 10th one, the last one, or when the last printed progress line is 60 seconds old. It gets
    the time since the start and an estimate of the time that is left. Each other line is printed as it comes."""

    def __init__(self, clock=time.monotonic):
        self.clock, self.start, self.shown, self.count = clock, clock(), None, 0

    def __call__(self, line):
        found = PROGRESS_LINE.fullmatch(line.rstrip("\r\n"))
        if not found:
            return echo_line(line)
        self.progress(line.rstrip("\r\n"), int(found.group(1)), int(found.group(2)), self.clock())
        return line

    def due(self, done, total, now):
        self.count += 1
        quiet = self.shown is not None and now - self.shown >= PROGRESS_QUIET_S
        return self.count == 1 or self.count % PROGRESS_EVERY == 0 or done >= total or quiet

    def progress(self, line, done, total, now):
        if not self.due(done, total, now):
            return
        self.shown = now
        elapsed = now - self.start
        rest = "about %s left" % duration(elapsed * (total - done) / done) if done > 0 else "no estimate yet"
        echo_line("%s (%s elapsed, %s)" % (line, duration(elapsed), rest))


def kill_group(proc):
    """Stop the process group of Godot, or Godot alone where the system has no process group."""
    done = False
    try:
        os.killpg(proc.pid, 9)
        done = True
    except (AttributeError, OSError):
        proc.kill()
    return done


def stop(proc):
    """Stop Godot and the child processes that it started (the jobs of the self-check)."""
    return kill_group(proc) if proc.poll() is None else None


def idle_guard(proc, timeout, state, done):
    """Stop Godot when it printed no line for `timeout` seconds."""
    step = min(IDLE_POLL_S, timeout / 4.0)
    while not done.wait(step) and time.monotonic() - state["last"] <= timeout:
        continue
    state["late"] = not done.is_set()
    return stop(proc) if state["late"] else None


def watch(proc, timeout, echo):
    """Read the output of Godot line by line, and give each line to `echo` when it comes. Stop Godot when it
    prints no line for `timeout` seconds. Return (exit code, output, True when Godot was stopped)."""
    state, done, lines = dict(last=time.monotonic(), late=False), threading.Event(), []
    guard = threading.Thread(target=idle_guard, args=(proc, timeout, state, done), daemon=True)
    guard.start()

    def seen(line):
        state["last"] = time.monotonic()
        return echo(line)

    try:
        lines += map(seen, proc.stdout)
        code = proc.wait()
    finally:
        done.set()
        guard.join()
        stop(proc)
    return code, "".join(lines), state["late"]


def godot_env(home):
    """The environment of Godot. With --home, HOME and the XDG folders are in that folder, so user://, the
    logs and the caches of Godot are not those of the user. None: the environment of this process."""
    if not home:
        return None
    home = os.path.abspath(home)
    folders = {name: os.path.join(home, *rel.split("/")) for name, rel in HOME_FOLDERS}
    for folder in folders.values():
        os.makedirs(folder, exist_ok=True)
    return dict(os.environ, HOME=home, **folders)


def run_godot(command, timeout, echo=no_echo, env=None):
    """Run Godot. Return (exit code, output: standard output and standard error in the order they came, True
    when Godot was stopped because it printed nothing for `timeout` seconds). Godot gets its own process group
    where the system has one, so that a stop also ends the child processes of the self-check."""
    error = proc = None
    try:
        proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
                                errors="replace", env=env, start_new_session=True)
    except OSError as exc:
        error = "cannot start %s: %s" % (command[0], exc)
    check(error, error)
    with proc:
        return watch(proc, timeout, echo)


def file_mark(path):
    """What tells that a file was written again: its time, its size and its inode. None for no file."""
    if not os.path.exists(path):
        return None
    info = os.stat(path)
    return info.st_mtime_ns, info.st_size, info.st_ino


def read_result(out, before):
    """Return (the result data, its bytes), or (None, None) for a result file that Godot did not write again."""
    if file_mark(out) in (None, before):
        return None, None
    raw = pathlib.Path(out).read_bytes()
    return read_json(out, "the result file"), raw


def late_message(a, out, before, kept):
    """The message for a Godot that was stopped. A result file that Godot wrote before the stop stays."""
    partial = kept and file_mark(out) not in (None, before)
    note = "; the result file that it wrote before the stop stays: %s" % out if partial else ""
    return ("Godot printed no line for %s seconds and was stopped (--timeout sets this time)%s"
            % (one_line(a.timeout), note))


def result_path(a, keep, folder):
    """The path of the result file: --out, or `keep`, or a file of the temporary folder."""
    out = os.path.abspath(a.out) if a.out else keep or os.path.join(folder, "result.json")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    return out


def godot_result(a, command, out, echo, kept):
    """Run Godot and read its result file. Return (exit code, result data or None, output, bytes or None)."""
    before = file_mark(out)
    code, output, late = run_godot(command + ["--out", out], a.timeout, echo, godot_env(a.home))
    check(late, late_message(a, out, before, kept))
    data, raw = read_result(out, before)
    return code, data, output, raw


def start_godot(a, root, name, bench_args, echo=no_echo, keep=None):
    """Start Godot with the command line of section 11. Return (exit code, result data or None, output, the
    bytes of the result file or None). The result file is --out, or `keep`, or a temporary file. A result file
    that Godot did not write again is no result."""
    head = [godot_program(a.godot), "--headless", "--path", root, "-s", CLI_SCRIPT, "--"] + bench_args
    check(not os.path.isdir(os.path.join(root, ADDON)), "%s/ is absent: run `pw_bench.py install` first" % ADDON)
    check(not a.timeout > 0, "--timeout must be above 0")
    folder = tempfile.mkdtemp(prefix="pw_bench_")
    say("pw_bench %s: %s" % (name, root))
    try:
        return godot_result(a, head, result_path(a, keep, folder), echo, bool(a.out or keep))
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def table(rows):
    """Lines of a table: each column is as wide as its longest cell."""
    rows = [[str(cell) for cell in row] for row in rows]
    widths = [max(len(row[i]) for row in rows) for i in range(len(rows[0]))] if rows else []
    return ["  ".join(cell.ljust(width) for cell, width in zip(row, widths)).rstrip() for row in rows]


def meter_row(m):
    value = m.get("text") or ("%s %s" % (scalar(m.get("value", "")), m.get("unit", ""))).strip()
    hint = ", ".join(map(str, m.get("hint") or []))
    return [m.get("id", ""), m.get("name", ""), value, m.get("target", ""), m.get("status", ""), hint]


def player_line(pid, p):
    seconds = p.get("length_s")
    hours = "%.2f h" % (seconds / 3600.0) if isinstance(seconds, (int, float)) else "?"
    ended = "ended" if p.get("ended") else "did not end"
    counts = (scalar(p.get("purchases", "?")), scalar(p.get("resets", "?")))
    return "player %s: %s, %s, %s purchases, %s resets" % ((pid, ended, hours) + counts)


def base_k(root):
    """The value k of each control that changes nothing and is not 1: 0 for a control with the op add."""
    there = os.path.exists(os.path.join(root, BENCH_JSON))
    controls = read_bench(root).get("controls") or [] if there else []
    return {c.get("id"): 0 for c in controls if isinstance(c, dict) and c.get("op") == "add"}


def run_lines(data, base=None):
    """The result file of --run as lines: the table of the meters, 1 line for each player, the controls."""
    meters = [m for m in data.get("meters") or [] if isinstance(m, dict)]
    players = [player_line(pid, p) for pid, p in (data.get("players") or {}).items() if isinstance(p, dict)]
    base = base or {}
    k = ["%s=%s" % (cid, scalar(value)) for cid, value in (data.get("k") or {}).items() if value != base.get(cid, 1)]
    moved = "controls not at the base value: %s" % (", ".join(k) or "none")
    more = ["%s: %s" % (m.get("id", ""), line) for m in meters for line in m.get("lines") or []]
    return table([METER_HEAD] + [meter_row(m) for m in meters]) + more + players + [moved]


def flat(value):
    return not isinstance(value, (list, dict))


def row_lines(row, line):
    inner = [item for value in row.values() if isinstance(value, list) for item in value]
    return ["  " + line] + ["    " + scalar(item) for item in inner]


# Godot writes the keys of a result file in the order of the alphabet. These columns come first in a table.
FIRST_COLUMNS = ("id", "name", "label", "ok", "value", "tried")


def row_table(rows):
    """The table of a list of dictionaries: a header line, then 1 line for each row. A column is a key whose
    values are not lists or dictionaries."""
    found = dict.fromkeys(key for row in rows for key in row)
    keys = [key for key in FIRST_COLUMNS if key in found] + [key for key in found if key not in FIRST_COLUMNS]
    columns = [c for c in keys if all(flat(row.get(c)) for row in rows)]
    return table([columns] + [[scalar(row.get(c, "")) for c in columns] for row in rows])


def list_lines(key, value):
    """A list of the self-check result: a table when each item is a dictionary, and 1 line for each item if not."""
    head = ["%s: %d" % (key, len(value))]
    rows = [item for item in value if isinstance(item, dict)]
    if len(rows) != len(value) or not rows:
        return head + ["  " + scalar(item) for item in value]
    lines = row_table(rows)
    return head + ["  " + lines[0]] + [out for row, line in zip(rows, lines[1:]) for out in row_lines(row, line)]


def selfcheck_lines(data):
    """The result file of --selfcheck as lines. The contract does not fix this file, so each list becomes a
    table or a list of lines, and each other key becomes 1 line."""
    parts = [list_lines(key, value) if isinstance(value, list) else ["%s: %s" % (key, scalar(value))]
             for key, value in data.items()]
    return [line for part in parts for line in part]


def finish(name, code, data, output, lines_of):
    """Print the result, and map the exit code of Godot: 0 ok, 1 a gate failed, each other code is 2."""
    fine = isinstance(data, dict) and code in (0, 1)
    say_lines(lines_of(data) if isinstance(data, dict) else [])
    if not fine:
        note = "" if isinstance(data, dict) else ", and it wrote no result file"
        sys.stderr.write("%s\npw_bench %s: Godot exit code %d%s\n" % (output[-4000:], name, code, note))
        return 2
    say("pw_bench %s: %s" % (name, "pass" if code == 0 else "fail"))
    return code


def k_fine(text):
    name, sep, value = text.partition("=")
    return bool(name and sep) and text_number(value) is not None


def k_values(pairs):
    """Check each --k value: <control>=<number>. Return the arguments for the bench."""
    bad = [text for text in pairs if not k_fine(text)]
    check(bad, "--k needs <control>=<number>, not %s" % ", ".join(bad))
    return [part for text in pairs for part in ("--k", text)]


def selfcheck_args(a):
    """The arguments of the self-check for the bench: --only and --jobs go to Godot as they are. With no --jobs
    the tool passes none, and the bench selects the count of its processes."""
    check(a.jobs is not None and a.jobs < 1, "--jobs must be 1 or above")
    only = ["--only", a.only] if a.only else []
    return ["--selfcheck"] + only + (["--jobs", str(a.jobs)] if a.jobs else [])


def run_selfcheck(a):
    """Start the self-check. Its result stays in bench/selfcheck.json (or --out): the bench reads this file for
    meter M8. Each line of Godot is printed when it comes."""
    root = project_root(a.project)
    keep = os.path.join(root, SELFCHECK_JSON)
    code, data, output, _ = start_godot(a, root, "selfcheck", selfcheck_args(a), Progress(), keep)
    return finish("selfcheck", code, data, output, selfcheck_lines)


def file_option(flag, path, what):
    """An option of the bench that names a file that must exist, as arguments with the absolute path."""
    check(path and not os.path.isfile(path), "the %s %s is absent" % (what, path))
    return [flag, os.path.abspath(path)] if path else []


def failed_meters(data):
    meters = [m for m in data.get("meters") or [] if isinstance(m, dict)]
    return sum(1 for m in meters if m.get("status") == "fail")


def meters_text(data):
    """Why `run` has the exit code 1: the meters, not the run."""
    count = failed_meters(data)
    why = "%d meters fail against the targets" % count if count else "the bench reports a failure"
    return why + " (the run itself is complete"


def run_end(code, data, compare):
    """The line after "pw_bench run: fail". Exit code 1 of a run is about the balance: the run itself is
    complete. With --compare the meters do not set the exit code."""
    if code != 1:
        return []
    tail = "; with --compare the exit code is about the bytes)" if compare else ")"
    return [("" if compare else "exit 1: ") + meters_text(data) + tail]


COMPARE_MAX = 10                                      # the lines of a comparison that `run --compare` prints


def differences(old, new):
    """The keys of 2 objects that differ, as texts: "<key> <old> -> <new>" for a plain value, and the key alone
    for a list or an object."""
    keys = [key for key in dict.fromkeys(list(new) + list(old)) if old.get(key) != new.get(key)]
    return ["%s %s -> %s" % (key, one_line(old.get(key)), one_line(new.get(key)))
            if flat(old.get(key)) and flat(new.get(key)) else str(key) for key in keys]


def by_id(items):
    """A list of objects with an id, or an object, as an object by id."""
    if isinstance(items, dict):
        return {key: value for key, value in items.items() if isinstance(value, dict)}
    rows = [item for item in items or [] if isinstance(item, dict)] if isinstance(items, list) else []
    return {str(row.get("id")): row for row in rows}


def group_lines(what, old, new):
    """1 line for each meter or player that differs between the 2 results."""
    old, new = by_id(old), by_id(new)
    names = [name for name in dict.fromkeys(list(new) + list(old)) if old.get(name) != new.get(name)]
    return ["%s %s: %s" % (what, name, "; ".join(differences(old.get(name) or {}, new.get(name) or {})))
            for name in names]


def compare_lines(golden, data):
    """What differs between the result of the file of --compare and the new result: the meters, the players,
    then each other key of the result."""
    if not isinstance(golden, dict):
        return ["the file of --compare is not a JSON object"]
    lines = group_lines("meter", golden.get("meters"), data.get("meters"))
    lines += group_lines("player", golden.get("players"), data.get("players"))
    others = [key for key in dict.fromkeys(list(data) + list(golden))
              if key not in ("meters", "players") and golden.get(key) != data.get(key)]
    lines += ["key %s differs" % key for key in others]
    return lines or ["the content is equal as JSON; the bytes differ (the order of the keys, the spaces or the "
                     "line ends)"]


def golden_bytes(a):
    """The bytes of the file of --compare, or None with no --compare. --out must name another file."""
    if not a.compare:
        return None
    check(not os.path.isfile(a.compare), "the result file %s of --compare is absent" % a.compare)
    same = bool(a.out) and os.path.abspath(a.out) == os.path.abspath(a.compare)
    check(same, "--out and --compare name the same file: the run would write over the record")
    return pathlib.Path(a.compare).read_bytes()


def compared_run(a, golden, data, raw):
    """Print the comparison of `run --compare`. Return the exit code: 0 for the same bytes, 1 if not."""
    if raw == golden:
        say("compare: same bytes as %s" % a.compare)
        return 0
    lines = compare_lines(loads(golden.decode("utf-8", "replace")), data)
    more = ["  ... and %d more" % (len(lines) - COMPARE_MAX)] if len(lines) > COMPARE_MAX else []
    say_lines(["compare: the result differs from %s" % a.compare] + ["  " + line for line in lines[:COMPARE_MAX]]
              + more + ["exit 1: the bytes of the result are not those of %s" % a.compare])
    return 1


def run_run(a):
    root = project_root(a.project)
    files = file_option("--setting", a.setting, "setting file") + file_option("--check", a.check, "self-check result")
    base = base_k(root)
    golden = golden_bytes(a)
    code, data, output, raw = start_godot(a, root, "run", ["--run"] + files + k_values(a.k))
    end = finish("run", code, data, output, lambda result: run_lines(result, base))
    if end == 2:
        return 2
    say_lines(run_end(end, data, golden is not None))
    return end if golden is None else compared_run(a, golden, data, raw)


# ----------------------------------------------------------------------------- the command line
HOME_HELP = ("run Godot with HOME, XDG_DATA_HOME, XDG_CONFIG_HOME and XDG_CACHE_HOME in this folder, so that "
             "user://, the logs and the caches of Godot are not those of the user (for a sandbox, a test or a "
             "run on a build server). Without it Godot writes its logs, and the panel its settings, into the "
             "home folder of the user")


def godot_args(timeout, idle):
    """The options of the commands that start Godot. --timeout is an idle time: it runs again on each line."""
    return ((("--godot",), dict(metavar="PATH", help="the Godot program (default: godot on the path)")),
            (("--out",), dict(metavar="FILE", help="keep the result file of the bench at this path")),
            (("--home",), dict(metavar="DIR", help=HOME_HELP)),
            (("--timeout",), dict(type=float, default=timeout, metavar="S",
                                  help="stop Godot when it prints no line for this many seconds (default %d). %s"
                                       % (timeout, idle))))


RUN_TIMEOUT = "A run prints its lines at the end, so this is the longest time of 1 run"
CHECK_TIMEOUT = ("The self-check prints a progress line each few numbers, so a long check that still prints is "
                 "not stopped. On a stop, a result file that the bench wrote stays")
ALL_ARG = (("--all",), dict(action="store_true", help="print each line of each list (default: the first %d)"
                            % LIST_MAX))
COMMANDS = {
    "install": (run_install, "copy the addon, add the autoload, write the adapter stub and the bench file",
                ((("--addon",), dict(metavar="DIR", help="the addon source (default: assets/godot/tuning_bench of "
                                                         "the skill)")),)),
    "inventory": (run_inventory, "write each number of the project into the ledger of bench/bench.json", (ALL_ARG,)),
    "init": (run_init, "write the phases, the controls, the players, the targets and sim where they are absent",
             ((("--phases-only",), dict(action="store_true", help="write the phases, the players, the targets and "
                                        "sim, and no control: the step before the first run and the inventory")),
              (("--refresh-controls",), dict(action="store_true", help="write each generated control again from "
                                             "the present ledger; keep each control that a person edited or added")),
              (("--targets",), dict(metavar="FILE", help="the targets file of the solver: take its length, its "
                                                         "purchase gap and its unlock targets")),
              (("--length-h",), dict(type=float, metavar="HOURS", help="the length target in hours")))),
    "confirm": (run_confirm, "remove the proposal mark of the numbers that the selectors name, after you read them",
                ((("--file",), dict(metavar="GLOB", help="select the numbers of the files that match (data/*.json)")),
                 (("--class",), dict(dest="cls", metavar="CLASS", help="select the numbers with this class")),
                 (("--rule",), dict(metavar="RULE", help="select the proposals of this rule; a glob (key cost*)")),
                 (("--id",), dict(action="append", default=[], metavar="ID", help="select this number; repeat it")),
                 (("--set-class",), dict(metavar="CLASS", help="write this class into each selected number")),
                 (("--role",), dict(metavar="ROLE", help="write this role into each selected number")),
                 (("--element",), dict(metavar="ELEMENT", help="write this element into each selected number")),
                 (("--phase",), dict(metavar="PHASE", help="write this phase into each selected number")),
                 (("--sense",), dict(type=int, choices=SENSES, metavar="SENSE",
                                     help="write this sense into each selected number: -1 for a number that a "
                                          "control divides by k (a divisor), 1 for a plain factor")),
                 (("--from",), dict(dest="rules", metavar="RULES_JSON",
                                    help="a JSON list of rules {\"match\": {file, pointer, key, class, role}, "
                                         "\"set\": {class, role, element, phase, label, sense, tunable, reason, "
                                         "dead_ok}}: for each number the first rule that matches writes its keys. "
                                         "In the pointer glob * is 1 part and ** is any count of parts. A text of "
                                         "\"set\" can use {owner}, {key}, {element}, {1}, {2}")),
                 (("--text-id",), dict(action="append", default=[], metavar="ID",
                                       help="accept this text of the list `texts`: its id, or a glob on the id "
                                            "(data/model.json#/review/*); repeat it; needs --ok")),
                 (("--text-file",), dict(metavar="GLOB", help="accept each text with no `ok` of the files that "
                                                              "match; needs --ok")),
                 (("--ok",), dict(metavar="REASON", help="why the selected texts need no change (the number is "
                                                         "structure, the text is for the developer)")),
                 ALL_ARG)),
    "check": (run_check, "the static gate: classes, roles, reasons, proposals, controls, phases",
              ((("--strict",), dict(action="store_true", help="fail on each proposal that is not confirmed, in "
                                                              "any file, and on each text with a number that "
                                                              "has no `ok`")), ALL_ARG)),
    "corecheck": (run_corecheck, "the static lint of the game core: no scene, no clock, no file, no hidden state",
                  ((("--core",), dict(action="append", default=[], metavar="PATH",
                                      help="a folder or a file of the core (default: the folder of the key "
                                           "\"core\" of the bench file, or core/); repeat the option")),)),
    "selfcheck": (run_selfcheck, "start Godot and run the self-check of the bench; the result stays in "
                  + SELFCHECK_JSON,
                  ((("--only",), dict(metavar="GLOB", help="check only the numbers whose id matches")),
                   (("--jobs",), dict(type=int, metavar="N", help="the count of Godot processes of the check "
                                                                  "(default: the bench selects it)")))
                  + godot_args(900.0, CHECK_TIMEOUT)),
    "run": (run_run, "start Godot, run the simulation and print the meters",
            ((("--setting",), dict(metavar="FILE", help="a saved setting of bench/settings/")),
             (("--k",), dict(action="append", default=[], metavar="CONTROL=K",
                             help="the value k of 1 control; repeat the option")),
             (("--check",), dict(metavar="FILE", help="a self-check result for meter M8 (default: the bench "
                                                      "reads %s by itself)" % SELFCHECK_JSON)),
             (("--compare",), dict(metavar="RESULT_JSON",
                                   help="compare the bytes of the new result file with this result file (the "
                                        "record of a refactor). Exit code 0 only for the same bytes; the meters "
                                        "do not set the exit code")))
            + godot_args(3600.0, RUN_TIMEOUT)),
}


def add_command(sub, name, text, options):
    cmd = sub.add_parser(name, help=text, description=text)
    cmd.add_argument("project", help="the folder of the Godot project")
    for flags, kwargs in options:
        cmd.add_argument(*flags, **kwargs)


def parse(argv):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0],
                                     epilog="Exit code 0: ok. 1: a gate failed. 2: bad input.")
    sub = parser.add_subparsers(dest="command", required=True)
    for name, (_, text, options) in COMMANDS.items():
        add_command(sub, name, text, options)
    return parser.parse_args(argv)


def main(argv=None):
    a = parse(argv)
    run = COMMANDS[a.command][0]
    try:
        return run(a)
    except InputError as exc:
        sys.stderr.write("pw_bench: %s\n" % exc)
    return 2


if __name__ == "__main__":
    sys.exit(main())
