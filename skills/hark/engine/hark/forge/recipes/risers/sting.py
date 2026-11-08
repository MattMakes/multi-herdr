"""sting: a logo sting or reveal in 4 idioms (chirp, reveal, launch, gesture). Every sample is computed.

The reveal hit and the chirp/reveal sub come from the impact and subdrop recipes. ``hall_ir``, ``measure``,
``study_extra`` and ``study_penalty`` are re-exported from the impact recipe.
"""

from __future__ import annotations

import types
from typing import Any

import numpy as np
from scipy.signal import fftconvolve

from hark.forge.engine import dsp, fx
from hark.forge.recipes.impacts import impact as _impact
from hark.forge.recipes.impacts import subdrop as _subdrop
from hark.forge.recipes.impacts.impact import hall_ir, measure, study_extra, study_penalty
from hark.forge.recipes.risers._common import bw, n_of, svf, t_of

__all__ = ["hall_ir", "measure", "study_extra", "study_penalty", "render", "space_for", "recipe_for",
           "fx_granular"]

SR = 48000
TARGET = "a bright logo sting: a chime chord ringing out"
ALTS = ["a doorbell", "a church bell", "a notification sound", "a gong"]
REFSET = "logo_sting"
MODULE = "hark.forge.recipes.risers.sting"
TARGETS = {
    "logo_sting": "a short bright logo reveal sting: a whoosh into a hit with a shimmering chime",
    "pro_logo_sting": "a designed logo sting: a falling chirp landing on a low hum",
}

CHORDS = {"fifth": (0, 7, 12), "major": (0, 4, 7, 12), "sus2": (0, 2, 7, 12), "add9": (0, 4, 7, 14),
          "octave": (0, 12, 24), "minor9": (0, 3, 7, 14)}
MATERIALS = ["bar", "glass", "tubular", "fm", "triangle"]
TRIANGLE = ((1, 2.76, 3.9, 5.4, 6.7, 8.93, 10.3, 13.34), (0.6, 1.0, 0.5, 0.8, 0.45, 0.5, 0.3, 0.25))
DYADS = {"one": (0,), "fifth": (0, 7), "octave": (0, 12), "fourth": (0, 5), "ninth": (0, 14), "third": (0, 4)}
LEADS = ["whoosh", "chirp", "swell", "none"]
GESTURES = ["chirp_hum", "zap_bloom", "sweep_hit", "taps_tone", "swell_sting"]
CHORDS_G = {**DYADS, "triad": (0, 7, 16)}
G_MATS = {
    "sine": (),
    "marimba": ((3.93, 0.5, 0.03), (9.2, 0.15, 0.012)),
    "glass": ((2.32, 0.45, 0.12), (4.25, 0.25, 0.06), (6.63, 0.1, 0.03)),
    "bar": ((2.756, 0.4, 0.08), (5.404, 0.15, 0.03)),
}

SPACE: dict[str, Any] = {
    "mode": ["reveal", "chirp", "launch", "gesture"], "root": (300.0, 2200.0, "log"), "chord": list(CHORDS),
    "material": list(MATERIALS), "bloom_decay": (0.6, 4.0, "log"), "bright": (0.0, 1.0),
    "open_ms": (10.0, 400.0, "log"), "strike": (0.0, 1.0), "shimmer": (0.0, 0.6), "hit": (0.0, 0.8),
    "sub": (0.0, 0.8), "swell": (0.0, 1.2), "swell_level": (0.05, 0.6, "log"), "verb": (0.0, 0.8),
    "verb_rt": (0.8, 5.0, "log"), "width": (0.0, 1.0), "low_cut": (25.0, 800.0, "log"),
    "chirp_f0": (700.0, 3500.0, "log"), "chirp_f1": (90.0, 320.0, "log"), "chirp_ms": (40.0, 170.0, "log"),
    "chirp_harm": (0.0, 1.0), "hum_f": (52.0, 110.0, "log"), "hum_sink": (0.0, 1.0),
    "hum_formant": (200.0, 900.0, "log"), "hum_buzz": (0.0, 1.0), "hum_level": (0.2, 1.5, "log"),
    "hum_fast": (0.02, 0.15, "log"), "hum_slow": (0.15, 0.9, "log"), "hum_mix": (0.03, 0.5, "log"),
    "land": (0.0, 1.0), "land_f": (400.0, 1300.0, "log"), "thud": (0.0, 1.0), "sub_db": (30.0, 130.0),
    "bell_decay": (0.08, 0.6, "log"), "length": (0.5, 1.8), "lead": list(LEADS), "lead_s": (0.1, 0.7),
    "lead_level": (0.0, 0.8), "lead_f0": (200.0, 2000.0, "log"), "lead_f1": (1500.0, 12000.0, "log"),
    "tick": (0.0, 1.5), "tick_f": (2500.0, 9000.0, "log"), "tick_ms": (2.0, 15.0, "log"), "thump": (0.0, 0.8),
    "thump_f": (70.0, 260.0, "log"), "notes": list(DYADS), "ring": (0.3, 1.2), "strike_tau": (0.04, 0.3, "log"),
    "ring_mix": (0.05, 0.5), "ring_tau": (0.15, 0.6, "log"), "bloom_att": (0.0, 0.15),
    "shimmer_tau": (0.15, 0.8, "log"), "bloom_delay": (0.0, 0.03), "low": (0.0, 1.0), "low_f": (38.0, 95.0, "log"),
    "low_tau": (0.06, 0.4, "log"), "zap": (0.0, 1.0), "zap_k": (3.0, 10.0), "ctick": (0.0, 1.0),
    "double": (0.0, 1.0), "swell_trim": (0.0, 0.08), "shimmer_smooth": (0.0, 1.0), "double_gap": (0.0, 0.12),
    "gesture": list(GESTURES), "g_f0": (150.0, 12000.0, "log"), "g_f1": (60.0, 4000.0, "log"),
    "g_ms": (40.0, 600.0, "log"), "g_lead": (0.0, 1.0), "g_root": (50.0, 1800.0, "log"),
    "g_notes": list(CHORDS_G), "g_warm": (0.0, 0.6), "g_att": (0.0, 0.12), "g_tau1": (0.02, 0.3, "log"),
    "g_mix": (0.03, 0.5, "log"), "g_tau2": (0.1, 0.9, "log"), "g_sink": (0.0, 1.0), "g_land": (0.0, 1.0),
    "g_land_f": (300.0, 1500.0, "log"), "g_air": (0.0, 1.0), "g_hit": (0.0, 1.0), "g_low_f": (38.0, 95.0, "log"),
    "g_low_tau": (0.05, 0.4, "log"), "g_snap": (0.0, 1.0), "g_snap_f": (1500.0, 9000.0, "log"),
    "g_snap_ms": (2.0, 20.0, "log"), "g_gap": (0.04, 0.14), "g_tap_f": (500.0, 3000.0, "log"),
    "g_wet": (0.0, 0.8), "g_rt": (0.4, 2.5, "log"), "g_length": (0.4, 1.6), "g_tone": (0.2, 1.5),
    "g_verb": ["hall", "fdn"], "g_mat": list(G_MATS), "g_bend": (0.0, 2.0), "g_chorus": (0.0, 1.0),
}

# ------------------------------------------------------------------------------------------------ bases (15.4)

REVEAL = dict(mode="reveal", root=880.0, chord="add9", material="bar", bloom_decay=2.2, bright=0.6, open_ms=120.0,
              strike=0.5, shimmer=0.25, hit=0.3, sub=0.25, swell=0.0, swell_level=0.2, verb=0.35, verb_rt=2.6,
              width=0.6, low_cut=25.0)
CHIRP_R2 = dict(mode="chirp", chirp_f0=1500.0, chirp_f1=150.0, chirp_ms=85.0, chirp_harm=0.7, hum_f=76.0,
                hum_sink=0.6, hum_formant=500.0, hum_buzz=0.9, hum_level=0.8, hum_fast=0.045, hum_slow=0.2,
                hum_mix=0.06, length=0.85, sub=0.0, shimmer=0.0, verb=0.05, verb_rt=0.8, width=0.0, root=880.0,
                material="bar", bright=0.5, land=0.0, land_f=800.0, thud=0.0, sub_db=45.0)
CHIRP_R3 = {**CHIRP_R2, **dict(land=0.5, land_f=1400.0, thud=0.5, hum_level=0.95, hum_fast=0.05, chirp_harm=1.0,
                               sub=0.25, sub_db=100.0)}
CHIRP_R4 = {**CHIRP_R3, **dict(zap=0.0, zap_k=6.0, ctick=0.0, double=0.0, double_gap=0.05)}
LAUNCH = dict(mode="launch", lead="whoosh", lead_s=0.45, lead_level=0.45, lead_f0=600.0, lead_f1=9000.0, tick=1.0,
              tick_f=5000.0, tick_ms=6.0, thump=0.08, thump_f=150.0, material="glass", root=1318.5, notes="fifth",
              bright=0.7, ring=0.9, strike_tau=0.12, ring_mix=0.25, ring_tau=0.3, bloom_att=0.0, shimmer=0.4,
              shimmer_tau=0.35, verb=0.2, verb_rt=1.0, width=0.6, low_cut=140.0, bloom_delay=0.012)
GESTURE = dict(mode="gesture", gesture="chirp_hum", g_f0=1600.0, g_f1=160.0, g_ms=85.0, g_lead=1.0, g_root=76.0,
               g_notes="one", g_warm=0.35, g_att=0.0, g_tau1=0.06, g_mix=0.12, g_tau2=0.3, g_sink=0.5, g_land=0.5,
               g_land_f=800.0, g_air=0.25, g_hit=0.0, g_low_f=55.0, g_low_tau=0.15, g_snap=0.2, g_snap_f=5000.0,
               g_snap_ms=5.0, g_gap=0.075, g_tap_f=1400.0, g_wet=0.3, g_rt=1.0, g_length=0.8, g_tone=1.0,
               g_mat="sine", g_bend=0.0, g_chorus=0.0, g_verb="hall")
BASES = {"chirp": CHIRP_R2, "launch": LAUNCH, "gesture": GESTURE, "reveal": REVEAL}

# ------------------------------------------------------------------------------------------------ families (15.5)


def _members(base: dict, rows: list[tuple[str, dict]]) -> list[tuple[str, dict]]:
    return [(name, {**base, **kw}) for name, kw in rows]


_LAUNCH_A = [
    ("whoosh_glass", {}),
    ("zip_ping", dict(lead="chirp", lead_s=0.16, lead_level=0.4, lead_f0=420.0, lead_f1=2600.0, tick=0.9,
                      tick_f=6000.0, tick_ms=4.0, thump=0.0, material="bar", root=1760.0, notes="octave",
                      strike_tau=0.1, ring_mix=0.2, bright=0.45, ring_tau=0.28, shimmer=0.15, verb=0.15, width=0.3,
                      low_cut=300.0)),
    ("reverse_bloom", dict(lead="swell", lead_s=0.5, lead_level=0.35, tick=0.8, tick_f=4500.0, tick_ms=8.0,
                           thump=0.15, thump_f=110.0, material="bar", root=1480.0, notes="ninth", strike_tau=0.15,
                           ring_mix=0.3, ring_tau=0.32, shimmer=0.3, verb=0.3, verb_rt=1.2, width=0.7,
                           low_cut=90.0)),
    ("spark", dict(lead="none", lead_level=0.3, tick=1.1, tick_f=7000.0, tick_ms=3.0, thump=0.0,
                   material="triangle", root=1318.5, notes="one", strike_tau=0.09, bright=0.35, ring_mix=0.3,
                   ring_tau=0.28, shimmer=0.6, shimmer_tau=0.4, verb=0.15, width=0.9, low_cut=300.0)),
    ("soft_bloom", dict(lead="whoosh", lead_s=0.3, lead_f0=300.0, lead_f1=4000.0, lead_level=0.3, tick=0.7,
                        tick_f=4000.0, tick_ms=10.0, thump=0.18, thump_f=130.0, material="tubular", root=987.8,
                        notes="fourth", bloom_att=0.06, strike_tau=0.16, ring_mix=0.25, ring_tau=0.32,
                        shimmer=0.25, verb=0.3, verb_rt=1.1, width=0.5, low_cut=100.0)),
]
_LAUNCH_B = {"whoosh_glass": (0.04, 55, 0.16), "zip_ping": (0.044, 73, 0.1), "reverse_bloom": (0.045, 46, 0.24),
             "spark": (0.03, 82, 0.09), "soft_bloom": (0.041, 62, 0.2)}
_LAUNCH_C = {
    "whoosh_glass": dict(lead_s=0.1896, lead_level=0.4168, tick=0.7978, tick_f=3631.0, thump=0.1648, bright=0.7076,
                         ring=0.9497, strike_tau=0.1913, ring_mix=0.2039, ring_tau=0.2963, shimmer=0.2202,
                         verb=0.2948, width=0.319, bloom_delay=0.0078, shimmer_smooth=1.0),
    "zip_ping": dict(shimmer=0.105, shimmer_smooth=1.0),
    "reverse_bloom": dict(swell_trim=0.045, lead_s=0.36, shimmer=0.21, shimmer_smooth=1.0),
    "spark": dict(shimmer=0.42, shimmer_smooth=1.0),
    "soft_bloom": dict(lead_s=0.2471, lead_level=0.3126, shimmer=0.175, shimmer_smooth=1.0),
}
_LOGO_R3 = _members(LAUNCH, _LAUNCH_A)
_LOGO_R4 = [(name, {**p, "low": float(_LAUNCH_B[name][0]), "low_f": float(_LAUNCH_B[name][1]),
                    "low_tau": float(_LAUNCH_B[name][2])}) for name, p in _LOGO_R3]
_LOGO_R5 = [(name, {**p, **_LAUNCH_C[name]}) for name, p in _LOGO_R4]

_GESTURE_ROWS = [
    ("chirp_hum", dict(g_f0=1500.0, g_f1=150.0, g_ms=85.0, g_root=76.0, g_warm=0.15, g_tau1=0.07, g_mix=0.15,
                       g_tau2=0.2, g_sink=0.5, g_land=0.8, g_land_f=900.0, g_air=0.1, g_snap=0.0, g_tone=0.6,
                       g_wet=0.45, g_rt=1.4, g_length=0.9, g_verb="fdn")),
    ("zap_bloom", dict(gesture="zap_bloom", g_f0=7000.0, g_f1=700.0, g_ms=60.0, g_lead=0.5, g_air=0.3,
                       g_root=261.6, g_notes="fifth", g_warm=0.35, g_att=0.04, g_tau1=0.08, g_mix=0.15, g_tau2=0.28,
                       g_snap=0.0, g_chorus=0.2, g_wet=0.4, g_rt=1.2, g_length=0.9, g_verb="fdn")),
    ("sweep_hit", dict(gesture="sweep_hit", g_f0=220.0, g_f1=1760.0, g_ms=320.0, g_lead=0.7, g_air=0.6, g_hit=0.6,
                       g_low_f=52.0, g_low_tau=0.12, g_tone=0.55, g_snap=0.6, g_snap_f=3500.0, g_snap_ms=8.0,
                       g_root=330.0, g_notes="fifth", g_warm=0.2, g_tau1=0.08, g_mix=0.12, g_tau2=0.25, g_wet=0.3,
                       g_rt=1.0, g_length=0.75)),
    ("taps_tone", dict(gesture="taps_tone", g_gap=0.075, g_tap_f=1046.5, g_lead=0.8, g_root=523.25, g_notes="one",
                       g_mat="bar", g_warm=0.35, g_att=0.0, g_bend=0.3, g_tau1=0.06, g_mix=0.12, g_tau2=0.25,
                       g_snap=0.6, g_snap_f=7000.0, g_snap_ms=3.0, g_wet=0.35, g_rt=1.1, g_length=0.8,
                       g_verb="fdn")),
    ("swell_sting", dict(gesture="swell_sting", g_ms=450.0, g_lead=0.6, g_root=293.66, g_notes="triad",
                         g_warm=0.12, g_chorus=0.4, g_tau1=0.08, g_mix=0.12, g_tau2=0.28, g_hit=0.0, g_low_f=48.0,
                         g_snap=0.0, g_air=0.5, g_wet=0.45, g_rt=1.2, g_length=0.85)),
]
_CHIRP_HIGH = dict(chirp_f0=2600.0, chirp_f1=240.0, chirp_ms=70.0, hum_f=98.0, hum_formant=600.0, length=0.7,
                   hum_slow=0.16)
_PRO_R4 = [
    ("ref", dict(ctick=0.4)),
    ("zap", {**_CHIRP_HIGH, **dict(land_f=1100.0, thud=0.3, hum_buzz=0.55, sub=0.15, sub_db=110.0, zap=0.6,
                                   zap_k=4.0, chirp_harm=0.5, ctick=0.2)}),
    ("deep", dict(chirp_f0=950.0, chirp_f1=100.0, chirp_ms=130.0, hum_f=56.0, hum_formant=380.0, sub=0.35,
                  length=1.0, hum_slow=0.26, thud=0.7, land=0.4, land_f=650.0, hum_level=1.05, hum_buzz=1.0,
                  sub_db=100.0, ctick=0.3, hum_sink=1.0)),
    ("bell", dict(shimmer=0.1, root=1100.0, hum_buzz=0.3, hum_formant=520.0, land=0.35, zap=0.2, zap_k=8.0,
                  thud=0.4, bell_decay=0.16, chirp_harm=0.4, ctick=0.25, hum_level=0.75)),
    ("double", dict(hum_slow=0.3, hum_mix=0.08, length=1.0, hum_sink=0.8, chirp_harm=0.8, land=0.3, thud=0.55,
                    sub=0.2, sub_db=90.0, double=0.7, double_gap=0.012, zap=0.4, zap_k=7.0, chirp_f0=1700.0,
                    hum_f=70.0)),
]
_PRO_R3 = [
    ("ref", {}),
    ("high", {**_CHIRP_HIGH, **dict(land_f=1100.0, thud=0.3, hum_buzz=0.55, sub=0.15, sub_db=110.0)}),
    ("deep", dict(chirp_f0=1200.0, chirp_f1=120.0, chirp_ms=105.0, hum_f=62.0, hum_formant=380.0, sub=0.35,
                  length=1.0, hum_slow=0.26, thud=0.7, land=0.4, land_f=650.0, hum_level=1.05, hum_buzz=1.0,
                  sub_db=100.0)),
    ("bell", dict(shimmer=0.1, root=1100.0, hum_buzz=0.4, hum_formant=520.0, land=0.35, thud=0.4, bell_decay=0.16,
                  chirp_f0=1900.0, chirp_f1=190.0, chirp_ms=75.0, hum_f=84.0)),
    ("long", dict(hum_slow=0.45, hum_mix=0.1, length=1.3, hum_sink=0.8, chirp_harm=0.7, verb=0.12, verb_rt=1.2,
                  land=0.3, thud=0.55, sub=0.2, sub_db=80.0)),
]
_REVEAL_R2 = [
    ("glass_bloom", dict(material="glass", root=1760.0, chord="add9", bloom_decay=1.2, bright=0.8, strike=0.8,
                         shimmer=0.35, hit=0.15, sub=0.0, low_cut=450.0, open_ms=30.0)),
    ("triangle", dict(material="triangle", root=1480.0, chord="fifth", bloom_decay=1.9, bright=0.85, open_ms=10.0,
                      strike=0.9, shimmer=0.15, hit=0.1, sub=0.0, verb=0.25, low_cut=500.0)),
    ("fm_reveal", dict(material="fm", root=1320.0, chord="major", bloom_decay=1.3, bright=0.8, swell=0.6,
                       swell_level=0.25, hit=0.2, sub=0.0, open_ms=60.0, low_cut=350.0)),
    ("chime_hit", dict(material="tubular", root=1175.0, chord="sus2", bloom_decay=1.5, bright=0.75, strike=0.8,
                       hit=0.3, sub=0.0, shimmer=0.1, verb=0.4, verb_rt=2.4, low_cut=300.0, open_ms=25.0)),
    ("bar_sparkle", dict(material="bar", root=1980.0, chord="octave", bloom_decay=1.1, bright=0.8, strike=0.9,
                         shimmer=0.55, hit=0.0, sub=0.0, width=0.9, open_ms=10.0, low_cut=500.0)),
]
_PRO_R2 = [
    ("ref", {}),
    ("high", dict(_CHIRP_HIGH)),
    ("deep", dict(chirp_f0=1200.0, chirp_f1=120.0, chirp_ms=105.0, hum_f=62.0, hum_formant=380.0, sub=0.25,
                  length=1.0, hum_slow=0.26)),
    ("bell", dict(shimmer=0.1, root=950.0, hum_buzz=0.4, hum_formant=520.0)),
    ("long", dict(hum_slow=0.45, hum_mix=0.1, length=1.3, hum_sink=0.8, chirp_harm=0.7, verb=0.12, verb_rt=1.2)),
]

FAMILIES = {"logo_sting": _LOGO_R5, "pro_logo_sting": _members(GESTURE, _GESTURE_ROWS)}
FAMILIES_R4 = {"logo_sting": _LOGO_R4, "pro_logo_sting": _members(CHIRP_R4, _PRO_R4)}
FAMILIES_R3 = {"logo_sting": _LOGO_R3, "pro_logo_sting": _members(CHIRP_R3, _PRO_R3)}
FAMILIES_R2 = {"logo_sting": _members(REVEAL, _REVEAL_R2), "pro_logo_sting": _members(CHIRP_R2, _PRO_R2)}
DEFAULTS = dict(_LOGO_R4[0][1])

STUDY_STING = {
    "logo_sting": {"t40": (650, 1150), "tilt": (1.5, 10), "sh150": (0, 0.12)},
    "pro_logo_sting": {"len50": (0.5, 1.5), "t20": (30, 110), "t40": (150, 450), "tilt": (-9, -5),
                       "corr": (0.9, 1.0), "ridge0": (900, 2400), "ridge1": (55, 110)},
}

# ------------------------------------------------------------------------------------------------ search (15.6)

SEARCH = {
    "logo_sting": (["lead_s", "lead_level", "tick", "tick_f", "thump", "ring", "strike_tau", "ring_mix", "ring_tau",
                    "shimmer", "verb", "width", "low_cut", "bright", "bloom_delay"],
                   {"lead_s": (0.15, 0.6), "ring_tau": (0.15, 0.45, "log"), "verb": (0.0, 0.45),
                    "low_cut": (60.0, 600.0, "log"), "lead_level": (0.25, 0.7), "tick": (0.6, 1.5),
                    "thump": (0.0, 0.4), "bloom_delay": (0.004, 0.025)}),
    "pro_logo_sting": (["g_lead", "g_warm", "g_att", "g_tau1", "g_mix", "g_tau2", "g_land", "g_air", "g_hit",
                        "g_low_tau", "g_snap", "g_snap_f", "g_wet", "g_rt", "g_length"],
                       {"g_warm": (0.0, 0.45), "g_wet": (0.1, 0.6), "g_length": (0.5, 1.2)}),
}
CHIRP_SEARCH = (["chirp_f0", "chirp_f1", "chirp_ms", "chirp_harm", "hum_f", "hum_sink", "hum_formant", "hum_buzz",
                 "hum_level", "hum_fast", "hum_slow", "hum_mix", "length", "land", "thud", "sub", "sub_db"],
                {"chirp_f0": (900.0, 3000.0, "log"), "chirp_f1": (100.0, 280.0, "log"),
                 "chirp_ms": (50.0, 130.0, "log"), "hum_f": (55.0, 105.0, "log"), "length": (0.6, 1.6),
                 "land": (0.2, 0.9), "thud": (0.2, 0.9), "sub": (0.1, 0.5), "sub_db": (60.0, 130.0)})


def space_for(cat: str) -> dict:
    """The search space of a category: its search keys, with the category limits where given."""
    keys, limits = SEARCH[cat]
    return {k: limits.get(k, SPACE[k]) for k in keys}


def recipe_for(cat: str, preset: int | str = 0) -> types.SimpleNamespace:
    """A recipe object that searches the category keys around one preset (index or name)."""
    members = FAMILIES[cat]
    name, params = members[preset] if isinstance(preset, int) else (preset, dict(members)[preset])
    return types.SimpleNamespace(MODULE=MODULE, SPACE=space_for(cat), DEFAULTS=dict(params), render=render,
                                 TARGET=TARGETS[cat], ALTS=list(ALTS), REFSET=cat, PRESET=name)


# ------------------------------------------------------------------------------------------------ small helpers


def _up(x: np.ndarray) -> np.ndarray:
    """Unit peak."""
    return x / (np.max(np.abs(x)) + 1e-12)


def _st(x: np.ndarray) -> np.ndarray:
    return x if x.ndim == 2 else np.column_stack([x, x])


def _fit(x: np.ndarray, n: int) -> np.ndarray:
    if len(x) >= n:
        return x[:n]
    return np.concatenate([x, np.zeros((n - len(x),) + x.shape[1:])])


def _delayed(x: np.ndarray, d: int, n: int) -> np.ndarray:
    out = np.zeros((n,) + x.shape[1:])
    if d < n:
        m = min(len(x), n - d)
        out[d:d + m] = x[:m]
    return out


def _conv(x: np.ndarray, h: np.ndarray, n: int) -> np.ndarray:
    return _fit(fftconvolve(x, h), n)


def _hall(rng, rt60: float, damp: float, size: float, predelay: float, seconds: float | None = None) -> np.ndarray:
    return hall_ir(rt60, damp, size, predelay, seconds, rng=rng)


def _hp_cut(x: np.ndarray, low_cut: float) -> np.ndarray:
    return bw(x, "high", low_cut, 2 if low_cut < 60 else 4)


def _widen(hi: np.ndarray, width: float) -> np.ndarray:
    """The part above 600 Hz made wide: decor(hi) - [hi, hi]."""
    return fx.decorrelate(hi, 0.8 * width, 250) - _st(hi)


def fx_granular(src: np.ndarray, seconds: float, rng: np.random.Generator) -> np.ndarray:
    """The coarse shimmer cloud: grains 50 ms, 50 per second, an octave up."""
    return fx.granular(src, seconds, rng, grain=0.05, density=50, pos=(0.0, 0.6), pos_jitter=0.05, pitch=12.0,
                       pitch_jitter=0.15, spread=0.9)


def _smooth_shimmer(src: np.ndarray, seconds: float, k: float, rng) -> np.ndarray:
    return fx.granular(src, seconds, rng, grain=0.05 * (1 + 0.8 * k), density=50 * (1 + 3 * k), pos=(0.0, 0.6),
                       pos_jitter=0.05, pitch=12.0, pitch_jitter=0.15, spread=0.9)


def _noise_tick(n: int, ms: float, rng, min_len: int) -> np.ndarray:
    """White noise with a cubic fall and a 0.2 ms rise, L samples long."""
    L = min(n, max(min_len, n_of(ms / 1000)))
    i = np.arange(L)
    return rng.standard_normal(L) * (1 - i / L) ** 3 * (1 - np.exp(-i / (0.0002 * SR)))


def low_strike(low_f: float, low_tau: float, n: int) -> np.ndarray:
    """A pitched low strike from 2.5 x low_f onto low_f, rising over 12 ms (no random draws)."""
    t = t_of(n)
    ph = 2 * np.pi * np.cumsum(low_f * (1 + 1.5 * np.exp(-t / 0.035))) / SR
    y = (np.sin(ph) + 0.25 * np.sin(2 * ph)) * np.clip(t / 0.012, 0, 1) ** 2 * np.exp(-t / low_tau)
    return _up(bw(y, "high", 25, 2))


# ------------------------------------------------------------------------------------------------ chirp (15.8)


def _chirp_track(p: dict, n: int) -> tuple[int, float, np.ndarray, np.ndarray]:
    T = p["chirp_ms"] / 1000
    L = min(n, n_of(1.25 * T))
    t = t_of(L)
    f = p["chirp_f0"] * (p["chirp_f1"] / p["chirp_f0"]) ** np.clip(t / T, 0, 1)
    return L, T, t, f


def chirp(p: dict, n: int, rng) -> np.ndarray:
    L, T, t, f = _chirp_track(p, n)
    ph = 2 * np.pi * np.cumsum(f) / SR + rng.uniform(0, 0.3)
    y = np.sin(ph) + p["chirp_harm"] * (0.5 * np.sin(2 * ph) + 0.25 * np.sin(3 * ph))
    y[2 * f >= 0.45 * SR] = 0.0
    e = (1 - np.exp(-t / 0.003)) * (0.3 + 0.7 * np.clip(t / T, 0, 1)) * np.clip((1.25 - t / T) / 0.25, 0, 1) ** 1.5
    out = np.zeros(n)
    out[:L] = y * e
    return _up(out)


def hum(p: dict, n: int, t0: float, rng) -> np.ndarray:
    t = t_of(n)
    tt = np.maximum(t - t0, 0)
    dur = max(t[-1] - t0, 0.1)
    f = p["hum_f"] * 2.0 ** (-p["hum_sink"] * np.clip(tt / dur, 0, 1) ** 0.7)
    ph = 2 * np.pi * np.cumsum(f) / SR
    y = np.zeros(n)
    for k in range(1, 13):
        a = (1 / k) ** (1.6 - 1.1 * p["hum_buzz"])
        form = 1 / (1 + (np.log2(k * f / p["hum_formant"]) / 0.7) ** 2)
        y += np.where(k * f < 0.45 * SR, a * (0.35 + form) * np.sin(k * ph + rng.uniform(0, 6.28)), 0.0)
    e = (np.exp(-tt / p["hum_fast"]) + p["hum_mix"] * np.exp(-tt / p["hum_slow"])) * (1 - np.exp(-tt / 0.004))
    return _up(y * np.where(t >= t0, e, 0.0))


def landing(p: dict, n: int, t0: float) -> np.ndarray:
    """A landing blob that bends down and a thud onto 2 x hum_f (no random draws)."""
    t = t_of(n)
    tt = np.maximum(t - t0, 0)
    on = (t >= t0) * (1 - np.exp(-tt / 0.002))
    live = tt > 0
    y = np.zeros(n)
    if p["land"] > 0.01:
        f = p["land_f"] * 2.0 ** (-0.5 * (1 - np.exp(-tt / 0.03)))
        y += p["land"] * np.sin(2 * np.pi * np.cumsum(f * live) / SR) * np.exp(-tt / 0.025) * on
    if p["thud"] > 0.01:
        f = 2 * p["hum_f"] * (1 + np.exp(-tt / 0.04))
        y += p["thud"] * np.sin(2 * np.pi * np.cumsum(f * live) / SR) * np.exp(-tt / 0.05) * on
    return y


def zap(p: dict, n: int, rng) -> np.ndarray:
    L, T, t, f = _chirp_track(p, n)
    y = svf(rng.standard_normal(L), np.minimum(p["zap_k"] * f, 15000), 3, "bandn")
    e = (1 - np.exp(-t / 0.002)) * (0.4 + 0.6 * np.clip(t / T, 0, 1)) * np.clip((1.25 - t / T) / 0.25, 0, 1) ** 1.5
    out = np.zeros(n)
    out[:L] = bw(y * e, "low", 13000, 2)
    return _up(out)


def ctick(n: int, t0: float, rng) -> np.ndarray:
    L = n_of(0.005)
    i = np.arange(L)
    w = bw(rng.standard_normal(L) * (1 - i / L) ** 3 * (1 - np.exp(-i / (0.0002 * SR))), "band", (5000, 13000), 2)
    return _delayed(w / (np.max(np.abs(w)) + 1e-12), n_of(t0), n)


def _chirp_sub(p: dict, n: int, t0: float, rng) -> np.ndarray:
    sd = {**dict(_subdrop.FAMILIES["pro_sub_drop"])["ref"], "f0": p["hum_f"], "f1": 0.66 * p["hum_f"],
          "bloom": 0.06, "click": 0.0, "length": p["length"], "decay_db": p.get("sub_db", 45.0)}
    s = _subdrop.render(sd, rng)
    s = s.mean(axis=1) if s.ndim == 2 else s
    return _delayed(s, n_of(t0), n)


def _pre_chirp(p: dict, rng) -> np.ndarray:
    q = {**p, "chirp_f0": min(3500.0, 1.35 * p["chirp_f0"]), "chirp_f1": 2.5 * p["chirp_f1"],
         "chirp_ms": 0.55 * p["chirp_ms"]}
    k = n_of(1.25 * q["chirp_ms"] / 1000)
    pre = chirp(q, k, rng)
    if p.get("zap", 0.0) > 0.005:
        pre = pre + p["zap"] * zap(q, k, rng)
    return np.concatenate([_up(pre), np.zeros(n_of(p.get("double_gap", 0.05)))])


def render_chirp(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A falling chirp landing on a low hum; L = R."""
    n = n_of(p["length"])
    t0 = 0.92 * p["chirp_ms"] / 1000
    y = chirp(p, n, rng) + p["hum_level"] * hum(p, n, t0, rng)
    if p.get("land", 0.0) > 0.01 or p.get("thud", 0.0) > 0.01:
        y = y + landing(p, n, t0)
    if p.get("zap", 0.0) > 0.005:
        y = y + p["zap"] * zap(p, n, rng)
    if p.get("ctick", 0.0) > 0.005:
        y = y + p["ctick"] * ctick(n, t0, rng)
    if p["sub"] > 0.01:
        y = y + p["sub"] * _chirp_sub(p, n, t0, rng)
    if p["shimmer"] > 0.01:
        b = dsp.bell(p["root"], max(0.3, p["length"] - t0), rng, "bar", decay=p.get("bell_decay", 0.35),
                     bright=0.3, strike=0.0)
        y = y + 0.6 * p["shimmer"] * _delayed(b, n_of(t0), n)
    if p["verb"] > 0.01:
        y = y + p["verb"] * _conv(y, _hall(rng, p["verb_rt"], 0.6, 0.5, 0.008), n)
    if p.get("double", 0.0) > 0.005:
        y = np.concatenate([p["double"] * np.max(np.abs(y)) * _pre_chirp(p, rng), y])
    y = dsp.fade(bw(y, "high", 30, 2), 0.0008, min(0.12, 0.2 * p["length"]))
    return np.column_stack([y, y])


# ------------------------------------------------------------------------------------------------ reveal (15.9)


def _bell_partials(material: str) -> Any:
    return TRIANGLE if material == "triangle" else material


def reveal_note(p: dict, f: float, s: float, rng) -> np.ndarray:
    if p["material"] == "fm":
        bd = p["bloom_decay"]
        x = dsp.fm(f, 1.4, 3 + 5 * p["bright"], s, decay=0.45 * bd, index_decay=0.12, attack=0.002)
        x = x + 0.35 * dsp.fm(2 * f, 3.5, 2.0, s, decay=0.2 * bd, index_decay=0.05, attack=0.002)
        return _up(x)
    tri = p["material"] == "triangle"
    return dsp.bell(f, s, rng, _bell_partials(p["material"]), decay=p["bloom_decay"] * (0.35 if tri else 0.5),
                    slope=0.5 if p["material"] in ("triangle", "glass") else 0.8, bright=p["bright"], beat=1.2,
                    strike=0.2 + 0.6 * p["strike"])


def bloom(p: dict, rng) -> np.ndarray:
    s = min(5.5, 1.6 * p["bloom_decay"] + 0.4)
    n = n_of(s)
    y = np.zeros(n)
    for j, st in enumerate(CHORDS[p["chord"]]):
        f = p["root"] * 2.0 ** (st / 12)
        if f > 9000:
            continue
        x = reveal_note(p, f, s, rng) * 0.85 ** j
        d = n_of(j * rng.uniform(0.004, 0.012))
        y[d:] += x[:n - d]
    fc = 900 + 17000 * (1 - np.exp(-t_of(n) / max(p["open_ms"] / 1000, 0.005)))
    return _up(svf(y, fc, 0.6, "low"))


def _reveal_layers(p: dict, b: np.ndarray, rng) -> np.ndarray:
    n = len(b)
    out = _st(b).copy()
    if p["shimmer"] > 0.01:
        sh = fx_granular(bw(b[:n_of(1)], "high", 2500, 2), n / SR, rng)
        sh = _fit(sh, n) * np.exp(-t_of(n) / max(0.4, 0.6 * p["bloom_decay"]))[:, None]
        out = out + p["shimmer"] * _up(sh)
    if p["width"] > 0.01:
        out = out + _widen(bw(b, "high", 600, 2), p["width"])
    if p["verb"] > 0.01:
        for c in range(2):
            out[:, c] += p["verb"] * _conv(out[:, c], _hall(rng, p["verb_rt"], 0.35, 0.8, 0.02 + 0.004 * c), n)
    return out


def _reveal_hits(p: dict, out: np.ndarray, rng) -> np.ndarray:
    n, pk = len(out), np.max(np.abs(out))
    if p["hit"] > 0.01:
        hp = {**dict(_impact.FAMILIES["impact"])["punch"], "roll_hold": 0.5, "roll_fall": 0.25, "hall_rt": 1.4,
              "hall_wet": 0.35, "floor_db": -110, "crack_f": 3500}
        h = _st(_impact.render(hp, rng))[:n]
        out[:len(h)] += p["hit"] * pk * _up(h)
    if p["sub"] > 0.01:
        sd = {**dict(_subdrop.FAMILIES["sub_drop"])["808"], "length": 1.6, "click": 0.0}
        s = _st(_subdrop.render(sd, rng))[:n]
        out[:len(s)] += p["sub"] * pk * _up(s)
    return out


def _reveal_swell(p: dict, b: np.ndarray, pk: float, rng) -> np.ndarray:
    k = n_of(p["swell"])
    h = _hall(rng, max(0.6, 1.8 * p["swell"]), 0.3, 0.7, 0.0, 1.3 * p["swell"] + 0.2)
    sw = _conv(b[:n_of(0.25)], h, k)[::-1] * np.linspace(0, 1, k) ** 2
    sw = p["swell_level"] * pk * _up(dsp.fade(sw, 0.01, 0.002))
    return _st(sw)


def render_reveal(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A chord bloom with an optional build, hit, sub, shimmer and hall."""
    b = bloom(p, rng)
    out = _reveal_hits(p, _reveal_layers(p, b, rng), rng)
    if p["swell"] > 0.02:
        out = np.concatenate([_reveal_swell(p, b, np.max(np.abs(out)), rng), out])
    out = dsp.trim_tail(_hp_cut(out, p["low_cut"]), -66.0, 0.05)
    return dsp.fade(out, 0.001, min(0.4, 0.15 * len(out) / SR))


# ------------------------------------------------------------------------------------------------ launch (15.10)


def lead_whoosh(p: dict, n: int, rng) -> np.ndarray:
    t = t_of(n)
    u = t / max(t[-1], 1e-6)
    fc = p["lead_f0"] * (p["lead_f1"] / p["lead_f0"]) ** (u ** 1.7)
    lvl = 0.03 + 0.97 * (np.exp(4.5 * u) - 1) / (np.exp(4.5) - 1)
    base = _fit(dsp.air(n / SR, rng, center=2000.0, flutter=0.35, rate=7.0), n)
    chans = []
    for _ in range(2):
        w = 0.5 * base + 0.5 * _fit(dsp.air(n / SR, rng, center=2000.0, flutter=0.35, rate=7.0), n)
        y = svf(w, fc, 1.1, "bandn") + 0.35 * svf(w, np.minimum(2.2 * fc, 16000), 0.8, "bandn")
        chans.append(y * lvl)
    return _up(dsp.fade(np.column_stack(chans), 0.03, 0.002))


def lead_chirp(p: dict, n: int, rng) -> np.ndarray:
    t = t_of(n)
    u = t / max(t[-1], 1e-6)
    f = p["lead_f0"] * (p["lead_f1"] / p["lead_f0"]) ** (u ** 1.3)
    ph = 2 * np.pi * np.cumsum(f) / SR + rng.uniform(0, 0.3)
    y = np.sin(ph) + 0.35 * np.sin(2 * ph) * (2 * f < 0.4 * SR) + 0.15 * np.sin(3 * ph) * (3 * f < 0.4 * SR)
    y = _up(dsp.fade(y * (0.05 + 0.95 * u ** 2.2), 0.004, 0.0))
    return _st(y)


def lead_swell(p: dict, n: int, b: np.ndarray, rng) -> np.ndarray:
    h = _hall(rng, max(0.6, 1.8 * p["lead_s"]), 0.3, 0.7, 0.0, 1.3 * p["lead_s"] + 0.2)
    d = int(round(p.get("swell_trim", 0.0) * SR))
    sw = _fit(fftconvolve(b[:n_of(0.25)], h)[d:d + n], n)[::-1]
    sw = sw * (0.03 + 0.97 * np.linspace(0, 1, n) ** 2)
    return _up(dsp.fade(fx.decorrelate(sw, 0.6, 250), 0.03, 0.0))


def launch_tick(p: dict, n: int, rng) -> np.ndarray:
    w = bw(_noise_tick(n, p["tick_ms"], rng, 16), "high", p["tick_f"], 2)
    y = np.zeros(n)
    y[:len(w)] = _up(w)
    k = min(n, n_of(0.0004))
    y[:k] += 0.35 * (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(k) / max(1, k - 1)))
    return _up(y)


def launch_thump(p: dict, n: int) -> np.ndarray:
    t = t_of(n)
    f = p["thump_f"] * (1 + np.exp(-t / 0.03))
    return _up(np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t / 0.07) * (1 - np.exp(-t / 0.001)))


def launch_note(p: dict, f: float, s: float, rng) -> np.ndarray:
    if p["material"] == "fm":
        x = dsp.fm(f, 1.4, 2 + 4 * p["bright"], s, decay=10, index_decay=0.06, attack=0.002)
        x = x + 0.25 * dsp.fm(2 * f, 3.5, 1.5, s, decay=10, index_decay=0.03, attack=0.002)
        return _up(x)
    return dsp.bell(f, s, rng, _bell_partials(p["material"]), decay=1.2,
                    slope=0.5 if p["material"] in ("triangle", "glass") else 0.7, bright=p["bright"], beat=1.5,
                    strike=0.0)


def short_bloom(p: dict, n: int, rng) -> np.ndarray:
    """1 or 2 bloom notes with a flam and a concave decay."""
    s = n / SR
    t = t_of(n)
    y = np.zeros(n)
    for j, st in enumerate(DYADS[p["notes"]]):
        f = p["root"] * 2.0 ** (st / 12)
        if f > 9500:
            continue
        x = _fit(launch_note(p, f, s, rng), n) * 0.7 ** j
        y += _delayed(x, n_of(j * rng.uniform(0.006, 0.014)), n)
    e = (1 - p["ring_mix"]) * np.exp(-t / p["strike_tau"]) + p["ring_mix"] * np.exp(-t / p["ring_tau"])
    att = p["bloom_att"]
    if att <= 0.003:
        return _up(y * e)
    e = np.where(t < att, e * (1 - (1 - t / att) ** 2), e)
    fc = 1500 + 16000 * (1 - np.exp(-t / max(1.5 * att, 0.005)))
    return _up(svf(y * e, fc, 0.6, "low"))


def _launch_body(p: dict, b: np.ndarray, n: int, rng) -> np.ndarray:
    hit = p["tick"] * launch_tick(p, n, rng)
    if p["thump"] > 0.01:
        hit = hit + p["thump"] * launch_thump(p, n)
    body = _st(p["ring"] * b + hit).copy()
    if p["width"] > 0.01:
        body = body + _widen(p["ring"] * bw(b, "high", 600, 2), p["width"])
    if p["shimmer"] > 0.01:
        src = bw(b[:n_of(0.5)], "band", (2500, 9000), 4)
        k = p.get("shimmer_smooth", 0.0)
        sh = fx_granular(src, n / SR, rng) if k <= 0.005 else _smooth_shimmer(src, n / SR, k, rng)
        t = t_of(n)
        sh = _fit(sh, n) * ((1 - np.exp(-t / 0.02)) * np.exp(-t / p["shimmer_tau"]))[:, None]
        body = body + p["shimmer"] * p["ring"] * _up(sh)
    if p["verb"] > 0.01:
        send = bw(body, "high", 500, 4)
        for c in range(2):
            h = _hall(rng, p["verb_rt"], 0.6, 0.5, 0.008 + 0.003 * c)
            body[:, c] += p["verb"] * _conv(send[:, c], h, n)
    return body


def _launch_lead(p: dict, kind: str, m: int, b: np.ndarray, rng) -> np.ndarray:
    if kind == "chirp":
        return lead_chirp(p, m, rng)
    if kind == "swell":
        return lead_swell(p, m, b, rng)
    return lead_whoosh(p, m, rng)


def render_launch(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A lead-in, a bright hit, a short bloom with shimmer, a short room and a smooth fade."""
    kind = p["lead"] if p["lead_level"] > 0.005 else "none"
    n_lead = 0 if kind == "none" else n_of(p["lead_s"])
    n = n_of(min(4.0, max(0.6, 5.5 * p["ring_tau"], 6 * p["strike_tau"]) + 0.3))
    b = short_bloom(p, n, rng)
    d = n_of(p.get("bloom_delay", 0.0)) if p.get("bloom_delay", 0.0) > 0 else 0
    if d > 1:
        b[:d] *= 1 - (1 - np.linspace(0, 1, d)) ** 3
    body = _launch_body(p, b, n, rng)
    pk = np.max(np.abs(body))
    placed = n_lead > 64
    if placed:
        k = n_of(0.012)
        ld = _launch_lead(p, kind, n_lead + k, b, rng)
        ld[n_lead:] *= (np.linspace(1, 0, k) ** 2)[:, None]
        body = np.concatenate([np.zeros((n_lead, 2)), body])
        body[:n_lead + k] += p["lead_level"] * pk * ld
    out = _hp_cut(body, p["low_cut"])
    if p.get("low", 0.0) > 0.005:
        i = n_lead if placed else 0
        out[i:i + n] += (p["low"] * pk * low_strike(p["low_f"], p["low_tau"], n))[:, None]
    out = dsp.trim_tail(bw(out, "low", 16500, 4), -62.0, 0.05)
    return dsp.fade(out, 0.0015, min(0.3, 0.2 * len(out) / SR))


# ------------------------------------------------------------------------------------------------ gesture (15.11)


def g_sweep(n: int, f0: float, f1: float, T: float, c: float, rng) -> tuple[np.ndarray, np.ndarray]:
    u = np.clip(t_of(n) / max(T, 1e-4), 0, 1) ** c
    f = f0 * (f1 / f0) ** u
    return 2 * np.pi * np.cumsum(f) / SR + rng.uniform(0, 0.3), f


def _g_partials(p: dict, harm_tau: float) -> tuple:
    if p["g_mat"] == "sine" or p["g_mat"] not in G_MATS:
        return ((2, 0.55, harm_tau), (3, 0.22, 0.6 * harm_tau))
    return G_MATS[p["g_mat"]]


def g_tone(n: int, f: float, p: dict, rng, sink: float = 0.0, att: float = 0.0, harm_tau: float = 0.12) -> np.ndarray:
    """A sine with at most a few extra partials, a concave decay and an optional opening swell-in."""
    t = t_of(n)
    dur = max(t[-1] if n else 0.0, 0.1)
    fr = f * 2.0 ** (-sink * np.clip(t / dur, 0, 1) ** 0.7)
    if p["g_bend"] > 0.01:
        fr = fr * 2.0 ** ((p["g_bend"] / 12) * np.exp(-t / 0.025))
    ph = 2 * np.pi * np.cumsum(fr) / SR + rng.uniform(0, 6.28)
    ch = p["g_chorus"]
    det = (2.0 ** (8 / 1200) - 1) * 2 * np.pi * np.cumsum(fr) / SR

    def osc(k: float, phi: float) -> np.ndarray:
        if ch <= 0.01:
            return np.sin(k * ph + phi)
        side = np.sin(k * (ph + det) + phi + 1.3) + np.sin(k * (ph - det) + phi + 2.6)
        return (np.sin(k * ph + phi) + 0.5 * ch * side) / (1 + ch)

    y = osc(1, 0.0)
    for k, a, tau in _g_partials(p, harm_tau):
        a2 = a * p["g_warm"]
        if k * np.max(fr) < 0.42 * SR and a2 > 0:
            y = y + a2 * np.exp(-t / tau) * osc(k, rng.uniform(0, 6.28))
    e = (1 - p["g_mix"]) * np.exp(-t / p["g_tau1"]) + p["g_mix"] * np.exp(-t / p["g_tau2"])
    if att > 0.003:
        e = e * (1 - (1 - np.clip(t / att, 0, 1)) ** 2)
        y = svf(y * e, 900 + 15000 * (1 - np.exp(-t / max(1.2 * att, 0.004))), 0.6, "low")
    else:
        y = y * e * (1 - np.exp(-t / 0.0015))
    return _up(y)


def g_chord(n: int, p: dict, rng, att: float = 0.0) -> np.ndarray:
    y = np.zeros(n)
    for j, st in enumerate(CHORDS_G[p["g_notes"]]):
        f = p["g_root"] * 2.0 ** (st / 12)
        if f > 9000:
            continue
        x = g_tone(n, f, p, rng, att=att) * 0.72 ** j
        y += _delayed(x, n_of(j * rng.uniform(0.005, 0.012)), n)
    return _up(y)


def noise_ride(n: int, f: np.ndarray, k: float, q: float, rng) -> np.ndarray:
    y = svf(rng.standard_normal(n), np.clip(k * f, 200, 14000), q, "bandn")
    return bw(y, "low", 13000, 2)


def g_snap(n: int, p: dict, rng) -> np.ndarray:
    w = _noise_tick(n, p["g_snap_ms"], rng, 32)
    w = bw(w, "band", (p["g_snap_f"] / 1.6, min(1.8 * p["g_snap_f"], 16000)), 2)
    y = np.zeros(n)
    y[:len(w)] = _up(w)
    return y


def g_hit(n: int, p: dict, rng) -> np.ndarray:
    y = p["g_hit"] * low_strike(p["g_low_f"], p["g_low_tau"], n)
    if p["g_snap"] > 0.005:
        y = y + p["g_snap"] * g_snap(n, p, rng)
    return y / np.max(np.abs(y)) if np.max(np.abs(y)) > 0 else y


def g_tail(dry: np.ndarray, p: dict, rng) -> np.ndarray:
    """2 independent wet returns, fed only with content above 250 Hz; the dry part stays mono."""
    n = len(dry)
    out = _st(dry).copy()
    if p["g_wet"] <= 0.005:
        return out
    send = bw(dry, "high", 250, 2)
    if p["g_verb"] == "fdn":
        wet = fx.reverb(send, rng, rt60=p["g_rt"], size=0.7, predelay=0.012, damp=0.5, wet=1.0, dry=0.0,
                        stereo=True, width=1.0)
        return out + p["g_wet"] * _fit(_st(wet), n)
    for c in range(2):
        out[:, c] += p["g_wet"] * _conv(send, _hall(rng, p["g_rt"], 0.55, 0.6, 0.012 + 0.005 * c), n)
    return out


def _gesture_chirp_hum(p: dict, rng) -> np.ndarray:
    T, L = p["g_ms"] / 1000, p["g_length"]
    n = n_of(T + L)
    t = t_of(n)
    ph, f = g_sweep(n, p["g_f0"], p["g_f1"], T, 1.0, rng)
    u = np.clip(t / T, 0, 1)
    ec = (1 - np.exp(-t / 0.003)) * (0.3 + 0.7 * u) * np.clip((1.2 - t / T) / 0.2, 0, 1) ** 1.5
    lead = (np.sin(ph) + 0.12 * np.sin(2 * ph)) * ec
    if p["g_air"] > 0.005:
        lead = lead + 0.5 * p["g_air"] * noise_ride(n, f, 5, 3, rng) * ec
    t0 = 0.92 * T
    i0 = min(n, n_of(t0))
    tone = np.zeros(n)
    tone[i0:] = g_tone(n - i0, p["g_root"], p, rng, sink=p["g_sink"], harm_tau=0.2)
    y = p["g_lead"] * _up(lead) + p["g_tone"] * tone
    if p["g_land"] > 0.005:
        y = y + landing({"land": p["g_land"], "land_f": p["g_land_f"], "thud": 0.0, "hum_f": p["g_root"]}, n, t0)
    if p["g_snap"] > 0.005:
        y[i0:] += p["g_snap"] * g_snap(n - i0, p, rng)
    return y


def _gesture_zap_bloom(p: dict, rng) -> np.ndarray:
    T, L = p["g_ms"] / 1000, p["g_length"]
    n = n_of(T + L)
    t = t_of(n)
    _, f = g_sweep(n, p["g_f0"], p["g_f1"], T, 0.6, rng)
    ez = (1 - np.exp(-t / 0.002)) * np.clip(1 - t / (1.15 * T), 0, 1) ** 0.7
    nz = noise_ride(n, f, 1.3, 3, rng) * ez
    ph, f = g_sweep(n, p["g_f0"], p["g_f1"], T, 0.6, rng)
    sz = (np.sin(ph) + 0.1 * np.sin(2 * ph) * (2 * f < 0.42 * SR)) * ez
    zp = (1 - p["g_air"]) * _up(sz) + p["g_air"] * _up(nz)
    i0 = min(n, n_of(0.7 * T))
    bl = np.zeros(n)
    bl[i0:] = g_chord(n - i0, p, rng, att=p["g_att"])
    return p["g_lead"] * _up(zp) + p["g_tone"] * bl


def _gesture_sweep_hit(p: dict, rng) -> np.ndarray:
    T, L = p["g_ms"] / 1000, p["g_length"]
    nl = n_of(T)
    n = nl + n_of(L)
    t = t_of(n)
    ph, f = g_sweep(n, p["g_f0"], p["g_f1"], T, 1.6, rng)
    u = np.clip(t / T, 0, 1)
    lvl = (0.02 + 0.98 * (np.exp(4 * u) - 1) / (np.exp(4) - 1)) * (t < T + 0.006)
    lvl = lvl * np.clip((T + 0.006 - t) / 0.006, 0, 1) ** 0.5
    lead = (np.sin(ph) + 0.15 * np.sin(2 * ph)) * lvl
    if p["g_air"] > 0.005:
        lead = lead + p["g_air"] * noise_ride(n, f, 4, 1.4, rng) * lvl
    lead = dsp.fade(lead, 0.02, 0.0)
    hit, ring = np.zeros(n), np.zeros(n)
    hit[nl:] = g_hit(n - nl, p, rng)
    ring[nl:] = g_chord(n - nl, p, rng)
    return p["g_lead"] * _up(lead) + hit + p["g_tone"] * ring


def _gesture_taps_tone(p: dict, rng) -> np.ndarray:
    gap = p["g_gap"]
    i2 = n_of(2 * gap)
    n = i2 + n_of(p["g_length"])
    y = np.zeros(n)
    k = n_of(0.05)
    tk = t_of(k)
    for j, lv in ((0, 0.55), (1, 0.75)):
        fq = p["g_tap_f"] * (1 + 0.06 * np.exp(-tk / 0.004)) * 2.0 ** (2 * j / 12)
        tap = np.sin(2 * np.pi * np.cumsum(fq) / SR) * np.exp(-tk / 0.011) * (1 - np.exp(-tk / 0.0004))
        tap = tap + 0.25 * g_snap(k, {**p, "g_snap_ms": 2.5}, rng)
        i = n_of(j * gap)
        y[i:i + k] += (p["g_lead"] * lv * _up(tap))[:max(0, min(k, n - i))]
    y[i2:] += p["g_tone"] * g_chord(n - i2, p, rng, att=p["g_att"])
    if p["g_snap"] > 0.005:
        y[i2:] += 0.5 * p["g_snap"] * g_snap(n - i2, p, rng)
    return y


def _gesture_swell_sting(p: dict, rng) -> np.ndarray:
    T = p["g_ms"] / 1000
    nl, nb = n_of(T), n_of(p["g_length"])
    stab = g_chord(nb, p, rng)
    h = _hall(rng, max(0.6, 2 * T), 0.4, 0.7, 0.0, 1.3 * T + 0.25)
    k = n_of(0.012)
    a = n_of(0.02)
    sw = _fit(fftconvolve(stab[:n_of(0.3)], h)[a:a + nl + k], nl + k)[::-1]
    sw = _up(dsp.fade(sw * (0.02 + 0.98 * np.linspace(0, 1, len(sw)) ** 2.2), 0.03, 0.0))
    if p["g_air"] > 0.005:
        u = np.linspace(0, 1, len(sw))
        air = bw(rng.standard_normal(len(sw)) * u ** 3, "band", (3000, 12000), 2)
        sw = dsp.fade(_up(sw + p["g_air"] * _up(air)), 0.03, 0.0)
    sw[nl:] *= np.linspace(1, 0, len(sw) - nl) ** 2
    n = nl + nb
    y = np.zeros(n)
    m = min(len(sw), n)
    y[:m] += p["g_lead"] * sw[:m]
    y[nl:] += p["g_tone"] * stab + 0.8 * g_hit(nb, p, rng)
    return y


_GESTURE_FNS = {"chirp_hum": _gesture_chirp_hum, "zap_bloom": _gesture_zap_bloom,
                "sweep_hit": _gesture_sweep_hit, "taps_tone": _gesture_taps_tone}


def render_gesture(p: dict, rng: np.random.Generator) -> np.ndarray:
    """A launch-reveal motion: a mono dry part and a wide tail; an unknown gesture renders swell_sting."""
    y = _GESTURE_FNS.get(p["gesture"], _gesture_swell_sting)(p, rng)
    out = g_tail(bw(y, "high", 30, 2), p, rng)
    out = dsp.trim_tail(bw(out, "low", 16000, 4), -64.0, 0.05)
    return dsp.fade(out, 0.0008, min(0.25, 0.2 * len(out) / SR))


# ------------------------------------------------------------------------------------------------ render (15.7)

_IDIOMS = {"chirp": render_chirp, "launch": render_launch, "gesture": render_gesture}


def render(p: dict, rng: np.random.Generator) -> np.ndarray:
    """One sting, (n, 2) float64, normalised to an absolute peak of 0.89; p overlays the base of its mode."""
    p = p or {}
    mode = p.get("mode", DEFAULTS["mode"])
    q = {**BASES.get(mode, REVEAL), **p}
    y = _IDIOMS.get(mode, render_reveal)(q, rng)
    return y * 0.89 / (np.max(np.abs(y)) + 1e-12)
