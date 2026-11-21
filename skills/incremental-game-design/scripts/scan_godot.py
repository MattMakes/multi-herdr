#!/usr/bin/env python3
"""Inventory the progression data and the feedback hooks of a Godot project.

    python3 scan_godot.py <project_dir> [--out scan.json] [--max 40] [--systems-out systems.json]
                          [--content-out content.json]

The script does not understand the game. It shows where to read: the autoloads, the data
files with economy keys, the GDScript lines with cost and growth math, the project's own
balance tooling, the save and unlock code, the signals that mark a beat with their handlers
and the feedback in each handler, and the feel checks of the genre. Each hit is a lead to
check, not a fact. Read the files that it ranks highest, then write the model by hand
(references/model-schema.md).

stdout is a short markdown report with a cap on each list. --out holds the full data.

The last report section, "Catalog map", maps the code to the 29 systems of references/systems-catalog.md:
found (yes, no or unsure), the evidence, the best variant of assets/systems.json, the extracted parameters,
the purchase lines with r, the reset exponents, the offline rule, the missing core systems, and each parameter
outside its proven range. --systems-out writes that map as JSON (references/reverse-engineering.md).
--content-out writes the content file: each purchase line, reward and system of the game with its unlock point.
The design solver reads it (pw_design.py --content, references/design-solver.md).
"""
import argparse
import bisect
import collections
import functools
import itertools
import json
import math
import os
import re
import sys

SKIP_DIRS = {"addons", "node_modules", "build", "builds", "export", "exports", "dist", "bin", "obj", "tmp", "temp",
             "out", "output", "logs", "log", "cache", "ai_docs", "__pycache__", "coverage", "screenshots",
             "test_output", "test-results"}
SKIP_PREFIX = ("external", "third_party", "thirdparty", "vendor")
SKIP_FILES = {"export_presets.cfg", "override.cfg", "default_bus_layout.tres"}
TESTS = re.compile(r"(^|/)(tests?|tools|fixtures|testing|spec|bench\w*|qa)/|(^|/)test_[^/]*$|_test\.\w+$", re.I)

ECON_TOK = re.compile(
    r"(?:^|_)(?:costs?|prices?|growth|mult|multi|multipliers?|rate|income|per_sec|prestige|rebirth|ascend|"
    r"ascension|upgrades?|unlock|unlocks|unlocked|rewards?|payout|yield|exp|xp|levels?|tiers?|ranks?|damage|"
    r"dmg|dps|health|hp|armor|scaling|curve|currency|currencies|gold|coins?|gems?)\d*(?:_|$)"
    r"|(?:^|_)cap\d*$|^cap_|^base_")
STRONG_TOK = re.compile(r"(?:^|_)(?:costs?|prices?|growth|unlock|unlocks|unlocked|currency|currencies|prestige|"
                        r"upgrades?)\d*(?:_|$)")
COST_TOK = re.compile(r"(?:^|_)(?:costs?|prices?)\d*(?:_|$)")
GROWTH_TOK = re.compile(r"(?:^|_)(?:growth|scaling)\d*(?:_|$)|(?:^|_)(?:cost|price)_(?:mult|multiplier|x|exp|"
                        r"factor|ratio|step|inc|increase|scale|curve)(?:_|$)")
IDENT = re.compile(r"[A-Za-z_]\w*")
MATH = re.compile(r"\bpow\s*\(|\*\*|\bexp\s*\(|\blog\s*\(|\bsqrt\s*\(|\blerp\s*\(|\bease\s*\(|Mathf\.Pow\s*\(")
POW = re.compile(r"\bpow\s*\(|\*\*|Mathf\.Pow\s*\(", re.I)
DECL = re.compile(r"^[ \t]*(?:@export[^\n]*?\s+)?(?:const|var|static var)\s+(\w+)[^\n]*", re.M)
SAVE = re.compile(r"user://|ConfigFile|ResourceSaver|store_var|store_string|store_line|JSON\.stringify|FileAccess\.WRITE")
SAVE_NAME = re.compile(r"save|store|persist|profile|slot", re.I)
UNLOCK = re.compile(r"\b(unlock\w*|prestige\w*|rebirth\w*|ascend\w*|milestone\w*|achievement\w*|tutorial\w*)\b", re.I)
UNLOCK_CORE = re.compile(r"^(unlock|prestige|rebirth|ascend|milestone)")
TELEMETRY = re.compile(r"telemetry|analytics|track_event|log_event", re.I)
OFFLINE = re.compile(r"\boffline_?\w*|\bidle_gain\w*|\bcatch_?up\w*|\bwelcome_?back\w*|\bwhile_(?:you_were_)?away\w*", re.I)
OFFLINE_GUARD = ("offline", "idle_gain", "catch_up", "catchup", "welcome", "while_")
ASSIGN = re.compile(r"(\w+)\s*(?::\s*\w+\s*)?:?=")

FEEDBACK = {
    "tween": re.compile(r"create_?tween|\btween_(?:property|method|callback|interval)|\btween\b"),
    "particles": re.compile(r"gpuparticles|cpuparticles|\.emitting\s*=|emit_particle|particles?\w*\.restart\("),
    "sound": re.compile(r"audiostreamplayer|(?:sfx|sound|audio|snd|jingle|chime)\w*\.play\w*\(|"
                        r"\bplay_(?:sfx|sound|audio|cue|jingle|sting)\w*\(|"
                        r"\b(?:audio|sfx|sounds?|soundmanager|soundsystem|audiomanager)\.\w+\(|pitch_scale"),
    "shake": re.compile(r"(?:camera|screen|trauma)\w*shake|shake\w*(?:camera|screen)|add_?trauma|\bshake\("),
    "popup/toast": re.compile(r"float\w*text|damage_?number|popup|toast|banner"),
    "animation": re.compile(r"animationplayer|animatedsprite|animationtree|\banim\w*\.play\(|\.play\(\s*&?\""),
    "hit stop": re.compile(r"time_?scale|hit_?stop|freeze_?frame|hit_?pause"),
    "haptics": re.compile(r"vibrat|haptic|rumble"),
}
FILE_HINT = {"popup/toast": re.compile(r"popup|toast|banner"), "sound": re.compile(r"audio|sfx|sound"),
             "haptics": re.compile(r"haptic|rumble|vibrat")}
CATS = list(FEEDBACK)
TYPED_VAR = re.compile(r"\bvar\s+(\w+)\s*:\s*(AudioStreamPlayer|GPUParticles|CPUParticles|AnimationPlayer|AnimatedSprite)")
TYPE_CAT = {"AudioStreamPlayer": "sound", "GPUParticles": "particles", "CPUParticles": "particles",
            "AnimationPlayer": "animation", "AnimatedSprite": "animation"}
SCENE_NODES = re.compile(r"^(?:GPUParticles|CPUParticles|AnimationPlayer|AnimatedSprite|AnimationTree|"
                         r"AudioStreamPlayer|ProgressBar|TextureProgressBar|Popup|AcceptDialog|ConfirmationDialog)")

BEAT_KINDS = [
    ("unlock", re.compile(r"unlock|discover|reveal")),
    ("purchase", re.compile(r"purchas|bought|(?:^|_)buy|upgrad")),
    ("reset", re.compile(r"prestig|rebirth|reborn|ascen|run_(?:ended|finished|over|complete|won|lost)|game_over")),
    ("level up", re.compile(r"level(?:ed|led)?_?up|leveled|rank_?up|tier_(?:up|changed|reached)|evolv|promot")),
    ("milestone", re.compile(r"milestone|achievement|new_(?:record|best|high)|quest_(?:done|complete)")),
    ("ladder step", re.compile(r"cleared|complet|defeat|victory|(?:stage|wave|depth|floor|zone)_(?:reached|won|up|done)")),
    ("reward", re.compile(r"reward|payout|earned|loot_(?:granted|dropped)")),
]
NOT_BEAT = re.compile(r"request|pressed|clicked|refused|denied|failed|hovered|selected|focus")
KIND_ORDER = {k: i for i, k in enumerate(["purchase", "unlock", "ladder step", "level up", "milestone", "reward", "reset"])}
TOOLING = re.compile(r"(?:^|[_\-./])(?:sim|sims|simulator|simulation|simulate|balance|balancing|diag|diagnostics?|"
                     r"tune|tuning|tuner|calibrat\w*|polic(?:y|ies)|sweeps?|autoplay\w*|playstyle\w*|headless|"
                     r"full_run|fun_targets|(?:\w+_)?targets|parity|fixtures?)(?:[_\-./]|$)", re.I)
TOOL_EXT = {".gd", ".cs", ".py", ".sh", ".json", ".cfg", ".csv", ".toml", ".yaml", ".yml"}

FUNC = re.compile(r"^([ \t]*)(?:static\s+)?func\s+(\w+)\s*\(", re.M)
SIGNAL_GD = re.compile(r"^[ \t]*signal\s+(\w+)", re.M)
SIGNAL_CS = re.compile(r"\[Signal\][^;{]*?delegate\s+void\s+(\w+?)EventHandler\s*\(")
EMIT = re.compile(r"\b(\w+)\.emit\(|emit_signal\(\s*&?\"(\w+)\"|EmitSignal\(\s*(?:SignalName\.|nameof\(|\")(\w+)")
CONNECT_STR = re.compile(r"\b[Cc]onnect\(\s*(?:&?\"(\w+)\"|(?:\w+\.)?SignalName\.(\w+))\s*,\s*([^\n]*)")
CONNECT_DOT = re.compile(r"\b(\w+)\.connect\(\s*([^\n]*)")
CONNECT_CS = re.compile(r"\b(\w+)\s*\+=\s*(\w+)\s*;")
SCENE_CONN = re.compile(r'^\[connection signal="(\w+)" from="([^"]*)" to="([^"]*)" method="(\w+)"', re.M)

VISUAL = re.compile(r"modulate|\.text|icon|texture|tooltip|visible|disabled|color|alpha|shader", re.I)
FEEL = {
    "number_format": ("Large-number formatting",
                      re.compile(r"(?i:func\s+_?(?:format|fmt|abbrev\w*|short(?:en)?|compact|humanize|pretty|big)_?"
                                 r"(?:num|number|big|short|value|amount|currency|money|count|int|float)\w*\s*\(|"
                                 r"func\s+_?(?:num|number|amount|money)_?(?:to_?str\w*|text|label|short|format|"
                                 r"abbrev\w*|suffix)\w*\s*\(|func\s+\w*(?:to_?suffix|notation|si_?prefix)\w*\s*\(|"
                                 r"num_scientific|suffix(?:es)?\s*:?=\s*\[)|"
                                 r"[\"']K[\"']\s*,\s*[\"']M[\"']|%\.?\d*e\b|%[\d.]*[df][kKMB]\b"), None),
    "affordability": ("Affordability cue", re.compile(r"func\s+\w*afford\w*\s*\(", re.I), None),
    "locked_tease": ("Locked or teased content",
                     re.compile(r"\b(?:is_)?locked\b|silhouette|\bteas(?:e|er|ed)\b|\"\?\?\?\"|coming_soon|padlock|"
                                r"lock_icon", re.I), VISUAL),
    "away_screen": ("\"Earned while away\" screen",
                    re.compile(r"welcome_?back|offline_?(?:earn|gain|report|popup|summary|panel|screen|reward|progress)|"
                               r"while_?(?:you_?were_?)?away|earned_?while|away_?(?:summary|report|screen|popup|earn|"
                               r"gain|reward)", re.I), None),
    "count_up": ("Count-up on rewards",
                 re.compile(r"tween_method\(\s*(?:func|[\w.]*(?:count|number|value|text|label|amount|score|total|"
                            r"tick|roll)\w*)|tween_property\([^\n]*\"(?:text|value|visible_ratio)\"|"
                            r"count_?up|roll_?up|tick_?up|counting_?(?:up|label)", re.I), None),
    "stat_export": ("Stat or save export",
                    re.compile(r"JSON\.stringify|JsonSerializer\.Serialize|clipboard_set|"
                               r"func\s+\w*(?:export|dump)_?\w*(?:save|stat|state|run|json|report)\w*\s*\(", re.I), None),
}
AFFORD_SET = re.compile(r"\bdisabled\s*=|modulate|theme_type_variation|add_theme_\w+_override|set_disabled\(", re.I)
AFFORD_CMP = re.compile(r"afford|(?:cost|price)\w*(?:\([^)\n]*\))?\s*[<>]=?|[<>]=?\s*[\w.]*(?:cost|price)", re.I)


CS_FUNC = re.compile(r"\b(?:void|int|bool|float|double|string|Task|\w+)\s+(\w+)\s*\([^;{)]*\)\s*\{")
CALL = re.compile(r"(?:\b(\w+)\.)?\b(\w+)\s*\(")
CALLABLE = re.compile(r"(?:new\s+)?Callable(?:\.From)?\(\s*(?:(\w+)\s*,\s*)?(?:&?\"(\w+)\"|nameof\((\w+)\)|(\w+))")
AUTOLOAD_LINE = re.compile(r'(\w+)="\*?(res://[^"]+)"')
SCENE_NODE = re.compile(r'^\[node name="[^"]*" type="(\w+)"', re.M)
SCENE_PROP = re.compile(r"^(\w+) = (-?\d[\d.e+-]*)$", re.M)
SCENE_SCRIPT = re.compile(r'path="(res://[^"]+\.(?:gd|cs))"')
NODE_HEAD = re.compile(r'\n\[node name="([^"]*)"')
DATA_NAME = re.compile(r"^\s*([A-Za-z_]\w*)\s*=", re.M)
NOT_BALANCE = re.compile(r"(^|/)(perf\w*|profil\w*|bench\w*|art|audio|shaders?)/", re.I)
LOOSE_JSON = re.compile(r"target|polic|tun|balanc|calibr|diag|sim", re.I)
TEST_FILE = re.compile(r"(^|/)tests?/|(^|/)test_")
SMALL_BEATS = ("unlock", "reset", "milestone", "level up")
TOP_JUICE = ("tween", "particles", "sound", "popup/toast")


def norm(name):
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).lower()


def is_econ(name):
    return ECON_TOK.search(norm(name)) is not None


def in_tests(f):
    return TESTS.search(f) is not None


def skip_dir(name):
    low = name.lower()
    return name.startswith(".") or low in SKIP_DIRS or low.startswith(SKIP_PREFIX)


HIDDEN_SKIP = (".git", ".godot", ".import", ".venv", ".cache")


def _keep_project_dirs(dirs, hidden=False):
    dirs[:] = [x for x in dirs if (hidden and x not in HIDDEN_SKIP or not x.startswith("."))
               and x not in ("addons", "node_modules")]


def find_projects(root, hidden=False):
    """All project.godot files under root, skipped folders left out. Hidden folders count only with hidden."""
    hits = []
    for d, dirs, files in os.walk(root):
        _keep_project_dirs(dirs, hidden)
        hits += [d] if "project.godot" in files else []
    return hits


NOT_GAME_FILES = (".gdignore", "SKILL.md")


def not_game(path):
    """True for a folder that is not a part of the game: Godot does not import a folder with a .gdignore file, and a
    folder with a SKILL.md file is an agent skill (with example models of other games), and a folder of --exclude."""
    return any(os.path.exists(os.path.join(path, f)) for f in NOT_GAME_FILES) or \
        os.path.normpath(path) in walk.exclude


def _sort_dir(d, x, keep, nested):
    """Put folder x of d in keep (a name to enter) or in nested (the path of another project). The scan does not
    enter a folder that is not a part of the game (not_game)."""
    path = os.path.join(d, x)
    if skip_dir(x) or not_game(path):
        return
    is_project = os.path.exists(os.path.join(path, "project.godot"))
    target, item = (nested, path) if is_project else (keep, x)
    target.append(item)


def _split_dirs(d, dirs, nested):
    keep = []
    for x in sorted(dirs):
        _sort_dir(d, x, keep, nested)
    dirs[:] = keep


def walk(root, exclude=()):
    """Files of one project. A folder with its own project.godot is another project: it is not entered. exclude
    holds folders (relative to root) to leave out."""
    nested = []
    walk.exclude = {os.path.normpath(os.path.join(root, x)) for x in exclude}
    for d, dirs, files in os.walk(root):
        _split_dirs(d, dirs, nested)
        yield from (os.path.join(d, f) for f in files)
    walk.nested = nested


walk.exclude = set()


def _read_file(path, limit):
    fh = open(path, "r", encoding="utf-8", errors="replace")
    with fh:
        return fh.read(limit)


def read(path, limit=2_000_000):
    text = ""
    try:
        text = _read_file(path, limit)
    except OSError:
        pass
    return text


def _group(m):
    return m.group(1) if m else None


def _autoload(line):
    m = AUTOLOAD_LINE.match(line.strip())
    return {"name": m.group(1), "path": m.group(2)} if m else None


def _autoloads(text):
    block = re.search(r"\[autoload\](.*?)(?:\n\[|\Z)", text, re.S)
    lines = block.group(1).strip().splitlines() if block else []
    found = [_autoload(line) for line in lines]
    return [x for x in found if x]


def project_info(root):
    text = read(os.path.join(root, "project.godot"))
    name = re.search(r'config/name="([^"]*)"', text)
    feat = re.search(r"config/features=PackedStringArray\(([^)]*)\)", text)
    main = re.search(r'run/main_scene="([^"]*)"', text)
    return {"name": _group(name), "features": _group(feat), "main_scene": _group(main), "autoloads": _autoloads(text)}


def _new_keys(it, keys):
    return [k for k in it if k not in keys] if isinstance(it, dict) else []


def item_keys(v):
    """Keys of the items of a container: first items of a list, first values of a dict of dicts."""
    items = v[:5] if isinstance(v, list) else list(v.values())[:5] if isinstance(v, dict) else []
    keys = []
    for it in items:
        keys += _new_keys(it, keys)
    return keys[:14]


def _describe_list(name, v):
    return {"key": name, "type": "array", "size": len(v), "item_keys": item_keys(v)}


def _describe_dict(name, v):
    ik = item_keys(v)
    return {"key": name, "type": "object", "size": len(v), "item_keys": ik,
            "keys": [] if ik else [str(k) for k in list(v)[:14]]}


def _describe_scalar(name, v):
    return {"key": name, "type": type(v).__name__, "size": 1, "item_keys": []}


def describe(name, v):
    if isinstance(v, list):
        return _describe_list(name, v)
    fn = _describe_dict if isinstance(v, dict) else _describe_scalar
    return fn(name, v)


def json_shape(obj):
    """Each top-level key with its type, size, and item keys."""
    if isinstance(obj, dict):
        return {"type": "object", "size": len(obj), "top_keys": [describe(str(k), v) for k, v in list(obj.items())[:40]]}
    if isinstance(obj, list):
        return {"type": "array", "size": len(obj), "top_keys": [describe("(root)", obj)]}
    return {"type": type(obj).__name__, "size": 1, "top_keys": []}


def _inner_tail(inner):
    if not inner:
        return ""
    return " of {" + ", ".join(inner[:8]) + ("..." if len(inner) > 8 else "") + "}"


def _container_line(t):
    tail = _inner_tail(t["item_keys"] or t.get("keys") or [])
    size = f"[{t['size']}]" if t["type"] in ("array", "object") else ""
    return f"{t['key']}: {t['type']}" + size + tail


def _scalar_line(scalars):
    return f"{len(scalars)} scalar keys: " + ", ".join(scalars[:10]) + ("..." if len(scalars) > 10 else "")


def shape_text(s, cap=6):
    tops = s.get("top_keys", [])
    scalars = [t["key"] for t in tops if t["type"] not in ("array", "object")]
    tops = [t for t in tops if t["type"] in ("array", "object")]
    out = [_container_line(t) for t in tops[:cap]]
    out += [f"(+{len(tops) - cap} more containers)"] if len(tops) > cap else []
    out += [_scalar_line(scalars)] if scalars else []
    return out


def _count_key(out, k, v, depth):
    out[str(k)] += 1
    keys_of(v, out, depth + 1)


def _keys_of_dict(obj, out, depth):
    pairs = obj.items()
    for k, v in pairs:
        _count_key(out, k, v, depth)


def _keys_of_list(obj, out, depth):
    items = obj[:200]
    for v in items:
        keys_of(v, out, depth + 1)


def _key_walker(obj):
    if isinstance(obj, dict):
        return _keys_of_dict
    return _keys_of_list if isinstance(obj, list) else None


def keys_of(obj, out, depth=0):
    walker = _key_walker(obj) if depth <= 6 else None
    if walker:
        walker(obj, out, depth)


def _is_clock_line(line):
    return "get_unix_time" in line and not line.lstrip().startswith("#")


def _clock_hit(lines, i, line):
    """True when the wall clock on line i is subtracted: on the line, or through a variable in the next four lines."""
    hit = re.search(r"\s-\s", line) is not None
    m = ASSIGN.search(line)
    if hit or not m:
        return hit
    v = re.escape(m.group(1))
    return re.search(r"\b%s\s+-\s|\s-\s+%s\b" % (v, v), "\n".join(lines[i + 1:i + 5])) is not None


def clock_hits(text):
    """Lines where the wall clock is subtracted: on the line, or through a variable in the next four lines."""
    lines = text.split("\n")
    rows = [(i, line) for i, line in enumerate(lines) if _is_clock_line(line)]
    return [(i + 1, line.strip()[:160]) for i, line in rows if _clock_hit(lines, i, line)]


def _match_starts(rx, text):
    """The start of each line with a match, once for each line."""
    starts = (text.rfind("\n", 0, m.start()) + 1 for m in rx.finditer(text))
    return (s for s, _ in itertools.groupby(starts))


def _line_text(text, start):
    end = text.find("\n", start)
    return text[start:end if end >= 0 else len(text)].strip()


def _keep_line(line, also):
    return not line.startswith(("#", "//")) and (also is None or also.search(line))


def line_hits(rx, text, cap=400, also=None):
    """(line number, line text) for each non-comment line with a match. One hit for each line."""
    rows = ((s, _line_text(text, s)) for s in _match_starts(rx, text))
    kept = ((s, line) for s, line in rows if _keep_line(line, also))
    hits = ((text.count("\n", 0, s) + 1, line[:160]) for s, line in kept)
    return list(itertools.islice(hits, max(cap, 1)))


def data_record(relp, size, keys, shape):
    """keys: Counter of key name -> count. None when the file does not look like economy data."""
    econ = {k: v for k, v in keys.items() if is_econ(k)}
    normed = [norm(k) for k in econ]
    strong = any(STRONG_TOK.search(k) for k in normed)
    if not econ or (len(econ) < 2 and not strong):
        return None
    has_cost = any(COST_TOK.search(k) for k in normed)
    has_growth = any(GROWTH_TOK.search(k) for k in normed)
    return {"file": relp, "bytes": size, "economy_key_hits": sum(econ.values()),
            "economy_keys": sorted(econ, key=econ.get, reverse=True)[:14],
            "has_cost_key": has_cost, "has_growth_key": has_growth, "shape": shape}


def _line_starts(lines):
    return [0] + list(itertools.accumulate(len(ln) + 1 for ln in lines))


def _signature_end(lines, i):
    """The index after the last line of a signature that starts on line i. The signature can cover several lines."""
    depths = itertools.accumulate(ln.count("(") - ln.count(")") for ln in lines[i:])
    return next((i + k + 1 for k, d in enumerate(depths) if d <= 0), len(lines))


def _ends_body(ln, indent):
    s = ln.lstrip()
    return bool(s) and not s.startswith("#") and len(ln[:len(ln) - len(s)].expandtabs(4)) <= indent


def _body_end(lines, j, indent):
    return next((k for k in range(j, len(lines)) if _ends_body(lines[k], indent)), len(lines))


def _add_gd_func(out, lines, starts, m):
    i = bisect.bisect_right(starts, m.start()) - 1
    indent = len(m.group(1).expandtabs(4))
    j = _body_end(lines, _signature_end(lines, i), indent)
    out.setdefault(m.group(2), (i + 1, "\n".join(lines[i:j])))


def _brace_end(text, k):
    """The index after the brace that closes the brace at index k."""
    depths = itertools.accumulate((text[x] == "{") - (text[x] == "}") for x in range(k, len(text)))
    return next((k + n + 1 for n, d in enumerate(depths) if d == 0), len(text))


class Funcs:
    """Function bodies of the scripts, parsed on demand."""

    def __init__(self, texts):
        self.texts, self.cache = texts, {}

    def of(self, relp):
        cache = self.cache
        if relp not in cache:
            cache[relp] = self._parse(relp)
        return cache[relp]

    def _parse(self, relp):
        text = self.texts.get(relp, "")
        return self._cs(text) if relp.endswith(".cs") else self._gd(text)

    @staticmethod
    def _gd(text):
        lines, out = text.split("\n"), {}
        starts = _line_starts(lines)
        for m in FUNC.finditer(text):
            _add_gd_func(out, lines, starts, m)
        return out

    @staticmethod
    def _cs(text):
        out = {}
        for m in CS_FUNC.finditer(text):
            k = _brace_end(text, m.end() - 1)
            out.setdefault(m.group(1), (text.count("\n", 0, m.start()) + 1, text[m.start():k]))
        return out


def lambda_body(text, pos):
    """The text of an inline `func(...)` handler: the rest of the line plus the deeper lines after it."""
    start = text.rfind("\n", 0, pos) + 1
    lines = text[start:start + 6000].split("\n")
    first = lines[0]
    indent = len(first[:len(first) - len(first.lstrip())].expandtabs(4))
    body = [first]
    for ln in lines[1:60]:
        s = ln.lstrip()
        if s and len(ln[:len(ln) - len(s)].expandtabs(4)) <= indent:
            break
        body.append(ln)
    return "\n".join(body)


def _code_lines(body):
    return "\n".join(l for l in body.split("\n") if not l.lstrip().startswith(("#", "//")))


def _direct_feedback(code, relp, ctx):
    low = code.lower()
    found = {cat: "direct" for cat, rx in FEEDBACK.items() if rx.search(low)}
    typed = ctx["typed"].get(relp, {}).items()
    for var, cat in typed:
        if cat not in found and re.search(r"\b" + re.escape(var) + r"\.", code):
            found[cat] = "direct"
    return found


def _call_target(obj, fn, relp, own, ctx, found):
    """The (file, function) of 1 call as a list of 0 or 1 items. A call on an audio autoload counts as sound."""
    if obj in (None, "self", "this") and fn in own:
        return [(relp, fn)]
    if obj not in ctx["autoload_file"]:
        return []
    target = ctx["autoload_file"][obj]
    if "sound" not in found and re.search(r"audio|sfx|sound|music", obj, re.I):
        found["sound"] = f"via {obj}.{fn}"
    return [(target, fn)] if fn in ctx["funcs"].of(target) else []


def _calls(code, relp, ctx, found):
    own = ctx["funcs"].of(relp)
    calls = []
    for m in CALL.finditer(code):
        calls += _call_target(m.group(1), m.group(2), relp, own, ctx, found)
    return calls


def _merge_sub(found, target, fn, ctx, depth, seen):
    if (target, fn) in seen:
        return
    seen.add((target, fn))
    sub = feedback_in(ctx["funcs"].of(target)[fn][1], target, fn, ctx, depth - 1, seen)
    for cat in sub:
        found.setdefault(cat, f"via {fn}")


def _called_feedback(found, code, relp, ctx, depth, seen):
    calls = _calls(code, relp, ctx, found)
    for target, fn in calls:
        _merge_sub(found, target, fn, ctx, depth, seen)


def feedback_in(body, relp, name, ctx, depth=2, seen=None):
    """Feedback categories in a function body. Value: 'direct', or 'via <function>' for a call one or two steps down."""
    seen = seen if seen is not None else {(relp, name)}
    code = _code_lines(body)
    found = _direct_feedback(code, relp, ctx)
    if depth > 0:
        _called_feedback(found, code, relp, ctx, depth, seen)
    return found


def _callable_handler(m):
    obj = m.group(1)
    return (None if obj in (None, "self", "this") else obj), m.group(2) or m.group(3) or m.group(4)


def _handler_chain(rest):
    m = re.match(r"[\w.]+", rest)
    chain = [c for c in m.group(0).split(".") if c] if m else []
    bound = bool(chain) and chain[-1] in ("bind", "unbind", "bindv")
    return chain[:-1] if bound else chain


def _chain_handler(rest):
    chain = _handler_chain(rest)
    if not chain:
        return None, None
    obj = chain[-2] if len(chain) > 1 else None
    return (None if obj in ("self", "this") else obj), chain[-1]


def parse_handler(rest):
    """The handler named in the argument text of a connect call: ('lambda', None) or (object or None, method)."""
    rest = rest.strip()
    if rest.startswith("func"):
        return "lambda", None
    m = CALLABLE.match(rest)
    return _callable_handler(m) if m else _chain_handler(rest)


def beat_kind(name):
    n = norm(name)
    return None if NOT_BEAT.search(n) else next((kind for kind, rx in BEAT_KINDS if rx.search(n)), None)


def _add_owner(owners, f, funcs):
    names = list(funcs)
    for fn in names:
        owners[fn].append(f)


def _function_owners(ctx):
    """Function name -> files that define it (production code)."""
    owners = collections.defaultdict(list)
    files = [f for f in ctx["texts"] if not in_tests(f)]
    for f in files:
        _add_owner(owners, f, ctx["funcs"].of(f))
    return owners


def _add_file_hints(h):
    if not h["resolved"]:
        return
    stem = os.path.splitext(os.path.basename(h["file"]))[0].lower()
    hints = [cat for cat, rx in FILE_HINT.items() if rx.search(stem)]
    for cat in hints:
        h["feedback"].setdefault(cat, "via file name")


def _add_unique(uniq, seen, h):
    key = (h["file"], h["function"], h["line"] if h["function"] == "(lambda)" else 0)
    is_new = key not in seen
    seen.add(key)
    if is_new:
        uniq.append(h)


def _unique_handlers(handlers):
    for h in handlers:
        _add_file_hints(h)
    handlers.sort(key=lambda h: (not h["feedback"], not h["resolved"]))
    uniq, seen = [], set()
    for h in handlers:
        _add_unique(uniq, seen, h)
    return uniq


def _hook_record(sig, declared_in, em, uniq, test_handlers):
    found = [c for c in CATS if any(c in h["feedback"] for h in uniq)]
    return {
        "signal": sig, "kind": beat_kind(sig),
        "declared_in": declared_in,
        "emitted_in": em[:20], "emit_count": len(em),
        "handlers": uniq, "test_handlers": test_handlers,
        "feedback_found": found,
        "feedback_indirect": [c for c in found if not any(h["feedback"].get(c) == "direct" for h in uniq)],
        "feedback_missing": [c for c in CATS if c not in found],
    }


def _hook_order(h):
    return not h["handlers"] and not h["emit_count"], not h["handlers"], KIND_ORDER[h["kind"]], h["signal"]


def _split_tests(rows):
    live = [c for c in rows if not in_tests(c["file"])]
    return live, len(rows) - len(live)


class HookScan:
    """Finds the handlers of the beat signals and the feedback in each handler."""

    def __init__(self, ctx, emits, connects, scene_conns):
        self.ctx, self.emits, self.connects, self.scene_conns = ctx, emits, connects, scene_conns
        self.owners = None

    def hook(self, sig, declared_in):
        key = norm(sig)
        handlers, test_handlers = self._handlers(key)
        uniq = _unique_handlers(handlers)
        em = [e for e in self.emits.get(key, []) if not in_tests(e["file"])]
        return _hook_record(sig, declared_in, em, uniq, test_handlers)

    def _handlers(self, key):
        live, tests = _split_tests(self.connects.get(key, []))
        scenes, scene_tests = _split_tests(self.scene_conns.get(key, []))
        handlers = [self._connect_handler(c) for c in live] + [self._scene_handler(c) for c in scenes]
        return handlers, tests + scene_tests

    def _connect_handler(self, c):
        h = {"file": c["file"], "function": None, "line": c["line"], "via": "connect", "resolved": False}
        fill = self._fill_lambda if c["obj"] == "lambda" else self._fill_method
        h["feedback"] = fill(h, c)
        return h

    def _fill_lambda(self, h, c):
        h.update(function="(lambda)", resolved=True)
        body = lambda_body(self.ctx["texts"][c["file"]], c["pos"])
        return feedback_in(body, c["file"], f"lambda@{c['line']}", self.ctx)

    def _fill_method(self, h, c):
        obj, method = c["obj"], c["method"]
        target = self._target(obj, method, c["file"])
        h["function"] = method if obj is None else f"{obj}.{method}"
        if not (target and method in self.ctx["funcs"].of(target)):
            return {}
        line, body = self.ctx["funcs"].of(target)[method]
        h.update(file=target, function=method, line=line, resolved=True, connected_in=c["file"])
        return feedback_in(body, target, method, self.ctx)

    def _target(self, obj, method, file):
        """The file that defines the handler: the named file, else the only production file with that function."""
        target = file if obj is None else self.ctx["autoload_file"].get(obj)
        if not method or (target is not None and method in self.ctx["funcs"].of(target)):
            return target
        owners = self._owners().get(method, [])
        return owners[0] if len(owners) == 1 else None

    def _owners(self):
        if self.owners is None:
            self.owners = _function_owners(self.ctx)
        return self.owners

    def _scene_handler(self, c):
        h = {"file": c["file"], "function": c["method"], "line": c["line"], "via": "scene", "resolved": False,
             "feedback": {}}
        funcs = self.ctx["funcs"]
        script = next((s for s in c["scripts"] if c["method"] in funcs.of(s)), None)
        if script is None:
            return h
        line, body = funcs.of(script)[c["method"]]
        h.update(file=script, line=line, resolved=True, connected_in=c["file"],
                 feedback=feedback_in(body, script, c["method"], self.ctx))
        return h


def build_beat_hooks(ctx, declared, emits, connects, scene_conns):
    scan = HookScan(ctx, emits, connects, scene_conns)
    hooks = [scan.hook(sig, declared[sig]) for sig in sorted(declared) if beat_kind(sig)]
    hooks.sort(key=_hook_order)
    return hooks


def verdict(formula_lines, cost_growth_files, unlock_core):
    score = (2 if formula_lines >= 1 else 0) + (1 if formula_lines >= 3 else 0) + \
            (2 if cost_growth_files >= 1 else 0) + (1 if unlock_core >= 20 else 0)
    level = "strong" if score >= 4 else "medium" if score >= 2 else "weak"
    note = {"strong": "an upgrade economy is in the code and the data. Model it.",
            "medium": "some economy evidence. Read the top files before you decide what to model.",
            "weak": "this project may not be an upgrade economy. Do not force a progression model onto it: say what "
                    "the game has in place of one, and review that."}[level]
    return {"level": level, "score": score, "cost_formula_lines": formula_lines,
            "data_files_with_cost_and_growth": cost_growth_files, "unlock_words": unlock_core, "note": note}


class Src:
    """1 source file of the project: its paths, its text, and whether it is test code."""

    def __init__(self, path, relp, ext, text):
        self.path, self.relp, self.ext, self.text = path, relp, ext, text
        self.base = os.path.basename(path)
        self.low = text.lower()
        self.test = in_tests(relp)

    def line_of(self, pos):
        return self.text.count("\n", 0, pos) + 1


def _econ_decls(text):
    return [{"line": text.count("\n", 0, m.start(1)) + 1, "text": m.group(0).strip()[:160]}
            for m in DECL.finditer(text) if is_econ(m.group(1))]


def _script_record(src, decl, math_lines):
    return {"file": src.relp, "lines": src.text.count("\n") + 1, "economy_declarations": len(decl),
            "math_lines": len(math_lines), "sample_declarations": decl[:8], "sample_math": math_lines[:8]}


def _is_formula(src, line, names):
    return not src.test and POW.search(line) and any(COST_TOK.search(x) for x in names)


def _afford_line(lines, i, line):
    return AFFORD_SET.search(line) and not line.lstrip().startswith("#") and \
        AFFORD_CMP.search("\n".join(lines[max(0, i - 5):i + 1]))


def _afford_hits(src):
    low = src.low
    if not (("afford" in low or "cost" in low or "price" in low) and AFFORD_SET.search(src.text)):
        return []
    lines = src.text.split("\n")
    return [{"file": src.relp, "line": i + 1, "text": line.strip()[:160]}
            for i, line in enumerate(lines) if _afford_line(lines, i, line)]


def _feel_hits(src, spec):
    _, rx, also = spec
    return [{"file": src.relp, "line": n, "text": t} for n, t in line_hits(rx, src.text, 40, also)]


def _away_hits(src):
    text = src.text
    if not ("away" in text.lower() or "ffline" in text or "elcome" in text):
        return []
    return [{"file": src.relp, "line": n, "text": t} for n, t in line_hits(FEEL["away_screen"][1], text, 10)]


def _scene_scripts(src):
    sc = [p[len("res://"):] for p in SCENE_SCRIPT.findall(src.text)]
    same = os.path.splitext(src.relp)[0] + ".gd"
    return ([same] if same not in sc else []) + sc


def _is_resource(text):
    head = text[:400]
    return 'type="Resource"' in head or "script_class=" in head


def _csv_names(text):
    return [c.strip().strip('"') for c in text.split("\n", 1)[0].split(",")]


def _tests_last(score, s):
    return in_tests(s["file"]), -score(s)


def _by_tests(score):
    return functools.partial(_tests_last, score)


def _script_score(s):
    return s["math_lines"] * 3 + s["economy_declarations"]


def _data_score(s):
    return s["economy_key_hits"] * (4 if s["has_cost_key"] and s["has_growth_key"] else 1)


def _save_score(s):
    return s["hits"] + (100 if s["save_name"] else 0)


def _hits(s):
    return s["hits"]


def _override_order(s):
    return in_tests(s["file"]), s["file"], s["line"]


class Scan:
    """The data that the scan of 1 project collects, file by file."""

    def __init__(self, root, info):
        self.root, self.info = root, info
        self.counts, self.texts, self.typed, self.tooling_files = collections.Counter(), {}, {}, []
        self.scripts, self.data_files, self.save_code, self.offline, self.telemetry = [], [], [], [], []
        self.unlock_words, self.unlock_core, self.formula = collections.Counter(), 0, []
        self.juice = {k: collections.Counter() for k in CATS}
        self.declared, self.emits = {}, collections.defaultdict(list)
        self.connects, self.scene_conns = collections.defaultdict(list), collections.defaultdict(list)
        self.scene_nodes, self.scene_overrides = collections.defaultdict(collections.Counter), []
        self.feel = {k: [] for k in FEEL}
        self.json_data = {}

    def rel(self, p):
        return os.path.relpath(p, self.root).replace(os.sep, "/")

    def scan_file(self, path):
        base = os.path.basename(path)
        ext = os.path.splitext(base)[1].lower()
        relp = self.rel(path)
        self._count(ext, relp)
        if base in SKIP_FILES:
            return
        scanner = FILE_SCANNERS.get(ext)
        if scanner:
            scanner(self, path, relp, ext)

    def _count(self, ext, relp):
        self.counts[ext] += 1
        if ext in TOOL_EXT and TOOLING.search(relp):
            self.tooling_files.append(relp)

    def scan_script(self, path, relp, ext):
        src = Src(path, relp, ext, read(path))
        self.texts[relp] = src.text
        for step in SCRIPT_STEPS:
            step(self, src)

    def scan_scene(self, path, relp, ext):
        src = Src(path, relp, ext, read(path))
        for step in SCENE_STEPS:
            step(self, src)

    def scan_json(self, path, relp, ext):
        if os.path.getsize(path) >= 5_000_000:
            return
        text = read(path, 5_000_000)
        try:
            obj = json.loads(text)
        except (ValueError, RecursionError):
            return
        self.json_data[relp] = (obj, text)
        keys = collections.Counter()
        keys_of(obj, keys)
        self._add_data(data_record(relp, os.path.getsize(path), keys, json_shape(obj)))

    def scan_text_data(self, path, relp, ext):
        text = read(path, 500_000)
        if ext == ".tres" and not _is_resource(text):
            return
        names = _csv_names(text) if ext == ".csv" else DATA_NAME.findall(text)
        shape = {"type": ext[1:], "top_keys": []}
        self._add_data(data_record(relp, os.path.getsize(path), collections.Counter(names), shape))

    def _add_data(self, rec):
        self.data_files += [rec] if rec else []

    def _math_hit(self, src, n, line):
        names = [norm(x) for x in IDENT.findall(line)]
        if not any(ECON_TOK.search(x) for x in names):
            return None
        if _is_formula(src, line, names):
            self.formula.append({"file": src.relp, "line": n, "text": line})
        return {"line": n, "text": line}

    def scan_economy(self, src):
        decl = _econ_decls(src.text)
        hits = [self._math_hit(src, n, line) for n, line in line_hits(MATH, src.text)]
        math_lines = [h for h in hits if h]
        if decl or math_lines:
            self.scripts.append(_script_record(src, decl, math_lines))

    def scan_save(self, src):
        h = len(SAVE.findall(src.text))
        if h:
            self.save_code.append({"file": src.relp, "hits": h, "save_name": SAVE_NAME.search(src.base) is not None})

    def scan_unlock(self, src):
        words = [w.lower() for w in UNLOCK.findall(src.text)]
        self.unlock_words.update(words)
        core = [] if src.test else [w for w in words if UNLOCK_CORE.match(w)]
        self.unlock_core += len(core)

    def scan_offline(self, src):
        oh = line_hits(OFFLINE, src.text) if any(g in src.low for g in OFFLINE_GUARD) else []
        clock = clock_hits(src.text) if "get_unix_time" in src.text else None
        oh = sorted(set(oh + clock)) if clock is not None else oh
        if oh:
            self.offline.append({"file": src.relp, "hits": len(oh), "line": oh[0][0], "text": oh[0][1],
                                 "in_tests": src.test})

    def scan_telemetry(self, src):
        h = len(TELEMETRY.findall(src.text))
        if h:
            self.telemetry.append({"file": src.relp, "hits": h})

    def scan_juice(self, src):
        pairs = ((k, len(rx.findall(src.low))) for k, rx in FEEDBACK.items())
        hits = [(k, n) for k, n in pairs if n]
        for k, n in hits:
            self.juice[k][src.relp] += n

    def scan_typed(self, src):
        tv = {m.group(1): TYPE_CAT[m.group(2)] for m in TYPED_VAR.finditer(src.text)}
        if tv:
            self.typed[src.relp] = tv

    def scan_signals(self, src):
        self._declare(src)
        if "emit" in src.text or "Emit" in src.text:
            self._emits(src)
        if "onnect(" in src.text:
            self._connects(src)
        if src.ext == ".cs" and "+=" in src.text:
            self._cs_connects(src)

    def _declare(self, src):
        rx = SIGNAL_CS if src.ext == ".cs" else SIGNAL_GD
        rows = [(m.group(1), src.line_of(m.start(1))) for m in rx.finditer(src.text)]
        for name, line in rows:
            self.declared.setdefault(name, []).append({"file": src.relp, "line": line})

    def _emits(self, src):
        names = ((m.group(1) or m.group(2) or m.group(3), m.start()) for m in EMIT.finditer(src.text))
        beats = [(name, pos) for name, pos in names if beat_kind(name)]
        for name, pos in beats:
            self.emits[norm(name)].append({"file": src.relp, "line": src.line_of(pos)})

    def _add_connect(self, src, name, pos, handler):
        obj, method = handler
        self.connects[norm(name)].append({"file": src.relp, "line": src.line_of(pos), "pos": pos, "obj": obj,
                                          "method": method})

    def _connects(self, src):
        text = src.text
        for m in CONNECT_STR.finditer(text):
            self._connect_str(src, m)
        for m in CONNECT_DOT.finditer(text):
            self._connect_dot(src, m)

    def _connect_str(self, src, m):
        name = m.group(1) or m.group(2)
        if beat_kind(name):
            self._add_connect(src, name, m.start(), parse_handler(m.group(3)))

    def _connect_dot(self, src, m):
        name, rest = m.group(1), m.group(2)
        if beat_kind(name) and not rest.lstrip().startswith(("\"", "&\"")):
            self._add_connect(src, name, m.start(), parse_handler(rest))

    def _cs_connects(self, src):
        rows = [m for m in CONNECT_CS.finditer(src.text) if beat_kind(m.group(1))]
        for m in rows:
            self._add_connect(src, m.group(1), m.start(), (None, m.group(2)))

    def scan_feel(self, src):
        if src.test:
            return
        self._feel_lines(src)
        self.feel["affordability"] += _afford_hits(src)

    def _feel_lines(self, src):
        feel = self.feel
        for key, spec in FEEL.items():
            feel[key] += _feel_hits(src, spec)

    def scan_scene_nodes(self, src):
        types = [t for t in SCENE_NODE.findall(src.text) if SCENE_NODES.match(t)]
        for t in types:
            self.scene_nodes[t][src.relp] += 1

    def scan_scene_conns(self, src):
        rows = [m for m in SCENE_CONN.finditer(src.text) if beat_kind(m.group(1))]
        sc = _scene_scripts(src) if rows else None
        for m in rows:
            self.scene_conns[norm(m.group(1))].append({"file": src.relp, "line": src.line_of(m.start()),
                                                       "method": m.group(4), "to": m.group(3), "scripts": sc})

    def scan_scene_overrides(self, src):
        rows = [m for m in SCENE_PROP.finditer(src.text) if is_econ(m.group(1))]
        for m in rows:
            self._scene_override(src, m)

    def _scene_override(self, src, m):
        text = src.text
        head = text.rfind("\n[", 0, m.start())
        node = NODE_HEAD.match(text[head:head + 200]) if head >= 0 else None
        if node:
            self.scene_overrides.append({"file": src.relp, "line": src.line_of(m.start()), "node": node.group(1),
                                         "property": m.group(1), "value": m.group(2)})

    def scan_scene_feel(self, src):
        self.feel["away_screen"] += [] if src.test else _away_hits(src)

    def beat_hooks(self):
        autoload_file = {x["name"]: x["path"][len("res://"):] for x in self.info["autoloads"]
                         if x["path"].endswith((".gd", ".cs"))}
        ctx = {"texts": self.texts, "funcs": Funcs(self.texts), "typed": self.typed, "autoload_file": autoload_file}
        return build_beat_hooks(ctx, self.declared, self.emits, self.connects, self.scene_conns)

    def sort_lists(self):
        self.scripts.sort(key=_by_tests(_script_score))
        self.data_files.sort(key=_by_tests(_data_score))
        self.save_code.sort(key=_by_tests(_save_score))
        self.offline.sort(key=_by_tests(_hits))
        self.telemetry.sort(key=_by_tests(_hits))
        self.scene_overrides.sort(key=_override_order)

    def economy_signal(self):
        cg = [d["file"] for d in self.data_files if d["has_cost_key"] and d["has_growth_key"] and not in_tests(d["file"])]
        v = verdict(len(self.formula), len(cg), self.unlock_core)
        v["cost_formula_samples"] = self.formula[:10]
        v["cost_and_growth_files"] = cg[:10]
        return v


FILE_SCANNERS = {".gd": Scan.scan_script, ".cs": Scan.scan_script, ".tscn": Scan.scan_scene, ".json": Scan.scan_json,
                 ".tres": Scan.scan_text_data, ".cfg": Scan.scan_text_data, ".csv": Scan.scan_text_data}
SCRIPT_STEPS = (Scan.scan_economy, Scan.scan_save, Scan.scan_unlock, Scan.scan_offline, Scan.scan_telemetry,
                Scan.scan_juice, Scan.scan_typed, Scan.scan_signals, Scan.scan_feel)
SCENE_STEPS = (Scan.scan_scene_nodes, Scan.scan_scene_conns, Scan.scan_scene_overrides, Scan.scan_scene_feel)


def _add_tool_folder(parts, hit, folders, inside):
    d = "/".join(parts[:hit + 1]) + "/"
    folders[d] += 1
    first = parts[hit + 1] + ("/" if len(parts) > hit + 2 else "")
    if first not in inside[d]:
        inside[d].append(first)


def _sort_tool(f, folders, loose, inside):
    """A file in a matching folder counts for the folder. Another matching file is a loose file."""
    parts = f.split("/")
    hit = next((i for i, x in enumerate(parts[:-1]) if TOOLING.search("/" + x + "/")), None)
    if hit is not None:
        _add_tool_folder(parts, hit, folders, inside)
    elif os.path.splitext(f)[1] != ".json" or LOOSE_JSON.search(f):
        loose.append(f)


def _tool_name_order(x):
    return TOOLING.search("/" + x) is None, x.startswith("test"), x


def _loose_order(f):
    return TEST_FILE.search(f) is not None, f


def balance_tooling(tooling_files):
    """Balance tooling: a matching folder is one row; other matching files are one row each."""
    folders, loose, inside = collections.Counter(), [], collections.defaultdict(list)
    files = [f for f in tooling_files if not NOT_BALANCE.search(f)]
    for f in files:
        _sort_tool(f, folders, loose, inside)
    order = lambda d: (in_tests("x/" + d) and not d.startswith("tools/"), -folders[d])
    rows = [{"path": d, "files": folders[d], "sample": sorted(inside[d], key=_tool_name_order)[:8]}
            for d in sorted(folders, key=order)]
    return rows + [{"path": f, "files": 1, "sample": []} for f in sorted(loose, key=_loose_order)]


def juice_row(c):
    prod = collections.Counter({f: n for f, n in c.items() if not in_tests(f)})
    return {"files": len(prod), "hits": sum(prod.values()), "top": prod.most_common(5),
            "hits_in_tests_and_tools": sum(c.values()) - sum(prod.values())}


def _result_head(scan, found):
    return {"project_root": scan.root, "project": scan.info,
            "project_files_found": [os.path.join(d, "project.godot") for d in found],
            "nested_projects_skipped": [scan.rel(d) for d in getattr(walk, "nested", [])],
            "file_counts": dict(scan.counts.most_common(12)),
            "economy_signal": scan.economy_signal()}


def _result_lists(scan, tooling, cap):
    return {"balance_tooling": tooling[:cap], "balance_tooling_total": len(tooling),
            "economy_scripts": scan.scripts[:cap], "economy_scripts_total": len(scan.scripts),
            "data_files": scan.data_files[:cap], "data_files_total": len(scan.data_files),
            "scene_overrides": scan.scene_overrides[:cap], "scene_overrides_total": len(scan.scene_overrides)}


def _result_meta(scan):
    return {"save_and_meta_code": scan.save_code[:15],
            "unlock_vocabulary": dict(scan.unlock_words.most_common(30)),
            "offline_progress_code": scan.offline[:10],
            "telemetry_code": scan.telemetry[:10]}


def _result_feedback(scan, hooks, cap):
    return {"feedback_categories": CATS,
            "beat_hooks": hooks,
            "juice": {k: juice_row(c) for k, c in scan.juice.items()},
            "scene_feedback_nodes": {t: {"count": sum(c.values()), "scenes": c.most_common(5)}
                                     for t, c in sorted(scan.scene_nodes.items())},
            "feel_checks": {k: {"title": FEEL[k][0], "total": len(v), "hits": v[:cap]} for k, v in scan.feel.items()}}


def build_result(scan, found, cap):
    hooks = scan.beat_hooks()
    scan.sort_lists()
    tooling = balance_tooling(scan.tooling_files)
    return {**_result_head(scan, found), **_result_lists(scan, tooling, cap), **_result_meta(scan),
            **_result_feedback(scan, hooks, cap)}


SYSTEMS_JSON = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "assets", "systems.json")
CATALOG_SCHEMA = 1
CORE_IDS = ("S01", "S02", "S03", "S04", "S05", "S06")
GATE_IDS = ("S07", "S08", "S09")
GATE_SLOT = {"slot": "presence gate", "members": list(GATE_IDS), "default": "S08"}

SIGNATURES = {
    "S01": ("Generator line", [r"\b\w*(?:cost|price)\w*\b[^\n]*\bpow\s*\("], [r"\b(?:owned|count)\s*\*"]),
    "S02": ("Multiplier upgrade", [r"\bpow\s*\([^\n]*\b(?:mult\w*|bonus\w*|factor)", r"\bbonus_pct\b"],
            [r"\bmult\w*\b"]),
    "S03": ("Spine with a wall", [r"\bpow\s*\([^\n]*\b(?:hp|health|requirement|wall|armor)\w*",
                                  r">=\s*\w*(?:hp|health|requirement)\w*\s*\("], [r"\b(?:stage|zone|wave)\b"]),
    "S04": ("Reset layer", [r"\bprestige_(?:currency|points)\b",
                            r"\bfunc\s+\w*(?:prestige|rebirth|ascend|reincarnat)\w*\s*\("],
            [r"\b(?:prestige|rebirth|ascen\w*)"]),
    "S05": ("Active action", [r"\bclick_(?:value|power|income|mult)\w*|\bper_click\b|\bon_click\w*|"
                              r"\bactive_(?:mult|bonus|lane)\w*|\btap_value\b"],
            [r"\bfunc\s+_(?:unhandled_)?(?:gui_)?input\s*\(", r"InputEvent(?:MouseButton|ScreenTouch)"]),
    "S06": ("Offline rule", [r"\boffline_\w*(?:cap|progress|gain|rate)\w*"], [r"\boffline\w*"]),
    "S07": ("Attempt on a cooldown", [r"\bcooldown_until\b|\bcarried_damage\b|\bfight_seconds\b|\battempts_left\b"],
            [r"\bcooldown\w*"]),
    "S08": ("Bank with a cap", [r"\bbank_(?:cap|max)\b|\blast_fill\w*|\bfill_time\b|\bstorage_cap\b|\bmax_storage\b"],
            [r"\bbank\s*=", r"\bstorage\b"]),
    "S09": ("Real-time clock", [], [r"\bget_ticks_msec\b|\bget_datetime\w*|\bTime\.get_date\w*"]),
    "S10": ("Random visitor", [r"\bspawn_timer\b|\bvisitor\w*|\brandom_event\w*"],
            [r"\brandf\s*\(|\brandi\s*\(|\brand_range\b|\bgolden_\w+"]),
    "S11": ("Timed buff", [r"\bbuff_(?:until|mult|time|duration|end)\b|\bactive_buffs\b"], [r"\bbuffs?\b"]),
    "S12": ("Milestone multiplier on count", [r"\bfloor\s*\(\s*[\w.()]*\s*/\s*\d+\s*\)",
                                              r"\bpow\s*\(\s*\d+(?:\.\d+)?\s*,\s*floor\s*\(|\bmilestone_mult\w*"], []),
    "S13": ("Cap and cap raiser", [r"\b(?:max_level|maximum|level_cap|cap)\w*\s*\+=",
                                   r"\bmin\w*\s*\(\s*\w*level\w*\s*,\s*\w*(?:cap|max)"], [r"\bmax_level\b|\bcap\b"]),
    "S14": ("Hold bonus", [r"\b(?:unspent|held|hold)_?\w*(?:bonus|mult)"], []),
    "S15": ("Prestige shop", [r"\bprestige_(?:shop|upgrade)\w*|\bperks?\b"], []),
    "S16": ("Second reset layer", [r"\btranscend\w*|\breincarnat\w*|\bsecond_prestige\b|\bprestige2\b|"
                                   r"\binfinity\b|\beternity\b"], []),
    "S17": ("Challenge run", [r"\bactive_challenge\b|\bin_challenge\b|\bchallenge_(?:id|active|mode)\b|\bbest_zone\b"],
            [r"\bchallenge\w*"]),
    "S18": ("Chained producers", [r"\[\s*(\w+)\s*\]\s*\+=\s*\w+\s*\[\s*\1\s*\+\s*1\s*\]"], []),
    "S19": ("Collection with set bonus", [r"\bowned_items\b|\bset_bonus\b|\bcollection_bonus\b"],
            [r"collect(?:ed|ion)\w*"]),
    "S20": ("Task list with a shop", [r"\bachievement\w*|\btasks?\b|\bquest\w*"], []),
    "S21": ("Allocation pool", [r"\ballocat\w*"], [r"\bpool\b"]),
    "S22": ("Skill track", [r"\bxp\b|\blevel_from_xp\b|\bexperience\b|\bskill_level\b"], []),
    "S23": ("Minigame with export", [r"\bmini_?game\w*"], []),
    "S24": ("Automation unlock", [r"\bauto_?(?:buy\w*|buyer|reset|prestige|click\w*|upgrade\w*)|\bautomat(?:ion|e|ed|es)\b"], []),
    "S25": ("Rotating event", [r"\bevent_(?:start|end)\b|\bseason_\w+|\bstart_date\b|\bend_date\b"], []),
    "S26": ("Premium currency with many faucets", [r"\b(?:gems?|premium\w*|diamonds?)\s*\+="],
            [r"\b(?:gems?|premium|diamonds?)\b"]),
    "S27": ("Line rebirth", [r"\brebirths?\s*\[|\brebirth_count\b|\bline_rebirth\b"], []),
    "S28": ("Hard end", [r"\bgame_(?:won|complete|completed|finished|over_win)\b|\b(?:show|play)_(?:ending|credits)\b|"
                         r"\bend_screen\b|\bvictory_screen\b"],
            [r"\bfinal_(?:stage|town|depth|zone|step)\b|\bthe_end\b|\bending\b"]),
    "S30": ("Upkeep", [r"\b(?:uptime|upkeep|satisfaction)\w*|\bmin\s*\(\s*1(?:\.0)?\s*,\s*\w*(?:supply|capacity|care)\w*\s*/",
                       r"\b\w*need\w*\s*=\s*\w*(?:count|units|owned)\w*\s*\*"],
            [r"\b(?:hunger|happiness|morale|efficiency|consumption)\b"]),
}


def _either(patterns):
    return re.compile("|".join("(?:%s)" % p for p in patterns), re.I) if patterns else None


SIG_RX = {sid: (_either(strong), _either(weak)) for sid, (_, strong, weak) in SIGNATURES.items()}

COST_KEYS = ("cost", "base_cost", "price", "base_price", "cost_base")
RATIO_KEYS = ("growth", "cost_growth", "ratio", "cost_ratio", "cost_mult", "price_growth", "cost_scaling", "scaling")
GEO_KEYS = ("mult", "multiplier", "factor", "value_mult")
ADD_KEYS = ("add", "value", "amount", "income")
BONUS_KEYS = ("bonus_pct", "bonus", "pct")
CAP_KEYS = ("cap", "max_add")
MAX_KEYS = ("max_level", "max", "level_cap")
CURRENCY_KEYS = ("currency", "cost_currency", "price_currency", "currency_id")
KIND_FAMILY = {"mult": "geometric", "add": "linear", "pct": "linear", "cap": "cap"}
LINE_WEIGHT = 25
DOC_DIRS = re.compile(r"(^|/)(docs?|design|notes|research)/", re.I)
WRAP = re.compile(r"^(?:float|int|double|real|floor|floorf|ceil|ceilf|abs)\s*\((.*)\)$")
NUMBER = re.compile(r"-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?")
QUOTED = re.compile(r"[\"'](\w+)[\"']")
GET_DEFAULT = re.compile(r"\.get\s*\(\s*[\"']\w+[\"']\s*,\s*(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)\s*\)")
CONST_NUM = re.compile(r"^[ \t]*(?:const|var|static var)\s+(\w+)\s*(?::\s*\w+\s*)?:?=\s*"
                       r"(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)\s*(?:#[^\n]*)?$", re.M)
POW_CALL = re.compile(r"\b(?:pow|powf|Mathf\.Pow)\s*\(")
GAIN_CALL = re.compile(r"\b(?:pow|powf|Mathf\.Pow|sqrt|sqrtf|Mathf\.Sqrt|log|log10|Mathf\.Log\w*)\s*\(")
RESET_FUNC = re.compile(r"^_?(?:do_)?(?:prestige|rebirth|ascen|reincarnat|regrow|transcend|soft_reset|reset_run)",
                        re.I)
GAIN_TARGET = re.compile(r"^\s*(\w+)\s*\+=\s*[\w.]*gain\w*(?:\(\s*\))?\s*$|"
                         r"\b_?(?:add|gain)\w*\(\s*[\"'](\w+)[\"']\s*,\s*[\w.]*gain", re.M)
OFFLINE_FUNC = re.compile(r"offline|away|catch_?up|welcome", re.I)
MIN_CALL = re.compile(r"(?:(?:var\s+)?(\w+)\s*(?::\s*\w+\s*)?:?=\s*)?\b(?:min|minf|mini|Mathf\.Min)\s*\(")
INCOME_CALL = re.compile(r"\w*(?:per_sec|income|production|rate)\w*\s*\(\s*\)", re.I)
SPINE_FUNC = re.compile(r"hp|health|requirement|wall|armor", re.I)
SAVE_FUNC = re.compile(r"save|load|offline|away|persist|restore|notification", re.I)
EXPORT_CALL = re.compile(r"\b([A-Z]\w*)\.get_\w*(?:mult|bonus|factor)\w*\s*\(")
CLOCK_READ = re.compile(r"get_unix_time\w*|get_ticks_\w+")
ALIAS = r"\bvar\s+(\w+)[^=\n]*=[^\n]*[\"']%s[\"']"
LINE_SYSTEMS = ("S01", "S02", "S13", "S15")
VALUE_RX = {"geometric": r"\bpow\s*\([^\n]*\b%s\b", "linear": r"[\"']%s[\"'][^\n]*\*", "cap": r"[\"']%s[\"']"}


def _numeric(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def _children(obj):
    if isinstance(obj, dict):
        return list(obj.values())
    return obj if isinstance(obj, list) else []


def _scalar_items(obj):
    """(key, value) of each number in a dict at any depth. The keys of the outer dict come first."""
    own = [(k, v) for k, v in obj.items() if _numeric(v)] if isinstance(obj, dict) else []
    return own + [item for c in _children(obj) for item in _scalar_items(c)]


def _text_line(text, pos):
    return text.count("\n", 0, max(pos, 0)) + 1


def _add_scalars(out, relp, text, obj):
    pairs = _scalar_items(obj)
    for k, v in pairs:
        out.setdefault(k, {"value": float(v), "place": "%s:%d" % (relp, _text_line(text, text.find('"%s"' % k)))})


def _data_scalars(data):
    out = {}
    for relp, (obj, text) in sorted(data.items()):
        _add_scalars(out, relp, text, obj)
    return out


def _code_consts(texts):
    rows = ((m.group(1), float(m.group(2)), "%s:%d" % (relp, _text_line(text, m.start())))
            for relp, text in sorted(texts.items()) for m in CONST_NUM.finditer(text))
    out = {}
    for name, value, place in rows:
        out.setdefault(name, {"value": value, "place": place})
    return out


def unwrap(expr):
    """The expression without the casts and the rounding calls around it."""
    s = expr.strip()
    m = WRAP.match(s)
    return unwrap(m.group(1)) if m else s


def call_args(text, start):
    """The top-level arguments of the call whose open parenthesis is just before text[start]."""
    depths = list(itertools.accumulate((ch in "([{") - (ch in ")]}") for ch in text[start:]))
    end = next((i for i, d in enumerate(depths) if d < 0), len(depths))
    cuts = [i for i in range(end) if depths[i] == 0 and text[start + i] == ","]
    return [text[start + a + 1:start + b].strip() for a, b in zip([-1] + cuts, cuts + [end])]


def _comment(line):
    return line.lstrip().startswith(("#", "//"))


def _body_hits(fn, rx):
    """(line number, line, match) of each non-comment line of a function body with a match of rx."""
    relp, name, start, body = fn
    rows = ((start + k, ln, rx.search(ln)) for k, ln in enumerate(body.split("\n")) if not _comment(ln))
    return [(n, ln, m) for n, ln, m in rows if m]


def _place(fn, n):
    return "%s:%d" % (fn[0], n)


def _evidence(relp, n, text, strength, source="code"):
    return {"file": relp, "line": n, "text": text[:160], "strength": strength, "source": source}


COPY_MIN = 200


def _skip_reason(relp, text, seen):
    if in_tests(relp):
        return "a test or tool file"
    if DOC_DIRS.search(relp):
        return "a design or document folder, not game data"
    return "a byte copy of " + seen[text] if text in seen and len(text) >= COPY_MIN else None


def game_data(json_data):
    """({file: (data, text)} of the game data files, [{file, reason}] of each file left out): no test, tool or
    design file, and no byte copy of another data file."""
    keep, skipped, seen = {}, [], {}
    rows = sorted(json_data.items(), key=_data_order)
    for relp, item in rows:
        _sort_data(keep, skipped, seen, relp, item)
    return keep, [x for x in skipped if x["reason"] != "a test or tool file"]


def _data_order(kv):
    return DOC_DIRS.search(kv[0]) is not None, kv[0]


def _sort_data(keep, skipped, seen, relp, item):
    why = _skip_reason(relp, item[1], seen)
    seen.setdefault(item[1], relp)
    target, value = (skipped, {"file": relp, "reason": why}) if why else (None, item)
    _ = target.append(value) if why else keep.update({relp: value})


CALL_EXPR = re.compile(r"(?:\w+\.)?(\w+)\(\s*\)")
RETURN_LINE = re.compile(r"^\s*return\s+([^\n#]+)", re.M)


class CodeIndex:
    """The game scripts and data files of 1 scan, with the lookups of the catalog map."""

    def __init__(self, scan):
        self.root, self.info = scan.root, scan.info
        self.texts = {k: v for k, v in scan.texts.items() if not in_tests(k)}
        self.all_texts = dict(scan.texts)
        self.data, self.skipped = game_data(scan.json_data)
        self.funcs = Funcs(self.texts)
        self.autoloads = {a["name"] for a in scan.info["autoloads"]}
        self.scalars, self.consts = _data_scalars(self.data), _code_consts(self.texts)

    def grep(self, rx, cap=3, files=None):
        """(file, line, text) of the first non-comment lines that match rx, in files or in each script."""
        names = sorted(self.texts) if files is None else files
        rows = ((relp, n, line) for relp in names for n, line in line_hits(rx, self.texts[relp], cap))
        return list(itertools.islice(rows, cap))

    def loaders(self, data_relp):
        """The scripts that name the data file. Each script when no script names it."""
        base = os.path.basename(data_relp)
        return tuple(f for f in sorted(self.texts) if base in self.texts[f]) or tuple(sorted(self.texts))

    def functions(self, name_rx):
        """(file, name, first line, body) of each function whose name matches name_rx."""
        return [(relp, name, start, body) for relp in sorted(self.texts)
                for name, (start, body) in self.funcs.of(relp).items() if name_rx.search(name)]

    def resolve(self, expr, body="", depth=0):
        """{"value", "place"} for an expression: a number, a data key, a local variable, a constant, a default."""
        s = unwrap(expr)
        steps = (self._as_number, self._as_key, self._as_local, self._as_const, self._as_default, self._as_call)
        return next((hit for hit in (f(s, body, depth) for f in steps) if hit), None)

    @staticmethod
    def _as_number(s, body, depth):
        return {"value": float(s), "place": None} if NUMBER.fullmatch(s) else None

    def _as_key(self, s, body, depth):
        keys = [k for k in QUOTED.findall(s) if k in self.scalars]
        return dict(self.scalars[keys[0]]) if keys else None

    def _as_local(self, s, body, depth):
        m = re.search(r"\bvar\s+%s\b[^=\n]*=\s*([^\n]+)" % re.escape(s), body) if IDENT.fullmatch(s) else None
        return self.resolve(m.group(1), body, depth + 1) if m and depth < 3 else None

    def _as_const(self, s, body, depth):
        return dict(self.consts[s]) if s in self.consts else None

    def _as_call(self, s, body, depth):
        """A call with no argument, `Obj.name()` or `name()`: the value of the return line of that function. expr
        holds the return expression (its unit words)."""
        m = CALL_EXPR.fullmatch(s) if depth < 3 else None
        fns = self.functions(re.compile(r"^%s$" % m.group(1))) if m else []
        ret = next((r.group(1) for fn in fns for r in [RETURN_LINE.search(fn[3])] if r), None)
        hit = self.resolve(ret, "", depth + 1) if ret else None
        return dict(hit, expr=ret) if hit else None

    @staticmethod
    def _as_default(s, body, depth):
        m = GET_DEFAULT.search(s)
        return {"value": float(m.group(1)), "place": None} if m else None


def _value_of(obj, keys):
    return next(((k, float(obj[k])) for k in keys if k in obj and _numeric(obj[k])), (None, None))


def _kind_value(eff):
    """(kind, value) of an effect that names its kind (`{"kind": "mult", "value": 1.4}`), else (None, None)."""
    kind, value = eff.get("kind"), eff.get("value")
    return (kind, value) if kind in KIND_FAMILY and _numeric(value) else (None, None)


def _effect_value(eff):
    """(key, value) of an effect: its kind when it names one, else the first value key that it holds."""
    kind = _kind_value(eff)
    return kind if kind[0] else _value_of(eff, GEO_KEYS + ADD_KEYS + BONUS_KEYS + CAP_KEYS)


def _first_key(obj, keys):
    return next((obj[k] for k in keys if isinstance(obj.get(k), str)), None)


def _effect_of(obj):
    eff = obj.get("effect")
    effs = obj.get("effects")
    first = effs[0] if isinstance(effs, list) and effs and isinstance(effs[0], dict) else None
    return eff if isinstance(eff, dict) else first or obj


def _value_family(key):
    families = ((GEO_KEYS, "geometric"), (ADD_KEYS + BONUS_KEYS, "linear"), (CAP_KEYS, "cap"))
    return next((fam for keys, fam in families if key in keys), None)


def _line_items(obj):
    """Each dict at any depth with a numeric cost key and a numeric ratio key that a player can buy."""
    own = [obj] if isinstance(obj, dict) and _is_line(obj) else []
    return own + [item for c in _children(obj) for item in _line_items(c)]


def _is_line(obj):
    has = _value_of(obj, COST_KEYS)[0] and _value_of(obj, RATIO_KEYS)[0]
    return bool(has) and obj.get("purchasable") is not False


def _line_place(relp, text, obj):
    ident = obj.get("id")
    pos = text.find(json.dumps(ident)) if isinstance(ident, str) else -1
    return relp, _text_line(text, pos if pos >= 0 else 0)


def _geo_r(rec):
    both = rec["cost_family"] == "geometric" and rec["value_family"] == "geometric"
    return round(math.log(rec["value_ratio"]) / math.log(rec["cost_ratio"]), 4) if both and rec["value_ratio"] > 0 else None


class LineScan:
    """The purchase lines of the data files, checked against the cost and value code."""

    def __init__(self, idx):
        self.idx, self.code_cache = idx, {}

    def code_place(self, key, rx_text, files):
        """The first line of files that uses key in the way that rx_text names, as file:line."""
        cache, ck = self.code_cache, (key, rx_text, files)
        if ck not in cache:
            hits = self._hits(key, rx_text, files) or self._hits(key, rx_text, None)
            cache[ck] = "%s:%d" % hits[0][:2] if hits else None
        return cache[ck]

    def _hits(self, key, rx_text, files):
        """The hits in files (each script for None): the key itself, else a local variable that reads it."""
        return self.idx.grep(re.compile(rx_text % re.escape(key)), 1, files) or self._alias_hits(key, rx_text, files)

    def _alias_hits(self, key, rx_text, files):
        """The use of a local variable that reads the key: `var growth = float(p.get("cost_growth"))`, then
        `pow(growth, level)`."""
        names = [m.group(1) for _, _, ln in self.idx.grep(re.compile(ALIAS % re.escape(key)), 3, files)
                 for m in [re.search(ALIAS % re.escape(key), ln)]]
        rows = (self.idx.grep(re.compile(rx_text % re.escape(n)), 1, files) for n in names if n != key)
        return next((r for r in rows if r), [])

    def lines(self):
        rows = [(relp, text, obj) for relp, (data, text) in sorted(self.idx.data.items()) for obj in _line_items(data)]
        return [self.record(relp, text, obj) for relp, text, obj in rows]

    def record(self, relp, text, obj):
        files = self.idx.loaders(relp)
        rec = self._cost_part(obj, files)
        rec.update(self._value_part(_effect_of(obj), files))
        rec.update(self._identity(relp, text, obj))
        rec["r"] = _geo_r(rec)
        cost_ok = rec["cost_place"] or rec["cost_family"] == "flat" and rec["levels"] == 1
        rec["status"] = "yes" if cost_ok and rec["value_place"] else "unsure"
        return rec

    @staticmethod
    def _identity(relp, text, obj):
        f, n = _line_place(relp, text, obj)
        cap = _value_of(obj, MAX_KEYS)[1]
        levels = int(cap) if cap is not None else None
        return {"id": obj.get("id"), "name": obj.get("name"), "file": f, "line": n,
                "currency": _first_key(obj, CURRENCY_KEYS), "cap": cap, "levels": levels, "effects": _effects_of(obj)}

    def _cost_part(self, obj, files):
        base_key, base = _value_of(obj, COST_KEYS)
        ratio_key, ratio = _value_of(obj, RATIO_KEYS)
        if obj.get("cost_formula") == "linear":
            return self._linear_cost(base, ratio, ratio_key, files)
        place = self.code_place(ratio_key, r"\bpow\s*\([^\n]*\b%s\b", files) if ratio > 1 else None
        family = ("geometric" if place else "unsure") if ratio > 1 else "flat"
        return {"cost_base": base, "cost_family": family, "cost_ratio": ratio, "cost_key": ratio_key,
                "cost_place": place, "cost_formula": "exp"}

    def _linear_cost(self, base, ratio, ratio_key, files):
        """A cost `base + ratio x level` (the data key cost_formula = linear)."""
        place = self.code_place("linear", VALUE_RX["cap"], files)
        return {"cost_base": base, "cost_family": "linear", "cost_ratio": ratio, "cost_key": ratio_key,
                "cost_place": place, "cost_formula": "linear"}

    def _value_part(self, eff, files):
        key, value = _effect_value(eff)
        family = _value_family(key)
        kind = _kind_value(eff)[0]
        rx = VALUE_RX["cap"] if kind else VALUE_RX.get(family)
        place = self.code_place(key, rx, files) if family else None
        place = place or (self.code_place("kind", rx, files) if kind else None)
        return {"stat": eff.get("stat") or eff.get("target"), "value_key": key, "value_family": family,
                "value_ratio": value if family == "geometric" else None,
                "value_step": value if family != "geometric" else None, "value_place": place}


def _first_hit(fns, rx):
    """(function, line number, line, match) of the first match of rx in the bodies of fns."""
    hits = ((fn, n, ln, m) for fn in fns for n, ln, m in _body_hits(fn, rx))
    return next(hits, None)


def _hit_value(hit, fallback_place):
    if not hit:
        return None, None
    return hit["value"], hit["place"] or fallback_place


def _gain_order(fn):
    return "gain" not in fn[1].lower()


class ResetScan:
    """The reset gain formula and the reset currency."""

    def __init__(self, idx, fns=None):
        self.idx = idx
        self.fns = idx.functions(RESET_FUNC) if fns is None else fns

    def gain(self):
        hit = _first_hit(sorted(self.fns, key=_gain_order), GAIN_CALL)
        return self._formula(*hit) if hit else {"found": bool(self.fns), "gain_family": None, "p": None}

    def _formula(self, fn, n, line, m):
        args = call_args(line, m.end())
        family = "log" if "log" in m.group(0).lower() else "power"
        out = {"found": True, "function": "%s %s" % (_place(fn, fn[2]), fn[1]), "gain_family": family,
               "gain_formula": line.strip()[:160], "gain_place": _place(fn, n)}
        out.update(self._exponent(fn, n, m.group(0).lower(), args))
        out.update(self._divisor(fn, n, args))
        return out

    def _exponent(self, fn, n, name, args):
        hit = {"value": 0.5, "place": None} if "sqrt" in name else None
        hit = self.idx.resolve(args[1], fn[3]) if "pow" in name and len(args) > 1 else hit
        p, place = _hit_value(hit, _place(fn, n))
        return {"p": p, "p_place": place}

    def _divisor(self, fn, n, args):
        base = args[0] if args else ""
        hit = self.idx.resolve(base.rsplit("/", 1)[1], fn[3]) if "/" in base else None
        c, place = _hit_value(hit, _place(fn, n))
        return {"divisor": c, "divisor_place": place}

    def currency(self):
        hit = _first_hit(self.fns, GAIN_TARGET)
        name = (hit[3].group(1) or hit[3].group(2)) if hit else None
        return {"currency": name, "currency_place": _place(hit[0], hit[1]) if hit else None}


PROFILE_RULES = {"eager": ("peak_rate", 0.95), "patient": ("peak_rate", 0.75), "doubler": ("ratio", 1.0)}
PEAK_RULE = re.compile(r"\w+\s*>=?\s*(\d*\.?\d+)\s*\*\s*\w*best\w*")
RATIO_RULE = re.compile(r"gain\w*(?:\(\s*\))?\)?\s*(?:<|>=)\s*([A-Za-z_][\w.]*|\d*\.?\d+)\s*\*\s*(?:float\(\s*)?"
                        r"[\w.]*lifetime\w*")
POLICY_DEFAULT = re.compile(r"""get\(\s*["']policy["']\s*,\s*["'](peak_rate|ratio)["']\s*\)""")
TOLERANCE_DEFAULT = re.compile(r"""["']peak_tolerance["']\s*:\s*(\d*\.?\d+)""")
GUARD_FUNC = re.compile(r"^_?(?:can_|try_)|prestige|reset|rebirth|regrow|move|ascen", re.I)
RESET_GUARD = re.compile(r"\b(?:prestiges|prestige_count|resets|reset_count|rebirths)\s*(<=?)\s*(.+?)\s*(?:\\|\band\b|$)")
OFFSET = re.compile(r"^(.*?)\s*([-+])\s*(\d+)$")
RULE_KEYS = {"peak_rate": "peak_tolerance", "ratio": "policy_ratio"}
ASK_RULE = ("the scan uses the reset rule %s %s (%s): confirm that the player of the game resets by it, else set "
            "values.policy of the reset with change in the confirm file")
ASK_MAX = ("the scan read the most resets %d from the default of %s: confirm the count, else set values.max_resets of the "
           "reset with change in the confirm file")
ASK_END = ("the scan puts the end of the game at %s (%s): confirm it, else set end in the confirm file "
           "({\"axis_step\": n}, {\"resets\": k}, {\"purchase\": id} or {\"milestone\": id})")


def _rule(policy, value, place, sure, source):
    return {"policy": policy, "value": value, "place": place, "sure": sure, "source": source}


def model_rule(model):
    """The reset rule of a model file: prestige.policy, else the profile of game.primary_profile. None with neither."""
    pr = model.obj.get("prestige") if model else None
    pr = pr if isinstance(pr, dict) else {}
    if pr.get("policy") in RULE_KEYS:
        key = RULE_KEYS[pr["policy"]]
        value = pr.get(key, 0.9 if key == "peak_tolerance" else 1.0)
        return _rule(pr["policy"], value, model.place('"policy"'), True, "data")
    profile = (model.obj.get("game") or {}).get("primary_profile") if model else None
    if profile in PROFILE_RULES:
        return _rule(*PROFILE_RULES[profile], model.place('"primary_profile"'), True, "data")
    return None


def code_rules(idx):
    """Each reset rule of a player in the scripts, also in the tests and the tools: a peak rule (rate > X x best) or
    a ratio rule (gain >= X x lifetime). A rule in a script that extends the script of another rule comes first (the
    player overrides the rule). A rule whose factor is named patient or slow (a slower player) comes last. Then the
    default policy of a loader of the model format."""
    names = sorted(idx.all_texts)
    hits = [(relp, n, line) for rx in (PEAK_RULE, RATIO_RULE) for relp in names
            for n, line in line_hits(rx, idx.all_texts[relp], 3)]
    rows = [r for r in (_code_rule(idx, *h) for h in hits) if r]
    rank = [(not _extends_other(idx, r, rows), r.pop("slow")) for r in rows]
    rows = [r for _, r in sorted(zip(rank, rows), key=lambda x: x[0])]
    hit = idx.grep(POLICY_DEFAULT, 1)
    return rows + ([_default_rule(idx, *hit[0])] if hit else [])


def _extends_other(idx, r, rows):
    """True when the script of rule r extends the script of another rule (by class_name or by path)."""
    text = idx.all_texts[r["place"].rsplit(":", 1)[0]]
    m = re.search(r"^extends\s+(\S+)", text, re.M)
    base = m.group(1).strip("\"'") if m else ""
    files = {x["place"].rsplit(":", 1)[0] for x in rows} - {r["place"].rsplit(":", 1)[0]}
    return any(base.endswith(f) or re.search(r"^class_name\s+%s\b" % re.escape(base), idx.all_texts[f], re.M)
               for f in files if base)


def _code_rule(idx, relp, n, line):
    peak = PEAK_RULE.search(line)
    m = peak or RATIO_RULE.search(line)
    hit = idx.resolve(m.group(1), idx.all_texts.get(relp, ""))
    if not hit:
        return None
    out = _rule("peak_rate" if peak else "ratio", hit["value"], "%s:%d" % (relp, n), False, "code")
    return dict(out, slow=bool(re.search(r"patient|slow", m.group(1), re.I)))


def _default_rule(idx, relp, n, line):
    policy = POLICY_DEFAULT.search(line).group(1)
    tol = TOLERANCE_DEFAULT.search(idx.texts[relp]) if policy == "peak_rate" else None
    return _rule(policy, float(tol.group(1)) if tol else (0.9 if policy == "peak_rate" else 1.0), "%s:%d" % (relp, n),
                 False, "loader default")


def reset_rule(idx, model):
    """The reset rule of the game: the model file wins, then the first rule of a player in the code, then the default
    of a loader. With none, the ratio rule G >= 1 x L and a question."""
    rows = ([model_rule(model)] if model_rule(model) else []) + code_rules(idx)
    pick = dict(rows[0]) if rows else _rule("ratio", 1.0, None, False, "no rule found")
    pick["others"] = ["%s %s (%s)" % (r["policy"], r["value"], r["place"]) for r in rows[1:]]
    pick["confirm"] = None if pick["sure"] else ASK_RULE % (pick["policy"], pick["value"], "from " + pick["place"] if pick["place"] else "no rule found in the code")
    return pick


def max_resets(idx):
    """The most resets: a guard `prestiges < N` (or `<= N`) in a function that tests or applies the reset. None when
    the code has no such guard."""
    rows = (_guard_count(idx, fn, hit) for fn in idx.functions(GUARD_FUNC) for hit in _body_hits(fn, RESET_GUARD))
    return next((r for r in rows if r), None)


def _guard_parts(expr):
    """(expression, offset) of `N - 1` or `N + 1`; offset 0 with no such tail."""
    o = OFFSET.match(unwrap(expr))
    return (o.group(1), int(o.group(3)) * (-1 if o.group(2) == "-" else 1)) if o else (expr, 0)


def _guard_count(idx, fn, hit):
    n, _line, m = hit
    expr, off = _guard_parts(m.group(2))
    got = idx.resolve(expr, fn[3])
    if not got:
        return None
    count = int(round(got["value"])) + off + (m.group(1) == "<=")
    sure = got["place"] is not None or NUMBER.fullmatch(unwrap(expr)) is not None
    out = {"value": count, "place": _place(fn, n), "sure": sure}
    out["confirm"] = None if sure else ASK_MAX % (count, _place(fn, n))
    return out


def hard_end(count, most):
    """The end of the game: the last reset when the game has a most count of resets, else the last stage. count is
    (the count of stages, its place)."""
    if not most and not count[0]:
        return None
    mark, why, place = (({"resets": most["value"]}, "the last reset that the reset guard allows", most["place"])
                        if most else ({"axis_step": int(count[0])}, "the last stage of the spine", count[1]))
    text = ", ".join("%s %s" % kv for kv in mark.items())
    return {"mark": mark, "reason": why, "place": place, "sure": False, "confirm": ASK_END % (text, why)}


MODEL_KEYS = ("currencies", "prestige", "ladders", "milestones")
UNLOCK_ORDER = ("resets", "level", "stage", "total", "milestone")
LOOP_SYSTEMS = {"automation": "S24", "minigame": "S23", "collection": "S19", "meta": "S04"}


def _dict_list(v):
    return [x for x in v if isinstance(x, dict)] if isinstance(v, list) else []


def is_model(obj):
    return isinstance(obj, dict) and isinstance(obj.get("purchases"), list) and any(k in obj for k in MODEL_KEYS)


def find_model(idx):
    """The model-format data file of the game: the first one that a script names, else the first one."""
    rows = [(relp, obj, text) for relp, (obj, text) in sorted(idx.data.items()) if is_model(obj)]
    named = [r for r in rows if any(os.path.basename(r[0]) in t for t in idx.texts.values())]
    pick = (named or rows)[:1]
    return ModelData(*pick[0]) if pick else None


def _req_stage(req, axis):
    return {"stage": int(req.get("step", 0))} if req.get("id") == axis else None


def _req_total(req, axis):
    return {"total": req.get("currency"), "amount": float(req.get("amount", 0))} if _numeric(req.get("amount")) else None


def _req_level(req, axis):
    return {"level": req.get("id"), "n": int(req.get("level", 1))}


def _req_resets(req, axis):
    return {"resets": int(req.get("amount", req.get("count", 1)))}


REQ_KINDS = {"ladder": _req_stage, "level": _req_level, "prestiges": _req_resets, "resets": _req_resets,
             "total": _req_total, "run_total": _req_total, "lifetime": _req_total}


def _unlock_rank(u):
    kind = next(iter(u), "milestone")
    return UNLOCK_ORDER.index(kind) if kind in UNLOCK_ORDER else len(UNLOCK_ORDER)


class ModelData:
    """The model-format data file of 1 game."""

    def __init__(self, relp, obj, text):
        self.relp, self.obj, self.text = relp, obj, text
        self.ms = {m["id"]: m for m in _dict_list(obj.get("milestones")) if isinstance(m.get("id"), str)}
        self.ladder = self._axis_ladder()
        self.axis = (self.ladder or {}).get("id")

    def place(self, needle):
        pos = self.text.find(needle)
        return "%s:%d" % (self.relp, _text_line(self.text, pos if pos >= 0 else 0))

    def _axis_ladder(self):
        ladders = _dict_list(self.obj.get("ladders"))
        axis = (self.obj.get("game") or {}).get("progress_axis")
        return next((x for x in ladders if x.get("id") == axis), ladders[0] if ladders else None)

    def currencies(self):
        return _dict_list(self.obj.get("currencies"))

    def base_income(self):
        return {c.get("id"): c["base_income"] for c in self.currencies() if _numeric(c.get("base_income"))}

    def systems(self):
        return _dict_list(self.obj.get("systems"))

    def unlocks(self, reqs, depth=0):
        """The unlocks of the content format from a list of requirements, the most specific first: a count of
        resets, a level of a line, a stage, a lifetime total, a milestone."""
        found = [u for u in (self._one(r, depth) for r in _dict_list(reqs)) if u]
        return sorted(found, key=_unlock_rank)

    def unlock(self, reqs, depth=0):
        """The 1 unlock that the content file holds: the most specific requirement."""
        return (self.unlocks(reqs, depth) or [{}])[0]

    def _one(self, req, depth):
        if req.get("type") == "milestone":
            return self._milestone(req.get("id"), depth)
        fn = REQ_KINDS.get(req.get("type"))
        return fn(req, self.axis) if fn else {"milestone": req.get("id") or req.get("type")}

    def _milestone(self, mid, depth):
        """A milestone with an effect is an element: the unlock opens with it. A milestone with no effect is only a
        gate: the unlock is its own requirement."""
        m = self.ms.get(mid) or {}
        if _dict_list(m.get("effects")):
            return {"milestone": mid}
        return self.unlock(m.get("requires"), depth + 1) if depth < 4 else {}

    def spine(self):
        lad = self.ladder or {}
        req = lad.get("requires") if isinstance(lad.get("requires"), dict) else {}
        if not _numeric(req.get("growth")):
            return None
        place = self.place('"id": "%s"' % lad.get("id"))
        return {"g": float(req["growth"]), "base": req.get("base"), "count": lad.get("count"), "stat": req.get("currency"),
                "test": req.get("type"), "g_place": place, "place": place, "source": "data", "function": lad.get("id"),
                "formula": "step n needs %s %s >= %s x %s^(n-1)" % (req.get("type"), req.get("currency"),
                                                                       req.get("base"), req["growth"])}

    def reset(self):
        pr = self.obj.get("prestige")
        if not isinstance(pr, dict) or pr.get("enabled") is False or not pr.get("currency"):
            return None
        place = self.place('"prestige"')
        out = {"found": True, "source": "data", "currency": pr["currency"], "currency_place": place, "p_place": place,
               "divisor_place": place, "gain_place": place, "min_gain": pr.get("min_gain"),
               "min_run_seconds": pr.get("min_run_seconds"), "unlock": self.unlock(pr.get("requires")),
               "unlock_also": self.unlocks(pr.get("requires"))[1:],
               "resets": pr.get("resets")}
        out.update(self._gain(pr))
        out.update(self._run_unlock(_dict_list(pr.get("requires")), place))
        return out

    def _run_unlock(self, reqs, place):
        """A reset that needs a run total of the stat of the spine needs a stage of the current run: the unlock is
        that stage, with unlock_in_run (the model as built tests the stage of the run, not the best stage)."""
        runs = [(self._run_stage(r), r) for r in reqs]
        n, req = max(((k, r) for k, r in runs if k), key=lambda x: x[0], default=(None, None))
        others = self.unlocks([r for r in reqs if r is not req])
        if n is None or any(u.get("stage", 0) > n for u in others):
            return {}
        return {"unlock": {"stage": n}, "unlock_also": others, "unlock_in_run": place}

    def _spine_run_test(self, req):
        """The requirement of the spine when it and req test the run total of the same stat, else {}."""
        lr = (self.ladder or {}).get("requires")
        lr = lr if isinstance(lr, dict) else {}
        same = req.get("type") == lr.get("type") == "run_total" and req.get("currency") == lr.get("currency")
        return lr if same else {}

    def _run_stage(self, req):
        """The stage that a run_total requirement on the stat of the spine equals, else None."""
        lr = self._spine_run_test(req)
        base, g, amount = lr.get("base"), lr.get("growth"), req.get("amount")
        if not _all_positive(base, g, amount) or g == 1:
            return None
        n = 1 + math.log(amount / base) / math.log(g)
        return int(round(n)) if abs(n - round(n)) < 0.01 and round(n) >= 1 else None

    def _gain(self, pr):
        lad = next((x for x in _dict_list(self.obj.get("ladders")) if x.get("id") == pr.get("basis_ladder")), None)
        if lad is not None:
            return _ladder_gain(pr, lad)
        if pr.get("formula") == "log":
            return {"gain_family": "log", "p": None, "divisor": pr.get("base"), "gain_key": "base",
                    "gain_formula": "k x log10(run %s / %s)" % (pr.get("basis_currency"), pr.get("base"))}
        return {"gain_family": "power", "p": pr.get("exponent"), "divisor": pr.get("base"), "gain_key": "exponent",
                "gain_formula": "k x (run %s / %s)^%s" % (pr.get("basis_currency"), pr.get("base"), pr.get("exponent"))}

    def rewards(self):
        """Each milestone with an effect: an element of kind reward."""
        return [m for m in self.ms.values() if _dict_list(m.get("effects"))]

    def side_system(self, cur):
        """The system of a side currency: its name, its unlock and its income (an add effect on the currency)."""
        sysid = next((x.get("system") for x in self.currencies() if x.get("id") == cur), None) or cur
        sysrec = next((x for x in self.systems() if x.get("id") == sysid), {})
        opener = self._opener(sysid)
        reqs = self.unlocks(opener.get("requires"))
        return {"id": sysid, "name": sysrec.get("name") or cur, "unlock": (reqs or [{}])[0], "unlock_also": reqs[1:],
                "rate": _add_rate(opener, cur), "place": self.place('"id": "%s"' % sysid), "opener": opener.get("id")}

    def _opener(self, sysid):
        """The milestone that opens a system."""
        rows = (m for m in self.ms.values() if m.get("system") == sysid and m.get("kind") == "system_unlock")
        return next(rows, {})

    def ladder_grants(self):
        g = [x for x in _dict_list((self.ladder or {}).get("grants")) if _numeric(x.get("base"))]
        return g[:1]

    def loop_evidence(self):
        """(system id, evidence) for each system of the data whose loop names a catalog card."""
        rows = [(LOOP_SYSTEMS.get(s.get("loop")), s) for s in self.systems()]
        return [(sid, self._loop_row(s)) for sid, s in rows if sid]

    def _loop_row(self, s):
        f, n = _split_place(self.place('"id": "%s"' % s.get("id")))
        return _evidence(f, n, "system %s: loop %s" % (s.get("id"), s.get("loop")), "weak", "data")

    def end_evidence(self):
        """The last step of the spine: a candidate of the hard end (S28)."""
        lad = self.ladder or {}
        if not _numeric(lad.get("count")):
            return []
        f, n = _split_place(self.place('"id": "%s"' % lad.get("id")))
        return [_evidence(f, n, "the spine %s has %d steps: the last step can be the hard end" % (lad.get("id"),
                                                                                            lad["count"]), "weak", "data")]


def _add_rate(milestone, cur):
    """The value of the add effect of a milestone on a currency: the income of a side system."""
    rows = (e.get("value") for e in _dict_list(milestone.get("effects")) if e.get("target") == cur and e.get("kind") == "add")
    return next(rows, None)


def _ladder_gain(pr, lad):
    """A gain `k x growth^step` of a ladder whose step n needs `base x w^(n-1)`: the step is 1 + log_w(E / base), so
    the gain is `(E / c)^p` with `p = ln growth / ln w` and `c = base x (k x growth)^(-1 / p)`."""
    req = lad.get("requires") if isinstance(lad.get("requires"), dict) else {}
    g, w, base, k = pr.get("growth"), req.get("growth"), req.get("base"), pr.get("k", 1.0)
    ok = pr.get("formula") == "exp" and _all_positive(g, w, base, k) and g != 1 and w > 1
    return _ladder_power(g, w, base, k, lad.get("id")) if ok else dict(LADDER_UNREAD, gain_formula=(
        "k x f(step of %s): the scan cannot convert it" % lad.get("id")))


LADDER_UNREAD = {"gain_family": "log", "p": None, "divisor": None, "gain_key": "growth"}


def _all_positive(*values):
    return all(_numeric(x) and x > 0 for x in values)


def _ladder_power(g, w, base, k, lid):
    p = math.log(g) / math.log(w)
    c = base * (k * g) ** (-1.0 / p)
    text = "k x %s^(step of %s), step n needs %s x %s^(n-1): (E / %.4g)^%.4f" % (g, lid, base, w, c, p)
    return {"gain_family": "power", "p": round(p, 4), "divisor": round(c, 4), "gain_key": "growth", "gain_formula": text}


CLEAR_LINE = re.compile(r"=\s*0(?:\.0*)?\s*$|\.clear\(\s*\)|\.erase\(")
COUNT_LINE = re.compile(r"\+=\s*1\s*$")
GAIN_LINE = re.compile(r"\+=\s*[A-Za-z_(]|\b_?(?:add|gain|grant|award|earn)\w*\s*\(")
RESET_WORDS = re.compile(r"prestige|rebirth|ascen|reincarnat|regrow|transcend|reset|rekindle|move_to|next_(?:town|"
                         r"world|area|zone|era|realm)|new_run|retire|travel|reborn", re.I)
NOT_RESET = re.compile(r"new_game|for_new|reset_state|^_init|^_ready|clear_save|wipe|^load|from_save|debug|test",
                       re.I)


def _shape_counts(body):
    lines = [ln.strip() for ln in body.split("\n")[1:] if ln.strip() and not _comment(ln)]
    clears = sum(1 for ln in lines if CLEAR_LINE.search(ln))
    gains = any(GAIN_LINE.search(ln) and not COUNT_LINE.search(ln) for ln in lines)
    return clears, gains, any(COUNT_LINE.search(ln) for ln in lines)


def reset_score(fn):
    """The score of a function as the reset: 0 when it does not clear 2 values and add to another."""
    clears, gains, counter = _shape_counts(fn[3])
    if clears < 2 or not gains:
        return 0
    name = fn[1]
    return min(clears, 4) + 2 * bool(RESET_WORDS.search(name)) + counter - 4 * bool(NOT_RESET.search(name))


def reset_candidates(idx):
    """The functions with a reset shape, best first: [(score, function)]."""
    rows = [(reset_score(fn), fn) for fn in idx.functions(re.compile("."))]
    return sorted([r for r in rows if r[0] > 0], key=_candidate_order)


def _candidate_order(row):
    return -row[0], row[1][0], row[1][2]


def _fn_label(fn):
    return "%s %s" % (_place(fn, fn[2]), fn[1])


def reset_shape(idx):
    """{"function", "unsure"}: the best reset candidate, and the 2 best when their scores are near."""
    rows = reset_candidates(idx)
    if not rows:
        return {}
    out = {"function": _fn_label(rows[0][1]), "shape_score": rows[0][0]}
    near = len(rows) > 1 and rows[1][0] >= rows[0][0] - 1
    out.update({"unsure": "2 functions have the shape of a reset: %s, %s" % (_fn_label(rows[0][1]),
                                                                           _fn_label(rows[1][1]))} if near else {})
    return out


def shape_fns(idx, label):
    """The reset function of the label and each function that it calls with `gain` in the name."""
    rel, _, name = label.partition(" ")
    rel = rel.rpartition(":")[0]
    fns = [fn for fn in idx.functions(re.compile(r"^%s$" % re.escape(name))) if fn[0] == rel]
    called = set(CALL.findall(fns[0][3])) if fns else set()
    names = {fn_name for _, fn_name in called if "gain" in fn_name.lower()}
    return fns + [fn for fn in idx.functions(re.compile("gain", re.I)) if fn[1] in names]


GAIN_FN = re.compile(RESET_WORDS.pattern + "|gain", re.I)
LADDER_WORD = re.compile(r"\b(?:step|level|ladder|pos|stage)\b", re.I)


def code_gain_place(idx, key, ladder_basis):
    """The code line of the gain branch that reads the data key: pow() with the quoted key, in a function with a
    reset or gain name. For a gain of the run currency, a line that reads a ladder step is the other branch."""
    rx = re.compile(r"[\"']%s[\"']" % re.escape(key))
    rows = [(fn, n, ln) for fn in idx.functions(GAIN_FN) for n, ln, _ in _body_hits(fn, GAIN_CALL) if rx.search(ln)]
    rows = [r for r in rows if ladder_basis or not LADDER_WORD.search(r[2])]
    return (_place(rows[0][0], rows[0][1]), rows[0][2].strip()[:160]) if rows else (None, None)


def main_currency(lines, reset_currency, base_income):
    """(main, unsure): the cost currency of the most purchase levels (a level cap counts up to LINE_WEIGHT levels, no
    cap counts LINE_WEIGHT). A currency with a base income wins a near tie. unsure names the 2 candidates of a tie."""
    w = collections.Counter()
    for ln in lines:
        _add_weight(w, ln, reset_currency)
    ranked = sorted(w, key=lambda c: (-w[c] - LINE_WEIGHT * bool(base_income.get(c)), c))
    return (ranked[0], _tie_reason(w, ranked)) if ranked else (None, None)


def _tie_reason(w, ranked):
    """The reason of a near tie of the 2 first currencies, else None."""
    if len(ranked) < 2 or w[ranked[1]] < 0.67 * w[ranked[0]]:
        return None
    a, b = ranked[:2]
    return "2 currencies buy a near count of levels: %s (%g), %s (%g)" % (a, w[a], b, w[b])


def _add_weight(w, ln, reset_currency):
    cur = ln["currency"]
    if cur and cur != reset_currency:
        w[cur] += min(ln["cap"] or LINE_WEIGHT, LINE_WEIGHT)


Q_METHOD = "r of the best uncapped geometric line priced in the reset currency (BM D5, D8)"


def _reward_row(ln):
    return {"id": ln["id"], "stat": ln["stat"], "r": ln["r"], "cap": ln["cap"], "place": "%s:%d" % (ln["file"], ln["line"])}


def reward(lines, currency):
    """The permanent reward: the lines priced in the reset currency and the exponent q of the best one."""
    shop = [ln for ln in lines if currency and ln["currency"] == currency]
    best = [ln for ln in shop if ln["r"] is not None and ln["cap"] is None]
    top = _reward_row(max(best, key=lambda ln: ln["r"])) if best else {}
    return {"q": top.get("r"), "multiplies": top.get("stat"), "q_line": top.get("id"), "q_place": top.get("place"),
            "q_method": Q_METHOD if top else "no uncapped geometric line in the reset currency",
            "reward_lines": [_reward_row(ln) for ln in shop]}


OFFLINE_NONE = {"rate": None, "rate_place": None, "rate_reason": None, "penalty": None, "cap_seconds": None,
                "cap_hours": None, "cap_place": None}


def _product(values):
    out = 1.0
    for v in values:
        out *= v
    return out


class OfflineScan:
    """The offline rule: the cap on the time away and the factor on the offline income."""

    def __init__(self, idx):
        self.idx = idx

    def result(self):
        fns = self.idx.functions(OFFLINE_FUNC)
        fns += [fn for fn in self.idx.functions(re.compile(".")) if fn not in fns and _offline_body(fn)]
        hit = _first_hit(fns, MIN_CALL)
        out = dict(OFFLINE_NONE, found=bool(fns), function=None)
        out = self._fill(out, *hit) if hit else self._uncapped(out, fns)
        out["found"] = "unsure" if out["found"] and out["rate"] is None and out["cap_seconds"] is None else out["found"]
        return out

    def _uncapped(self, out, fns):
        """An offline gain with no cap: a line `gain = income() * <time away>`."""
        hit = _first_hit(fns, UNCAPPED)
        if not hit:
            return out
        fn, n, line, m = hit
        out.update({"function": "%s %s" % (_place(fn, fn[2]), fn[1]), "cap_reason": "no cap on the time away"})
        out.update(self._factors(fn, m.group(1) or m.group(2), n, line.split("=", 1)[1]))
        out["rate_reason"] = out["rate_reason"].replace("the capped time", "the time away")
        return out

    def _fill(self, out, fn, n, line, m):
        out.update({"function": "%s %s" % (_place(fn, fn[2]), fn[1])})
        out.update(self._cap(fn, n, line, m))
        out.update(self._rate(fn, m.group(1)))
        out.update(self._cap_factor(fn, n, line, m, out))
        return out

    def _cap_factor(self, fn, n, line, m, out):
        """A factor after the cap call on the same line, `min(away, cap) * rate()`: it multiplies the rate."""
        rest = line[m.end() + len(",".join(call_args(line, m.end()))) + 1:]
        others = [unwrap(f) for f in rest.split("*")[1:] if f.strip()]
        hits = [self.idx.resolve(f, fn[3]) for f in others]
        if not others or None in hits or out.get("rate") is None:
            return {}
        rate = out["rate"] * _product(h["value"] for h in hits)
        return {"rate": rate, "rate_place": _place(fn, n), "penalty": round(1.0 - rate, 6),
                "rate_reason": "the offline gain is the income times the capped time times " + " x ".join(others)}

    def _cap(self, fn, n, line, m):
        args = call_args(line, m.end())
        hits = [(a, self.idx.resolve(a, fn[3])) for a in args]
        arg, hit = next(((a, h) for a, h in hits if h), (None, None))
        value, place = _hit_value(hit, _place(fn, n))
        unit = (arg + " " + (hit or {}).get("expr", "")).lower()
        seconds = value * 3600.0 if value is not None and "hour" in unit else value
        hours = round(seconds / 3600.0, 4) if seconds is not None else None
        return {"cap_seconds": seconds, "cap_hours": hours, "cap_place": place, "cap_formula": line.strip()[:160]}

    def _rate(self, fn, target):
        rx = re.compile(r"=\s*([^\n]*\b%s\b[^\n]*)" % re.escape(target)) if target else None
        rows = [(n, m.group(1)) for n, ln, m in _body_hits(fn, rx) if INCOME_CALL.search(ln)] if rx else []
        none = {"rate_reason": "no line multiplies an income call by the capped time"}
        return self._factors(fn, target, *rows[0]) if rows else none

    def _factors(self, fn, target, n, rhs):
        others = [unwrap(f) for f in rhs.split("*")]
        others = [f for f in others if f != target and not INCOME_CALL.search(f)]
        hits = [self.idx.resolve(f, fn[3]) for f in others]
        missing = [f for f, h in zip(others, hits) if not h]
        rate = None if missing else _product(h["value"] for h in hits)
        return {"rate": rate, "rate_place": _place(fn, n), "penalty": None if missing else round(1.0 - rate, 6),
                "rate_reason": _rate_reason(others, missing)}


UNCAPPED = re.compile(r"=[^\n]*\w*(?:per_sec|income|production|rate)\w*\s*\(\s*\)\s*\*\s*(\w*(?:offline|away|elapsed|"
                      r"seconds)\w*)\b|=[^\n]*\b(\w*(?:offline|away|elapsed)\w*)\s*\*\s*\w*(?:per_sec|income|production|rate)",
                      re.I)


def _offline_body(fn):
    return CLOCK_READ.search(fn[3]) is not None and OFFLINE.search(fn[3]) is not None


def _rate_reason(others, missing):
    if missing:
        return "the factor `%s` is not a number that the scan can read" % missing[0]
    if not others:
        return "the offline gain is the income times the capped time, with no other factor"
    return "the offline gain is the income times the capped time times " + " x ".join(others)


RULE_KEY = re.compile(r"^(?:stage|level|wall|step)_?(?:rule|test|mode)$", re.I)
TABLE_KEY = re.compile(r"(?:requirement|threshold|table|target)s?$", re.I)


def _table_key(obj):
    return next((k for k, v in obj.items() if TABLE_KEY.search(k) and isinstance(v, list) and len(v) > 1
                 and all(_numeric(x) for x in v)), None)


def _rule_growth(obj, table):
    """The growth key after the table: a numeric key with 'growth' and a word of the table key."""
    word = TABLE_KEY.search(table).group(0).rstrip("s").lower()
    return next((k for k, v in obj.items() if "growth" in k.lower() and word in k.lower() and _numeric(v)), None)


def rule_spine(idx):
    """The spine that a data file states: a rule key (stage_rule "rate"), a table of the amounts of the first stages
    and a growth for each later stage. The code selects its function by the rule, so other wall keys of the same file
    (an hp growth) are not the spine. None when no data file has the 3 keys."""
    found = (_data_rule(relp, obj, text) for relp, (obj, text) in sorted(idx.data.items()) if isinstance(obj, dict))
    return next((x for x in found if x), None)


def _rule_keys(obj):
    """(rule key, table key, growth key) of a data object, or None."""
    rule = next((k for k, v in obj.items() if RULE_KEY.search(k) and isinstance(v, str)), None)
    table = _table_key(obj) if rule else None
    grow = _rule_growth(obj, table) if table else None
    return (rule, table, grow) if grow else None


def _data_rule(relp, obj, text):
    keys = _rule_keys(obj)
    if not keys:
        return None
    rule, table, grow = keys
    at = {k: "%s:%d" % (relp, _text_line(text, max(text.find('"%s"' % k), 0))) for k in keys}
    return {"g": float(obj[grow]), "g_place": at[grow], "place": at[table], "source": "data",
            "table": [float(x) for x in obj[table]], "base": float(obj[table][0]), "test": obj[rule],
            "count": obj.get("count") if _numeric(obj.get("count")) else None, "function": rule,
            "formula": "%s %s: step n needs %s[n], then x %s (%s)" % (rule, obj[rule], table, obj[grow], grow)}


def spine(idx):
    """The wall formula: the first pow() in a function with a requirement name, and its ratio g."""
    hit = _first_hit(idx.functions(SPINE_FUNC), POW_CALL)
    if not hit:
        return None
    fn, n, line, m = hit
    args = call_args(line, m.end())
    g, place = _hit_value(idx.resolve(args[0], fn[3]) if args else None, _place(fn, n))
    return {"g": g, "g_place": place, "place": _place(fn, n), "formula": line.strip()[:160], "function": fn[1]}


def export_rows(idx, currencies):
    """Lines where the main code reads a multiplier of another autoload, when a third currency exists."""
    rows = [r for r in idx.grep(EXPORT_CALL, 5) if EXPORT_CALL.search(r[2]).group(1) in idx.autoloads]
    return rows if currencies else []


def _clock_rows(fn):
    return [(fn[0], fn[2] + i - 1, ln) for i, ln in clock_hits(fn[3])]


def clock_rows(idx):
    """Wall-clock subtractions outside the save, load and offline functions."""
    fns = [fn for fn in idx.functions(re.compile(".")) if not SAVE_FUNC.search(fn[1]) and CLOCK_READ.search(fn[3])]
    return [row for fn in fns for row in _clock_rows(fn)]


def _load_variants(path=SYSTEMS_JSON):
    """{variant id: {parameter: (min, max)}} from assets/systems.json. Empty when the file is absent."""
    try:
        data = json.loads(read(path))
    except ValueError:
        return {}
    variants = (v for s in data.get("systems", []) for v in s.get("variants", []))
    return {v["id"]: {k: (p.get("min"), p.get("max")) for k, p in (v.get("parameters") or {}).items()
                      if isinstance(p, dict)} for v in variants}


def _distance(value, rng):
    return max(rng[0] - value, value - rng[1], 0.0)


def _pick_variant(ids, name, value, variants):
    """The first variant whose range of name holds value, else the variant with the nearest range."""
    ranges = [(vid, variants.get(vid, {}).get(name)) for vid in ids]
    ranges = [(vid, r) for vid, r in ranges if r and None not in r]
    return min(ranges, key=lambda x: _distance(value, x[1]))[0] if ranges else ids[0]


LINE_PARAMS = {
    ("S01", "linear"): (("S01.geometric", "S01.steep"), (("b", "cost_ratio"),)),
    ("S02", "geometric"): (("S02.levelled_factor",), (("b", "cost_ratio"), ("d", "value_ratio"))),
    ("S02", "linear"): (("S02.add_in_group",), (("b", "cost_ratio"), ("v", "value_step"))),
    ("S15", None): (("S15.geometric",), (("b", "cost_ratio"),)),
}


def line_system(rec, reset_currency):
    rules = ((reset_currency is not None and rec["currency"] == reset_currency, "S15"),
             (rec["value_family"] == "cap", "S13"),
             (rec["value_key"] in ADD_KEYS, "S01"))
    return next((sid for ok, sid in rules if ok), "S02")


def _param(name, value, place, variant, line=None):
    return {"name": name, "value": value, "place": place, "variant": variant, "line": line}


def line_params(rec, variants):
    """The catalog parameters of 1 purchase line, each with the variant that it is checked against."""
    spec = LINE_PARAMS.get((rec["system"], rec["value_family"])) or LINE_PARAMS.get((rec["system"], None))
    if not spec or rec["levels"] == 1 or rec["cost_family"] not in ("geometric", "unsure"):
        return []
    ids, fields = spec
    vid = _pick_variant(ids, fields[0][0], rec[fields[0][1]], variants)
    place = "%s:%d" % (rec["file"], rec["line"])
    return [_param(name, rec[field], place, vid, rec["id"]) for name, field in fields if rec[field] is not None]


def _split_place(place):
    f, _, n = place.rpartition(":")
    return f, int(n) if n.isdigit() else 0


def _place_evidence(place, text, strength="strong"):
    f, n = _split_place(place)
    return _evidence(f, n, text, strength, "data" if f.endswith(".json") else "code")


def _unique(evidence):
    """The first 5 evidence rows, each place 1 time, the strong rows first."""
    seen = set()
    keep = [e for e in evidence if (e["file"], e["line"]) not in seen and not seen.add((e["file"], e["line"]))]
    return sorted(keep, key=lambda e: e["strength"] != "strong")[:5]


def _line_evidence(rec):
    value = rec["value_ratio"] if rec["value_ratio"] is not None else rec["value_step"]
    text = "line %s: cost %s x %s^level, %s %s on %s" % (rec["id"], rec["cost_base"], rec["cost_ratio"],
                                                       rec["value_key"], value, rec["stat"])
    return _evidence(rec["file"], rec["line"], text, "strong" if rec["status"] == "yes" else "weak", "data")


def _range_row(p, variants):
    rng = variants.get(p["variant"], {}).get(p["name"])
    bad = rng and None not in rng and not rng[0] <= p["value"] <= rng[1]
    return dict(p, min=rng[0], max=rng[1]) if bad else None


def out_of_range(params, variants):
    rows = (_range_row(p, variants) for p in params)
    return [r for r in rows if r]


def _found(evidence):
    strengths = {e["strength"] for e in evidence}
    return "yes" if "strong" in strengths else "unsure" if strengths else "no"


FOUND_REASON = {"yes": "a strong signature or an analyzer of the scan matches",
                "unsure": "only a weak signature matches: read the code to confirm",
                "no": "no signature of the card matches"}


class CatalogMap:
    """The map of the 29 systems of the catalog for 1 scanned project (S29 has no signature yet)."""

    def __init__(self, scan, pattern=None):
        self.idx, self.variants, self.pattern = CodeIndex(scan), _load_variants(), pattern
        self.evidence = {sid: [] for sid in SIGNATURES}
        self.params = {sid: [] for sid in SIGNATURES}
        self.notes, self.reset, self.lines, self.offline = {}, {}, [], {}
        self.model, self.spine, self.currencies = None, None, {}

    def build(self):
        self._signatures()
        self.model = find_model(self.idx)
        self.reset = self._reset()
        self._lines()
        self.reset.update(reward(self.lines, self.reset["currency"]))
        self.offline = OfflineScan(self.idx).result()
        self.spine = (self.model.spine() if self.model else None) or rule_spine(self.idx) or spine(self.idx)
        self.reset.update(self._rules())
        self.currencies = self._currencies()
        for step in BUILD_STEPS:
            step(self)
        return self._result()

    def _result(self):
        systems = [self._system(sid) for sid in SIGNATURES]
        params = [p for sid in SIGNATURES for p in self.params[sid]]
        return {"schema_version": CATALOG_SCHEMA, "project": {"name": self.idx.info["name"], "root": self.idx.root},
                "pattern": self.pattern, "systems": systems, "missing_core": missing_core(systems, self.pattern),
                "missing_core_by_pattern": {pid: missing_core(systems, pid) for pid in PATTERN_IDS},
                "model_file": self.model.relp if self.model else None, "data_skipped": self.idx.skipped,
                "currencies": self.currencies, "spine": self.spine, "lines": self.lines, "reset": self.reset,
                "offline": self.offline, "out_of_range": out_of_range(params, self.variants),
                "end": hard_end(self._stage_count(), self.reset.get("max_resets"))}

    def _stage_count(self):
        """(the count of stages, its place): the spine of a model file, else the data key count."""
        sp, sc = self.spine or {}, self.idx.scalars.get("count", {})
        return (sp["count"], sp.get("place")) if sp.get("count") else (sc.get("value"), sc.get("place"))

    def _reset(self):
        """The reset: the gain function by its name, else by the shape of the code; the data of a model file wins
        for p, the divisor and the currency."""
        shape = reset_shape(self.idx)
        out = _code_reset(self.idx, shape)
        data = self.model.reset() if self.model else None
        out.update(self._data_reset(data))
        out["clear_function"] = shape.get("function")
        sure = data or out.get("gain_family") and out.get("currency")
        out.update({} if sure or "unsure" not in shape else {"unsure": shape["unsure"]})
        return out

    def _rules(self):
        """The reset rule and the most resets of the game, for a game with a reset."""
        if not self.reset.get("gain_family"):
            return {}
        return {"rule": reset_rule(self.idx, self.model), "max_resets": max_resets(self.idx)}

    def _data_reset(self, data):
        return dict(data, **self._data_gain_place(data)) if data else {}

    def _data_gain_place(self, data):
        place, line = code_gain_place(self.idx, data["gain_key"], data["gain_key"] == "growth")
        return {"gain_place": place, "gain_code": line} if place else {}

    def _currencies(self):
        base = self.model.base_income() if self.model else {}
        main, why = main_currency(self.lines, self.reset.get("currency"), base)
        side = sorted({ln["currency"] for ln in self.lines} - {main, self.reset.get("currency"), None})
        out = {"main": main, "reset": self.reset.get("currency"), "side": side}
        out.update({"unsure": why} if why else {})
        return out

    def _signatures(self):
        jobs = [(sid, rx, strength, cap) for sid, (strong, weak) in SIG_RX.items()
                for rx, strength, cap in ((strong, "strong", 3), (weak, "weak", 2))]
        for job in jobs:
            self._grep_evidence(*job)

    def _grep_evidence(self, sid, rx, strength, cap):
        rows = self.idx.grep(rx, cap) if rx else []
        self.evidence[sid] += [_evidence(f, n, t, strength) for f, n, t in rows]

    def _lines(self):
        currency = self.reset["currency"]
        for rec in LineScan(self.idx).lines():
            self._add_line(rec, currency)

    def _add_line(self, rec, currency):
        rec["system"] = line_system(rec, currency)
        self.lines.append(rec)
        self.evidence[rec["system"]].insert(0, _line_evidence(rec))
        self.params[rec["system"]] += line_params(rec, self.variants)

    def _note(self, sid, variant, reason, params=()):
        self.notes[sid] = (variant, reason)
        self.params[sid] += [p for p in params if p["value"] is not None]

    def _apply_reset(self):
        r = self.reset
        if not r.get("gain_family"):
            return
        self.evidence["S04"].insert(0, _place_evidence(r["gain_place"], r["gain_formula"]))
        vid = "S04.power" if r["gain_family"] == "power" else "S04.log"
        reason = "the gain is %s in %s" % (r["gain_formula"], r["gain_place"])
        self._note("S04", vid, reason, [_param("p", r["p"], r["p_place"], vid)] if vid == "S04.power" else [])

    def _apply_offline(self):
        o = self.offline
        if o.get("cap_seconds") is None and o.get("rate") is None:
            return
        text = "offline cap %s s" % o["cap_seconds"] if o.get("cap_place") else "offline rate %s, no cap" % o["rate"]
        self.evidence["S06"].insert(0, _place_evidence(o.get("cap_place") or o["rate_place"], text))
        vid = "S06.penalty_cap"
        params = [_param("f_closed", o["rate"], o["rate_place"], vid), _param("T_cap_h", o["cap_hours"], o["cap_place"], vid)]
        how = "capped" if o.get("cap_place") else "not capped"
        self._note("S06", vid, "the time away is %s in %s; %s" % (how, o["function"], o["rate_reason"]), params)

    def _apply_spine(self):
        s = self.spine
        if not s:
            return
        self.evidence["S03"].insert(0, _place_evidence(s["place"], s["formula"]))
        vid = "S03.exponential"
        how = "the data ladder %s" % s["function"] if s.get("source") == "data" else "pow() in %s" % s["function"]
        self._note("S03", vid, "the requirement grows by %s" % how, [_param("g", s["g"], s["g_place"], vid)])

    def _apply_model(self):
        """The evidence of a model file: the loops of its systems, its active-only effects (S05), its level
        milestones (S12) and the last step of its spine (S28)."""
        m = self.model
        self.evidence["S05"][:0] = [self._data_ev(ln, "an effect pays only while the player plays (active)")
                                    for ln in self.lines if any(e["active_only"] for e in ln["effects"])][:2]
        if not m:
            return
        for sid, ev in m.loop_evidence() + [("S28", e) for e in m.end_evidence()]:
            self.evidence[sid].append(ev)
        self.evidence["S12"][:0] = [_place_evidence(m.place('"id": "%s"' % x["id"]), "milestone %s: a level of %s "
                                                    "opens a mult effect" % (x["id"], _level_of(x)))
                                    for x in m.rewards() if _level_of(x) and _has_mult(x)][:3]

    @staticmethod
    def _data_ev(ln, text):
        return _evidence(ln["file"], ln["line"], "line %s: %s" % (ln["id"], text), "strong", "data")

    def _apply_export(self):
        main = self.currencies["main"]
        third = {ln["currency"] for ln in self.lines} - {main, self.reset["currency"], None}
        rows = export_rows(self.idx, third)
        self.evidence["S23"][:0] = [_evidence(f, n, t, "strong") for f, n, t in rows]
        reason = "the main code multiplies by the export of a sub-game with the currency " + ", ".join(sorted(third))
        self.notes.update({"S23": ("S23.product_export", reason)} if rows else {})

    def _apply_clock(self):
        rows = clock_rows(self.idx)
        self.evidence["S09"][:0] = [_evidence(f, n, t, "strong") for f, n, t in rows]


    def _system(self, sid):
        ev = _unique(self.evidence[sid])
        found = _found(ev)
        variant, vreason = self.notes.get(sid) or self._param_variant(sid)
        return {"id": sid, "name": SIGNATURES[sid][0], "class": _class_of(sid, self.pattern), "found": found,
                "reason": FOUND_REASON[found], "evidence": ev, "variant": variant, "variant_reason": vreason,
                "parameters": self.params[sid]}

    def _param_variant(self, sid):
        c = collections.Counter(p["variant"] for p in self.params[sid])
        if not c:
            return None, "the scan extracts no parameter for this system"
        vid, n = c.most_common(1)[0]
        return vid, "%d of %d extracted parameters fit this family" % (n, sum(c.values()))


def _code_reset(idx, shape):
    """The reset of the code: the gain function by its name, else the reset function by its shape and the gain
    functions that it calls."""
    rs = ResetScan(idx)
    out = {**rs.gain(), **rs.currency()}
    if out.get("gain_family") or not shape:
        return out
    alt = ResetScan(idx, shape_fns(idx, shape["function"]))
    return {**alt.gain(), **alt.currency(), "found": True}


def _level_of(m):
    return next((r.get("id") for r in _dict_list(m.get("requires")) if r.get("type") == "level"), None)


def _has_mult(m):
    return any(e.get("kind") == "mult" for e in _dict_list(m.get("effects")))


BUILD_STEPS = (CatalogMap._apply_reset, CatalogMap._apply_offline, CatalogMap._apply_spine, CatalogMap._apply_model,
               CatalogMap._apply_export, CatalogMap._apply_clock)


CONTENT_SCHEMA = 1
UNLOCK_PREFIX = "unlock_"
REWARD_KINDS = (("mult", GEO_KEYS), ("add", ADD_KEYS), ("pct", BONUS_KEYS), ("cap", CAP_KEYS))
FAMILY_OF = {"geometric": "mult", "linear": "add", "cap": "cap"}


def _family(rec):
    if rec["value_key"] in BONUS_KEYS:
        return "pct"
    return FAMILY_OF.get(rec["value_family"], "unsure")


def _objects_by_id(data):
    """{id: (file, dict)} of each dict with a string id at any depth of the data files."""
    out = {}
    pairs = [(relp, item) for relp, (obj, _) in sorted(data.items()) for item in _dicts(obj)]
    for relp, item in pairs:
        out.setdefault(item["id"], (relp, item))
    return out


def _dicts(obj):
    own = [obj] if isinstance(obj, dict) and isinstance(obj.get("id"), str) else []
    return own + [item for c in _children(obj) for item in _dicts(c)]


def _unlock_of(obj, currency_of_key):
    """The unlock point of a data entry: {"stage": n} or {"total": currency, "amount": n}; {} for none."""
    keys = [(k, v) for k, v in sorted(obj.items()) if k.startswith(UNLOCK_PREFIX) and _numeric(v) and v > 0]
    if not keys:
        return {"stage": 0} if "unlock_stage" in obj else {}
    k, v = keys[0]
    if k == "unlock_stage":
        return {"stage": int(v)}
    if RESET_WORD.search(k[len(UNLOCK_PREFIX):]):
        return {"resets": int(v)}
    return {"total": currency_of_key(k[len(UNLOCK_PREFIX):]), "amount": float(v)}


RESET_WORD = re.compile(r"(prestige|reset|rebirth|ascension)s?(_count)?$", re.I)


def _effect_row(eff, kind, value):
    """1 effect of the content format: stat (and the old key target), kind, value and active_only."""
    stat = eff.get("stat") or eff.get("target")
    return {"stat": stat, "target": stat, "kind": kind, "value": value, "active_only": eff.get("active") is True}


def _list_effect(eff):
    kind, value = _kind_value(eff)
    return _effect_row(eff, kind or eff.get("kind"), value if kind else eff.get("value"))


def _effects_of(obj):
    """The effects of a data entry: [{"stat", "target", "kind", "value", "active_only"}]."""
    effs = obj.get("effects")
    if isinstance(effs, list):
        return [_list_effect(e) for e in effs if isinstance(e, dict)]
    eff = obj.get("effect") if isinstance(obj.get("effect"), dict) else {}
    found = [(kind, k) for kind, keys in REWARD_KINDS for k in keys if _numeric(eff.get(k))]
    return [_effect_row(eff, kind, float(eff[k])) for kind, k in found[:1]]


def _drop_side_openers(elements, openers):
    """Remove the add effect on the side currency from each opener reward of a side system that has its rate. A
    reward with no effect left goes, and each unlock of it points at the side system."""
    sides = _rated_sides(elements)
    pairs = [(e, openers.get(e["id"])) for e in elements if e["kind"] == "reward"]
    moved = {e["id"]: sides[cur] for e, cur in pairs if cur in sides and not _drop_side_rate(e, cur)}
    for e in elements:
        _point_unlocks(e, moved)
    return [e for e in elements if e["id"] not in moved]


def _rated_sides(elements):
    """{side currency: side system id} of each side system whose rate the scan read."""
    rows = [e for e in elements if e["kind"] == "system" and e["system"] == "S23"]
    return {e["currency"]: e["id"] for e in rows if (e.get("values") or {}).get("rate_per_second") is not None}


def _drop_side_rate(e, cur):
    """Remove the add effects on cur from the reward e. Return the effects left."""
    e["effects"] = [f for f in e["effects"] if not (f["stat"] == cur and f["kind"] == "add")]
    first = (e["effects"] or [None])[0]
    e.update({"affects": first["stat"], "family": first["kind"], "value": first["value"]} if first else {})
    return e["effects"]


def _point_unlocks(e, moved):
    """Point each unlock of a removed reward at its side system."""
    point = lambda u: {"milestone": moved[u["milestone"]]} if u.get("milestone") in moved else u  # noqa: E731
    e["unlock"] = point(e.get("unlock") or {}) if e.get("unlock") is not None else e.get("unlock")
    e.update({"unlock_also": [point(u) for u in e["unlock_also"]]} if e.get("unlock_also") else {})


def _side_unlocks(elements):
    """An element of a side currency with no unlock opens with its side system."""
    side = {x["currency"]: x["id"] for x in elements if x["kind"] == "system" and x["system"] == "S23"}
    for x in elements:
        x["unlock"] = x["unlock"] or ({"system": side[x["currency"]]} if x["currency"] in side else {})


class ContentScan:
    """The content file of a built game: each purchase line, each reward and each system, with its unlock point."""

    def __init__(self, idx, catalog, model=None):
        self.idx, self.cat, self.model = idx, catalog, model
        self.objs = _objects_by_id(idx.data)
        self.cur = content_currencies(catalog)
        self.main, self.reset_cur = self.cur["main"], self.cur["reset"]

    def currency_of_key(self, word):
        """The currency of an unlock key such as lifetime_mycelium or lifetime_dew."""
        names = {ln["currency"] for ln in self.cat["lines"] if ln["currency"]}
        return next((c for c in sorted(names) if word.endswith(c)), word)

    def unlock(self, obj):
        """The unlock of a data entry: its `requires` list in a model file, else its unlock_ keys."""
        if self.model and isinstance(obj.get("requires"), list):
            return self.model.unlock(obj["requires"])
        return _unlock_of(obj, self.currency_of_key)

    def unlock_also(self, obj):
        """{"unlock_also": [...]}: the other requirements of a model entry, which the content file cannot hold."""
        more = self.model.unlocks(obj["requires"])[1:] if self.model and isinstance(obj.get("requires"), list) else []
        return {"unlock_also": more} if more else {}

    def build(self):
        lines = [self.line(rec) for rec in self.cat["lines"]]
        line_ids = {ln["id"] for ln in lines}
        rewards = [self.reward(relp, obj) for relp, obj in self.reward_objs() if obj["id"] not in line_ids]
        elements = _drop_side_openers(lines + rewards + self.systems(), self.side_openers())
        _side_unlocks(elements)
        out = {"schema_version": CONTENT_SCHEMA, "game": self.idx.info["name"], "source": "scan_godot.py --content-out",
               "currencies": self.cur, "stats": self.stats(lines), "elements": elements}
        out.update(self.spine_stat(out["stats"], elements))
        out.update({"end": self.cat["end"]} if self.cat.get("end") else {})
        return out

    def side_openers(self):
        """{milestone id: side system element}: the milestone of a model file that opens a side system and pays its
        rate. The side system holds that rate, so the milestone must not pay it a second time."""
        rows = [(self.model.side_system(cur)["opener"], cur) for cur in self.cur["side"]] if self.model else []
        return {mid: cur for mid, cur in rows if mid}

    def reward_objs(self):
        if self.model:
            return [(self.model.relp, m) for m in self.model.rewards()]
        return [(relp, obj) for relp, obj in self.objs.values() if _effects_of(obj) and self.unlock(obj)]

    def spine_stat(self, stats, elements):
        """{"spine_stat": stat} when the spine tests a stat that is not the main currency (BM L10). An element or a
        base income that adds to the stat makes it a second income: stats keeps it apart. Else the stat can also be a
        part of the main income: stats keeps it there, and `spine_unsure` names the 2 candidates."""
        st = (self.cat.get("spine") or {}).get("stat")
        if not st or st == self.main:
            return {}
        if st in (self.reset_cur, *self.cur["side"]) or self.has_income(st, elements):
            stats[st] = st if stats.get(st) in (None, self.main) else stats[st]
            return {"spine_stat": st}
        stats.setdefault(st, self.main)
        return {"spine_stat": st, "spine_unsure": _spine_unsure(st, self.main)}

    def has_income(self, st, elements):
        """True when an element adds to the stat or the model file gives it a base income."""
        base = self.model.base_income() if self.model else {}
        adds = (f for e in elements for f in e.get("effects") or [] if f.get("kind") == "add")
        return bool(base.get(st)) or any(f.get("stat") == st for f in adds)

    def stats(self, lines):
        """{stat: currency}: a stat that names a cost currency is that currency; another stat that a line paid in the
        main currency affects is a part of the main income."""
        costs = {ln["currency"] for ln in lines if ln["currency"]}
        pairs = _stat_pairs(lines, costs)
        out = {st: (st if st in costs else self.main) for st, cur in pairs if cur == self.main}
        side = [(st, cur) for st, cur in pairs if st not in out and cur not in (self.main, self.reset_cur)]
        out.update({st: (st if st in costs else cur) for st, cur in side})
        return dict(sorted(out.items()))

    def line(self, rec):
        relp, obj = self.objs.get(rec["id"], (rec["file"], {}))
        cost = {"base": rec["cost_base"], "ratio": rec["cost_ratio"]}
        cost.update({"formula": "linear"} if rec.get("cost_formula") == "linear" else {})
        return {"id": rec["id"], "name": rec["name"], "kind": "line", "system": rec.get("system"),
                "currency": rec["currency"], "affects": rec["stat"], "family": _family(rec), "cost": cost,
                "value": rec["value_ratio"] if rec["value_ratio"] is not None else rec["value_step"],
                "effects": rec["effects"], "levels": rec["levels"], "cap": rec["cap"], "unlock": self.unlock(obj),
                "role": self.role(rec, obj), "place": "%s:%d" % (rec["file"], rec["line"]), **self.unlock_also(obj)}

    def role(self, rec, obj):
        """The hint of the solver: `manual` for an add line whose main effect pays only while the player plays,
        `generator` for an add line of a system whose loop is automation. Else null."""
        effs = [e for e in rec["effects"] if e["kind"] == "add"][:1]
        if rec["currency"] != self.main or not effs:
            return None
        return "manual" if effs[0]["active_only"] else ("generator" if self.automated(obj) else None)

    def automated(self, obj):
        loops = {s.get("id"): s.get("loop") for s in self.model.systems()} if self.model else {}
        return loops.get(obj.get("system")) == "automation"

    def reward(self, relp, obj):
        effs = _effects_of(obj)
        out = {"id": obj["id"], "name": obj.get("name") or obj["id"], "kind": "reward", "system": None,
               "currency": None, "affects": effs[0]["stat"], "family": effs[0]["kind"], "cost": None,
               "value": effs[0]["value"], "effects": effs, "levels": None, "cap": None, "unlock": self.unlock(obj),
               "role": None, "place": self.model.place('"id": "%s"' % obj["id"]) if self.model else relp,
               **self.unlock_also(obj)}
        out.update({"grants": obj["grants"]} if isinstance(obj.get("grants"), list) else {})
        return out

    def systems(self):
        sc = self.idx.scalars
        out = [self.stage_system(sc)] + self.reset_system(sc) + self.side_systems() + self.grant(sc)
        return [x for x in out if x]

    def stage_system(self, sc):
        s = next((x for x in self.cat["systems"] if x["id"] == "S03"), {})
        sp = self.cat.get("spine") or {}
        data = sp.get("source") == "data"
        count = sp.get("count") if data else sc.get("count", {}).get("value")
        return {"id": "stages", "name": "Stages", "kind": "system", "system": "S03", "currency": None,
                "affects": sp.get("stat") if data else None, "family": s.get("variant"), "cost": None, "value": None,
                "effects": [], "cap": int(count) if count else None, "unlock": {"stage": 0}, "role": "main",
                "place": sp.get("place") if data else sc.get("count", {}).get("place"), **stage_values(sp)}

    def reset_system(self, sc):
        r = self.cat["reset"]
        if not r.get("gain_family"):
            return []
        stage = sc.get("prestige_unlock_stage", {}).get("value", 0)
        unlock = r.get("unlock") if "unlock" in r else {"stage": int(stage)}
        also = {"unlock_also": r["unlock_also"]} if r.get("unlock_also") else {}
        return [{**also, "id": "reset", "name": "Reset (" + str(r.get("currency")) + ")", "kind": "system", "system": "S04",
                 "currency": r.get("currency"), "affects": None, "family": r["gain_family"], "cost": None,
                 "value": r.get("p"), "effects": [], "cap": None, "unlock": unlock, "role": "main",
                 "place": r.get("gain_place"), **reset_values(r)}]

    def side_systems(self):
        """1 system for each side currency."""
        fn = self.model_side if self.model else self.side_system
        return [fn(cur) for cur in self.cur["side"]]

    def model_side(self, cur):
        s = self.model.side_system(cur)
        values = {"rate_per_second": s["rate"]} if _numeric(s["rate"]) else {}
        return {"id": s["id"], "name": "Side system (%s): %s" % (cur, s["name"]), "kind": "system", "system": "S23",
                "currency": cur, "affects": None, "family": "product_export", "cost": None, "value": None,
                "effects": [], "cap": None, "unlock": s["unlock"] or {"stage": 0}, "role": "support",
                "place": s["place"], "values": values, **({"unlock_also": s["unlock_also"]} if s["unlock_also"] else {})}

    def side_system(self, cur):
        """The data file of the lines of a side currency, with the unlock_stage of that file."""
        relp = next(ln["file"] for ln in self.cat["lines"] if ln["currency"] == cur)
        obj = self.idx.data[relp][0]
        obj = obj if isinstance(obj, dict) else {}
        sid = os.path.splitext(os.path.basename(relp))[0]
        unlock = self.coded_unlock(sid) or _unlock_of(obj, self.currency_of_key)
        return {"id": sid, "name": "Side system (" + cur + ")",
                "kind": "system", "system": "S23", "currency": cur, "affects": None, "family": "product_export",
                "cost": None, "value": None, "effects": [], "cap": None, "unlock": unlock or {"stage": 0},
                "role": "support", "place": relp, "values": {k: v for k, v in sorted(obj.items()) if _numeric(v)}}

    def coded_unlock(self, sid):
        """The unlock of system sid from a data key <sid>_unlock_<x> > 0 that the code reads (for example
        garden_unlock_lifetime_spores): the code opens the system by that key, not by an unlock key of its own file
        that the code does not read. {} when no such key exists."""
        keys = [k for k, v in sorted(self.idx.scalars.items()) if k.startswith(sid + "_unlock_") and v["value"] > 0
                and any(k in t for t in self.idx.texts.values())]
        return _unlock_of({k[len(sid) + 1:]: self.idx.scalars[k]["value"] for k in keys[:1]}, self.currency_of_key)

    def grant(self, sc):
        g = self.model.ladder_grants() if self.model else []
        if g:
            return [self._grant(g[0]["base"], g[0].get("growth", 1.0), g[0].get("currency") or self.main,
                                self.cat["spine"]["place"])]
        if "reward_base" not in sc:
            return []
        growth = sc.get("reward_growth", {}).get("value", 1.0)
        return [self._grant(sc["reward_base"]["value"], growth, self.main, sc["reward_base"]["place"])]

    def _grant(self, base, growth, cur, place):
        value = {"base": base, "growth": growth}
        value.update({"currency": cur, "trigger": "stage"} if cur != self.main else {})
        return {"id": "stage_rewards", "name": "Stage rewards", "kind": "grant", "system": "S03", "currency": cur,
                "affects": cur, "family": "grant", "cost": None, "value": value, "effects": [], "cap": None,
                "unlock": {"stage": 1}, "role": "support", "place": place}


def reset_values(r):
    """{"values", "evidence", "confirm"} of the reset element: the reset rule (values.policy with peak_tolerance or
    policy_ratio), the most resets (values.max_resets) and a reset that tests the stage of the run
    (values.unlock_in_run), each with its code place, and the questions of the values that the scan is not sure of."""
    rule, most = r.get("rule"), r.get("max_resets")
    values, evidence = {}, {}
    if rule:
        values.update({"policy": rule["policy"], RULE_KEYS[rule["policy"]]: rule["value"]})
        evidence["policy"] = rule["place"]
    if most:
        values["max_resets"], evidence["max_resets"] = most["value"], most["place"]
    if r.get("unlock_in_run"):
        values["unlock_in_run"], evidence["unlock_in_run"] = True, r["unlock_in_run"]
    asks = [x["confirm"] for x in (rule, most) if x and x.get("confirm")]
    evidence = {k: v for k, v in evidence.items() if v}
    return {"values": values, "evidence": evidence, "confirm": asks} if values else {}


def stage_values(sp):
    """{"values": ...} of the stages from a data spine: base, growth, the table of the first stages and the test,
    with the stat when the data names it. {} when the spine is not in the data."""
    if sp.get("source") != "data" or not _numeric(sp.get("base")):
        return {}
    v = {"base": float(sp["base"]), "growth": sp["g"], "test": sp.get("test") or "rate"}
    v.update({k: sp[k] for k in ("table", "stat") if sp.get(k)})
    return {"values": v}


def _spine_unsure(st, main):
    """The note of a spine stat that the scan cannot place: its 2 candidates."""
    return ("the stages test {s}, and no element adds to {s}: {s} is a second income (stats {s}: {s}) or a part of "
            "the main income (stats {s}: {m}); confirm it with stats in the confirm file").format(s=st, m=main)


def _stat_pairs(lines, costs):
    """(stat, cost currency of the line) of each effect. A stat that names a line (a cap effect) is not a stat."""
    ids = {ln["id"] for ln in lines} - costs
    pairs = [(e["stat"], ln["currency"]) for ln in lines for e in ln["effects"]]
    return [(st, cur) for st, cur in pairs if st and st not in ids]


def content_currencies(catalog):
    """currencies of the content file. main and reset are never null when the game has them: with no reset found,
    the reset is the best guess and `unsure` says so."""
    cur = dict(catalog.get("currencies") or {"main": None, "reset": None, "side": []})
    cur["side"] = list(cur.get("side") or [])
    if cur.get("reset") or not cur["side"]:
        return cur
    guess = cur["side"].pop(0)
    why = "no reset found: %s is a guess (the first other cost currency)" % guess
    cur.update(reset=guess, unsure="; ".join(x for x in (cur.get("unsure"), why) if x))
    return cur


def _load_patterns(path=SYSTEMS_JSON):
    """{pattern id: (core ids, gate slot is core)} from assets/systems.json."""
    try:
        data = json.loads(read(path))
    except ValueError:
        return {}
    return {p["id"]: (tuple(p.get("core") or ()), bool(p.get("gate_min"))) for p in data.get("play_patterns", [])}


PATTERNS = _load_patterns()
PATTERN_IDS = tuple(sorted(PATTERNS))


def _core_of(pattern):
    """(core ids, gate slot is core) of a play pattern. With no pattern: S01 to S06 and the gate slot."""
    return PATTERNS.get(pattern) or (CORE_IDS, True)


def _class_of(sid, pattern=None):
    core, gate = _core_of(pattern)
    return "core" if sid in core else "core_slot" if gate and sid in GATE_IDS else "optional"


def missing_core(systems, pattern=None):
    """Each core system of the pattern with found = no, and the presence gate slot when it is core and no member has
    found = yes."""
    by = {s["id"]: s for s in systems}
    ids, gate_core = _core_of(pattern)
    core = [{"kind": "system", "id": sid, "name": by[sid]["name"]} for sid in ids if by[sid]["found"] == "no"]
    gate = not gate_core or any(by[g]["found"] == "yes" for g in GATE_IDS)
    return core + ([] if gate else [dict(GATE_SLOT, kind="slot")])


CONFIRM_KEYS = ("systems", "currencies", "reset", "stats", "spine_stat", "end", "base_income", "active_factor", "add",
                "remove", "change", "notes")
COPY_KEYS = ("base_income", "active_factor")
END_KEYS = ("axis_step", "resets", "purchase", "milestone")
FOUND_VALUES = ("yes", "no", "unsure")


def read_confirm(path):
    """The confirm file as a dict. Exit with a message on a bad file."""
    try:
        data = json.loads(read(path) or "null")
    except ValueError as e:
        sys.exit("scan_godot: the confirm file %s is not JSON: %s" % (path, e))
    stop_if(not isinstance(data, dict), "scan_godot: the confirm file %s needs a JSON object" % path)
    bad = sorted(set(data) - set(CONFIRM_KEYS))
    stop_if(bool(bad), "scan_godot: the confirm file has unknown keys %s; the keys are %s" % (bad, ", ".join(CONFIRM_KEYS)))
    return data


def _confirm_system(s, edit):
    stop_if(edit.get("found") not in FOUND_VALUES + (None,), "scan_godot: confirm %s: found must be yes, no or unsure"
            % s["id"])
    s.update({k: edit[k] for k in ("found", "variant") if k in edit})
    s["reason"] = "confirmed in the code: " + str(edit.get("reason") or "no reason given")
    return "%s found %s" % (s["id"], s["found"])


def confirm_map(catalog, edits):
    """Apply the confirm edits to the catalog map, in place. Return the list of the edits applied."""
    by = {s["id"]: s for s in catalog["systems"]}
    done = [_confirm_system(by[sid], e) for sid, e in sorted((edits.get("systems") or {}).items()) if _known(by, sid)]
    done += _confirm_block(catalog["reset"], edits, "reset")
    done += _confirm_block(catalog["currencies"], edits, "currencies")
    catalog["reset"]["currency"] = catalog["currencies"].get("reset") or catalog["reset"].get("currency")
    _confirm_no_offline(catalog, (edits.get("systems") or {}).get("S06") or {})
    _core_again(catalog)
    catalog["confirmed"] = done + list(edits.get("notes") or [])
    return done


def _confirm_no_offline(catalog, edit):
    """A confirmed S06 "no" clears the offline rule of the map and its out of range rows: the game has no offline
    gain, so no offline number applies."""
    if edit.get("found") != "no":
        return
    catalog["offline"] = dict(OFFLINE_NONE, found=False, function=None, rate_reason="the confirm file sets S06 to no")
    catalog["out_of_range"] = [r for r in catalog.get("out_of_range") or [] if not r["variant"].startswith("S06.")]


def _confirm_block(target, edits, key):
    """Update target with edits[key]. A confirmed block of currencies is sure: its `unsure` goes."""
    block = edits.get(key) or {}
    target.update(block)
    _ = target.pop("unsure", None) if block and key == "currencies" else None
    return ["%s: %s" % (key, ", ".join(sorted(block)))] if block else []


def _core_again(catalog):
    systems = catalog["systems"]
    catalog["missing_core"] = missing_core(systems, catalog.get("pattern"))
    catalog["missing_core_by_pattern"] = {pid: missing_core(systems, pid) for pid in PATTERN_IDS}


def _known(by, ident):
    stop_if(ident not in by, "scan_godot: the confirm file names %r, which the map or the content file does not hold"
            % ident)
    return True


def confirm_content(content, edits):
    """Apply the confirm edits to the content file, in place."""
    _confirm_block(content["currencies"], edits, "currencies")
    _confirm_spine(content, edits)
    by = {e["id"]: e for e in content["elements"]}
    gone = {i for i in edits.get("remove") or [] if _known(by, i)}
    _change_elements(by, edits.get("change") or {})
    _confirm_reset(by.get("reset"), edits.get("reset") or {})
    _confirm_end(content, edits.get("end"))
    content.update({k: edits[k] for k in COPY_KEYS if k in edits})
    content["elements"] = [e for e in content["elements"] if e["id"] not in gone] + list(edits.get("add") or [])
    content["confirmed"] = _content_log(edits, gone)


def _confirm_spine(content, edits):
    """Apply stats and spine_stat. A confirmed spine stat is sure: its `spine_unsure` goes."""
    stats = edits.get("stats") or {}
    content["stats"].update(stats)
    content.update({"spine_stat": edits["spine_stat"]} if "spine_stat" in edits else {})
    sure = "spine_stat" in edits or content.get("spine_stat") in stats
    _ = content.pop("spine_unsure", None) if sure else None


def _confirm_end(content, mark):
    """The end of the game from the confirm file: 1 key of END_KEYS. A confirmed end is sure."""
    if mark is None:
        return
    keys = [k for k in END_KEYS if k in (mark if isinstance(mark, dict) else {})]
    stop_if(len(keys) != 1 or len(mark) != 1, "scan_godot: confirm end needs 1 key of %s" % ", ".join(END_KEYS))
    content["end"] = {"mark": dict(mark), "reason": "confirmed in the confirm file", "place": None, "sure": True,
                      "confirm": None}


def _change_elements(by, changes):
    """Apply each change to its element. The keys of values add to the values of the scan, so a change of 1 value
    keeps the others (for example the reset rule and the most resets)."""
    rows = sorted(changes.items())
    for ident, change in rows:
        _ = _known(by, ident) and _change_one(by[ident], change)


def _change_one(el, change):
    values = dict(el.get("values") or {}, **(change.get("values") or {}))
    el.update(change)
    _ = el.update(values=values) if values else None


def _confirm_reset(el, r):
    if not el:
        return
    el.update({k: r[v] for k, v in (("value", "p"), ("currency", "currency"), ("family", "gain_family")) if v in r})
    el.update({"place": r["function"]} if r.get("function") else {})


def _content_log(edits, gone):
    log = ["removed: " + ", ".join(sorted(gone))] if gone else []
    log += ["added: " + ", ".join(str(e.get("id")) for e in edits.get("add") or [])] if edits.get("add") else []
    log += ["changed: " + ", ".join(sorted(edits["change"]))] if edits.get("change") else []
    log += ["%s: %s" % (k, json.dumps(edits[k], sort_keys=True)) for k in ("currencies", "stats", "spine_stat", "reset",
                                                                         "end") if k in edits]
    return log + list(edits.get("notes") or [])


def stop_if(bad, msg):
    """Exit with msg when bad is true."""
    if not bad:
        return
    sys.exit(msg)


def project_weight(d):
    """Shallowest first, then the one with more in it."""
    info = project_info(d)
    return d.count(os.sep), -(len(info["autoloads"]) + bool(info["main_scene"])), d


def choose_root(asked):
    """(root, found): the project to scan and each project folder under asked, in order of weight. With none, the
    projects in hidden folders (a git worktree in .worktrees/) count, and the report names them."""
    stop_if(not os.path.isdir(asked), f"not a folder: {asked}")
    found = find_projects(asked) or find_projects(asked, hidden=True)
    stop_if(not found, f"no project.godot under {asked}, also in hidden folders")
    found.sort(key=project_weight)
    return (asked if asked in found else found[0]), found


def parse_args():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("project")
    ap.add_argument("--out")
    ap.add_argument("--max", type=int, default=40, help="rows to keep in each ranked list of the JSON output")
    ap.add_argument("--systems-out", help="write the catalog map of the 29 systems as JSON to this file")
    ap.add_argument("--content-out", help="write the content file of the game (references/reverse-engineering.md)")
    ap.add_argument("--pattern", choices=PATTERN_IDS, help="the play pattern: it sets the core systems of the map")
    ap.add_argument("--confirm", help="a confirm file of corrections to apply to the map and the content file")
    ap.add_argument("--exclude", action="append", default=[], metavar="FOLDER",
                    help="a folder of the project (relative to its root) that is not a part of the game; repeat it")
    return ap.parse_args()


def write_json(path, result):
    fh = open(path, "w", encoding="utf-8")
    with fh:
        json.dump(result, fh, indent=1)


def main():
    a = parse_args()
    asked = os.path.abspath(a.project)
    root, found = choose_root(asked)
    scan = Scan(root, project_info(root))
    for path in walk(root, a.exclude):
        scan.scan_file(path)
    result = build_result(scan, found, a.max)
    edits = read_confirm(a.confirm) if a.confirm else None
    cmap = CatalogMap(scan, a.pattern)
    catalog = cmap.build()
    content = ContentScan(cmap.idx, catalog, cmap.model).build() if a.content_out else None
    _apply_confirm(edits, catalog, content)
    if a.content_out:
        write_json(a.content_out, content)
    if a.out:
        write_json(a.out, result)
    if a.systems_out:
        write_json(a.systems_out, catalog)
    report(result, scan.counts, asked, a.out, catalog)


def _apply_confirm(edits, catalog, content):
    if not edits:
        return
    confirm_map(catalog, edits)
    if content is not None:
        confirm_content(content, edits)


def print_lines(lines):
    """Print each line, as print() does. An empty list prints nothing."""
    sys.stdout.write("".join(line + "\n" for line in lines))


TOOLING_NOTE = ("These names look like a simulator, diagnostics, balance tests, or tuning targets. Open them: if the "
                "project has its own simulator, it is a better source of truth than a new model.")
HOOK_TABLE_HEAD = ["| signal | kind | emitted in | handled in | feedback found | feedback missing |",
                   "| --- | --- | --- | --- | --- | --- |"]
HOOKS_NONE = "none found. Search for the functions that buy, unlock, and reset, and read what they call."
HOOKS_TAIL = "Feedback can also come from a direct call with no signal. Open the handlers before you report a gap."


def _chosen_lines(r, asked):
    if len(r["project_files_found"]) <= 1 and r["project_root"] == asked:
        return []
    head = f"project.godot files under {asked}: {len(r['project_files_found'])}. Chosen: {r['project_root']}"
    return [head] + [f"- {f}" for f in r["project_files_found"][:8]]


def _head_lines(r, counts, asked):
    p, nested = r["project"], r["nested_projects_skipped"]
    lines = [f"# {p['name']}  (Godot features: {p['features']})\nroot: {r['project_root']}"]
    lines += _chosen_lines(r, asked)
    lines += ["nested projects not scanned: " + ", ".join(nested[:6])] if nested else []
    lines.append(f"main scene: {p['main_scene']}")
    lines.append("files: " + ", ".join(f"{k or '(none)'} {n}" for k, n in counts.most_common(8)))
    return lines


def _signal_lines(r):
    v = r["economy_signal"]
    lines = [f"\n**Economy signal: {v['level']}** ({v['cost_formula_lines']} cost-formula lines, "
             f"{v['data_files_with_cost_and_growth']} data files with both a cost key and a growth key, "
             f"{v['unlock_words']} unlock words). {v['note'][0].upper() + v['note'][1:]}"]
    lines += [f"- {s['file']}:{s['line']}: {s['text'][:120]}" for s in v["cost_formula_samples"][:3]]
    lines.append("\nEach hit below is a lead to check in the file, not a fact. Tests and tools come last in each list.")
    return lines


def _autoload_lines(r):
    rows = "\n".join(f"- {x['name']}: {x['path']}" for x in r["project"]["autoloads"])
    return ["\n## Autoloads (global state lives here)", rows or "none"]


def _loose_tool_lines(loose):
    files = [f for f in loose if not TEST_FILE.search(f)]
    tests = [f for f in loose if f not in files]
    lines = ["- files: " + ", ".join(files[:8]) + (f" (+{len(files) - 8})" if len(files) > 8 else "")] if files else []
    lines += [f"- test files: {len(tests)} ({', '.join(tests[:2])}{', ...' if len(tests) > 2 else ''})"] if tests else []
    return lines


def _tooling_lines(r):
    tools = r["balance_tooling"]
    dirs = [t for t in tools if t["sample"]]
    lines = [f"\n## Existing balance tooling ({r['balance_tooling_total']} found)"]
    lines += [TOOLING_NOTE] if tools else []
    lines += [f"- {t['path']} ({t['files']} files: {', '.join(t['sample'][:6])})" for t in dirs[:6]]
    lines += _loose_tool_lines([t["path"] for t in tools if not t["sample"]])
    return lines + ([] if tools else ["none found"])


def _data_file_lines(i, d):
    mark = " [cost + growth]" if d["has_cost_key"] and d["has_growth_key"] else ""
    head = f"- {d['file']} ({d['bytes']} bytes, {d['economy_key_hits']} hits){mark}: {', '.join(d['economy_keys'][:8])}"
    return [head] + ([f"    {line}" for line in shape_text(d["shape"])] if i < 4 else [])


def _data_lines(r):
    lines = [f"\n## Data files with economy keys ({r['data_files_total']} found)"]
    for i, d in enumerate(r["data_files"][:10]):
        lines += _data_file_lines(i, d)
    return lines + ([] if r["data_files"] else ["none found"])


def _script_row(s):
    head = f"- {s['file']} ({s['lines']} lines): {s['math_lines']} math lines, {s['economy_declarations']} declarations"
    return [head] + [f"    L{m['line']}: {m['text'][:130]}" for m in s["sample_math"][:1]]


def _script_lines(r):
    lines = [f"\n## Scripts with economy math ({r['economy_scripts_total']} found)"]
    for s in r["economy_scripts"][:10]:
        lines += _script_row(s)
    return lines + ([] if r["economy_scripts"] else ["none found"])


def _override_lines(r):
    rows = r["scene_overrides"]
    if not rows:
        return []
    head = f"\n## Scene overrides of economy properties ({r['scene_overrides_total']} found)"
    return [head] + [f"- {s['file']}:{s['line']}: {s['node']}.{s['property']} = {s['value']}" for s in rows[:6]]


def _offline_line(r):
    rows = r["offline_progress_code"]
    off = [s for s in rows if not s["in_tests"]]
    found = "; ".join(f"{s['file']}:{s['line']} `{s['text'][:70]}` ({s['hits']})" for s in off[:3])
    in_tools = f" ({len(rows)} files in tests and tools)" if rows else ""
    return "- offline progress: " + (found or "none found in game code" + in_tools)


def _meta_lines(r):
    save = ", ".join(f"{s['file']} ({s['hits']})" for s in r["save_and_meta_code"][:5]) or "none found"
    unlock = ", ".join(f"{k} {n}" for k, n in list(r["unlock_vocabulary"].items())[:12]) or "none"
    telemetry = ", ".join(f"{s['file']} ({s['hits']})" for s in r["telemetry_code"][:4]) or "none found"
    return ["\n## Save, unlock, offline, telemetry", "- save and meta code: " + save, "- unlock words: " + unlock,
            _offline_line(r), "- telemetry: " + telemetry]


def _emit_cell(h):
    files = collections.OrderedDict()
    for e in h["emitted_in"]:
        files.setdefault(e["file"], []).append(str(e["line"]))
    em = "; ".join(f"{f}:{','.join(ls[:3])}" for f, ls in list(files.items())[:2]) or "not emitted"
    more = f" (+{len(files) - 2} files)" if len(files) > 2 else ""
    return em + more


def _handler_cell(h):
    handlers = h["handlers"]
    hs = "; ".join(f"{x['file']} {x['function']}" + ("" if x["resolved"] else " (body not found)")
                   for x in handlers[:3]) or "no handler"
    return hs + (f" (+{len(handlers) - 3})" if len(handlers) > 3 else "")


def _missing_cell(h):
    if not h["handlers"]:
        return "all (no handler)"
    return "all" if not h["feedback_found"] else ", ".join(h["feedback_missing"]) or "none"


def _hook_row(h):
    fb = ", ".join(c + ("*" if c in h["feedback_indirect"] else "") for c in h["feedback_found"]) or "none"
    return f"| {h['signal']} | {h['kind']} | {_emit_cell(h)} | {_handler_cell(h)} | {fb} | {_missing_cell(h)} |"


def _small_beats(handled):
    """Bigger beats with no more feedback than the purchase with the most feedback."""
    buy = [set(h["feedback_found"]) for h in handled if h["kind"] == "purchase" and h["feedback_found"]]
    if not buy:
        return []
    top = max(buy, key=len)
    return [h["signal"] for h in handled if h["kind"] in SMALL_BEATS and h["feedback_found"]
            and set(h["feedback_found"]) <= top]


def _gap_lines(hooks):
    handled = [h for h in hooks if h["handlers"]]
    bare = [h["signal"] for h in handled if not h["feedback_found"]]
    small = _small_beats(handled)
    lines = ["Handled with no feedback found: " + ", ".join(bare[:10])] if bare else []
    return lines + (["Bigger beats with no more feedback than a purchase: " + ", ".join(small[:10])] if small else [])


def _hook_table(hooks):
    shown = hooks[:14]
    lines = HOOK_TABLE_HEAD + [_hook_row(h) for h in shown]
    lines += [f"({len(hooks) - 14} more in the JSON)"] if len(hooks) > 14 else []
    indirect = any(h["feedback_indirect"] for h in shown)
    lines += ["`*` = found in a function that the handler calls, not in the handler itself."] if indirect else []
    return lines + _gap_lines(hooks) + [HOOKS_TAIL]


def _hook_lines(r):
    hooks = r["beat_hooks"]
    head = [f"\n## Beat hooks ({len(hooks)} signals with economy or unlock words)"]
    return head + (_hook_table(hooks) if hooks else [HOOKS_NONE])


def _juice_lines(r):
    juice = r["juice"]
    scripts = ", ".join(f"{k} {x['hits']}/{x['files']}" for k, x in juice.items() if x["hits"]) or "none found"
    tops = [f"    {k}: " + ", ".join(f"{f} {n}" for f, n in x["top"][:3]) for k, x in juice.items()
            if x["top"] and k in TOP_JUICE]
    nodes = ", ".join(f"{t} {x['count']}" for t, x in r["scene_feedback_nodes"].items()) or "none found"
    return ["\n## Feedback code in the whole project", "- scripts (hits/files): " + scripts] + tops + \
        ["- scene nodes: " + nodes]


def _feel_check_lines(x):
    if not x["hits"]:
        return [f"- {x['title']}: none found"]
    return [f"- {x['title']}: {x['total']} leads"] + [f"    {h['file']}:{h['line']}: {h['text'][:110]}"
                                                     for h in x["hits"][:3]]


def _feel_report_lines(r):
    lines = ["\n## Feel checks of the genre"]
    for x in r["feel_checks"].values():
        lines += _feel_check_lines(x)
    return lines


CATALOG_TAIL = ("Each line above is what the code shows. Confirm each unsure item in the code, then follow "
                "references/reverse-engineering.md. --systems-out writes the full map.")


def _system_cell(s):
    return s["id"] + " " + s["name"] + (" (%s)" % s["variant"] if s["variant"] else "")


def _unsure_cell(s):
    e = s["evidence"][0]
    return "%s %s (%s:%s `%s`)" % (s["id"], s["name"], e["file"], e["line"], e["text"][:50])


def _missing_core_cell(m):
    return "%s %s" % (m["id"], m["name"]) if m["kind"] == "system" else "slot %s (%s)" % (m["slot"], ", ".join(m["members"]))


def _r_cell(ln):
    return "%s %s" % (ln["id"], "%.3f" % ln["r"] if ln["r"] is not None else "-")


def _catalog_line_lines(lines):
    geo = [ln for ln in lines if ln["r"] is not None]
    return ["- purchase lines: %d (%d with geometric cost and value). r: %s" % (
        len(lines), len(geo), ", ".join(_r_cell(ln) for ln in geo[:12]) or "none")]


def _catalog_reset_line(r):
    if not r.get("gain_family"):
        return "- reset: no gain formula found"
    head = "- reset: %s gain, p = %s (%s); currency %s; " % (r["gain_family"], r["p"], r["gain_place"], r["currency"])
    reward_text = "reward %s, q = %s (%s)" % (r["multiplies"], r["q"], r["q_line"]) if r["q"] is not None else r["q_method"]
    clear = "; the run state clears in %s" % r["clear_function"] if r.get("clear_function") else ""
    return head + reward_text + clear + ("; unsure: %s" % r["unsure"] if r.get("unsure") else "")


def _catalog_offline_line(o):
    if o.get("cap_seconds") is None and o.get("rate") is not None:
        return "- offline: rate %s, no cap (%s)" % (o["rate"], o["function"])
    if o.get("cap_seconds") is None:
        return "- offline: no capped offline rule found"
    return "- offline: rate %s, cap %s h (%s)" % (o["rate"], o["cap_hours"], o["function"])


def _oor_cell(x):
    return "%s.%s = %s (range %s to %s)" % (x["variant"], x["name"], x["value"], x["min"], x["max"])


def _missing_lines(m):
    """1 line for the pattern of the scan, else 1 line for each play pattern, labelled."""
    rows = {m["pattern"]: m["missing_core"]} if m.get("pattern") else m.get("missing_core_by_pattern") or {}
    return ["- missing core (%s): %s" % (pid, "; ".join(_missing_core_cell(x) for x in rows[pid]) or "none")
            for pid in sorted(rows)]


def _currency_line(c):
    side = ", ".join(c.get("side") or []) or "none"
    tail = "; unsure: %s" % c["unsure"] if c.get("unsure") else ""
    return "- currencies: main %s, reset %s, side %s%s" % (c.get("main"), c.get("reset"), side, tail)


def _spine_line(sp):
    if not sp:
        return "- spine: no wall formula found"
    return "- spine: g = %s, %s steps, tests %s (%s)" % (sp["g"], sp.get("count") or "?", sp.get("stat") or "the income",
                                                        sp["place"])


def _source_lines(m):
    out = ["- model file: %s is in the model format of this skill (references/model-schema.md): the map reads its "
           "spine, reset, rewards and unlocks" % m["model_file"]] if m.get("model_file") else []
    out += ["- data left out: " + "; ".join("%s (%s)" % (x["file"], x["reason"]) for x in m["data_skipped"][:4])
            ] if m.get("data_skipped") else []
    return out + (["- confirmed: " + "; ".join(m["confirmed"])] if m.get("confirmed") else [])


def _catalog_system_lines(systems, missing):
    groups = {f: [s for s in systems if s["found"] == f] for f in ("yes", "unsure", "no")}
    lines = ["\n## Catalog map (%d systems: %d found, %d unsure, %d not found)" % (
        len(systems), len(groups["yes"]), len(groups["unsure"]), len(groups["no"]))]
    lines.append("- found: " + (", ".join(_system_cell(s) for s in groups["yes"]) or "none"))
    return lines + (["- unsure: " + "; ".join(_unsure_cell(s) for s in groups["unsure"])] if groups["unsure"] else [])


def _catalog_oor_line(oor):
    return "- out of range: %d%s" % (len(oor), (": " + "; ".join(_oor_cell(x) for x in oor[:6])) if oor else "")


def catalog_lines(m):
    """The report section "Catalog map"."""
    lines = _catalog_system_lines(m["systems"], m["missing_core"]) + _missing_lines(m) + _source_lines(m)
    lines += [_currency_line(m["currencies"]), _spine_line(m["spine"])] + _catalog_line_lines(m["lines"])
    lines += [_catalog_reset_line(m["reset"])] + _rule_lines(m["reset"], m.get("end"))
    lines += [_catalog_offline_line(m["offline"]), _catalog_oor_line(m["out_of_range"])]
    return lines + [CATALOG_TAIL]


def _rule_lines(r, end):
    """The reset rule, the most resets and the end, each with its place; then the confirm questions."""
    rule, most = r.get("rule"), r.get("max_resets")
    out = ["- reset rule: %s %s (%s, %s)%s" % (rule["policy"], rule["value"], rule["source"], rule["place"] or "no place",
                                              "; other rules: " + "; ".join(rule["others"]) if rule["others"] else "")
           ] if rule else []
    out += ["- most resets: %s" % ("%d (%s)" % (most["value"], most["place"]) if most else "no limit found")] if rule else []
    out += ["- end: %s (%s, %s)" % (json.dumps(end["mark"]), end["reason"], end["place"] or "no place")] if end else []
    asks = [x["confirm"] for x in (rule, most, end) if x and x.get("confirm")]
    return out + ["- confirm: " + a for a in asks]


REPORT_SECTIONS = (_signal_lines, _autoload_lines, _tooling_lines, _data_lines, _script_lines, _override_lines,
                   _meta_lines, _hook_lines, _juice_lines, _feel_report_lines)


def report(r, counts, asked, out_path, catalog):
    print_lines(_head_lines(r, counts, asked))
    for section in REPORT_SECTIONS:
        print_lines(section(r))
    print_lines(catalog_lines(catalog))
    print_lines([f"\nfull result: {out_path}"] if out_path else [])


if __name__ == "__main__":
    main()
