"""The SUP rules of `intake scan` (spec 4.2): dependencies.

Manifests are parsed as data only (json, tomllib, configparser, regex). Nothing
in them is installed, resolved, imported or run.

SUP-04 compares a name with the bundled list `references/popular-packages.txt`
after PEP 503 style folding (lower case; `_` and `.` read as `-`). An exact
match is not a hit. Edit distance is optimal string alignment, so a swap of 2
neighbour letters is 1 edit. Names under 4 characters are not compared by
distance (too many short names are 1 edit apart); they still get the
confusable-character check.
"""

from __future__ import annotations

import configparser
import json
import posixpath
import re
import tomllib
from collections import namedtuple
from functools import lru_cache
from pathlib import Path

from . import evidence

Dep = namedtuple("Dep", "name spec line eco")  # eco: npm, pypi, cargo, go, gem, composer

POPULAR = Path(__file__).resolve().parents[2] / "references" / "popular-packages.txt"
VENDORED_DIRS = frozenset(("node_modules", "vendor", "site-packages", "third_party"))
REQUIREMENTS = re.compile(r"^requirements[\w.-]*\.txt$")
LOCKFILES = {
    "package.json": ("package-lock.json", "npm-shrinkwrap.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb", "bun.lock"),
    "pyproject.toml": ("poetry.lock", "uv.lock", "pdm.lock", "pylock.toml"),
    "Pipfile": ("Pipfile.lock",),
    "Cargo.toml": ("Cargo.lock",),
    "go.mod": ("go.sum",),
    "Gemfile": ("Gemfile.lock",),
    "composer.json": ("composer.lock",),
    "deno.json": ("deno.lock",),
}

_B = r"(?<![\w.-])"
RUNTIME_INSTALL = re.compile(
    _B + r"(?:npx|bunx|uvx)(?:\s+-\S+)*\s+[^\s|;&-]|" + _B + r"(?:pnpm|yarn)\s+dlx\b|" + _B + r"pipx\s+run\b"
    r"|" + _B + r"pip[\d.]*\s+install\b|\b-m\s+pip\s+install\b"
    r"|" + _B + r"(?:npm|pnpm)\s+(?:install|i|add)\b|" + _B + r"(?:yarn|bun)\s+add\b"
    r"|" + _B + r"(?:go|cargo|brew|gem)\s+install\b|" + _B + r"deno\s+(?:run|install)\b[^\n]*\bhttps?://"
    r"|\bimport\b[^\n]*?\bfrom\s*['\"]https?://|\bimport\s*\(?\s*['\"]https?://"
)
# The package arguments of an install line. Runners take 1 package; installers take many.
RUNNER = re.compile(_B + r"(npx|bunx|uvx|(?:pnpm|yarn)\s+dlx|pipx\s+run)((?:\s+[^\s|;&<>`)]+)*)")
INSTALLER = re.compile(_B + r"(pip[\d.]*\s+install|(?:npm|pnpm)\s+(?:install|i|add)|(?:yarn|bun)\s+add)((?:\s+[^\s|;&<>`)]+)*)")
PYPI_TOOLS = ("uvx", "pipx", "pip")
VALUE_FLAGS = frozenset(("-r", "--requirement", "-c", "--constraint", "-e", "--editable", "-i", "--index-url",
                         "--extra-index-url", "-f", "--find-links", "-p", "--package", "--from"))
LATEST = re.compile(r"@latest\b")
OTHER_INDEX = re.compile(r"--(?:extra-)?index-url\b|git\+ssh://")
REGISTRY_LINES = {
    ".npmrc": re.compile(r"^\s*(?:@[\w.-]+:)?registry\s*="),
    ".yarnrc": re.compile(r"^\s*\"?(?:@[\w.-]+:)?registry\"?\s"),
    ".yarnrc.yml": re.compile(r"^\s*(?:npmRegistryServer|npmScopes)\s*:"),
    "bunfig.toml": re.compile(r"^\s*registry\s*=|^\s*\[install\.scopes\]"),
    "Gemfile": re.compile(r"^\s*source\s+['\"](?!https://rubygems\.org/?['\"])"),
    "Pipfile": re.compile(r"^\s*url\s*=\s*['\"](?!https://pypi\.org/simple/?['\"])"),
    "Cargo.toml": re.compile(r"\bregistry(?:-index)?\s*="),
}
MCP_ADD = re.compile(r"\b(?:claude|codex|gemini)\s+mcp\s+add\b|\bmcp\s+add-json\b")
LFS_POINTER = "version https://git-lfs.github.com/spec/v1"
LICENCE_HEADER = re.compile(
    r"SPDX-License-Identifier:|@license\b|\bLicensed under the\b|Permission is hereby granted, free of charge"
    r"|This (?:program|library) is free software|\bCopyright\s+(?:\(c\)|\d{4})",
    re.I,
)
HEADER_EXTS = frozenset(".py .js .mjs .cjs .ts .tsx .jsx .css .sh .rb .go .rs .c .h .cc .cpp .java .php .lua".split())

HEX40 = re.compile(r"\b[0-9a-f]{40}\b")
URL = re.compile(r"^(?:@\s*)?(?:https?|ftp)://")
GIT = re.compile(r"^(?:@\s*)?(?:git\+|git://|git@|github:|gitlab:|bitbucket:)|\.git(?:#|@|$)")
NPM_SHORTHAND = re.compile(r"^[\w-]+/[\w.-]+(?:#.*)?$")
NPM_EXACT = re.compile(r"(?:npm:[^@\s]+@)?=?v?\d+\.\d+\.\d+(?:[-+][\w.+-]*)?")
PY_EXACT = re.compile(r"===?\s*[\w.+!-]+")
PLAIN_EXACT = re.compile(r"=?\s*v?\d+(?:\.\d+)*(?:[-.+][\w.]+)?")
PEP508 = re.compile(r"^\s*([A-Za-z0-9][A-Za-z0-9._-]*)\s*(?:\[[^\]]*\])?\s*(.*)$")


def hit(rule: str, severity: str, rel: str, line: int, text: str) -> dict:
    return {
        "rule": rule,
        "severity": severity,
        "file": evidence.escape(rel),
        "line": line,
        "evidence": evidence.untrusted(text.strip()),
        "malicious": False,
    }


def vendored_root(rel: str) -> str | None:
    """The outermost vendored folder that holds `rel` (SUP-07), or None."""
    parts = rel.split("/")
    for i, part in enumerate(parts[:-1]):
        if part in VENDORED_DIRS:
            return "/".join(parts[: i + 1])
    return None


def licence_header(rel: str, lines: list[str]) -> int:
    """The line of a third-party licence header in a code file (SUP-07), or 0."""
    if posixpath.splitext(rel)[1].lower() not in HEADER_EXTS:
        return 0
    for n, line in enumerate(lines[:30], 1):
        if LICENCE_HEADER.search(line):
            return n
    return 0


# ---- names (SUP-04) ---------------------------------------------------------

CONFUSABLE = {
    **{chr(cp): ch for cp, ch in (
        (0x430, "a"), (0x435, "e"), (0x43E, "o"), (0x440, "p"), (0x441, "c"), (0x443, "y"),
        (0x445, "x"), (0x456, "i"), (0x455, "s"), (0x458, "j"), (0x3BF, "o"), (0x3B1, "a"),
        (0x3BD, "v"), (0x3C1, "p"), (0x131, "i"),
    )},
    "0": "o", "1": "l", "3": "e", "5": "s",
}


def canonical(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name.strip().lower())


def skeleton(name: str) -> str:
    s = "".join(CONFUSABLE.get(c, c) for c in canonical(name))
    return s.replace("rn", "m").replace("vv", "w")


@lru_cache(maxsize=1)
def popular() -> tuple[frozenset, dict, tuple, frozenset]:
    """(names, skeleton -> name, [(name, char set)] of 4+ chars, npm scopes)."""
    names = []
    for line in POPULAR.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            names.append(canonical(line))
    skel = {skeleton(n): n for n in names}
    long = tuple((n, frozenset(n)) for n in names if len(n) >= 4)
    scopes = frozenset(n[1:].split("/")[0] for n in names if n.startswith("@") and "/" in n)
    return frozenset(names), skel, long, scopes


def distance(a: str, b: str, limit: int) -> int:
    """Optimal string alignment distance, or limit + 1 once it is over `limit`."""
    if abs(len(a) - len(b)) > limit:
        return limit + 1
    prev2: list[int] = []
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i] + [0] * len(b)
        for j, cb in enumerate(b, 1):
            v = min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb))
            if i > 1 and j > 1 and ca == b[j - 2] and a[i - 2] == cb:
                v = min(v, prev2[j - 2] + 1)
            cur[j] = v
        if min(cur) > limit:
            return limit + 1
        prev2, prev = prev, cur
    return prev[-1]


def typosquat(name: str) -> str | None:
    """Why `name` looks like a popular package it is not, or None."""
    names, skel, long, scopes = popular()
    c = canonical(name)
    if c in names:
        return None
    if any(ord(ch) > 0x7E for ch in c):
        return "confusable characters"
    near = skel.get(skeleton(c))
    if near:
        return f"lookalike of {near}"
    if c.startswith("@") and "/" in c:
        scope = c[1:].split("/")[0]
        for s in scopes:
            if scope != s and len(s) >= 3 and distance(scope, s, 1) <= 1:
                return f"scope lookalike of @{s}"
    if len(c) < 4:
        return None
    limit = 2 if len(c) > 8 else 1
    chars = frozenset(c)
    for n, nchars in long:
        if len(chars ^ nchars) <= 2 * limit and distance(c, n, limit) <= limit:
            return f"{distance(c, n, limit)} edit(s) from {n}"
    return None


# ---- manifests (SUP-01) -----------------------------------------------------


def _line_of(lines: list[str], name: str) -> int:
    pat = re.compile(r"(?<![\w.@/-])" + re.escape(name) + r"(?![\w-])")
    for n, line in enumerate(lines, 1):
        if pat.search(line):
            return n
    return 0


def _npm_split(spec: str) -> tuple[str, str]:
    """`@scope/name@^1` -> (`@scope/name`, `^1`)."""
    at = spec.rfind("@")
    return (spec[:at], spec[at + 1:]) if at > 0 else (spec, "")


def _table_spec(value) -> str:
    """A Poetry, Pipfile or Cargo dependency value as one spec string."""
    if not isinstance(value, dict):
        return str(value)
    if "git" in value:
        ref = value.get("rev") or value.get("ref") or value.get("tag") or value.get("branch") or ""
        return f"git+{value['git']}" + (f"@{ref}" if ref else "")
    if "path" in value or "file" in value:
        return "file:" + str(value.get("path") or value.get("file"))
    return str(value.get("url") or value.get("version") or "")


def _from_map(section, lines, eco, skip=()) -> list[Dep]:
    if not isinstance(section, dict):
        return []
    return [Dep(k, _table_spec(v), _line_of(lines, k), eco) for k, v in section.items() if k not in skip]


def _pep508_list(items, lines) -> list[Dep]:
    out = []
    for item in items if isinstance(items, list) else []:
        m = PEP508.match(item) if isinstance(item, str) else None
        if m:
            out.append(Dep(m.group(1), m.group(2).split(";")[0].strip(), _line_of(lines, m.group(1)), "pypi"))
    return out


def _package_json(text, lines):
    data = json.loads(text)
    out = []
    for key in ("dependencies", "devDependencies", "peerDependencies", "optionalDependencies"):
        out += _from_map(data.get(key), lines, "npm")
    return out


def _composer_json(text, lines):
    data = json.loads(text)
    out = []
    for key in ("require", "require-dev"):
        sec = data.get(key)
        if isinstance(sec, dict):
            out += [Dep(k, str(v), _line_of(lines, k), "composer") for k, v in sec.items()
                    if k != "php" and not k.startswith(("ext-", "lib-"))]
    return out


def _deno_json(text, lines):
    out = []
    imports = json.loads(text).get("imports")
    for alias, target in (imports.items() if isinstance(imports, dict) else []):
        target = str(target)
        if target.startswith("npm:"):
            name, ver = _npm_split(target[4:])
            out.append(Dep(name, ver, _line_of(lines, alias), "npm"))
        else:
            out.append(Dep(alias, target, _line_of(lines, alias), "deno"))
    return out


def _pyproject(text, lines):
    data = tomllib.loads(text)
    project = data.get("project", {})
    out = _pep508_list(project.get("dependencies"), lines)
    for group in (project.get("optional-dependencies") or {}).values():
        out += _pep508_list(group, lines)
    for group in (data.get("dependency-groups") or {}).values():
        out += _pep508_list(group, lines)
    out += _pep508_list(data.get("build-system", {}).get("requires"), lines)
    poetry = data.get("tool", {}).get("poetry", {})
    out += _from_map(poetry.get("dependencies"), lines, "pypi", ("python",))
    out += _from_map(poetry.get("dev-dependencies"), lines, "pypi")
    for group in (poetry.get("group") or {}).values():
        out += _from_map(group.get("dependencies") if isinstance(group, dict) else None, lines, "pypi")
    return out


def _pipfile(text, lines):
    data = tomllib.loads(text)
    return _from_map(data.get("packages"), lines, "pypi") + _from_map(data.get("dev-packages"), lines, "pypi")


def _cargo(text, lines):
    data = tomllib.loads(text)
    tables = [data, data.get("workspace", {})] + list((data.get("target") or {}).values())
    out = []
    for t in tables:
        for key in ("dependencies", "dev-dependencies", "build-dependencies"):
            out += _from_map(t.get(key) if isinstance(t, dict) else None, lines, "cargo")
    return out


def _requirements(text, lines):
    out = []
    for n, raw in enumerate(lines, 1):
        line = re.sub(r"(?:^|\s)#.*$", "", raw).strip().rstrip("\\").strip()
        editable = re.match(r"^(?:-e|--editable)\s+(\S+)", line)
        if editable:
            url = editable.group(1)
            egg = re.search(r"#egg=([\w.-]+)", url)
            out.append(Dep(egg.group(1) if egg else url, url, n, "pypi"))
            continue
        if not line or line.startswith("-"):
            continue
        if URL.match(line) or GIT.match(line):
            out.append(Dep(line, line, n, "pypi"))
            continue
        m = PEP508.match(line)
        if m:
            out.append(Dep(m.group(1), m.group(2).split(";")[0].strip(), n, "pypi"))
    return out


def _setup_py(text, lines):
    out = []
    for m in re.finditer(r"\b(?:install_requires|setup_requires|tests_require)\s*=\s*\[(.*?)\]", text, re.S):
        out += _pep508_list(re.findall(r"['\"]([^'\"]+)['\"]", m.group(1)), lines)
    return out


def _setup_cfg(text, lines):
    cfg = configparser.ConfigParser(interpolation=None)
    cfg.read_string(text)
    values = []
    if cfg.has_section("options"):
        values += [cfg.get("options", k) for k in ("install_requires", "setup_requires", "tests_require")
                   if cfg.has_option("options", k)]
    if cfg.has_section("options.extras_require"):
        values += [v for _, v in cfg.items("options.extras_require")]
    items = [x.strip() for v in values for x in v.splitlines() if x.strip()]
    return _pep508_list(items, lines)


def _go_mod(text, lines):
    out, block = [], False
    for n, line in enumerate(lines, 1):
        s = line.split("//")[0].strip()
        if s.startswith("require ("):
            block = True
            continue
        if block and s == ")":
            block = False
            continue
        m = re.match(r"^(?:require\s+)?(\S+)\s+(v\S+)$", s) if block or s.startswith("require ") else None
        if m:
            out.append(Dep(m.group(1), m.group(2), n, "go"))
    return out


def _gemfile(text, lines):
    out = []
    for n, line in enumerate(lines, 1):
        m = re.match(r"^\s*gem\s+['\"]([^'\"]+)['\"]\s*(?:,\s*(.*))?$", line)
        if not m:
            continue
        rest = m.group(2) or ""
        git = re.search(r"\b(?:git|github)\s*(?::|=>)\s*['\"]([^'\"]+)['\"]", rest)
        ref = re.search(r"\b(?:ref|tag|branch)\s*(?::|=>)\s*['\"]([^'\"]+)['\"]", rest)
        path = re.search(r"\bpath\s*(?::|=>)\s*['\"]([^'\"]+)['\"]", rest)
        if git:
            spec = f"git+{git.group(1)}" + (f"@{ref.group(1)}" if ref else "")
        elif path:
            spec = "file:" + path.group(1)
        else:
            spec = ",".join(re.findall(r"['\"]([~<>=!\d][^'\"]*)['\"]", rest))
        out.append(Dep(m.group(1), spec, n, "gem"))
    return out


PARSERS = {
    "package.json": _package_json, "composer.json": _composer_json, "deno.json": _deno_json,
    "pyproject.toml": _pyproject, "Pipfile": _pipfile, "Cargo.toml": _cargo, "setup.py": _setup_py,
    "setup.cfg": _setup_cfg, "go.mod": _go_mod, "Gemfile": _gemfile,
}


def manifest_deps(name: str, text: str, lines: list[str]) -> list[Dep] | None:
    """The declared dependencies of a manifest, or None when `name` is not one.
    A manifest that does not parse gives no dependency."""
    parser = _requirements if REQUIREMENTS.match(name) else PARSERS.get(name)
    if parser is None:
        return None
    try:
        return parser(text, lines)
    except (ValueError, TypeError, AttributeError, configparser.Error):
        return []


# ---- the rules ----------------------------------------------------------------


def unpinned(spec: str, eco: str) -> str | None:
    """Why a dependency spec is not pinned (SUP-03), or None."""
    s = spec.strip()
    if GIT.search(s) or (eco == "npm" and NPM_SHORTHAND.match(s)):
        return None if HEX40.search(s) else "git dependency without a commit SHA"
    if URL.match(s):
        return "URL dependency"
    if s.startswith(("file:", "link:", "workspace:", "portal:")) or eco == "go":
        return None
    if not s or s == "*":
        return "no version"
    exact = {"npm": NPM_EXACT, "pypi": PY_EXACT}.get(eco, PLAIN_EXACT)
    if eco == "cargo":
        return None if s.startswith("=") and PLAIN_EXACT.fullmatch(s) else "version range"
    return None if exact.fullmatch(s) else "version range"


def outside_tree(rel: str, spec: str) -> bool:
    """A `file:` dependency that points outside the quarantined tree (SUP-05)."""
    s = spec.strip()
    if not s.startswith("file:"):
        return False
    path = s[5:].lstrip()
    if path.startswith(("/", "~")):
        return True
    return posixpath.normpath(posixpath.join(posixpath.dirname(rel), path)).startswith("..")


def _dep_hits(rel: str, deps: list[Dep]) -> list[dict]:
    hits = []
    for d in deps:
        text = f"{d.name} {d.spec}".strip()
        hits.append(hit("SUP-01", "info", rel, d.line, text))
        why = unpinned(d.spec, d.eco)
        if why:
            hits.append(hit("SUP-03", "medium", rel, d.line, f"{text} ({why})"))
        if d.eco in ("npm", "pypi"):
            squat = typosquat(d.name)
            if squat:
                hits.append(hit("SUP-04", "high", rel, d.line, f"{d.name} ({squat})"))
        if "git+ssh://" in d.spec or d.spec.startswith("git@") or outside_tree(rel, d.spec):
            hits.append(hit("SUP-05", "high", rel, d.line, text))
    return hits


def _lockfile_hits(rel: str, name: str, deps: list[Dep], tree: frozenset | None) -> list[dict]:
    locks = LOCKFILES.get(name)
    if not locks or not deps or tree is None:
        return []
    folder = posixpath.dirname(rel)
    if any(posixpath.join(folder, lock) in tree for lock in locks):
        return []
    return [hit("SUP-03", "medium", rel, 0, f"{name}: no lockfile ({', '.join(locks)})")]


def install_names(line: str) -> list[tuple[str, str]]:
    """(package name, eco) for each package an install or runner line names."""
    out = []
    for pattern, many in ((RUNNER, False), (INSTALLER, True)):
        for m in pattern.finditer(line):
            eco = "pypi" if m.group(1).startswith(PYPI_TOOLS) else "npm"
            skip = False
            for token in m.group(2).split():
                if skip or token.startswith("-") or ":" in token or token.startswith((".", "/", "~", "$")):
                    skip = token in VALUE_FLAGS
                    continue
                name = _npm_split(token)[0] if eco == "npm" else re.split(r"[=<>!~\[;@]", token)[0]
                if name:
                    out.append((name, eco))
                if not many:
                    break
    return out


def _line_hits(rel: str, name: str, lines: list[str]) -> list[dict]:
    hits = []
    registry = REGISTRY_LINES.get(name)
    for n, line in enumerate(lines, 1):
        if RUNTIME_INSTALL.search(line):
            hits.append(hit("SUP-02", "high", rel, n, line))
            if LATEST.search(line):
                hits.append(hit("SUP-03", "medium", rel, n, line))
            for pkg, eco in install_names(line):
                squat = typosquat(pkg)
                if squat:
                    hits.append(hit("SUP-04", "high", rel, n, f"{pkg} ({squat})"))
        if OTHER_INDEX.search(line) or (registry and registry.search(line)):
            hits.append(hit("SUP-05", "high", rel, n, line))
        if MCP_ADD.search(line):
            hits.append(hit("SUP-06", "high", rel, n, line))
    return hits


def _wiring_hits(rel: str, name: str, lines: list[str]) -> list[dict]:
    """SUP-06 MCP or plugin wiring files and SUP-08 fetch-time indirection."""
    hits = []
    plugin = name == "plugin.json" or "/.claude-plugin/" in "/" + rel
    for n, line in enumerate(lines, 1):
        if (name == ".mcp.json" or plugin) and '"mcpServers"' in line:
            hits.append(hit("SUP-06", "high", rel, n, line))
        if name == ".gitattributes" and re.search(r"\bfilter=", line):
            hits.append(hit("SUP-08", "medium", rel, n, line))
    if name == ".mcp.json" and not hits:
        hits.append(hit("SUP-06", "high", rel, 0, ".mcp.json"))
    if name == ".gitmodules":
        hits.append(hit("SUP-08", "medium", rel, 0, ".gitmodules"))
    if lines and lines[0].strip() == LFS_POINTER:
        hits.append(hit("SUP-08", "medium", rel, 1, lines[0]))
    return hits


def file_hits(rel: str, name: str, text: str, lines: list[str], tree: frozenset | None) -> list[dict]:
    """Every SUP hit in one text file. `tree` (every path in the tree) enables
    the lockfile check. Manifests inside a vendored folder are not parsed: the
    folder is 1 SUP-07 item."""
    hits = _line_hits(rel, name, lines) + _wiring_hits(rel, name, lines)
    if vendored_root(rel) is None:
        deps = manifest_deps(name, text, lines)
        if deps is not None:
            hits += _dep_hits(rel, deps) + _lockfile_hits(rel, name, deps, tree)
        header = licence_header(rel, lines)
        if header:
            hits.append(hit("SUP-07", "high", rel, header, lines[header - 1]))
    return hits


def vendored_hits(rels) -> list[tuple[dict, str]]:
    """(SUP-07 hit, root) for each vendored folder in the tree."""
    roots = sorted({r for r in map(vendored_root, rels) if r})
    return [(hit("SUP-07", "high", root, 0, root + "/"), root) for root in roots]
