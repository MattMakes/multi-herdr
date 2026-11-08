"""A tiny recipe for the forge engine tests. It is not part of the package.

The conftest fixture `tiny` registers it as hark.forge.recipes.ui.forgetest_tiny.
"""

import numpy as np

SR = 48000

SPACE = {
    "freq": (200.0, 2000.0, "log"),
    "tau": (0.02, 0.2),
    "noise": (0.0, 1.0),
    "parts": (1, 4, "int"),
    "shape": ["sine", "tri"],
}
DEFAULTS = {"freq": 600.0, "tau": 0.08, "noise": 0.3, "parts": 2, "shape": "sine"}
TARGET = "a short bright ping"
ALTS = ["a dull thud", "a long hiss"]


def render(p, r):
    n = int(0.25 * SR)
    t = np.arange(n) / SR
    e = np.exp(-t / p["tau"]) * np.minimum(1.0, t / 0.002)
    y = np.zeros(n)
    for k in range(1, int(p["parts"]) + 1):
        ph = 2 * np.pi * p["freq"] * k * t + r.uniform(0, 2 * np.pi)
        w = np.sin(ph) if p["shape"] == "sine" else 2 / np.pi * np.arcsin(np.sin(ph))
        y += w / k
    y = y * e + p["noise"] * r.standard_normal(n) * e * 0.3
    fade = np.cos(np.linspace(0, np.pi / 2, 480)) ** 2
    fade[-1] = 0.0
    y[-480:] *= fade
    return np.column_stack([y, 0.9 * y + 0.1 * np.roll(y, 7)])


def render_k(p, r):
    """A second render path that reads an optional key (fill test)."""
    k = p.get("k", 1.0)
    return render(p, r) * (0.5 if k == 1.0 else 0.25)


K_DEFAULTS = {"k": 1.0}
