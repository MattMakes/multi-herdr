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

_NOTE_TEXT = {
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

_CHAR_WORDS = (
    (lambda cv: cv.get("gliding"), "a short round pop with a pitch glide"),
    (lambda cv: cv.get("tonal") and cv.get("t40_ms", 0) > 400, "a bright ringing chime"),
    (lambda cv: cv.get("tonal"), "a short pitched ding"),
    (lambda cv: cv.get("impulsive") and cv.get("dur_s", 1) < 0.5, "a short crisp click"),
    (lambda cv: cv.get("impulsive"), "a deep hit with a long tail"),
    (lambda cv: cv.get("noisy"), "an air whoosh"),
)

_LICENCE_LINE = ("A fit derives from its recording: keep the source's attribution and licence with this file, and prefer "
            "CC0 sources for anything you ship.")


# ---------------------------------------------------------------- model literal


_INFINITY = {math.inf: "float('inf')", -math.inf: "float('-inf')"}


def _num_text(val: Any) -> str:
    fv = float(val)
    return "float('nan')" if math.isnan(fv) else _INFINITY.get(fv, repr(fv))


_ATOM_TYPES = (
    ((bool, np.bool_), lambda val: "True" if val else "False"),
    ((int, np.integer), lambda val: str(int(val))),
    ((float, np.floating), _num_text),
    ((str, type(None)), repr),
)


def _atom_text(val: Any) -> str | None:
    """Source text of a scalar, or None when v is not a scalar."""
    return next((text(val) for types, text in _ATOM_TYPES if isinstance(val, types)), None)


def _wrap_lines(items: list, indent: int) -> str:
    single = "[" + ", ".join(items) + "]"
    if len(single) + indent <= WIDTH:
        return single
    line_l, now_v = [], ""
    for itv in items:
        if now_v and len(now_v) + len(itv) + indent + MARGIN > WIDTH:
            line_l.append(now_v.rstrip())
            now_v = ""
        now_v += itv + ", "
    if now_v:
        line_l.append(now_v.rstrip())
    padv = " " * (indent + 4)
    return "[\n" + "\n".join(padv + lnv for lnv in line_l) + "\n" + " " * indent + "]"


def _seq_text(val: list | tuple, indent: int) -> str:
    if not val:
        return "[]"
    items = [_atom_text(x) for x in val]
    if all(sv is not None for sv in items):
        return _wrap_lines(items, indent)
    padv = " " * (indent + 4)
    return "[\n" + "".join(f"{padv}{_value_text(x, indent + 4)},\n" for x in val) + " " * indent + "]"


def _dict_text(dv: dict, indent: int) -> str:
    if not dv:
        return "{}"
    padv = " " * (indent + 4)
    acc = ["{"]
    for key_, val in dv.items():
        text = _value_text(val, indent + 4)
        note_v = _NOTE_TEXT.get(key_) if isinstance(key_, str) else None
        if note_v and "\n" in text:
            acc.append(f"{padv}# {note_v}")
            acc.append(f"{padv}{key_!r}: {text},")
        elif note_v:
            acc.append(f"{padv}{key_!r}: {text},  # {note_v}")
        else:
            acc.append(f"{padv}{key_!r}: {text},")
    acc.append(" " * indent + "}")
    return "\n".join(acc)


def _box_text(val: Any, indent: int) -> str:
    write = next((fnv for types, fnv in _BOX_TYPES if isinstance(val, types)), None)
    if write is None:
        raise TypeError(f"literal: cannot write a value of type {type(val).__name__}")
    return write(val, indent)


def _value_text(val: Any, indent: int) -> str:
    sv = _atom_text(val)
    return sv if sv is not None else _box_text(val, indent)


_BOX_TYPES = (
    (dict, _dict_text),
    (np.ndarray, lambda val, indent: _seq_text(val.tolist(), indent)),
    ((list, tuple), _seq_text),
)


def literal(model: dict, name: str = "MODEL") -> str:
    """'<name> = <literal>' for the model without its fit_engine key."""
    return f"{name} = " + _value_text({key_: val for key_, val in model.items() if key_ not in DROP}, 0)


# ---------------------------------------------------------------- critic words


def _normalise(pv: str) -> str:
    return os.path.normcase(os.path.abspath(pv))


def _registry_paths(ent: dict) -> list:
    return list(ent.get("forge") or []) + list(ent.get("bench") or [])


def _lookup_category(regv: dict, wanted: str) -> str | None:
    cat_l = regv.get("categories", {}).items()
    return next((catv for catv, ent in cat_l if any(_normalise(fv) == wanted for fv in _registry_paths(ent))), None)


def _cat_for(src_v: str | None) -> str | None:
    if not src_v:
        return None
    try:
        from hark.forge.engine import refsets

        return _lookup_category(refsets.load(), _normalise(src_v))
    except Exception:
        return None


def _try_load(recipes: ModuleType, name: str) -> ModuleType | None:
    try:
        return recipes.load(name)
    except Exception:
        return None


def _target_of_recipe(modv: ModuleType, cat_v: str) -> tuple[str, list]:
    tgt_l = getattr(modv, "TARGETS", None)
    tgt = tgt_l[cat_v] if isinstance(tgt_l, dict) and cat_v in tgt_l else modv.TARGET
    return tgt, list(getattr(modv, "ALTS", None) or [])


def _words_of_recipe(cat_v: str) -> tuple[str, list] | None:
    from hark.forge import recipes

    mod_l = (_try_load(recipes, cnt) for cnt in recipes.names())
    modv = next((mv for mv in mod_l if mv is not None and getattr(mv, "REFSET", None) == cat_v), None)
    return None if modv is None else _target_of_recipe(modv, cat_v)


def _words_of_category(cat_v: str) -> str | None:
    try:
        from hark.forge.engine import refsets

        text = refsets.CATEGORIES.get(cat_v)
    except Exception:
        return None
    return re.sub(r"\s*\([^()]*\)\s*$", "", text) if text else None


def _target_of_character(character: dict | None) -> str:
    cv = character or {}
    for rule_v, word_l in _CHAR_WORDS:
        if rule_v(cv):
            return word_l
    return "a sound effect"


def critic_words(source: str | None = None, category: str | None = None,
                 character: dict | None = None) -> tuple[str, list, str | None]:
    """(TARGET, ALTS, REFSET): the critic's words for a fitted recipe."""
    category = category or _cat_for(source)
    if category:
        try:
            got = _words_of_recipe(category)
        except Exception:
            got = None
        if got is not None:
            return got[0], got[1], category
        desc_v = _words_of_category(category)
        if desc_v:
            return desc_v, [], category
    return _target_of_character(character), [], None


# ---------------------------------------------------------------- recipe text


def _escaped(text: str) -> str:
    return text.replace('"""', "'''").replace("\\", "/")


def _src_label(src_v: str | None, model: dict) -> str:
    if src_v:
        return os.path.basename(str(src_v))
    return str(model.get("source") or "unknown")


def _doc_text(name: str, model: dict, kind_v: str, src_v: str | None, ref_set: str | None, quality: dict | None,
               floor: dict | None, licence: str | None, first_knob: str | None) -> str:
    from hark.fit.pipeline import quality as Q
    from hark.fit.pipeline.render import summary

    line_l = [f"{name}: a recipe that hark fit made from a recording ({kind_v} model).", ""]
    line_l += [lnv.rstrip() for lnv in summary(model).splitlines()]
    src = f"Source: {_src_label(src_v, model)}" + (f" (refset {ref_set})" if ref_set else "")
    line_l += ["", src, licence or _LICENCE_LINE]
    if quality is not None:
        line_l += ["", "Measured: the original against this recipe at DEFAULTS and seed 0:", "  " + Q.text(quality, floor)]
    line_l += ["", "Usage:", "  import numpy as np", "  y = render(DEFAULTS, np.random.default_rng(1))   # one take",
              "  y = render_seed(0)                              # the fit as measured"]
    if first_knob:
        line_l.append(f"  y = render_seed(0, {first_knob}=SPACE[{first_knob!r}][1])   # a variation")
    line_l += [f"  hark fit --forge {name}.py --budget 96 --dir made/{name}",
              "  As a forge recipe: copy this file into a recipe folder of hark/forge/recipes/.", "",
              "SPACE holds the knob search ranges. DEFAULTS is the identity: it re-renders the fit."]
    return _escaped("\n".join(line_l))


def _lines_for_space(knobs: dict) -> list:
    acc = ["SPACE = {"]
    for key_, (rng, _ident, desc_v) in knobs.items():
        acc.append(f"    {key_!r}: {_value_text(tuple(rng), 4)},  # {desc_v}")
    acc.append("}")
    return acc


def _ref_list(src_v: str | None, ref_l: list | None) -> list:
    if ref_l is not None:
        return [str(rv) for rv in ref_l]
    if src_v and os.path.exists(src_v):
        return [os.path.abspath(src_v).replace("\\", "/")]
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


def _critic_line(model: dict, src_v: str | None, tgt: str | None, alt_l: list | None,
            ref_set: str | None) -> tuple[str, list, str | None]:
    """TARGET, ALTS and REFSET: the given values, else the critic's words."""
    character = (model.get("fit_engine") or {}).get("character")
    t, a, catv = critic_words(src_v, ref_set, character)
    return tgt or t, (a if alt_l is None else list(alt_l)), ref_set or catv


def _head_text(tgt: str, alt_l: list, ref_set: str | None, ref_l: list, kind_v: str) -> list:
    acc = ["", "import numpy as np", "", "import hark.fit as _hark_fit", "", f"TARGET = {tgt!r}",
           f"ALTS = {alt_l!r}"]
    if ref_set:
        acc.append(f"REFSET = {ref_set!r}  # the refset category of the critic's default references")
    return acc + [f"REFS = {ref_l!r}", f"KIND = {kind_v!r}"]


def _line_of_defaults(ident_v: dict) -> str:
    defaults = ", ".join(f"{key_!r}: {_value_text(val, 0)}" for key_, val in ident_v.items())
    return "DEFAULTS = {" + defaults + "}  # these values re-render the fit"


def _parts_of_model(model: dict) -> tuple[str, dict, dict]:
    """(kind, knob table, identity knob values) of a model."""
    from hark.fit.pipeline.render import identity, kind_of, knobs_of

    return kind_of(model), knobs_of(model), identity(model)


def to_recipe(model: dict, name: str = "fitted", source: str | None = None, quality: dict | None = None,
              floor: dict | None = None, target: str | None = None, alts: list | None = None,
              refset: str | None = None, refs: list | None = None, licence: str | None = None) -> str:
    """The full source text of a recipe module."""
    target, alts, refset = _critic_line(model, source, target, alts, refset)
    kind_v, knobs, ident_v = _parts_of_model(model)
    doc = _doc_text(name, model, kind_v, source, refset, quality, floor, licence, next(iter(knobs), None))
    acc = ['"""' + doc, '"""'] + _head_text(target, alts, refset, _ref_list(source, refs), kind_v)
    acc += _lines_for_space(knobs) + [_line_of_defaults(ident_v), literal(model)]
    return "\n".join(acc) + _BODY.replace("__WAV__", repr(name + ".wav"))


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
    with open(path, "w", encoding="utf-8") as fv:
        fv.write(to_recipe(model, **kw))
    return path


_COUNTER = itertools.count()


def load_recipe(path: str) -> ModuleType:
    """Import a recipe file as a new, independent module object."""
    name = clean_name(path)
    spec_v = importlib.util.spec_from_file_location(f"_hark_fit_recipe_{next(_COUNTER)}_{name}", str(path))
    if spec_v is None or spec_v.loader is None:
        raise ImportError(f"load_recipe: cannot import {path}")
    modv = importlib.util.module_from_spec(spec_v)
    spec_v.loader.exec_module(modv)
    modv.__name__ = name
    return modv
