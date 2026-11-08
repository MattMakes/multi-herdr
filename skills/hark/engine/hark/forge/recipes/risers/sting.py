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

REVEAL = {"mode": "reveal", "root": 880.0, "chord": "add9", "material": "bar", "bloom_decay": 2.2, "bright": 0.6,
          "open_ms": 120.0,
              "strike": 0.5, "shimmer": 0.25, "hit": 0.3, "sub": 0.25, "swell": 0.0, "swell_level": 0.2, "verb": 0.35,
              "verb_rt": 2.6,
              "width": 0.6, "low_cut": 25.0}
CHIRP_R2 = {"mode": "chirp", "chirp_f0": 1500.0, "chirp_f1": 150.0, "chirp_ms": 85.0, "chirp_harm": 0.7, "hum_f": 76.0,
                "hum_sink": 0.6, "hum_formant": 500.0, "hum_buzz": 0.9, "hum_level": 0.8, "hum_fast": 0.045,
                "hum_slow": 0.2,
                "hum_mix": 0.06, "length": 0.85, "sub": 0.0, "shimmer": 0.0, "verb": 0.05, "verb_rt": 0.8,
                "width": 0.0, "root": 880.0,
                "material": "bar", "bright": 0.5, "land": 0.0, "land_f": 800.0, "thud": 0.0, "sub_db": 45.0}
CHIRP_R3 = {**CHIRP_R2, **{"land": 0.5, "land_f": 1400.0, "thud": 0.5, "hum_level": 0.95, "hum_fast": 0.05,
                           "chirp_harm": 1.0,
                               "sub": 0.25, "sub_db": 100.0}}
CHIRP_R4 = {**CHIRP_R3, **{"zap": 0.0, "zap_k": 6.0, "ctick": 0.0, "double": 0.0, "double_gap": 0.05}}
LAUNCH = {"mode": "launch", "lead": "whoosh", "lead_s": 0.45, "lead_level": 0.45, "lead_f0": 600.0, "lead_f1": 9000.0,
          "tick": 1.0,
              "tick_f": 5000.0, "tick_ms": 6.0, "thump": 0.08, "thump_f": 150.0, "material": "glass", "root": 1318.5,
              "notes": "fifth",
              "bright": 0.7, "ring": 0.9, "strike_tau": 0.12, "ring_mix": 0.25, "ring_tau": 0.3, "bloom_att": 0.0,
              "shimmer": 0.4,
              "shimmer_tau": 0.35, "verb": 0.2, "verb_rt": 1.0, "width": 0.6, "low_cut": 140.0, "bloom_delay": 0.012}
GESTURE = {"mode": "gesture", "gesture": "chirp_hum", "g_f0": 1600.0, "g_f1": 160.0, "g_ms": 85.0, "g_lead": 1.0,
           "g_root": 76.0,
               "g_notes": "one", "g_warm": 0.35, "g_att": 0.0, "g_tau1": 0.06, "g_mix": 0.12, "g_tau2": 0.3,
               "g_sink": 0.5, "g_land": 0.5,
               "g_land_f": 800.0, "g_air": 0.25, "g_hit": 0.0, "g_low_f": 55.0, "g_low_tau": 0.15, "g_snap": 0.2,
               "g_snap_f": 5000.0,
               "g_snap_ms": 5.0, "g_gap": 0.075, "g_tap_f": 1400.0, "g_wet": 0.3, "g_rt": 1.0, "g_length": 0.8,
               "g_tone": 1.0,
               "g_mat": "sine", "g_bend": 0.0, "g_chorus": 0.0, "g_verb": "hall"}
BASES = {"chirp": CHIRP_R2, "launch": LAUNCH, "gesture": GESTURE, "reveal": REVEAL}

# ------------------------------------------------------------------------------------------------ families (15.5)


def _members(bed: dict, spec_rows: list[tuple[str, dict]]) -> list[tuple[str, dict]]:
    return [(label, {**bed, **extra}) for label, extra in spec_rows]


_LAUNCH_A = [
    ("whoosh_glass", {}),
    ("zip_ping", {"lead": "chirp", "lead_s": 0.16, "lead_level": 0.4, "lead_f0": 420.0, "lead_f1": 2600.0, "tick": 0.9,
                      "tick_f": 6000.0, "tick_ms": 4.0, "thump": 0.0, "material": "bar", "root": 1760.0,
                      "notes": "octave",
                      "strike_tau": 0.1, "ring_mix": 0.2, "bright": 0.45, "ring_tau": 0.28, "shimmer": 0.15,
                      "verb": 0.15, "width": 0.3,
                      "low_cut": 300.0}),
    ("reverse_bloom", {"lead": "swell", "lead_s": 0.5, "lead_level": 0.35, "tick": 0.8, "tick_f": 4500.0,
                       "tick_ms": 8.0,
                           "thump": 0.15, "thump_f": 110.0, "material": "bar", "root": 1480.0, "notes": "ninth",
                           "strike_tau": 0.15,
                           "ring_mix": 0.3, "ring_tau": 0.32, "shimmer": 0.3, "verb": 0.3, "verb_rt": 1.2, "width": 0.7,
                           "low_cut": 90.0}),
    ("spark", {"lead": "none", "lead_level": 0.3, "tick": 1.1, "tick_f": 7000.0, "tick_ms": 3.0, "thump": 0.0,
                   "material": "triangle", "root": 1318.5, "notes": "one", "strike_tau": 0.09, "bright": 0.35,
                   "ring_mix": 0.3,
                   "ring_tau": 0.28, "shimmer": 0.6, "shimmer_tau": 0.4, "verb": 0.15, "width": 0.9, "low_cut": 300.0}),
    ("soft_bloom", {"lead": "whoosh", "lead_s": 0.3, "lead_f0": 300.0, "lead_f1": 4000.0, "lead_level": 0.3,
                    "tick": 0.7,
                        "tick_f": 4000.0, "tick_ms": 10.0, "thump": 0.18, "thump_f": 130.0, "material": "tubular",
                        "root": 987.8,
                        "notes": "fourth", "bloom_att": 0.06, "strike_tau": 0.16, "ring_mix": 0.25, "ring_tau": 0.32,
                        "shimmer": 0.25, "verb": 0.3, "verb_rt": 1.1, "width": 0.5, "low_cut": 100.0}),
]
_LAUNCH_B = {"whoosh_glass": (0.04, 55, 0.16), "zip_ping": (0.044, 73, 0.1), "reverse_bloom": (0.045, 46, 0.24),
             "spark": (0.03, 82, 0.09), "soft_bloom": (0.041, 62, 0.2)}
_LAUNCH_C = {
    "whoosh_glass": {"lead_s": 0.1896, "lead_level": 0.4168, "tick": 0.7978, "tick_f": 3631.0, "thump": 0.1648,
                     "bright": 0.7076,
                         "ring": 0.9497, "strike_tau": 0.1913, "ring_mix": 0.2039, "ring_tau": 0.2963,
                         "shimmer": 0.2202,
                         "verb": 0.2948, "width": 0.319, "bloom_delay": 0.0078, "shimmer_smooth": 1.0},
    "zip_ping": {"shimmer": 0.105, "shimmer_smooth": 1.0},
    "reverse_bloom": {"swell_trim": 0.045, "lead_s": 0.36, "shimmer": 0.21, "shimmer_smooth": 1.0},
    "spark": {"shimmer": 0.42, "shimmer_smooth": 1.0},
    "soft_bloom": {"lead_s": 0.2471, "lead_level": 0.3126, "shimmer": 0.175, "shimmer_smooth": 1.0},
}
_LOGO_R3 = _members(LAUNCH, _LAUNCH_A)
_LOGO_R4 = [(label, {**cfg, "low": float(_LAUNCH_B[label][0]), "low_f": float(_LAUNCH_B[label][1]),
                    "low_tau": float(_LAUNCH_B[label][2])}) for label, cfg in _LOGO_R3]
_LOGO_R5 = [(label, {**cfg, **_LAUNCH_C[label]}) for label, cfg in _LOGO_R4]

_GESTURE_ROWS = [
    ("chirp_hum", {"g_f0": 1500.0, "g_f1": 150.0, "g_ms": 85.0, "g_root": 76.0, "g_warm": 0.15, "g_tau1": 0.07,
                   "g_mix": 0.15,
                       "g_tau2": 0.2, "g_sink": 0.5, "g_land": 0.8, "g_land_f": 900.0, "g_air": 0.1, "g_snap": 0.0,
                       "g_tone": 0.6,
                       "g_wet": 0.45, "g_rt": 1.4, "g_length": 0.9, "g_verb": "fdn"}),
    ("zap_bloom", {"gesture": "zap_bloom", "g_f0": 7000.0, "g_f1": 700.0, "g_ms": 60.0, "g_lead": 0.5, "g_air": 0.3,
                       "g_root": 261.6, "g_notes": "fifth", "g_warm": 0.35, "g_att": 0.04, "g_tau1": 0.08,
                       "g_mix": 0.15, "g_tau2": 0.28,
                       "g_snap": 0.0, "g_chorus": 0.2, "g_wet": 0.4, "g_rt": 1.2, "g_length": 0.9, "g_verb": "fdn"}),
    ("sweep_hit", {"gesture": "sweep_hit", "g_f0": 220.0, "g_f1": 1760.0, "g_ms": 320.0, "g_lead": 0.7, "g_air": 0.6,
                   "g_hit": 0.6,
                       "g_low_f": 52.0, "g_low_tau": 0.12, "g_tone": 0.55, "g_snap": 0.6, "g_snap_f": 3500.0,
                       "g_snap_ms": 8.0,
                       "g_root": 330.0, "g_notes": "fifth", "g_warm": 0.2, "g_tau1": 0.08, "g_mix": 0.12,
                       "g_tau2": 0.25, "g_wet": 0.3,
                       "g_rt": 1.0, "g_length": 0.75}),
    ("taps_tone", {"gesture": "taps_tone", "g_gap": 0.075, "g_tap_f": 1046.5, "g_lead": 0.8, "g_root": 523.25,
                   "g_notes": "one",
                       "g_mat": "bar", "g_warm": 0.35, "g_att": 0.0, "g_bend": 0.3, "g_tau1": 0.06, "g_mix": 0.12,
                       "g_tau2": 0.25,
                       "g_snap": 0.6, "g_snap_f": 7000.0, "g_snap_ms": 3.0, "g_wet": 0.35, "g_rt": 1.1, "g_length": 0.8,
                       "g_verb": "fdn"}),
    ("swell_sting", {"gesture": "swell_sting", "g_ms": 450.0, "g_lead": 0.6, "g_root": 293.66, "g_notes": "triad",
                         "g_warm": 0.12, "g_chorus": 0.4, "g_tau1": 0.08, "g_mix": 0.12, "g_tau2": 0.28, "g_hit": 0.0,
                         "g_low_f": 48.0,
                         "g_snap": 0.0, "g_air": 0.5, "g_wet": 0.45, "g_rt": 1.2, "g_length": 0.85}),
]
_CHIRP_HIGH = {"chirp_f0": 2600.0, "chirp_f1": 240.0, "chirp_ms": 70.0, "hum_f": 98.0, "hum_formant": 600.0,
               "length": 0.7,
                   "hum_slow": 0.16}
_PRO_R4 = [
    ("ref", {"ctick": 0.4}),
    ("zap", {**_CHIRP_HIGH, **{"land_f": 1100.0, "thud": 0.3, "hum_buzz": 0.55, "sub": 0.15, "sub_db": 110.0,
                               "zap": 0.6,
                                   "zap_k": 4.0, "chirp_harm": 0.5, "ctick": 0.2}}),
    ("deep", {"chirp_f0": 950.0, "chirp_f1": 100.0, "chirp_ms": 130.0, "hum_f": 56.0, "hum_formant": 380.0, "sub": 0.35,
                  "length": 1.0, "hum_slow": 0.26, "thud": 0.7, "land": 0.4, "land_f": 650.0, "hum_level": 1.05,
                  "hum_buzz": 1.0,
                  "sub_db": 100.0, "ctick": 0.3, "hum_sink": 1.0}),
    ("bell", {"shimmer": 0.1, "root": 1100.0, "hum_buzz": 0.3, "hum_formant": 520.0, "land": 0.35, "zap": 0.2,
              "zap_k": 8.0,
                  "thud": 0.4, "bell_decay": 0.16, "chirp_harm": 0.4, "ctick": 0.25, "hum_level": 0.75}),
    ("double", {"hum_slow": 0.3, "hum_mix": 0.08, "length": 1.0, "hum_sink": 0.8, "chirp_harm": 0.8, "land": 0.3,
                "thud": 0.55,
                    "sub": 0.2, "sub_db": 90.0, "double": 0.7, "double_gap": 0.012, "zap": 0.4, "zap_k": 7.0,
                    "chirp_f0": 1700.0,
                    "hum_f": 70.0}),
]
_PRO_R3 = [
    ("ref", {}),
    ("high", {**_CHIRP_HIGH, **{"land_f": 1100.0, "thud": 0.3, "hum_buzz": 0.55, "sub": 0.15, "sub_db": 110.0}}),
    ("deep", {"chirp_f0": 1200.0, "chirp_f1": 120.0, "chirp_ms": 105.0, "hum_f": 62.0, "hum_formant": 380.0,
              "sub": 0.35,
                  "length": 1.0, "hum_slow": 0.26, "thud": 0.7, "land": 0.4, "land_f": 650.0, "hum_level": 1.05,
                  "hum_buzz": 1.0,
                  "sub_db": 100.0}),
    ("bell", {"shimmer": 0.1, "root": 1100.0, "hum_buzz": 0.4, "hum_formant": 520.0, "land": 0.35, "thud": 0.4,
              "bell_decay": 0.16,
                  "chirp_f0": 1900.0, "chirp_f1": 190.0, "chirp_ms": 75.0, "hum_f": 84.0}),
    ("long", {"hum_slow": 0.45, "hum_mix": 0.1, "length": 1.3, "hum_sink": 0.8, "chirp_harm": 0.7, "verb": 0.12,
              "verb_rt": 1.2,
                  "land": 0.3, "thud": 0.55, "sub": 0.2, "sub_db": 80.0}),
]
_REVEAL_R2 = [
    ("glass_bloom", {"material": "glass", "root": 1760.0, "chord": "add9", "bloom_decay": 1.2, "bright": 0.8,
                     "strike": 0.8,
                         "shimmer": 0.35, "hit": 0.15, "sub": 0.0, "low_cut": 450.0, "open_ms": 30.0}),
    ("triangle", {"material": "triangle", "root": 1480.0, "chord": "fifth", "bloom_decay": 1.9, "bright": 0.85,
                  "open_ms": 10.0,
                      "strike": 0.9, "shimmer": 0.15, "hit": 0.1, "sub": 0.0, "verb": 0.25, "low_cut": 500.0}),
    ("fm_reveal", {"material": "fm", "root": 1320.0, "chord": "major", "bloom_decay": 1.3, "bright": 0.8, "swell": 0.6,
                       "swell_level": 0.25, "hit": 0.2, "sub": 0.0, "open_ms": 60.0, "low_cut": 350.0}),
    ("chime_hit", {"material": "tubular", "root": 1175.0, "chord": "sus2", "bloom_decay": 1.5, "bright": 0.75,
                   "strike": 0.8,
                       "hit": 0.3, "sub": 0.0, "shimmer": 0.1, "verb": 0.4, "verb_rt": 2.4, "low_cut": 300.0,
                       "open_ms": 25.0}),
    ("bar_sparkle", {"material": "bar", "root": 1980.0, "chord": "octave", "bloom_decay": 1.1, "bright": 0.8,
                     "strike": 0.9,
                         "shimmer": 0.55, "hit": 0.0, "sub": 0.0, "width": 0.9, "open_ms": 10.0, "low_cut": 500.0}),
]
_PRO_R2 = [
    ("ref", {}),
    ("high", dict(_CHIRP_HIGH)),
    ("deep", {"chirp_f0": 1200.0, "chirp_f1": 120.0, "chirp_ms": 105.0, "hum_f": 62.0, "hum_formant": 380.0,
              "sub": 0.25,
                  "length": 1.0, "hum_slow": 0.26}),
    ("bell", {"shimmer": 0.1, "root": 950.0, "hum_buzz": 0.4, "hum_formant": 520.0}),
    ("long", {"hum_slow": 0.45, "hum_mix": 0.1, "length": 1.3, "hum_sink": 0.8, "chirp_harm": 0.7, "verb": 0.12,
              "verb_rt": 1.2}),
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
    klist, lims = SEARCH[cat]
    return {kn: lims.get(kn, SPACE[kn]) for kn in klist}


_RECIPE_TABLES = (FAMILIES, TARGETS, ALTS)


def recipe_for(cat: str, preset: int | str = 0) -> types.SimpleNamespace:
    """A recipe object that searches the category keys around one preset (index or name)."""
    fams, tgts, alt_list = _RECIPE_TABLES
    label, pset = fams[cat][preset] if isinstance(preset, int) else (preset, dict(fams[cat])[preset])
    return types.SimpleNamespace(MODULE=MODULE, SPACE=space_for(cat), DEFAULTS=dict(pset), render=render,
                                 TARGET=tgts[cat], ALTS=list(alt_list), REFSET=cat, PRESET=label)


# ------------------------------------------------------------------------------------------------ small helpers


def _up(wav: np.ndarray) -> np.ndarray:
    """Unit peak."""
    return wav / (np.max(np.abs(wav)) + 1e-12)


def _st(wav: np.ndarray) -> np.ndarray:
    return wav if wav.ndim == 2 else np.column_stack([wav, wav])


def _fit(wav: np.ndarray, ns: int) -> np.ndarray:
    if len(wav) >= ns:
        return wav[:ns]
    return np.concatenate([wav, np.zeros((ns - len(wav),) + wav.shape[1:])])


def _delayed(wav: np.ndarray, lag: int, ns: int) -> np.ndarray:
    mix = np.zeros((ns,) + wav.shape[1:])
    if lag < ns:
        cnt = min(len(wav), ns - lag)
        mix[lag:lag + cnt] = wav[:cnt]
    return mix


def _conv(wav: np.ndarray, ir: np.ndarray, ns: int) -> np.ndarray:
    return _fit(fftconvolve(wav, ir), ns)


def _hall(gen, rtime: float, hfd: float, room: float, pdel: float, span_s: float | None = None) -> np.ndarray:
    return hall_ir(rtime, hfd, room, pdel, span_s, rng=gen)


def _hp_cut(wav: np.ndarray, lcut: float) -> np.ndarray:
    return bw(wav, "high", lcut, 2 if lcut < 60 else 4)


def _widen(upper: np.ndarray, wd: float) -> np.ndarray:
    """The part above 600 Hz made wide: decor(hi) - [hi, hi]."""
    return fx.decorrelate(upper, 0.8 * wd, 250) - _st(upper)


def fx_granular(feed: np.ndarray, span_s: float, gen: np.random.Generator) -> np.ndarray:
    """The coarse shimmer cloud: grains 50 ms, 50 per second, an octave up."""
    return fx.granular(feed, span_s, gen, grain=0.05, density=50, pos=(0.0, 0.6), pos_jitter=0.05, pitch=12.0,
                       pitch_jitter=0.15, spread=0.9)


def _smooth_shimmer(feed: np.ndarray, span_s: float, kn: float, gen) -> np.ndarray:
    return fx.granular(feed, span_s, gen, grain=0.05 * (1 + 0.8 * kn), density=50 * (1 + 3 * kn), pos=(0.0, 0.6),
                       pos_jitter=0.05, pitch=12.0, pitch_jitter=0.15, spread=0.9)


def _noise_tick(ns: int, msec: float, gen, floor_n: int) -> np.ndarray:
    """White noise with a cubic fall and a 0.2 ms rise, L samples long."""
    span = min(ns, max(floor_n, n_of(msec / 1000)))
    ix = np.arange(span)
    return gen.standard_normal(span) * (1 - ix / span) ** 3 * (1 - np.exp(-ix / (0.0002 * SR)))


def low_strike(lf: float, lt: float, ns: int) -> np.ndarray:
    """A pitched low strike from 2.5 x low_f onto low_f, rising over 12 ms (no random draws)."""
    tax = t_of(ns)
    phase = 2 * np.pi * np.cumsum(lf * (1 + 1.5 * np.exp(-tax / 0.035))) / SR
    sig = (np.sin(phase) + 0.25 * np.sin(2 * phase)) * np.clip(tax / 0.012, 0, 1) ** 2 * np.exp(-tax / lt)
    return _up(bw(sig, "high", 25, 2))


# ------------------------------------------------------------------------------------------------ chirp (15.8)


def _chirp_track(cfg: dict, ns: int) -> tuple[int, float, np.ndarray, np.ndarray]:
    dur = cfg["chirp_ms"] / 1000
    span = min(ns, n_of(1.25 * dur))
    tax = t_of(span)
    freq = cfg["chirp_f0"] * (cfg["chirp_f1"] / cfg["chirp_f0"]) ** np.clip(tax / dur, 0, 1)
    return span, dur, tax, freq


def chirp(cfg: dict, ns: int, gen) -> np.ndarray:
    span, dur, tax, freq = _chirp_track(cfg, ns)
    phase = 2 * np.pi * np.cumsum(freq) / SR + gen.uniform(0, 0.3)
    sig = np.sin(phase) + cfg["chirp_harm"] * (0.5 * np.sin(2 * phase) + 0.25 * np.sin(3 * phase))
    sig[2 * freq >= 0.45 * SR] = 0.0
    env = (1 - np.exp(-tax / 0.003)) * (0.3 + 0.7 * np.clip(tax / dur, 0, 1)) * np.clip((1.25 - tax / dur) / 0.25, 0,
                                                                                        1) ** 1.5
    mix = np.zeros(ns)
    mix[:span] = sig * env
    return _up(mix)


def _hum_partials(cfg: dict, freq: np.ndarray, gen) -> np.ndarray:
    """12 harmonics of the hum, shaped by a formant at hum_formant and the buzz amount."""
    phase = 2 * np.pi * np.cumsum(freq) / SR
    sig = np.zeros(len(freq))
    for kn in range(1, 13):
        amp = (1 / kn) ** (1.6 - 1.1 * cfg["hum_buzz"])
        fmt = 1 / (1 + (np.log2(kn * freq / cfg["hum_formant"]) / 0.7) ** 2)
        sig += np.where(kn * freq < 0.45 * SR, amp * (0.35 + fmt) * np.sin(kn * phase + gen.uniform(0, 6.28)), 0.0)
    return sig


def hum(cfg: dict, ns: int, onset: float, gen) -> np.ndarray:
    tax = t_of(ns)
    since = np.maximum(tax - onset, 0)
    freq = cfg["hum_f"] * 2.0 ** (-cfg["hum_sink"] * np.clip(since / max(tax[-1] - onset, 0.1), 0, 1) ** 0.7)
    decays = np.exp(-since / cfg["hum_fast"]) + cfg["hum_mix"] * np.exp(-since / cfg["hum_slow"])
    env = decays * (1 - np.exp(-since / 0.004))
    return _up(_hum_partials(cfg, freq, gen) * np.where(tax >= onset, env, 0.0))


def landing(cfg: dict, ns: int, onset: float) -> np.ndarray:
    """A landing blob that bends down and a thud onto 2 x hum_f (no random draws)."""
    tax = t_of(ns)
    since = np.maximum(tax - onset, 0)
    gate = (tax >= onset) * (1 - np.exp(-since / 0.002))
    alive = since > 0
    sig = np.zeros(ns)
    if cfg["land"] > 0.01:
        freq = cfg["land_f"] * 2.0 ** (-0.5 * (1 - np.exp(-since / 0.03)))
        sig += cfg["land"] * np.sin(2 * np.pi * np.cumsum(freq * alive) / SR) * np.exp(-since / 0.025) * gate
    if cfg["thud"] > 0.01:
        freq = 2 * cfg["hum_f"] * (1 + np.exp(-since / 0.04))
        sig += cfg["thud"] * np.sin(2 * np.pi * np.cumsum(freq * alive) / SR) * np.exp(-since / 0.05) * gate
    return sig


def zap(cfg: dict, ns: int, gen) -> np.ndarray:
    span, dur, tax, freq = _chirp_track(cfg, ns)
    sig = svf(gen.standard_normal(span), np.minimum(cfg["zap_k"] * freq, 15000), 3, "bandn")
    env = (1 - np.exp(-tax / 0.002)) * (0.4 + 0.6 * np.clip(tax / dur, 0, 1)) * np.clip((1.25 - tax / dur) / 0.25, 0,
                                                                                        1) ** 1.5
    mix = np.zeros(ns)
    mix[:span] = bw(sig * env, "low", 13000, 2)
    return _up(mix)


def ctick(ns: int, onset: float, gen) -> np.ndarray:
    span = n_of(0.005)
    ix = np.arange(span)
    burst = bw(gen.standard_normal(span) * (1 - ix / span) ** 3 * (1 - np.exp(-ix / (0.0002 * SR))), "band", (5000,
                                                                                                              13000), 2)
    return _delayed(burst / (np.max(np.abs(burst)) + 1e-12), n_of(onset), ns)


def _chirp_sub(cfg: dict, ns: int, onset: float, gen) -> np.ndarray:
    subcfg = {**dict(_subdrop.FAMILIES["pro_sub_drop"])["ref"], "f0": cfg["hum_f"], "f1": 0.66 * cfg["hum_f"],
          "bloom": 0.06, "click": 0.0, "length": cfg["length"], "decay_db": cfg.get("sub_db", 45.0)}
    secs = _subdrop.render(subcfg, gen)
    secs = secs.mean(axis=1) if secs.ndim == 2 else secs
    return _delayed(secs, n_of(onset), ns)


def _pre_chirp(cfg: dict, gen) -> np.ndarray:
    shifted = {**cfg, "chirp_f0": min(3500.0, 1.35 * cfg["chirp_f0"]), "chirp_f1": 2.5 * cfg["chirp_f1"],
         "chirp_ms": 0.55 * cfg["chirp_ms"]}
    kn = n_of(1.25 * shifted["chirp_ms"] / 1000)
    pre = chirp(shifted, kn, gen)
    if cfg.get("zap", 0.0) > 0.005:
        pre = pre + cfg["zap"] * zap(shifted, kn, gen)
    return np.concatenate([_up(pre), np.zeros(n_of(cfg.get("double_gap", 0.05)))])


def _chirp_core(cfg: dict, ns: int, onset: float, gen) -> np.ndarray:
    """The chirp, the hum it lands on, the landing blob and thud, the zap and the landing tick."""
    sig = chirp(cfg, ns, gen) + cfg["hum_level"] * hum(cfg, ns, onset, gen)
    if cfg.get("land", 0.0) > 0.01 or cfg.get("thud", 0.0) > 0.01:
        sig = sig + landing(cfg, ns, onset)
    if cfg.get("zap", 0.0) > 0.005:
        sig = sig + cfg["zap"] * zap(cfg, ns, gen)
    if cfg.get("ctick", 0.0) > 0.005:
        sig = sig + cfg["ctick"] * ctick(ns, onset, gen)
    return sig


def _chirp_extras(cfg: dict, sig: np.ndarray, onset: float, gen) -> np.ndarray:
    """The sub drop and the bell partial from the landing, then the hall."""
    ns = len(sig)
    if cfg["sub"] > 0.01:
        sig = sig + cfg["sub"] * _chirp_sub(cfg, ns, onset, gen)
    if cfg["shimmer"] > 0.01:
        bl = dsp.bell(cfg["root"], max(0.3, cfg["length"] - onset), gen, "bar", decay=cfg.get("bell_decay", 0.35),
                     bright=0.3, strike=0.0)
        sig = sig + 0.6 * cfg["shimmer"] * _delayed(bl, n_of(onset), ns)
    if cfg["verb"] > 0.01:
        sig = sig + cfg["verb"] * _conv(sig, _hall(gen, cfg["verb_rt"], 0.6, 0.5, 0.008), ns)
    return sig


def render_chirp(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """A falling chirp landing on a low hum; L = R."""
    onset = 0.92 * cfg["chirp_ms"] / 1000
    sig = _chirp_extras(cfg, _chirp_core(cfg, n_of(cfg["length"]), onset, gen), onset, gen)
    if cfg.get("double", 0.0) > 0.005:
        sig = np.concatenate([cfg["double"] * np.max(np.abs(sig)) * _pre_chirp(cfg, gen), sig])
    sig = dsp.fade(bw(sig, "high", 30, 2), 0.0008, min(0.12, 0.2 * cfg["length"]))
    return np.column_stack([sig, sig])


# ------------------------------------------------------------------------------------------------ reveal (15.9)


def _bell_partials(matname: str) -> Any:
    return TRIANGLE if matname == "triangle" else matname


def reveal_note(cfg: dict, freq: float, secs: float, gen) -> np.ndarray:
    if cfg["material"] == "fm":
        bdec = cfg["bloom_decay"]
        wav = dsp.fm(freq, 1.4, 3 + 5 * cfg["bright"], secs, decay=0.45 * bdec, index_decay=0.12, attack=0.002)
        wav = wav + 0.35 * dsp.fm(2 * freq, 3.5, 2.0, secs, decay=0.2 * bdec, index_decay=0.05, attack=0.002)
        return _up(wav)
    is_tri = cfg["material"] == "triangle"
    return dsp.bell(freq, secs, gen, _bell_partials(cfg["material"]),
                    decay=cfg["bloom_decay"] * (0.35 if is_tri else 0.5),
                    slope=0.5 if cfg["material"] in ("triangle", "glass") else 0.8, bright=cfg["bright"], beat=1.2,
                    strike=0.2 + 0.6 * cfg["strike"])


def bloom(cfg: dict, gen) -> np.ndarray:
    secs = min(5.5, 1.6 * cfg["bloom_decay"] + 0.4)
    ns = n_of(secs)
    sig = np.zeros(ns)
    for nth, semi in enumerate(CHORDS[cfg["chord"]]):
        freq = cfg["root"] * 2.0 ** (semi / 12)
        if freq > 9000:
            continue
        wav = reveal_note(cfg, freq, secs, gen) * 0.85 ** nth
        lag = n_of(nth * gen.uniform(0.004, 0.012))
        sig[lag:] += wav[:ns - lag]
    cut = 900 + 17000 * (1 - np.exp(-t_of(ns) / max(cfg["open_ms"] / 1000, 0.005)))
    return _up(svf(sig, cut, 0.6, "low"))


def _reveal_layers(cfg: dict, bl: np.ndarray, gen) -> np.ndarray:
    ns = len(bl)
    mix = _st(bl).copy()
    if cfg["shimmer"] > 0.01:
        gr = fx_granular(bw(bl[:n_of(1)], "high", 2500, 2), ns / SR, gen)
        gr = _fit(gr, ns) * np.exp(-t_of(ns) / max(0.4, 0.6 * cfg["bloom_decay"]))[:, None]
        mix = mix + cfg["shimmer"] * _up(gr)
    if cfg["width"] > 0.01:
        mix = mix + _widen(bw(bl, "high", 600, 2), cfg["width"])
    if cfg["verb"] > 0.01:
        for chan in range(2):
            mix[:, chan] += cfg["verb"] * _conv(mix[:, chan], _hall(gen, cfg["verb_rt"], 0.35, 0.8,
                                                                    0.02 + 0.004 * chan), ns)
    return mix


def _reveal_hits(cfg: dict, mix: np.ndarray, gen) -> np.ndarray:
    ns, peak = len(mix), np.max(np.abs(mix))
    if cfg["hit"] > 0.01:
        hitcfg = {**dict(_impact.FAMILIES["impact"])["punch"], "roll_hold": 0.5, "roll_fall": 0.25, "hall_rt": 1.4,
              "hall_wet": 0.35, "floor_db": -110, "crack_f": 3500}
        ir = _st(_impact.render(hitcfg, gen))[:ns]
        mix[:len(ir)] += cfg["hit"] * peak * _up(ir)
    if cfg["sub"] > 0.01:
        subcfg = {**dict(_subdrop.FAMILIES["sub_drop"])["808"], "length": 1.6, "click": 0.0}
        secs = _st(_subdrop.render(subcfg, gen))[:ns]
        mix[:len(secs)] += cfg["sub"] * peak * _up(secs)
    return mix


def _reveal_swell(cfg: dict, bl: np.ndarray, peak: float, gen) -> np.ndarray:
    kn = n_of(cfg["swell"])
    ir = _hall(gen, max(0.6, 1.8 * cfg["swell"]), 0.3, 0.7, 0.0, 1.3 * cfg["swell"] + 0.2)
    rise = _conv(bl[:n_of(0.25)], ir, kn)[::-1] * np.linspace(0, 1, kn) ** 2
    rise = cfg["swell_level"] * peak * _up(dsp.fade(rise, 0.01, 0.002))
    return _st(rise)


def render_reveal(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """A chord bloom with an optional build, hit, sub, shimmer and hall."""
    bl = bloom(cfg, gen)
    mix = _reveal_hits(cfg, _reveal_layers(cfg, bl, gen), gen)
    if cfg["swell"] > 0.02:
        mix = np.concatenate([_reveal_swell(cfg, bl, np.max(np.abs(mix)), gen), mix])
    mix = dsp.trim_tail(_hp_cut(mix, cfg["low_cut"]), -66.0, 0.05)
    return dsp.fade(mix, 0.001, min(0.4, 0.15 * len(mix) / SR))


# ------------------------------------------------------------------------------------------------ launch (15.10)


def lead_whoosh(cfg: dict, ns: int, gen) -> np.ndarray:
    tax = t_of(ns)
    prog = tax / max(tax[-1], 1e-6)
    cut = cfg["lead_f0"] * (cfg["lead_f1"] / cfg["lead_f0"]) ** (prog ** 1.7)
    gain = 0.03 + 0.97 * (np.exp(4.5 * prog) - 1) / (np.exp(4.5) - 1)
    bed = _fit(dsp.air(ns / SR, gen, center=2000.0, flutter=0.35, rate=7.0), ns)
    pair = []
    for _ in range(2):
        burst = 0.5 * bed + 0.5 * _fit(dsp.air(ns / SR, gen, center=2000.0, flutter=0.35, rate=7.0), ns)
        sig = svf(burst, cut, 1.1, "bandn") + 0.35 * svf(burst, np.minimum(2.2 * cut, 16000), 0.8, "bandn")
        pair.append(sig * gain)
    return _up(dsp.fade(np.column_stack(pair), 0.03, 0.002))


def lead_chirp(cfg: dict, ns: int, gen) -> np.ndarray:
    tax = t_of(ns)
    prog = tax / max(tax[-1], 1e-6)
    freq = cfg["lead_f0"] * (cfg["lead_f1"] / cfg["lead_f0"]) ** (prog ** 1.3)
    phase = 2 * np.pi * np.cumsum(freq) / SR + gen.uniform(0, 0.3)
    sig = (np.sin(phase) + 0.35 * np.sin(2 * phase) * (2 * freq < 0.4 * SR)
           + 0.15 * np.sin(3 * phase) * (3 * freq < 0.4 * SR))
    sig = _up(dsp.fade(sig * (0.05 + 0.95 * prog ** 2.2), 0.004, 0.0))
    return _st(sig)


def lead_swell(cfg: dict, ns: int, bl: np.ndarray, gen) -> np.ndarray:
    ir = _hall(gen, max(0.6, 1.8 * cfg["lead_s"]), 0.3, 0.7, 0.0, 1.3 * cfg["lead_s"] + 0.2)
    lag = int(round(cfg.get("swell_trim", 0.0) * SR))
    rise = _fit(fftconvolve(bl[:n_of(0.25)], ir)[lag:lag + ns], ns)[::-1]
    rise = rise * (0.03 + 0.97 * np.linspace(0, 1, ns) ** 2)
    return _up(dsp.fade(fx.decorrelate(rise, 0.6, 250), 0.03, 0.0))


def launch_tick(cfg: dict, ns: int, gen) -> np.ndarray:
    burst = bw(_noise_tick(ns, cfg["tick_ms"], gen, 16), "high", cfg["tick_f"], 2)
    sig = np.zeros(ns)
    sig[:len(burst)] = _up(burst)
    kn = min(ns, n_of(0.0004))
    sig[:kn] += 0.35 * (0.5 - 0.5 * np.cos(2 * np.pi * np.arange(kn) / max(1, kn - 1)))
    return _up(sig)


def launch_thump(cfg: dict, ns: int) -> np.ndarray:
    tax = t_of(ns)
    freq = cfg["thump_f"] * (1 + np.exp(-tax / 0.03))
    return _up(np.sin(2 * np.pi * np.cumsum(freq) / SR) * np.exp(-tax / 0.07) * (1 - np.exp(-tax / 0.001)))


def launch_note(cfg: dict, freq: float, secs: float, gen) -> np.ndarray:
    if cfg["material"] == "fm":
        wav = dsp.fm(freq, 1.4, 2 + 4 * cfg["bright"], secs, decay=10, index_decay=0.06, attack=0.002)
        wav = wav + 0.25 * dsp.fm(2 * freq, 3.5, 1.5, secs, decay=10, index_decay=0.03, attack=0.002)
        return _up(wav)
    return dsp.bell(freq, secs, gen, _bell_partials(cfg["material"]), decay=1.2,
                    slope=0.5 if cfg["material"] in ("triangle", "glass") else 0.7, bright=cfg["bright"], beat=1.5,
                    strike=0.0)


def short_bloom(cfg: dict, ns: int, gen) -> np.ndarray:
    """1 or 2 bloom notes with a flam and a concave decay."""
    secs = ns / SR
    tax = t_of(ns)
    sig = np.zeros(ns)
    for nth, semi in enumerate(DYADS[cfg["notes"]]):
        freq = cfg["root"] * 2.0 ** (semi / 12)
        if freq > 9500:
            continue
        wav = _fit(launch_note(cfg, freq, secs, gen), ns) * 0.7 ** nth
        sig += _delayed(wav, n_of(nth * gen.uniform(0.006, 0.014)), ns)
    env = (1 - cfg["ring_mix"]) * np.exp(-tax / cfg["strike_tau"]) + cfg["ring_mix"] * np.exp(-tax / cfg["ring_tau"])
    atk = cfg["bloom_att"]
    if atk <= 0.003:
        return _up(sig * env)
    env = np.where(tax < atk, env * (1 - (1 - tax / atk) ** 2), env)
    cut = 1500 + 16000 * (1 - np.exp(-tax / max(1.5 * atk, 0.005)))
    return _up(svf(sig * env, cut, 0.6, "low"))


def _launch_shimmer(cfg: dict, bl: np.ndarray, ns: int, gen) -> np.ndarray:
    """The granular layer from the bloom's 2.5 to 9 kHz band: coarse, or smooth when shimmer_smooth is set."""
    feed = bw(bl[:n_of(0.5)], "band", (2500, 9000), 4)
    kn = cfg.get("shimmer_smooth", 0.0)
    gr = fx_granular(feed, ns / SR, gen) if kn <= 0.005 else _smooth_shimmer(feed, ns / SR, kn, gen)
    tax = t_of(ns)
    return _up(_fit(gr, ns) * ((1 - np.exp(-tax / 0.02)) * np.exp(-tax / cfg["shimmer_tau"]))[:, None])


def _launch_room(cfg: dict, core: np.ndarray, gen) -> np.ndarray:
    """A short hall per channel, fed only above 500 Hz."""
    feedv = bw(core, "high", 500, 4)
    for chan in range(2):
        ir = _hall(gen, cfg["verb_rt"], 0.6, 0.5, 0.008 + 0.003 * chan)
        core[:, chan] += cfg["verb"] * _conv(feedv[:, chan], ir, len(core))
    return core


def _launch_body(cfg: dict, bl: np.ndarray, ns: int, gen) -> np.ndarray:
    hit_sig = cfg["tick"] * launch_tick(cfg, ns, gen)
    if cfg["thump"] > 0.01:
        hit_sig = hit_sig + cfg["thump"] * launch_thump(cfg, ns)
    core = _st(cfg["ring"] * bl + hit_sig).copy()
    if cfg["width"] > 0.01:
        core = core + _widen(cfg["ring"] * bw(bl, "high", 600, 2), cfg["width"])
    if cfg["shimmer"] > 0.01:
        core = core + cfg["shimmer"] * cfg["ring"] * _launch_shimmer(cfg, bl, ns, gen)
    return _launch_room(cfg, core, gen) if cfg["verb"] > 0.01 else core


def _launch_lead(cfg: dict, style: str, cnt: int, bl: np.ndarray, gen) -> np.ndarray:
    if style == "chirp":
        return lead_chirp(cfg, cnt, gen)
    if style == "swell":
        return lead_swell(cfg, cnt, bl, gen)
    return lead_whoosh(cfg, cnt, gen)


def _with_lead(cfg: dict, core: np.ndarray, bl: np.ndarray, n_lead: int, gen) -> np.ndarray:
    """The lead-in before the hit; it runs 12 ms past the hit start and fades out there."""
    style = cfg["lead"] if cfg["lead_level"] > 0.005 else "none"
    kn = n_of(0.012)
    lead_sig = _launch_lead(cfg, style, n_lead + kn, bl, gen)
    lead_sig[n_lead:] *= (np.linspace(1, 0, kn) ** 2)[:, None]
    mix = np.concatenate([np.zeros((n_lead, 2)), core])
    mix[:n_lead + kn] += cfg["lead_level"] * np.max(np.abs(core)) * lead_sig
    return mix


def _lead_samples(cfg: dict) -> int:
    """The lead length; 0 for no lead, a quiet lead, or a lead of 64 samples or less."""
    n_lead = 0 if cfg["lead"] == "none" or cfg["lead_level"] <= 0.005 else n_of(cfg["lead_s"])
    return n_lead if n_lead > 64 else 0


def _bloom_fade_in(bl: np.ndarray, pause: float) -> np.ndarray:
    lag = n_of(pause) if pause > 0 else 0
    if lag > 1:
        bl[:lag] *= 1 - (1 - np.linspace(0, 1, lag)) ** 3
    return bl


def render_launch(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """A lead-in, a bright hit, a short bloom with shimmer, a short room and a smooth fade."""
    ns = n_of(min(4.0, max(0.6, 5.5 * cfg["ring_tau"], 6 * cfg["strike_tau"]) + 0.3))
    bl = _bloom_fade_in(short_bloom(cfg, ns, gen), cfg.get("bloom_delay", 0.0))
    core = _launch_body(cfg, bl, ns, gen)
    peak = np.max(np.abs(core))
    n_lead = _lead_samples(cfg)
    mix = _hp_cut(_with_lead(cfg, core, bl, n_lead, gen) if n_lead else core, cfg["low_cut"])
    if cfg.get("low", 0.0) > 0.005:
        mix[n_lead:n_lead + ns] += (cfg["low"] * peak * low_strike(cfg["low_f"], cfg["low_tau"], ns))[:, None]
    mix = dsp.trim_tail(bw(mix, "low", 16500, 4), -62.0, 0.05)
    return dsp.fade(mix, 0.0015, min(0.3, 0.2 * len(mix) / SR))


# ------------------------------------------------------------------------------------------------ gesture (15.11)


def g_sweep(ns: int, fa: float, fb: float, dur: float, chan: float, gen) -> tuple[np.ndarray, np.ndarray]:
    prog = np.clip(t_of(ns) / max(dur, 1e-4), 0, 1) ** chan
    freq = fa * (fb / fa) ** prog
    return 2 * np.pi * np.cumsum(freq) / SR + gen.uniform(0, 0.3), freq


def _g_partials(cfg: dict, ovt_tau: float) -> tuple:
    if cfg["g_mat"] == "sine" or cfg["g_mat"] not in G_MATS:
        return ((2, 0.55, ovt_tau), (3, 0.22, 0.6 * ovt_tau))
    return G_MATS[cfg["g_mat"]]


def _g_freq(cfg: dict, freq: float, tax: np.ndarray, drop: float) -> np.ndarray:
    """The tone pitch: it sinks sink octaves over the tone, and starts g_bend semitones sharp."""
    fcurve = freq * 2.0 ** (-drop * np.clip(tax / max(tax[-1] if len(tax) else 0.0, 0.1), 0, 1) ** 0.7)
    if cfg["g_bend"] > 0.01:
        fcurve = fcurve * 2.0 ** ((cfg["g_bend"] / 12) * np.exp(-tax / 0.025))
    return fcurve


def _g_osc(phase: np.ndarray, dtn: np.ndarray, chor: float, kn: float, phs0: float) -> np.ndarray:
    """A partial k of the tone, with +-8 cent chorus copies when g_chorus is set."""
    if chor <= 0.01:
        return np.sin(kn * phase + phs0)
    flank = np.sin(kn * (phase + dtn) + phs0 + 1.3) + np.sin(kn * (phase - dtn) + phs0 + 2.6)
    return (np.sin(kn * phase + phs0) + 0.5 * chor * flank) / (1 + chor)


def _g_voice(cfg: dict, tax: np.ndarray, fcurve: np.ndarray, ovt_tau: float, gen) -> np.ndarray:
    cyc = 2 * np.pi * np.cumsum(fcurve) / SR
    phase = cyc + gen.uniform(0, 6.28)
    dtn = (2.0 ** (8 / 1200) - 1) * cyc
    sig = _g_osc(phase, dtn, cfg["g_chorus"], 1, 0.0)
    for kn, amp, tc in _g_partials(cfg, ovt_tau):
        if kn * np.max(fcurve) < 0.42 * SR and amp * cfg["g_warm"] > 0:
            sig = sig + amp * cfg["g_warm"] * np.exp(-tax / tc) * _g_osc(phase, dtn, cfg["g_chorus"], kn,
                                                                         gen.uniform(0, 6.28))
    return sig


def _g_env(cfg: dict, tax: np.ndarray, sig: np.ndarray, atk: float) -> np.ndarray:
    """The concave decay, with an opening swell-in (and low-pass) when att is set."""
    env = (1 - cfg["g_mix"]) * np.exp(-tax / cfg["g_tau1"]) + cfg["g_mix"] * np.exp(-tax / cfg["g_tau2"])
    if atk <= 0.003:
        return sig * env * (1 - np.exp(-tax / 0.0015))
    env = env * (1 - (1 - np.clip(tax / atk, 0, 1)) ** 2)
    return svf(sig * env, 900 + 15000 * (1 - np.exp(-tax / max(1.2 * atk, 0.004))), 0.6, "low")


def g_tone(ns: int, freq: float, cfg: dict, gen, drop: float = 0.0, atk: float = 0.0,
           ovt_tau: float = 0.12) -> np.ndarray:
    """A sine with at most a few extra partials, a concave decay and an optional opening swell-in."""
    tax = t_of(ns)
    fcurve = _g_freq(cfg, freq, tax, drop)
    return _up(_g_env(cfg, tax, _g_voice(cfg, tax, fcurve, ovt_tau, gen), atk))


def g_chord(ns: int, cfg: dict, gen, atk: float = 0.0) -> np.ndarray:
    sig = np.zeros(ns)
    for nth, semi in enumerate(CHORDS_G[cfg["g_notes"]]):
        freq = cfg["g_root"] * 2.0 ** (semi / 12)
        if freq <= 9000:
            sig += _delayed(g_tone(ns, freq, cfg, gen, atk=atk) * 0.72 ** nth, n_of(nth * gen.uniform(0.005, 0.012)),
                            ns)
    return _up(sig)


def noise_ride(ns: int, freq: np.ndarray, kn: float, shifted: float, gen) -> np.ndarray:
    sig = svf(gen.standard_normal(ns), np.clip(kn * freq, 200, 14000), shifted, "bandn")
    return bw(sig, "low", 13000, 2)


def g_snap(ns: int, cfg: dict, gen) -> np.ndarray:
    burst = _noise_tick(ns, cfg["g_snap_ms"], gen, 32)
    burst = bw(burst, "band", (cfg["g_snap_f"] / 1.6, min(1.8 * cfg["g_snap_f"], 16000)), 2)
    sig = np.zeros(ns)
    sig[:len(burst)] = _up(burst)
    return sig


def g_hit(ns: int, cfg: dict, gen) -> np.ndarray:
    sig = cfg["g_hit"] * low_strike(cfg["g_low_f"], cfg["g_low_tau"], ns)
    if cfg["g_snap"] > 0.005:
        sig = sig + cfg["g_snap"] * g_snap(ns, cfg, gen)
    return sig / np.max(np.abs(sig)) if np.max(np.abs(sig)) > 0 else sig


def g_tail(drysig: np.ndarray, cfg: dict, gen) -> np.ndarray:
    """2 independent wet returns, fed only with content above 250 Hz; the dry part stays mono."""
    ns = len(drysig)
    mix = _st(drysig).copy()
    if cfg["g_wet"] <= 0.005:
        return mix
    feedv = bw(drysig, "high", 250, 2)
    if cfg["g_verb"] == "fdn":
        wetv = fx.reverb(feedv, gen, rt60=cfg["g_rt"], size=0.7, predelay=0.012, damp=0.5, wet=1.0, dry=0.0,
                        stereo=True, width=1.0)
        return mix + cfg["g_wet"] * _fit(_st(wetv), ns)
    for chan in range(2):
        mix[:, chan] += cfg["g_wet"] * _conv(feedv, _hall(gen, cfg["g_rt"], 0.55, 0.6, 0.012 + 0.005 * chan), ns)
    return mix


def _after(ns: int, at: int, wav: np.ndarray) -> np.ndarray:
    """x placed from sample i0 in an n-sample buffer."""
    mix = np.zeros(ns)
    mix[at:] = wav
    return mix


def _hum_lead(cfg: dict, ns: int, dur: float, gen) -> np.ndarray:
    """The falling chirp of chirp_hum: a crescendo into the landing, gone soon after T."""
    tax = t_of(ns)
    phase, freq = g_sweep(ns, cfg["g_f0"], cfg["g_f1"], dur, 1.0, gen)
    env_c = (1 - np.exp(-tax / 0.003)) * (0.3 + 0.7 * np.clip(tax / dur, 0, 1)) * np.clip((1.2 - tax / dur) / 0.2, 0,
                                                                                          1) ** 1.5
    intro = (np.sin(phase) + 0.12 * np.sin(2 * phase)) * env_c
    if cfg["g_air"] > 0.005:
        intro = intro + 0.5 * cfg["g_air"] * noise_ride(ns, freq, 5, 3, gen) * env_c
    return _up(intro)


def _gesture_chirp_hum(cfg: dict, gen) -> np.ndarray:
    dur = cfg["g_ms"] / 1000
    ns = n_of(dur + cfg["g_length"])
    at = min(ns, n_of(0.92 * dur))
    tone = _after(ns, at, g_tone(ns - at, cfg["g_root"], cfg, gen, drop=cfg["g_sink"], ovt_tau=0.2))
    sig = cfg["g_lead"] * _hum_lead(cfg, ns, dur, gen) + cfg["g_tone"] * tone
    if cfg["g_land"] > 0.005:
        sig = sig + landing({"land": cfg["g_land"], "land_f": cfg["g_land_f"], "thud": 0.0, "hum_f": cfg["g_root"]}, ns,
                        0.92 * dur)
    if cfg["g_snap"] > 0.005:
        sig[at:] += cfg["g_snap"] * g_snap(ns - at, cfg, gen)
    return sig


def _zap_lead(cfg: dict, ns: int, dur: float, gen) -> np.ndarray:
    """A noise zap and a sine zap on the same falling sweep, blended by g_air."""
    tax = t_of(ns)
    _, freq = g_sweep(ns, cfg["g_f0"], cfg["g_f1"], dur, 0.6, gen)
    env_z = (1 - np.exp(-tax / 0.002)) * np.clip(1 - tax / (1.15 * dur), 0, 1) ** 0.7
    hiss = noise_ride(ns, freq, 1.3, 3, gen) * env_z
    phase, freq = g_sweep(ns, cfg["g_f0"], cfg["g_f1"], dur, 0.6, gen)
    tone_z = (np.sin(phase) + 0.1 * np.sin(2 * phase) * (2 * freq < 0.42 * SR)) * env_z
    return _up((1 - cfg["g_air"]) * _up(tone_z) + cfg["g_air"] * _up(hiss))


def _gesture_zap_bloom(cfg: dict, gen) -> np.ndarray:
    dur = cfg["g_ms"] / 1000
    ns = n_of(dur + cfg["g_length"])
    at = min(ns, n_of(0.7 * dur))
    intro = _zap_lead(cfg, ns, dur, gen)
    return cfg["g_lead"] * intro + cfg["g_tone"] * _after(ns, at, g_chord(ns - at, cfg, gen, atk=cfg["g_att"]))


def _sweep_lead(cfg: dict, ns: int, dur: float, gen) -> np.ndarray:
    """A rising sweep that grows exponentially and stops 6 ms after T."""
    tax = t_of(ns)
    phase, freq = g_sweep(ns, cfg["g_f0"], cfg["g_f1"], dur, 1.6, gen)
    prog = np.clip(tax / dur, 0, 1)
    stop = (tax < dur + 0.006) * np.clip((dur + 0.006 - tax) / 0.006, 0, 1) ** 0.5
    gain = (0.02 + 0.98 * (np.exp(4 * prog) - 1) / (np.exp(4) - 1)) * stop
    intro = (np.sin(phase) + 0.15 * np.sin(2 * phase)) * gain
    if cfg["g_air"] > 0.005:
        intro = intro + cfg["g_air"] * noise_ride(ns, freq, 4, 1.4, gen) * gain
    return _up(dsp.fade(intro, 0.02, 0.0))


def _gesture_sweep_hit(cfg: dict, gen) -> np.ndarray:
    dur = cfg["g_ms"] / 1000
    n_head = n_of(dur)
    ns = n_head + n_of(cfg["g_length"])
    intro = _sweep_lead(cfg, ns, dur, gen)
    hit_sig = _after(ns, n_head, g_hit(ns - n_head, cfg, gen))
    return cfg["g_lead"] * intro + hit_sig + cfg["g_tone"] * _after(ns, n_head, g_chord(ns - n_head, cfg, gen))


def _tap(cfg: dict, nth: int, kn: int, gen) -> np.ndarray:
    """Tap j: a short sine ping, the second a whole tone higher, plus a little snap."""
    tap_t = t_of(kn)
    tap_f = cfg["g_tap_f"] * (1 + 0.06 * np.exp(-tap_t / 0.004)) * 2.0 ** (2 * nth / 12)
    ping = np.sin(2 * np.pi * np.cumsum(tap_f) / SR) * np.exp(-tap_t / 0.011) * (1 - np.exp(-tap_t / 0.0004))
    return _up(ping + 0.25 * g_snap(kn, {**cfg, "g_snap_ms": 2.5}, gen))


def _gesture_taps_tone(cfg: dict, gen) -> np.ndarray:
    at2 = n_of(2 * cfg["g_gap"])
    ns = at2 + n_of(cfg["g_length"])
    kn = n_of(0.05)
    sig = np.zeros(ns + kn)
    for nth, lvl0 in ((0, 0.55), (1, 0.75)):
        ix = n_of(nth * cfg["g_gap"])
        sig[ix:ix + kn] += cfg["g_lead"] * lvl0 * _tap(cfg, nth, kn, gen)
    sig = sig[:ns]
    sig[at2:] += cfg["g_tone"] * g_chord(ns - at2, cfg, gen, atk=cfg["g_att"])
    if cfg["g_snap"] > 0.005:
        sig[at2:] += 0.5 * cfg["g_snap"] * g_snap(ns - at2, cfg, gen)
    return sig


def _swell_air(cfg: dict, rise: np.ndarray, gen) -> np.ndarray:
    prog = np.linspace(0, 1, len(rise))
    hiss2 = bw(gen.standard_normal(len(rise)) * prog ** 3, "band", (3000, 12000), 2)
    return dsp.fade(_up(rise + cfg["g_air"] * _up(hiss2)), 0.03, 0.0)


def _swell_lead(cfg: dict, chord_sig: np.ndarray, n_head: int, gen) -> np.ndarray:
    """The stab's hall response reversed in time, so it swells into the stab; it ends 12 ms after it."""
    dur = cfg["g_ms"] / 1000
    ir = _hall(gen, max(0.6, 2 * dur), 0.4, 0.7, 0.0, 1.3 * dur + 0.25)
    kn, amp = n_of(0.012), n_of(0.02)
    rise = _fit(fftconvolve(chord_sig[:n_of(0.3)], ir)[amp:amp + n_head + kn], n_head + kn)[::-1]
    rise = _up(dsp.fade(rise * (0.02 + 0.98 * np.linspace(0, 1, len(rise)) ** 2.2), 0.03, 0.0))
    if cfg["g_air"] > 0.005:
        rise = _swell_air(cfg, rise, gen)
    rise[n_head:] *= np.linspace(1, 0, len(rise) - n_head) ** 2
    return rise


def _gesture_swell_sting(cfg: dict, gen) -> np.ndarray:
    n_head, n_body = n_of(cfg["g_ms"] / 1000), n_of(cfg["g_length"])
    chord_sig = g_chord(n_body, cfg, gen)
    rise = _swell_lead(cfg, chord_sig, n_head, gen)
    sig = np.zeros(n_head + n_body)
    cnt = min(len(rise), len(sig))
    sig[:cnt] += cfg["g_lead"] * rise[:cnt]
    sig[n_head:] += cfg["g_tone"] * chord_sig + 0.8 * g_hit(n_body, cfg, gen)
    return sig


_GESTURE_FNS = {"chirp_hum": _gesture_chirp_hum, "zap_bloom": _gesture_zap_bloom,
                "sweep_hit": _gesture_sweep_hit, "taps_tone": _gesture_taps_tone}


def render_gesture(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """A launch-reveal motion: a mono dry part and a wide tail; an unknown gesture renders swell_sting."""
    sig = _GESTURE_FNS.get(cfg["gesture"], _gesture_swell_sting)(cfg, gen)
    mix = g_tail(bw(sig, "high", 30, 2), cfg, gen)
    mix = dsp.trim_tail(bw(mix, "low", 16000, 4), -64.0, 0.05)
    return dsp.fade(mix, 0.0008, min(0.25, 0.2 * len(mix) / SR))


# ------------------------------------------------------------------------------------------------ render (15.7)

_DISPATCH = ({"chirp": render_chirp, "launch": render_launch, "gesture": render_gesture}, BASES, REVEAL, DEFAULTS)


def render(cfg: dict, gen: np.random.Generator) -> np.ndarray:
    """One sting, (n, 2) float64, normalised to an absolute peak of 0.89; p overlays the base of its mode."""
    table, bset, rbase, dflt = _DISPATCH
    cfg = cfg or {}
    md = cfg.get("mode", dflt["mode"])
    sig = table.get(md, render_reveal)({**bset.get(md, rbase), **cfg}, gen)
    return sig * 0.89 / (np.max(np.abs(sig)) + 1e-12)
