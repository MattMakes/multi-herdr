#!/usr/bin/env python3
"""Inventory the progression data and the feedback hooks of a Godot project.

    python3 scan_godot.py <project_dir> [--out scan.json] [--max 40]

The script does not understand the game. It shows where to read: the autoloads, the data
files with economy keys, the GDScript lines with cost and growth math, the project's own
balance tooling, the save and unlock code, the signals that mark a beat with their handlers
and the feedback in each handler, and the feel checks of the genre. Each hit is a lead to
check, not a fact. Read the files that it ranks highest, then write the model by hand
(references/model-schema.md).

stdout is a short markdown report with a cap on each list. --out holds the full data.
"""
import argparse
import bisect
import collections
import json
import os
import re
import sys

SKIP_DIRS = {"addons", "node_modules", "build", "builds", "export", "exports", "dist", "bin", "obj", "tmp", "temp",
             "out", "output", "logs", "log", "cache", "ai_docs", "__pycache__", "coverage", "screenshots",
             "test_output", "test-results"}
SKIP_PREFIX = ("external", "third_party", "thirdparty", "vendor")
SKIP_FILES = {"export_presets.cfg", "override.cfg", "default_bus_layout.tres"}
TESTS = re.compile(r"(^|/)(tests?|tools|fixtures|testing|spec|bench\w*|qa)/|(^|/)test_[^/]*$|_test\.\w+$", re.I)

# Economy words match as whole tokens of a snake_case or camelCase name: `cost_x`, `maxLevel`, `hp`.
# `export_filter`, `expand_margin`, `scale_w`, and `caption` do not match.
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
# Offline progress: an offline word, or the wall clock near a subtraction. The word `away` alone is not evidence.
OFFLINE = re.compile(r"\boffline_?\w*|\bidle_gain\w*|\bcatch_?up\w*|\bwelcome_?back\w*|\bwhile_(?:you_were_)?away\w*", re.I)
OFFLINE_GUARD = ("offline", "idle_gain", "catch_up", "catchup", "welcome", "while_")
ASSIGN = re.compile(r"(\w+)\s*(?::\s*\w+\s*)?:?=")

# Patterns run on lower-case text.
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

# Signals that mark a beat. The kind is a guess from the name.
BEAT_KINDS = [
    ("unlock", re.compile(r"unlock|discover|reveal")),
    ("purchase", re.compile(r"purchas|bought|(?:^|_)buy|upgrad")),
    ("reset", re.compile(r"prestig|rebirth|reborn|ascen|run_(?:ended|finished|over|complete|won|lost)|game_over")),
    ("level up", re.compile(r"level(?:ed|led)?_?up|leveled|rank_?up|tier_(?:up|changed|reached)|evolv|promot")),
    ("milestone", re.compile(r"milestone|achievement|new_(?:record|best|high)|quest_(?:done|complete)")),
    ("ladder step", re.compile(r"cleared|complet|defeat|victory|(?:stage|wave|depth|floor|zone)_(?:reached|won|up|done)")),
    ("reward", re.compile(r"reward|payout|earned|loot_(?:granted|dropped)")),
]
NOT_BEAT = re.compile(r"request|pressed|clicked|refused|denied|failed|hovered|selected|focus")  # UI intent
KIND_ORDER = {k: i for i, k in enumerate(["purchase", "unlock", "ladder step", "level up", "milestone", "reward", "reset"])}
TOOLING = re.compile(r"(?:^|[_\-./])(?:sim|sims|simulator|simulation|simulate|balance|balancing|diag|diagnostics?|"
                     r"tune|tuning|tuner|calibrat\w*|polic(?:y|ies)|sweeps?|autoplay\w*|playstyle\w*|headless|"
                     r"full_run|fun_targets|(?:\w+_)?targets)(?:[_\-./]|$)", re.I)
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


def norm(name):
    return re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", name).lower()


def is_econ(name):
    return ECON_TOK.search(norm(name)) is not None


def skip_dir(name):
    low = name.lower()
    return name.startswith(".") or low in SKIP_DIRS or low.startswith(SKIP_PREFIX)


def find_projects(root):
    """All project.godot files under root, hidden and skipped folders left out."""
    hits = []
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if not x.startswith(".") and x not in ("addons", "node_modules")]
        if "project.godot" in files:
            hits.append(d)
    return hits


def walk(root):
    """Files of one project. A folder with its own project.godot is another project: it is not entered."""
    nested = []
    for d, dirs, files in os.walk(root):
        keep = []
        for x in sorted(dirs):
            if skip_dir(x):
                continue
            if os.path.exists(os.path.join(d, x, "project.godot")):
                nested.append(os.path.join(d, x))
                continue
            keep.append(x)
        dirs[:] = keep
        for f in files:
            yield os.path.join(d, f)
    walk.nested = nested


def read(path, limit=2_000_000):
    try:
        with open(path, "r", encoding="utf-8", errors="replace") as fh:
            return fh.read(limit)
    except OSError:
        return ""


def project_info(root):
    text = read(os.path.join(root, "project.godot"))
    name = re.search(r'config/name="([^"]*)"', text)
    feat = re.search(r"config/features=PackedStringArray\(([^)]*)\)", text)
    main = re.search(r'run/main_scene="([^"]*)"', text)
    autoloads = []
    block = re.search(r"\[autoload\](.*?)(?:\n\[|\Z)", text, re.S)
    if block:
        for line in block.group(1).strip().splitlines():
            m = re.match(r'(\w+)="\*?(res://[^"]+)"', line.strip())
            if m:
                autoloads.append({"name": m.group(1), "path": m.group(2)})
    return {"name": name.group(1) if name else None, "features": feat.group(1) if feat else None,
            "main_scene": main.group(1) if main else None, "autoloads": autoloads}


def item_keys(v):
    """Keys of the items of a container: first items of a list, first values of a dict of dicts."""
    items = v[:5] if isinstance(v, list) else list(v.values())[:5] if isinstance(v, dict) else []
    keys = []
    for it in items:
        if isinstance(it, dict):
            keys += [k for k in it if k not in keys]
    return keys[:14]


def describe(name, v):
    if isinstance(v, list):
        return {"key": name, "type": "array", "size": len(v), "item_keys": item_keys(v)}
    if isinstance(v, dict):
        ik = item_keys(v)
        return {"key": name, "type": "object", "size": len(v), "item_keys": ik,
                "keys": [] if ik else [str(k) for k in list(v)[:14]]}
    return {"key": name, "type": type(v).__name__, "size": 1, "item_keys": []}


def json_shape(obj):
    """Each top-level key with its type, size, and item keys."""
    if isinstance(obj, dict):
        return {"type": "object", "size": len(obj), "top_keys": [describe(str(k), v) for k, v in list(obj.items())[:40]]}
    if isinstance(obj, list):
        return {"type": "array", "size": len(obj), "top_keys": [describe("(root)", obj)]}
    return {"type": type(obj).__name__, "size": 1, "top_keys": []}


def shape_text(s, cap=6):
    out = []
    tops = s.get("top_keys", [])
    scalars = [t["key"] for t in tops if t["type"] not in ("array", "object")]
    tops = [t for t in tops if t["type"] in ("array", "object")]
    for t in tops[:cap]:
        inner = t["item_keys"] or t.get("keys") or []
        tail = (" of {" + ", ".join(inner[:8]) + ("..." if len(inner) > 8 else "") + "}") if inner else ""
        out.append(f"{t['key']}: {t['type']}" + (f"[{t['size']}]" if t["type"] in ("array", "object") else "") + tail)
    if len(tops) > cap:
        out.append(f"(+{len(tops) - cap} more containers)")
    if scalars:
        out.append(f"{len(scalars)} scalar keys: " + ", ".join(scalars[:10]) + ("..." if len(scalars) > 10 else ""))
    return out


def keys_of(obj, out, depth=0):
    if depth > 6:
        return
    if isinstance(obj, dict):
        for k, v in obj.items():
            out[str(k)] += 1
            keys_of(v, out, depth + 1)
    elif isinstance(obj, list):
        for v in obj[:200]:
            keys_of(v, out, depth + 1)


def clock_hits(text):
    """Lines where the wall clock is subtracted: on the line, or through a variable in the next four lines."""
    out, lines = [], text.split("\n")
    for i, line in enumerate(lines):
        if "get_unix_time" not in line or line.lstrip().startswith("#"):
            continue
        hit = re.search(r"\s-\s", line) is not None
        m = ASSIGN.search(line)
        if not hit and m:
            v = re.escape(m.group(1))
            hit = re.search(r"\b%s\s+-\s|\s-\s+%s\b" % (v, v), "\n".join(lines[i + 1:i + 5])) is not None
        if hit:
            out.append((i + 1, line.strip()[:160]))
    return out


def line_hits(rx, text, cap=400, also=None):
    """(line number, line text) for each non-comment line with a match. One hit for each line."""
    out, last = [], -1
    for m in rx.finditer(text):
        start = text.rfind("\n", 0, m.start()) + 1
        if start == last:
            continue
        last = start
        end = text.find("\n", m.start())
        line = text[start:end if end >= 0 else len(text)].strip()
        if line.startswith(("#", "//")) or (also is not None and not also.search(line)):
            continue
        out.append((text.count("\n", 0, start) + 1, line[:160]))
        if len(out) >= cap:
            break
    return out


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


class Funcs:
    """Function bodies of the scripts, parsed on demand."""

    def __init__(self, texts):
        self.texts, self.cache = texts, {}

    def of(self, relp):
        if relp not in self.cache:
            text = self.texts.get(relp, "")
            self.cache[relp] = self._cs(text) if relp.endswith(".cs") else self._gd(text)
        return self.cache[relp]

    @staticmethod
    def _gd(text):
        lines, out = text.split("\n"), {}
        starts = [0]
        for ln in lines:
            starts.append(starts[-1] + len(ln) + 1)
        for m in FUNC.finditer(text):
            i = bisect.bisect_right(starts, m.start()) - 1
            indent = len(m.group(1).expandtabs(4))
            depth, j = 0, i
            while j < len(lines):                       # the signature can cover several lines
                depth += lines[j].count("(") - lines[j].count(")")
                j += 1
                if depth <= 0:
                    break
            while j < len(lines):
                ln = lines[j]
                s = ln.lstrip()
                if s and not s.startswith("#") and len(ln[:len(ln) - len(s)].expandtabs(4)) <= indent:
                    break
                j += 1
            out.setdefault(m.group(2), (i + 1, "\n".join(lines[i:j])))
        return out

    @staticmethod
    def _cs(text):
        out = {}
        for m in re.finditer(r"\b(?:void|int|bool|float|double|string|Task|\w+)\s+(\w+)\s*\([^;{)]*\)\s*\{", text):
            depth, k = 0, m.end() - 1
            while k < len(text):
                depth += text[k] == "{"
                depth -= text[k] == "}"
                k += 1
                if depth == 0:
                    break
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


def feedback_in(body, relp, name, ctx, depth=2, seen=None):
    """Feedback categories in a function body. Value: 'direct', or 'via <function>' for a call one or two steps down."""
    seen = seen if seen is not None else {(relp, name)}
    found = {}
    code = "\n".join(l for l in body.split("\n") if not l.lstrip().startswith(("#", "//")))
    low = code.lower()
    for cat, rx in FEEDBACK.items():
        if rx.search(low):
            found[cat] = "direct"
    for var, cat in ctx["typed"].get(relp, {}).items():
        if cat not in found and re.search(r"\b" + re.escape(var) + r"\.", code):
            found[cat] = "direct"
    if depth <= 0:
        return found
    own = ctx["funcs"].of(relp)
    calls = []
    for m in re.finditer(r"(?:\b(\w+)\.)?\b(\w+)\s*\(", code):
        obj, fn = m.group(1), m.group(2)
        if obj in (None, "self", "this") and fn in own:
            calls.append((relp, fn))
        elif obj in ctx["autoload_file"]:
            target = ctx["autoload_file"][obj]
            if fn in ctx["funcs"].of(target):
                calls.append((target, fn))
            if "sound" not in found and re.search(r"audio|sfx|sound|music", obj, re.I):
                found["sound"] = f"via {obj}.{fn}"
    for target, fn in calls:
        if (target, fn) in seen:
            continue
        seen.add((target, fn))
        sub = feedback_in(ctx["funcs"].of(target)[fn][1], target, fn, ctx, depth - 1, seen)
        for cat in sub:
            found.setdefault(cat, f"via {fn}")
    return found


def parse_handler(rest):
    """The handler named in the argument text of a connect call: ('lambda', None) or (object or None, method)."""
    rest = rest.strip()
    if rest.startswith("func"):
        return "lambda", None
    m = re.match(r"(?:new\s+)?Callable(?:\.From)?\(\s*(?:(\w+)\s*,\s*)?(?:&?\"(\w+)\"|nameof\((\w+)\)|(\w+))", rest)
    if m:
        obj = m.group(1)
        return (None if obj in (None, "self", "this") else obj), m.group(2) or m.group(3) or m.group(4)
    m = re.match(r"[\w.]+", rest)
    if not m:
        return None, None
    chain = [c for c in m.group(0).split(".") if c]
    if chain and chain[-1] in ("bind", "unbind", "bindv"):
        chain = chain[:-1]
    if not chain:
        return None, None
    obj = chain[-2] if len(chain) > 1 else None
    return (None if obj in ("self", "this") else obj), chain[-1]


def beat_kind(name):
    n = norm(name)
    if NOT_BEAT.search(n):
        return None
    for kind, rx in BEAT_KINDS:
        if rx.search(n):
            return kind
    return None


def build_beat_hooks(ctx, declared, emits, connects, scene_conns, in_tests):
    texts, funcs = ctx["texts"], ctx["funcs"]
    func_owner = None
    hooks = []
    for sig in sorted(declared):
        kind = beat_kind(sig)
        if not kind:
            continue
        handlers, test_handlers = [], 0
        for c in connects.get(norm(sig), []):
            if in_tests(c["file"]):
                test_handlers += 1
                continue
            h = {"file": c["file"], "function": None, "line": c["line"], "via": "connect", "resolved": False}
            obj, method = c["obj"], c["method"]
            if obj == "lambda":
                h.update(function="(lambda)", resolved=True)
                fb = feedback_in(lambda_body(texts[c["file"]], c["pos"]), c["file"], f"lambda@{c['line']}", ctx)
            else:
                target = c["file"] if obj is None else ctx["autoload_file"].get(obj)
                if (target is None or method not in funcs.of(target)) and method:
                    if func_owner is None:              # function name -> files that define it (production code)
                        func_owner = collections.defaultdict(list)
                        for f in texts:
                            if not in_tests(f):
                                for fn in funcs.of(f):
                                    func_owner[fn].append(f)
                    owners = func_owner.get(method, [])
                    target = owners[0] if len(owners) == 1 else target if target and method in funcs.of(target) else None
                h["function"] = method if obj is None else f"{obj}.{method}"
                if target and method in funcs.of(target):
                    line, body = funcs.of(target)[method]
                    h.update(file=target, function=method, line=line, resolved=True, connected_in=c["file"])
                    fb = feedback_in(body, target, method, ctx)
                else:
                    fb = {}
            h["feedback"] = fb
            handlers.append(h)
        for c in scene_conns.get(norm(sig), []):
            if in_tests(c["file"]):
                test_handlers += 1
                continue
            h = {"file": c["file"], "function": c["method"], "line": c["line"], "via": "scene", "resolved": False,
                 "feedback": {}}
            for script in c["scripts"]:
                if c["method"] in funcs.of(script):
                    line, body = funcs.of(script)[c["method"]]
                    h.update(file=script, line=line, resolved=True, connected_in=c["file"],
                             feedback=feedback_in(body, script, c["method"], ctx))
                    break
            handlers.append(h)
        for h in handlers:
            if h["resolved"]:
                stem = os.path.splitext(os.path.basename(h["file"]))[0].lower()
                for cat, rx in FILE_HINT.items():
                    if rx.search(stem):
                        h["feedback"].setdefault(cat, "via file name")
        handlers.sort(key=lambda h: (not h["feedback"], not h["resolved"]))
        uniq, seen = [], set()
        for h in handlers:
            key = (h["file"], h["function"], h["line"] if h["function"] == "(lambda)" else 0)
            if key not in seen:
                seen.add(key)
                uniq.append(h)
        found = [c for c in CATS if any(c in h["feedback"] for h in uniq)]
        em = [e for e in emits.get(norm(sig), []) if not in_tests(e["file"])]
        hooks.append({
            "signal": sig, "kind": kind,
            "declared_in": declared[sig],
            "emitted_in": em[:20], "emit_count": len(em),
            "handlers": uniq, "test_handlers": test_handlers,
            "feedback_found": found,
            "feedback_indirect": [c for c in found if not any(h["feedback"].get(c) == "direct" for h in uniq)],
            "feedback_missing": [c for c in CATS if c not in found],
        })
    hooks.sort(key=lambda h: (not h["handlers"] and not h["emit_count"], not h["handlers"], KIND_ORDER[h["kind"]],
                              h["signal"]))
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


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("project")
    ap.add_argument("--out")
    ap.add_argument("--max", type=int, default=40, help="rows to keep in each ranked list of the JSON output")
    a = ap.parse_args()
    asked = os.path.abspath(a.project)
    if not os.path.isdir(asked):
        sys.exit(f"not a folder: {asked}")
    found = find_projects(asked)
    if not found:
        sys.exit(f"no project.godot under {asked}")

    def weight(d):                                      # shallowest first, then the one with more in it
        info = project_info(d)
        return (d.count(os.sep), -(len(info["autoloads"]) + bool(info["main_scene"])), d)
    found.sort(key=weight)
    root = asked if asked in found else found[0]
    rel = lambda p: os.path.relpath(p, root).replace(os.sep, "/")
    in_tests = lambda f: TESTS.search(f) is not None
    info = project_info(root)
    autoload_file = {x["name"]: x["path"][len("res://"):] for x in info["autoloads"] if x["path"].endswith((".gd", ".cs"))}

    counts = collections.Counter()
    texts, typed = {}, {}
    scripts, data_files, save_code, offline, telemetry = [], [], [], [], []
    unlock_words, unlock_core = collections.Counter(), 0
    formula = []
    juice = {k: collections.Counter() for k in CATS}
    declared, emits, connects, scene_conns = {}, collections.defaultdict(list), collections.defaultdict(list), \
        collections.defaultdict(list)
    scene_nodes, scene_overrides = collections.defaultdict(collections.Counter), []
    feel = {k: [] for k in FEEL}
    tooling_files = []

    for path in walk(root):
        base = os.path.basename(path)
        ext = os.path.splitext(base)[1].lower()
        counts[ext] += 1
        relp = rel(path)
        if ext in TOOL_EXT and TOOLING.search(relp):
            tooling_files.append(relp)
        if base in SKIP_FILES:
            continue
        test = in_tests(relp)
        if ext in (".gd", ".cs"):
            text = read(path)
            texts[relp] = text
            n_lines = text.count("\n") + 1
            decl = [{"line": text.count("\n", 0, m.start(1)) + 1, "text": m.group(0).strip()[:160]}
                    for m in DECL.finditer(text) if is_econ(m.group(1))]
            math_lines = []
            for n, line in line_hits(MATH, text):
                names = [norm(x) for x in IDENT.findall(line)]
                if any(ECON_TOK.search(x) for x in names):
                    math_lines.append({"line": n, "text": line})
                    if not test and POW.search(line) and any(COST_TOK.search(x) for x in names):
                        formula.append({"file": relp, "line": n, "text": line})
            if decl or math_lines:
                scripts.append({"file": relp, "lines": n_lines, "economy_declarations": len(decl),
                                "math_lines": len(math_lines), "sample_declarations": decl[:8],
                                "sample_math": math_lines[:8]})
            h = len(SAVE.findall(text))
            if h:
                save_code.append({"file": relp, "hits": h, "save_name": SAVE_NAME.search(base) is not None})
            for w in UNLOCK.findall(text):
                w = w.lower()
                unlock_words[w] += 1
                if not test and UNLOCK_CORE.match(w):
                    unlock_core += 1
            low = text.lower()
            oh = line_hits(OFFLINE, text) if any(g in low for g in OFFLINE_GUARD) else []
            if "get_unix_time" in text:
                oh = sorted(set(oh + clock_hits(text)))
            if oh:
                offline.append({"file": relp, "hits": len(oh), "line": oh[0][0], "text": oh[0][1], "in_tests": test})
            h = len(TELEMETRY.findall(text))
            if h:
                telemetry.append({"file": relp, "hits": h})
            for k, rx in FEEDBACK.items():
                h = len(rx.findall(low))
                if h:
                    juice[k][relp] += h
            tv = {m.group(1): TYPE_CAT[m.group(2)] for m in TYPED_VAR.finditer(text)}
            if tv:
                typed[relp] = tv
            # signals
            for m in (SIGNAL_CS if ext == ".cs" else SIGNAL_GD).finditer(text):
                declared.setdefault(m.group(1), []).append({"file": relp, "line": text.count("\n", 0, m.start(1)) + 1})
            if "emit" in text or "Emit" in text:
                for m in EMIT.finditer(text):
                    name = m.group(1) or m.group(2) or m.group(3)
                    if beat_kind(name):
                        emits[norm(name)].append({"file": relp, "line": text.count("\n", 0, m.start()) + 1})
            if "onnect(" in text:
                for m in CONNECT_STR.finditer(text):
                    name = m.group(1) or m.group(2)
                    if beat_kind(name):
                        obj, method = parse_handler(m.group(3))
                        connects[norm(name)].append({"file": relp, "line": text.count("\n", 0, m.start()) + 1,
                                                     "pos": m.start(), "obj": obj, "method": method})
                for m in CONNECT_DOT.finditer(text):
                    if beat_kind(m.group(1)) and not m.group(2).lstrip().startswith(("\"", "&\"")):
                        obj, method = parse_handler(m.group(2))
                        connects[norm(m.group(1))].append({"file": relp, "line": text.count("\n", 0, m.start()) + 1,
                                                           "pos": m.start(), "obj": obj, "method": method})
            if ext == ".cs" and "+=" in text:
                for m in CONNECT_CS.finditer(text):
                    if beat_kind(m.group(1)):
                        connects[norm(m.group(1))].append({"file": relp, "line": text.count("\n", 0, m.start()) + 1,
                                                           "pos": m.start(), "obj": None, "method": m.group(2)})
            # feel checks
            if not test:
                for key, (_, rx, also) in FEEL.items():
                    feel[key] += [{"file": relp, "line": n, "text": t} for n, t in line_hits(rx, text, 40, also)]
                if ("afford" in low or "cost" in low or "price" in low) and AFFORD_SET.search(text):
                    lines = text.split("\n")
                    for i, line in enumerate(lines):
                        if AFFORD_SET.search(line) and not line.lstrip().startswith("#") and \
                                AFFORD_CMP.search("\n".join(lines[max(0, i - 5):i + 1])):
                            feel["affordability"].append({"file": relp, "line": i + 1, "text": line.strip()[:160]})
        elif ext == ".tscn":
            text = read(path)
            for m in re.finditer(r'^\[node name="[^"]*" type="(\w+)"', text, re.M):
                if SCENE_NODES.match(m.group(1)):
                    scene_nodes[m.group(1)][relp] += 1
            sc = None
            for m in SCENE_CONN.finditer(text):
                if beat_kind(m.group(1)):
                    if sc is None:
                        sc = [p[len("res://"):] for p in re.findall(r'path="(res://[^"]+\.(?:gd|cs))"', text)]
                        same = os.path.splitext(relp)[0] + ".gd"
                        sc = ([same] if same not in sc else []) + sc
                    scene_conns[norm(m.group(1))].append({"file": relp, "line": text.count("\n", 0, m.start()) + 1,
                                                          "method": m.group(4), "to": m.group(3), "scripts": sc})
            for m in re.finditer(r"^(\w+) = (-?\d[\d.e+-]*)$", text, re.M):
                if is_econ(m.group(1)):
                    head = text.rfind("\n[", 0, m.start())
                    node = re.match(r'\n\[node name="([^"]*)"', text[head:head + 200]) if head >= 0 else None
                    if node:
                        scene_overrides.append({"file": relp, "line": text.count("\n", 0, m.start()) + 1,
                                                "node": node.group(1), "property": m.group(1), "value": m.group(2)})
            if not test:
                feel["away_screen"] += [{"file": relp, "line": n, "text": t}
                                        for n, t in line_hits(FEEL["away_screen"][1], text, 10)] \
                    if "away" in text.lower() or "ffline" in text or "elcome" in text else []
        elif ext == ".json" and os.path.getsize(path) < 5_000_000:
            try:
                obj = json.loads(read(path, 5_000_000))
            except (ValueError, RecursionError):
                continue
            keys = collections.Counter()
            keys_of(obj, keys)
            rec = data_record(relp, os.path.getsize(path), keys, json_shape(obj))
            if rec:
                data_files.append(rec)
        elif ext in (".tres", ".cfg", ".csv"):
            text = read(path, 500_000)
            if ext == ".tres":
                head = text[:400]
                if 'type="Resource"' not in head and "script_class=" not in head:
                    continue                            # a theme, a style box, a material: engine data, not economy
            if ext == ".csv":
                names = [c.strip().strip('"') for c in text.split("\n", 1)[0].split(",")]
            else:
                names = re.findall(r"^\s*([A-Za-z_]\w*)\s*=", text, re.M)
            rec = data_record(relp, os.path.getsize(path), collections.Counter(names), {"type": ext[1:], "top_keys": []})
            if rec:
                data_files.append(rec)

    ctx = {"texts": texts, "funcs": Funcs(texts), "typed": typed, "autoload_file": autoload_file}
    hooks = build_beat_hooks(ctx, declared, emits, connects, scene_conns, in_tests)
    by_tests = lambda score: (lambda s: (in_tests(s["file"]), -score(s)))
    scripts.sort(key=by_tests(lambda s: s["math_lines"] * 3 + s["economy_declarations"]))
    data_files.sort(key=by_tests(lambda s: s["economy_key_hits"] * (4 if s["has_cost_key"] and s["has_growth_key"] else 1)))
    save_code.sort(key=by_tests(lambda s: s["hits"] + (100 if s["save_name"] else 0)))
    for lst in (offline, telemetry):
        lst.sort(key=by_tests(lambda s: s["hits"]))
    scene_overrides.sort(key=lambda s: (in_tests(s["file"]), s["file"], s["line"]))
    cg = [d["file"] for d in data_files if d["has_cost_key"] and d["has_growth_key"] and not in_tests(d["file"])]
    v = verdict(len(formula), len(cg), unlock_core)
    v["cost_formula_samples"] = formula[:10]
    v["cost_and_growth_files"] = cg[:10]

    # balance tooling: a matching folder is one row; other matching files are one row each. Performance tools are not
    # balance tools.
    not_balance = re.compile(r"(^|/)(perf\w*|profil\w*|bench\w*|art|audio|shaders?)/", re.I)
    folders, loose, inside = collections.Counter(), [], collections.defaultdict(list)
    for f in tooling_files:
        if not_balance.search(f):
            continue
        parts = f.split("/")
        hit = next((i for i, x in enumerate(parts[:-1]) if TOOLING.search("/" + x + "/")), None)
        if hit is not None:
            d = "/".join(parts[:hit + 1]) + "/"
            folders[d] += 1
            first = parts[hit + 1] + ("/" if len(parts) > hit + 2 else "")
            if first not in inside[d]:
                inside[d].append(first)
        elif os.path.splitext(f)[1] != ".json" or re.search(r"target|polic|tun|balanc|calibr|diag|sim", f, re.I):
            loose.append(f)
    is_test_file = lambda f: re.search(r"(^|/)tests?/|(^|/)test_", f) is not None
    named = lambda x: (TOOLING.search("/" + x) is None, x.startswith("test"), x)
    order = lambda d: (in_tests("x/" + d) and not d.startswith("tools/"), -folders[d])
    tooling = [{"path": d, "files": folders[d], "sample": sorted(inside[d], key=named)[:8]} for d in sorted(folders, key=order)] + \
              [{"path": f, "files": 1, "sample": []} for f in sorted(loose, key=lambda f: (is_test_file(f), f))]

    def juice_row(c):
        prod = collections.Counter({f: n for f, n in c.items() if not in_tests(f)})
        return {"files": len(prod), "hits": sum(prod.values()), "top": prod.most_common(5),
                "hits_in_tests_and_tools": sum(c.values()) - sum(prod.values())}
    feel_out = {k: {"title": FEEL[k][0], "total": len(v_), "hits": v_[:a.max]} for k, v_ in feel.items()}
    result = {
        "project_root": root, "project": info,
        "project_files_found": [os.path.join(d, "project.godot") for d in found],
        "nested_projects_skipped": [rel(d) for d in getattr(walk, "nested", [])],
        "file_counts": dict(counts.most_common(12)),
        "economy_signal": v,
        "balance_tooling": tooling[:a.max], "balance_tooling_total": len(tooling),
        "economy_scripts": scripts[:a.max], "economy_scripts_total": len(scripts),
        "data_files": data_files[:a.max], "data_files_total": len(data_files),
        "scene_overrides": scene_overrides[:a.max], "scene_overrides_total": len(scene_overrides),
        "save_and_meta_code": save_code[:15],
        "unlock_vocabulary": dict(unlock_words.most_common(30)),
        "offline_progress_code": offline[:10],
        "telemetry_code": telemetry[:10],
        "feedback_categories": CATS,
        "beat_hooks": hooks,
        "juice": {k: juice_row(c) for k, c in juice.items()},
        "scene_feedback_nodes": {t: {"count": sum(c.values()), "scenes": c.most_common(5)}
                                 for t, c in sorted(scene_nodes.items())},
        "feel_checks": feel_out,
    }
    if a.out:
        with open(a.out, "w", encoding="utf-8") as fh:
            json.dump(result, fh, indent=1)
    report(result, counts, asked, a.out)


def report(r, counts, asked, out_path):
    p, v = r["project"], r["economy_signal"]
    P = print
    P(f"# {p['name']}  (Godot features: {p['features']})\nroot: {r['project_root']}")
    if len(r["project_files_found"]) > 1 or r["project_root"] != asked:
        P(f"project.godot files under {asked}: {len(r['project_files_found'])}. Chosen: {r['project_root']}")
        for f in r["project_files_found"][:8]:
            P(f"- {f}")
    if r["nested_projects_skipped"]:
        P("nested projects not scanned: " + ", ".join(r["nested_projects_skipped"][:6]))
    P(f"main scene: {p['main_scene']}")
    P("files: " + ", ".join(f"{k or '(none)'} {n}" for k, n in counts.most_common(8)))
    P(f"\n**Economy signal: {v['level']}** ({v['cost_formula_lines']} cost-formula lines, "
      f"{v['data_files_with_cost_and_growth']} data files with both a cost key and a growth key, "
      f"{v['unlock_words']} unlock words). {v['note'][0].upper() + v['note'][1:]}")
    for s in v["cost_formula_samples"][:3]:
        P(f"- {s['file']}:{s['line']}: {s['text'][:120]}")
    P("\nEach hit below is a lead to check in the file, not a fact. Tests and tools come last in each list.")

    P("\n## Autoloads (global state lives here)")
    P("\n".join(f"- {x['name']}: {x['path']}" for x in p["autoloads"]) or "none")

    P(f"\n## Existing balance tooling ({r['balance_tooling_total']} found)")
    if r["balance_tooling"]:
        P("These names look like a simulator, diagnostics, balance tests, or tuning targets. Open them: if the project "
          "has its own simulator, it is a better source of truth than a new model.")
    dirs = [t for t in r["balance_tooling"] if t["sample"]]
    for t in dirs[:6]:
        P(f"- {t['path']} ({t['files']} files: {', '.join(t['sample'][:6])})")
    loose = [t["path"] for t in r["balance_tooling"] if not t["sample"]]
    files = [f for f in loose if not re.search(r"(^|/)tests?/|(^|/)test_", f)]
    tests = [f for f in loose if f not in files]
    if files:
        P("- files: " + ", ".join(files[:8]) + (f" (+{len(files) - 8})" if len(files) > 8 else ""))
    if tests:
        P(f"- test files: {len(tests)} ({', '.join(tests[:2])}{', ...' if len(tests) > 2 else ''})")
    if not r["balance_tooling"]:
        P("none found")

    P(f"\n## Data files with economy keys ({r['data_files_total']} found)")
    for i, d in enumerate(r["data_files"][:10]):
        mark = " [cost + growth]" if d["has_cost_key"] and d["has_growth_key"] else ""
        P(f"- {d['file']} ({d['bytes']} bytes, {d['economy_key_hits']} hits){mark}: {', '.join(d['economy_keys'][:8])}")
        if i < 4:
            for line in shape_text(d["shape"]):
                P(f"    {line}")
    if not r["data_files"]:
        P("none found")

    P(f"\n## Scripts with economy math ({r['economy_scripts_total']} found)")
    for s in r["economy_scripts"][:10]:
        P(f"- {s['file']} ({s['lines']} lines): {s['math_lines']} math lines, {s['economy_declarations']} declarations")
        for m in s["sample_math"][:1]:
            P(f"    L{m['line']}: {m['text'][:130]}")
    if not r["economy_scripts"]:
        P("none found")

    if r["scene_overrides"]:
        P(f"\n## Scene overrides of economy properties ({r['scene_overrides_total']} found)")
        for s in r["scene_overrides"][:6]:
            P(f"- {s['file']}:{s['line']}: {s['node']}.{s['property']} = {s['value']}")

    P("\n## Save, unlock, offline, telemetry")
    P("- save and meta code: " + (", ".join(f"{s['file']} ({s['hits']})" for s in r["save_and_meta_code"][:5]) or "none found"))
    P("- unlock words: " + (", ".join(f"{k} {n}" for k, n in list(r["unlock_vocabulary"].items())[:12]) or "none"))
    off = [s for s in r["offline_progress_code"] if not s["in_tests"]]
    P("- offline progress: " + ("; ".join(f"{s['file']}:{s['line']} `{s['text'][:70]}` ({s['hits']})" for s in off[:3])
                                or "none found in game code" + (f" ({len(r['offline_progress_code'])} files in tests "
                                                                "and tools)" if r["offline_progress_code"] else "")))
    P("- telemetry: " + (", ".join(f"{s['file']} ({s['hits']})" for s in r["telemetry_code"][:4]) or "none found"))

    hooks = r["beat_hooks"]
    P(f"\n## Beat hooks ({len(hooks)} signals with economy or unlock words)")
    if hooks:
        P("| signal | kind | emitted in | handled in | feedback found | feedback missing |")
        P("| --- | --- | --- | --- | --- | --- |")
        for h in hooks[:14]:
            files = collections.OrderedDict()
            for e in h["emitted_in"]:
                files.setdefault(e["file"], []).append(str(e["line"]))
            em = "; ".join(f"{f}:{','.join(ls[:3])}" for f, ls in list(files.items())[:2]) or "not emitted"
            if len(files) > 2:
                em += f" (+{len(files) - 2} files)"
            hs = "; ".join(f"{x['file']} {x['function']}" + ("" if x["resolved"] else " (body not found)")
                           for x in h["handlers"][:3]) or "no handler"
            if len(h["handlers"]) > 3:
                hs += f" (+{len(h['handlers']) - 3})"
            fb = ", ".join(c + ("*" if c in h["feedback_indirect"] else "") for c in h["feedback_found"]) or "none"
            miss = "all (no handler)" if not h["handlers"] else "all" if not h["feedback_found"] else \
                ", ".join(h["feedback_missing"]) or "none"
            P(f"| {h['signal']} | {h['kind']} | {em} | {hs} | {fb} | {miss} |")
        if len(hooks) > 14:
            P(f"({len(hooks) - 14} more in the JSON)")
        if any(h["feedback_indirect"] for h in hooks[:14]):
            P("`*` = found in a function that the handler calls, not in the handler itself.")
        handled = [h for h in hooks if h["handlers"]]
        bare = [h["signal"] for h in handled if not h["feedback_found"]]
        if bare:
            P("Handled with no feedback found: " + ", ".join(bare[:10]))
        buy = [set(h["feedback_found"]) for h in handled if h["kind"] == "purchase" and h["feedback_found"]]
        if buy:
            small = [h["signal"] for h in handled if h["kind"] in ("unlock", "reset", "milestone", "level up")
                     and h["feedback_found"] and set(h["feedback_found"]) <= max(buy, key=len)]
            if small:
                P("Bigger beats with no more feedback than a purchase: " + ", ".join(small[:10]))
        P("Feedback can also come from a direct call with no signal. Open the handlers before you report a gap.")
    else:
        P("none found. Search for the functions that buy, unlock, and reset, and read what they call.")

    P("\n## Feedback code in the whole project")
    P("- scripts (hits/files): " + (", ".join(f"{k} {x['hits']}/{x['files']}" for k, x in r["juice"].items() if x["hits"])
                                    or "none found"))
    for k, x in r["juice"].items():
        if x["top"] and k in ("tween", "particles", "sound", "popup/toast"):
            P(f"    {k}: " + ", ".join(f"{f} {n}" for f, n in x["top"][:3]))
    P("- scene nodes: " + (", ".join(f"{t} {x['count']}" for t, x in r["scene_feedback_nodes"].items()) or "none found"))

    P("\n## Feel checks of the genre")
    for k, x in r["feel_checks"].items():
        if not x["hits"]:
            P(f"- {x['title']}: none found")
            continue
        P(f"- {x['title']}: {x['total']} leads")
        for h in x["hits"][:3]:
            P(f"    {h['file']}:{h['line']}: {h['text'][:110]}")
    if out_path:
        P(f"\nfull result: {out_path}")


if __name__ == "__main__":
    main()
