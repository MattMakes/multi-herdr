"""From a fitted model to a self-contained Python recipe module.

The module holds the model as a readable literal, the knob ranges and the
identity values, and renders through ``hark.fit.render``. At ``DEFAULTS`` and a
given seed it renders the same samples as the engine, bit for bit.
"""

from __future__ import annotations

import importlib.util
import itertools
import math
import os
import re
from types import ModuleType
from typing import Any

import numpy as np

DROP = ("fit_engine",)
WIDTH = 116
MARGIN = 6

_NOTES = {
    "strikes": "one entry per strike: time, mode ramp, damped modes and contact modes",
    "modes": "rows [Hz, tau s, amplitude, phase rad] from the strike time",
    "attack": "contact as fast modes, rows [Hz, tau s, amplitude, phase rad]",
    "noise": "band-noise power envelopes, [t s, dB] breakpoints per band",
    "sub": "residual under 20 Hz, sampled at 100 Hz",
    "side": "stereo side: per band ratio to the mid, or a pan and diffuse model",
    "partials": "tracked partials, strongest first: life, phase, frequency and level curves",
    "transients": "strike atoms per strike, rows [Hz, tau s, amplitude, phase rad]",
    "residual": "third-octave noise levels per keyframe (-150 dB is off)",
    "env": "keyframe times and band levels in dB",
    "stereo": "per keyframe and band group: level difference and correlation",
    "texture": "grain and amplitude-modulation texture of the noise",
    "ridges": "tonal ridges in the noise: time, Hz, dB, jitter",
    "layers": "hybrid layers in order: role, gain and the layer model",
}

_CHARACTER_WORDS = (
    (lambda c: c.get("gliding"), "a short round pop with a pitch glide"),
    (lambda c: c.get("tonal") and c.get("t40_ms", 0) > 400, "a bright ringing chime"),
    (lambda c: c.get("tonal"), "a short pitched ding"),
    (lambda c: c.get("impulsive") and c.get("dur_s", 1) < 0.5, "a short crisp click"),
    (lambda c: c.get("impulsive"), "a deep hit with a long tail"),
    (lambda c: c.get("noisy"), "an air whoosh"),
)

_LICENCE = ("A fit derives from its recording: keep the source's attribution and licence with this file, and prefer "
            "CC0 sources for anything you ship.")


# ---------------------------------------------------------------- model literal


def _scalar(v: Any) -> str | None:
    """Source text of a scalar, or None when v is not a scalar."""
    if isinstance(v, (bool, np.bool_)):
        return "True" if v else "False"
    if isinstance(v, (int, np.integer)):
        return str(int(v))
    if isinstance(v, (float, np.floating)):
        f = float(v)
        if math.isnan(f):
            return "float('nan')"
        if math.isinf(f):
            return "float('inf')" if f > 0 else "float('-inf')"
        return repr(f)
    if v is None or isinstance(v, str):
        return repr(v)
    return None


def _wrap(items: list, ind: int) -> str:
    one = "[" + ", ".join(items) + "]"
    if len(one) + ind <= WIDTH:
        return one
    lines, cur = [], ""
    for it in items:
        if cur and len(cur) + len(it) + ind + MARGIN > WIDTH:
            lines.append(cur.rstrip())
            cur = ""
        cur += it + ", "
    if cur:
        lines.append(cur.rstrip())
    pad = " " * (ind + 4)
    return "[\n" + "\n".join(pad + ln for ln in lines) + "\n" + " " * ind + "]"


def _seq(v: list | tuple, ind: int) -> str:
    if not v:
        return "[]"
    items = [_scalar(x) for x in v]
    if all(s is not None for s in items):
        return _wrap(items, ind)
    pad = " " * (ind + 4)
    return "[\n" + "".join(f"{pad}{_value(x, ind + 4)},\n" for x in v) + " " * ind + "]"


def _dict(d: dict, ind: int) -> str:
    if not d:
        return "{}"
    pad = " " * (ind + 4)
    out = ["{"]
    for k, v in d.items():
        text = _value(v, ind + 4)
        note = _NOTES.get(k) if isinstance(k, str) else None
        if note and "\n" in text:
            out.append(f"{pad}# {note}")
            out.append(f"{pad}{k!r}: {text},")
        elif note:
            out.append(f"{pad}{k!r}: {text},  # {note}")
        else:
            out.append(f"{pad}{k!r}: {text},")
    out.append(" " * ind + "}")
    return "\n".join(out)


def _value(v: Any, ind: int) -> str:
    s = _scalar(v)
    if s is not None:
        return s
    if isinstance(v, dict):
        return _dict(v, ind)
    if isinstance(v, np.ndarray):
        return _seq(v.tolist(), ind)
    if isinstance(v, (list, tuple)):
        return _seq(v, ind)
    raise TypeError(f"literal: cannot write a value of type {type(v).__name__}")


def literal(model: dict, name: str = "MODEL") -> str:
    """'<name> = <literal>' for the model without its fit_engine key."""
    return f"{name} = " + _value({k: v for k, v in model.items() if k not in DROP}, 0)


# ---------------------------------------------------------------- critic words


def _norm(p: str) -> str:
    return os.path.normcase(os.path.abspath(p))


def _category_of(source: str | None) -> str | None:
    if not source:
        return None
    try:
        from hark.forge.engine import refsets

        reg = refsets.load()
        want = _norm(source)
        for cat, entry in reg.get("categories", {}).items():
            files = list(entry.get("forge") or []) + list(entry.get("bench") or [])
            if any(_norm(f) == want for f in files):
                return cat
    except Exception:
        return None
    return None


def _recipe_words(category: str) -> tuple[str, list] | None:
    from hark.forge import recipes

    for name in recipes.names():
        try:
            mod = recipes.load(name)
        except Exception:
            continue
        if getattr(mod, "REFSET", None) != category:
            continue
        targets = getattr(mod, "TARGETS", None)
        target = targets[category] if isinstance(targets, dict) and category in targets else mod.TARGET
        return target, list(getattr(mod, "ALTS", None) or [])
    return None


def _category_words(category: str) -> str | None:
    try:
        from hark.forge.engine import refsets

        text = refsets.CATEGORIES.get(category)
    except Exception:
        return None
    return re.sub(r"\s*\([^()]*\)\s*$", "", text) if text else None


def _character_target(character: dict | None) -> str:
    c = character or {}
    for rule, words in _CHARACTER_WORDS:
        if rule(c):
            return words
    return "a sound effect"


def critic_words(source: str | None = None, category: str | None = None,
                 character: dict | None = None) -> tuple[str, list, str | None]:
    """(TARGET, ALTS, REFSET): the critic's words for a fitted recipe."""
    category = category or _category_of(source)
    if category:
        try:
            found = _recipe_words(category)
        except Exception:
            found = None
        if found is not None:
            return found[0], found[1], category
        desc = _category_words(category)
        if desc:
            return desc, [], category
    return _character_target(character), [], None


# ---------------------------------------------------------------- recipe text


def _escape(text: str) -> str:
    return text.replace('"""', "'''").replace("\\", "/")


def _source_name(source: str | None, model: dict) -> str:
    if source:
        return os.path.basename(str(source))
    return str(model.get("source") or "unknown")


def _docstring(name: str, model: dict, kind: str, source: str | None, refset: str | None, quality: dict | None,
               floor: dict | None, licence: str | None, first_knob: str | None) -> str:
    from hark.fit.pipeline import quality as Q
    from hark.fit.pipeline.render import summary

    lines = [f"{name}: a recipe that hark fit made from a recording ({kind} model).", ""]
    lines += [ln.rstrip() for ln in summary(model).splitlines()]
    src = f"Source: {_source_name(source, model)}" + (f" (refset {refset})" if refset else "")
    lines += ["", src, licence or _LICENCE]
    if quality is not None:
        lines += ["", "Measured: the original against this recipe at DEFAULTS and seed 0:", "  " + Q.text(quality, floor)]
    lines += ["", "Usage:", "  import numpy as np", "  y = render(DEFAULTS, np.random.default_rng(1))   # one take",
              "  y = render_seed(0)                              # the fit as measured"]
    if first_knob:
        lines.append(f"  y = render_seed(0, {first_knob}=SPACE[{first_knob!r}][1])   # a variation")
    lines += [f"  hark fit --forge {name}.py --budget 96 --dir made/{name}",
              "  As a forge recipe: copy this file into a recipe folder of hark/forge/recipes/.", "",
              "SPACE holds the knob search ranges. DEFAULTS is the identity: it re-renders the fit."]
    return _escape("\n".join(lines))


def _space_lines(knobs: dict) -> list:
    out = ["SPACE = {"]
    for k, (rng, _ident, desc) in knobs.items():
        out.append(f"    {k!r}: {_value(tuple(rng), 4)},  # {desc}")
    out.append("}")
    return out


def _refs_for(source: str | None, refs: list | None) -> list:
    if refs is not None:
        return [str(r) for r in refs]
    if source and os.path.exists(source):
        return [os.path.abspath(source).replace("\\", "/")]
    return []


_BODY = '''

def render(p, r):
    """One take: knob dict p (other keys ignored), numpy Generator r (one draw)."""
    q = dict(DEFAULTS)
    q.update({k: v for k, v in p.items() if k in DEFAULTS})
    seed = int(r.integers(0, 2**31 - 1))
    return _hark_fit.render(MODEL, seed=seed, **q)


def render_seed(seed=0, **knobs):
    """A render with an explicit seed. render_seed(0) is the fit as measured."""
    q = dict(DEFAULTS)
    q.update(knobs)
    return _hark_fit.render(MODEL, seed=seed, **q)


if __name__ == "__main__":
    import sys

    import soundfile

    _seed = int(sys.argv[1]) if len(sys.argv) > 1 else 0
    _out = sys.argv[2] if len(sys.argv) > 2 else __WAV__
    soundfile.write(_out, np.asarray(render_seed(_seed), dtype=np.float32), 48000, subtype="FLOAT")
    print(_out)
'''


def to_recipe(model: dict, name: str = "fitted", source: str | None = None, quality: dict | None = None,
              floor: dict | None = None, target: str | None = None, alts: list | None = None,
              refset: str | None = None, refs: list | None = None, licence: str | None = None) -> str:
    """The full source text of a recipe module."""
    from hark.fit.pipeline.render import identity, kind_of, knobs_of

    kind = kind_of(model)
    knobs = knobs_of(model)
    character = (model.get("fit_engine") or {}).get("character")
    t, a, cat = critic_words(source, refset, character)
    target = target or t
    alts = a if alts is None else list(alts)
    refset = refset or cat
    doc = _docstring(name, model, kind, source, refset, quality, floor, licence, next(iter(knobs), None))
    out = ['"""' + doc, '"""', "", "import numpy as np", "", "import hark.fit as _hark_fit", "",
           f"TARGET = {target!r}", f"ALTS = {alts!r}"]
    if refset:
        out.append(f"REFSET = {refset!r}  # the refset category of the critic's default references")
    out += [f"REFS = {_refs_for(source, refs)!r}", f"KIND = {kind!r}"]
    out += _space_lines(knobs)
    defaults = ", ".join(f"{k!r}: {_value(v, 0)}" for k, v in identity(model).items())
    out.append("DEFAULTS = {" + defaults + "}  # these values re-render the fit")
    out.append(literal(model))
    return "\n".join(out) + _BODY.replace("__WAV__", repr(name + ".wav"))


# ---------------------------------------------------------------- files


def clean_name(path: str) -> str:
    """The file's stem with every non-word character replaced by '_'."""
    stem = os.path.splitext(os.path.basename(str(path)))[0]
    return re.sub(r"\W", "_", stem)


def write_recipe(path: str, model: dict, **kw: Any) -> str:
    """Write to_recipe(model, **kw) to path and return path."""
    kw.setdefault("name", clean_name(path))
    folder = os.path.dirname(os.path.abspath(str(path)))
    os.makedirs(folder, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        f.write(to_recipe(model, **kw))
    return path


_COUNTER = itertools.count()


def load_recipe(path: str) -> ModuleType:
    """Import a recipe file as a new, independent module object."""
    name = clean_name(path)
    spec = importlib.util.spec_from_file_location(f"_hark_fit_recipe_{next(_COUNTER)}_{name}", str(path))
    if spec is None or spec.loader is None:
        raise ImportError(f"load_recipe: cannot import {path}")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    mod.__name__ = name
    return mod
