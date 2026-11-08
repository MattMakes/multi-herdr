"""hark sheet: one annotated PNG of a timeline result, for agents that can look at images (sheet-video part 2)."""

from __future__ import annotations

import argparse
import os
from dataclasses import dataclass, field

import numpy as np

from hark.timeline.timeline import add_window_flags, timeline

__all__ = ["sheet", "stagger", "log_spectrogram", "tick_step", "add_cli", "KIND_COLOURS", "STATUS_COLOURS"]

DPI = 100
KIND_COLOURS = {"WHOOSH": "#1f77b4", "RISER": "#17becf", "HIT": "#d62728", "KEY": "#2ca02c", "UI": "#8c564b",
                "STEP": "#bcbd22", "FOLEY": "#e377c2", "NOTE": "#9467bd", "DRUM": "#ff7f0e", "VOICE": "#0b6e4f",
                "-": "#7f7f7f"}
STATUS_COLOURS = {"OK": "#2ca02c", "near": "#ff9f1c", "swell": "#1f77b4", "peak": "#9467bd", "off": "#d62728",
                  "no sound": "#d62728"}
UNKNOWN = "#777777"
CUT_COLOUR = "#00a6d6"
CUT_DASH = (0, (4, 3))
STEPS = (0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300)
LANE_H = {"frames": 1.55, "sections": 0.6, "events": 1.55, "spectrogram": 3.6, "waveform": 0.95, "loudness": 1.75,
          "sync": 0.75}
LANE_TITLES = {"frames": "frames", "sections": "sections", "events": "events", "spectrogram": "spectrogram",
               "waveform": "waveform", "loudness": "loudness", "sync": "sync /\nbeats"}
INSIDE_TITLES = ("spectrogram", "waveform", "loudness")
SPEC_ROWS, SPEC_LO, SPEC_HI, SPEC_RANGE = 240, 30.0, 16000.0, 75.0
ROWS = 4


def fmt_t(t) -> str:
    from hark.core.report import fmt_t as core_fmt

    return core_fmt(t)


# ---------------------------------------------------------------- geometry

@dataclass
class Geometry:
    """Pixel and inch sizes of one sheet."""

    width: int
    has_picture: bool
    fs: float = 0.0
    left: int = 0
    right: int = 64
    plot_px: int = 0
    lf: float = 0.0
    char_px: float = 0.0
    lanes: list = field(default_factory=list)

    def __post_init__(self):
        self.width = int(min(max(self.width, 1000), 2400))
        self.fs = max(11.0, self.width / 125)
        self.left = max(92, int(6.4 * self.fs) + 8)
        self.plot_px = self.width - self.left - self.right
        self.lf = 0.85 * self.fs
        self.char_px = 0.602 * self.lf * 100 / 72
        names = (["frames"] if self.has_picture else []) + ["sections", "events", "spectrogram", "waveform",
                                                             "loudness", "sync"]
        self.lanes = [(n, LANE_H[n]) for n in names]


def tick_step(span: float) -> float:
    """The first step that gives at most 24 ticks over the span, else 600 s."""
    for s in STEPS:
        if span / s <= 24:
            return float(s)
    return 600.0


def tick_label(t: float, step: float) -> str:
    m, s = divmod(t + 1e-9, 60)
    if step >= 1:
        return f"{int(m)}:{int(s):02d}"
    if step == 0.25:
        return f"{int(m)}:{s:05.2f}"
    return f"{int(m)}:{s:04.1f}"


def ticks(t0: float, t1: float) -> tuple[np.ndarray, float]:
    step = tick_step(max(t1 - t0, 1e-6))
    first = np.ceil(t0 / step - 1e-9) * step
    return np.arange(first, t1 + 1e-9, step), step


# ---------------------------------------------------------------- header

def _sync_line(kind: str, st: dict) -> str:
    fr = "within_1_frame" if "frame_ms" in st else "within_40ms"
    word = "1 frame" if fr == "within_1_frame" else "40 ms"
    s = f"{kind}: {st['n']}, OK within {word} {100 * st['hit_' + fr]:.0f}% (chance {100 * st.get('chance_' + fr, 0):.0f}%)"
    if "accent_" + fr in st:
        s += f", accents {100 * st['accent_' + fr]:.0f}% (chance {100 * st['accent_chance_' + fr]:.0f}%)"
    if st.get("answered_by_level_peak"):
        s += f", {st['answered_by_level_peak']} met only by a level peak"
    return s + f", {len(st['no_sound'])} without a hit"


def _level_items(r: dict) -> list[str]:
    L = r["loudness"]
    if L.get("integrated_lufs") is not None:
        s = f"{L['integrated_lufs']:.1f} LUFS   TP {L['true_peak_dbtp']:.1f} dBTP"
        return [s + (f"   LRA {L['lra_lu']:.1f} LU" if L.get("lra_lu") is not None else "")]
    if not r.get("has_audio", True):
        return ["no audio stream"]
    return ["digital silence"] if L.get("digital_silence") else []


def header_lines(r: dict, fs: float) -> list[tuple[str, float, str, str]]:
    """(text, size pt, weight, colour) per header line."""
    name = os.path.basename(r["file"])
    lines = [(f"{name}   {fmt_t(r['start'])}-{fmt_t(r['end'])}", 1.25 * fs, "bold", "black")]
    items = _level_items(r)
    mu = r["music"]
    if mu.get("tempo") is not None and mu.get("confidence") != "none":
        alt = f" (or {mu['tempo_alt']})" if mu.get("tempo_alt") else ""
        items.append(f"~{mu['tempo']}{alt} BPM ({mu['confidence']})")
    items.append(f"{len(r['events'])} events")
    lines.append(("    ".join(items), 0.95 * fs, "normal", "#444444"))
    for kind in ("cuts", "cues"):
        block = r["sync"].get(kind)
        if block and block["items"]:
            lines.append((_sync_line(kind, block["stats"]), 0.95 * fs, "normal", "#444444"))
    if r["sync"]:
        legend = ("markers: v cue, <> cut; colours: green OK, amber near, blue swell, violet level peak, "
                  "red off or no sound")
        if r["cuts"]:
            legend += "; dashed lines are picture cuts"
        lines.append((legend, 0.72 * fs, "normal", "#777777"))
    if any(e.get("label") for e in r["events"]):
        lines.append(("labels are kind + CLAP tag, hints only; pale with ? = under 12 dB over the bed",
                      0.72 * fs, "normal", "#777777"))
    return lines


def header_height(lines) -> float:
    return 0.22 + sum(size * 1.45 / 72 for _, size, _, _ in lines)


# ---------------------------------------------------------------- event labels

def stagger(items, t0: float, t1: float, plot_px: float, char_px: float, rows: int = ROWS) -> list[tuple]:
    """Greedy label rows. items: (priority, t, text). Returns (row, t, span start, text); never overlapping."""
    pps = plot_px / max(t1 - t0, 1e-9)
    taken: list[list[tuple[float, float]]] = [[] for _ in range(rows)]
    placed = []
    for _, t, text in sorted(items, key=lambda it: it[0]):
        w = (len(text) * char_px + 16) / pps
        a, b = t - 0.15 * w, t + 0.85 * w
        if b > t1:
            a, b = t1 - w, t1
        a = max(a, t0)
        row = next((k for k in range(rows) if all(b <= s or a >= e for s, e in taken[k])), None)
        if row is None:
            continue
        taken[row].append((a, b))
        placed.append((row, t, a, text))
    return placed


def label_items(r: dict) -> list[tuple]:
    answered = set()
    for block in r["sync"].values():
        answered.update(it.get("event_t") for it in block["items"] if it.get("event_t") is not None)
    items = []
    for e in r["events"]:
        if e.get("label"):
            text = f"{e['kind']}{'' if e.get('exposed') else '?'} {e['label']}"
            items.append(((0 if e["t"] in answered else 1, -e.get("prominence_db", 0)), e["t"], text))
    return items


# ---------------------------------------------------------------- spectrogram

def _log_bank(n_fft: int) -> tuple[np.ndarray, np.ndarray]:
    edges = np.geomspace(SPEC_LO, SPEC_HI, SPEC_ROWS + 2)
    f = np.fft.rfftfreq(n_fft, 1 / 48000)
    bank = np.zeros((SPEC_ROWS, f.size))
    for k in range(SPEC_ROWS):
        a, c, b = edges[k], edges[k + 1], edges[k + 2]
        tri = np.clip(np.minimum((f - a) / (c - a), (b - f) / (b - c)), 0, None)
        if not tri.any():
            tri[int(np.argmin(np.abs(f - c)))] = 1.0
        bank[k] = tri / tri.sum()
    return bank, edges[1:-1]


def log_spectrogram(m: np.ndarray, plot_px: int) -> tuple[np.ndarray, np.ndarray]:
    """(dB matrix of shape (240, columns), row centre frequencies)."""
    n_fft = 4096
    m = np.asarray(m, dtype=np.float64)
    hop = max(64, int(len(m) / max(plot_px, 1)))
    y = np.pad(m, n_fft // 2)
    n = 1 + max(0, y.size - n_fft) // hop
    win = np.hanning(n_fft + 1)[:-1]
    bank, centres = _log_bank(n_fft)
    cols = []
    for i0 in range(0, n, 512):
        idx = (np.arange(i0, min(n, i0 + 512)) * hop)[:, None] + np.arange(n_fft)[None, :]
        spec = np.fft.rfft(y[idx] * win, axis=1)
        cols.append(bank @ (spec.real ** 2 + spec.imag ** 2).T)
    return 10 * np.log10(np.concatenate(cols, axis=1) + 1e-14), centres


def spec_scale(S: np.ndarray) -> tuple[float, float]:
    top = max(float(np.percentile(S, 99.7)), -40.0)
    return top - SPEC_RANGE, top


# ---------------------------------------------------------------- drawing context

@dataclass
class Canvas:
    fig: object
    geo: Geometry
    r: dict
    t0: float
    t1: float
    axes: dict = field(default_factory=dict)
    titles: dict = field(default_factory=lambda: dict(LANE_TITLES))

    @property
    def span(self) -> float:
        return max(self.t1 - self.t0, 1e-9)

    @property
    def pps(self) -> float:
        return self.geo.plot_px / self.span


def _layout(fig, geo: Geometry, top_in: float, height_in: float) -> dict:
    axes = {}
    y = height_in - top_in
    for name, h in geo.lanes:
        y -= h
        rect = (geo.left / geo.width, y / height_in, geo.plot_px / geo.width, (h - 0.06) / height_in)
        axes[name] = fig.add_axes(rect)
    return axes


def _style_axis(ax, name: str, cv: Canvas, bottom: bool) -> None:
    pos, step = ticks(cv.t0, cv.t1)
    ax.set_xlim(cv.t0, cv.t1)
    ax.set_xticks(pos)
    ax.set_xticklabels([tick_label(t, step) for t in pos] if bottom else [], fontsize=0.8 * cv.geo.fs)
    if bottom:
        ax.set_xlabel("time (m:ss)", fontsize=0.85 * cv.geo.fs)
    for sp in ax.spines.values():
        sp.set_color("#bbbbbb")
        sp.set_linewidth(0.6)
    ax.tick_params(axis="both", labelsize=0.72 * cv.geo.fs, length=3, color="#bbbbbb")
    _lane_title(ax, name, cv.titles[name], cv.geo.fs)


def _lane_title(ax, name: str, title: str, fs: float) -> None:
    if name in INSIDE_TITLES:
        ax.text(0.004, 0.96, title, transform=ax.transAxes, ha="left", va="top", fontsize=0.8 * fs, zorder=20,
                bbox={"boxstyle": "square,pad=0.2", "fc": "white", "ec": "none", "alpha": 0.75})
    else:
        ax.text(-0.004, 0.5, title, transform=ax.transAxes, ha="right", va="center", fontsize=0.72 * fs,
                color="#333333")


def _draw_header(fig, lines, height_in: float) -> None:
    y = 0.12
    for text, size, weight, colour in lines:
        y += size * 0.725 / 72
        fig.text(0.008, 1 - y / height_in, text, ha="left", va="center", fontsize=size, fontweight=weight,
                 color=colour)
        y += size * 0.725 / 72


def _cut_lines(cv: Canvas) -> None:
    for name, ax in cv.axes.items():
        if name == "events":
            continue
        for c in cv.r["cuts"]:
            if cv.t0 <= c <= cv.t1:
                ax.axvline(c, color=CUT_COLOUR, linestyle=CUT_DASH, linewidth=1.0,
                           zorder=0 if name == "frames" else 15)


# ---------------------------------------------------------------- lanes

def _frame_time(cuts, a: float, b: float) -> float:
    for c in cuts:
        if a <= c < b - 0.2:
            return min(c + 0.15, b - 0.05)
    return (a + b) / 2


def draw_frames(cv: Canvas, ax) -> int:
    from hark.timeline import video as vid

    info = vid.info(cv.r["video"])
    aspect = (info.get("width") or 16) / (info.get("height") or 9)
    th = LANE_H["frames"] * DPI - 18
    tw = th * aspect
    slots = max(1, int(cv.geo.plot_px // (tw + 6)))
    ax.set_ylim(0, 1)
    ax.set_yticks([])
    drawn = 0
    for k in range(slots):
        a, b = cv.t0 + k * cv.span / slots, cv.t0 + (k + 1) * cv.span / slots
        t = _frame_time(cv.r["cuts"], a, b)
        img = vid.frame(cv.r["video"], t, width=int(1.5 * tw))
        ax.plot([t, t], [0, 0.12], color="#222222", linewidth=1.0)
        if img is None:
            continue
        mid, half = (a + b) / 2, tw / cv.pps / 2
        ax.imshow(np.asarray(img), extent=(mid - half, mid + half, 0.12, 1.0), aspect="auto", zorder=2)
        drawn += 1
    ax.set_xlim(cv.t0, cv.t1)
    return drawn


def draw_sections(cv: Canvas, ax) -> None:
    from matplotlib.patches import Rectangle

    fs = cv.geo.fs
    ax.set_ylim(0, 1)
    ax.set_yticks([])
    for k, s in enumerate(cv.r["sections"]):
        a, b = max(s["start"], cv.t0), min(s["end"], cv.t1)
        if b <= a:
            continue
        ax.add_patch(Rectangle((a, 0.08), b - a, 0.84, facecolor="#dbe8f5" if k % 2 == 0 else "#f3e9d2",
                               edgecolor="#999999", linewidth=0.6))
        cap = int((b - a) * cv.pps / (0.55 * fs * 0.9 * 100 / 72)) - 1
        if cap >= 4:
            text = s["label"] if len(s["label"]) <= cap else s["label"][: cap - 2] + ".."
            ax.text(a + 4 / cv.pps, 0.5, text, ha="left", va="center", fontsize=0.9 * fs, clip_on=True)


def draw_events(cv: Canvas, ax) -> tuple[int, int, list]:
    items = label_items(cv.r)
    placed = stagger(items, cv.t0, cv.t1, cv.geo.plot_px, cv.geo.char_px)
    kind_of = {e["t"]: e["kind"] for e in cv.r["events"] if e.get("label")}
    ax.set_ylim(0, ROWS)
    ax.set_yticks([])
    for row, t, a, text in placed:
        colour = KIND_COLOURS.get(kind_of.get(t, "-"), UNKNOWN)
        y = ROWS - row - 0.5
        alpha = 0.55 if text.split(" ", 1)[0].endswith("?") else 1.0
        ax.plot([t, t], [0, y], color=colour, linewidth=0.8, alpha=alpha)
        ax.text(a + 8 / cv.pps, y, text, ha="left", va="center", fontsize=cv.geo.lf, family="monospace",
                color="white", clip_on=True,
                bbox={"boxstyle": "square,pad=0.25", "fc": colour, "ec": "none", "alpha": alpha})
    kinds = sorted({kind_of.get(t, "-") for _, t, _, _ in placed})
    return len(placed), len(items), kinds


def draw_kind_chips(fig, kinds, geo: Geometry, height_in: float) -> None:
    x = 1 - 8 / geo.width
    for kind in reversed(kinds):
        fig.text(x, 1 - 0.2 / height_in, kind, ha="right", va="center", fontsize=0.72 * geo.fs, color="white",
                     bbox={"boxstyle": "round,pad=0.3", "fc": KIND_COLOURS.get(kind, UNKNOWN), "ec": "none"})
        x -= (len(kind) * 0.65 * 0.72 * geo.fs * 100 / 72 + 18) / geo.width


def draw_spectrogram(cv: Canvas, ax, m: np.ndarray) -> None:
    S, centres = log_spectrogram(m, cv.geo.plot_px)
    lo, hi = spec_scale(S)
    ax.imshow(S, origin="lower", aspect="auto", cmap="magma", vmin=lo, vmax=hi, interpolation="bilinear",
              extent=(cv.t0, cv.t1, 0, SPEC_ROWS))
    marks = [50, 100, 200, 500, 1000, 2000, 5000, 10000]
    ax.set_yticks(np.interp(np.log(marks), np.log(centres), np.arange(SPEC_ROWS) + 0.5))
    ax.set_yticklabels(["50", "100", "200", "500", "1k", "2k", "5k", "10k"])
    for e in cv.r["events"]:
        if cv.t0 <= e["t"] <= cv.t1:
            big = bool(e.get("label"))
            h = (0.07 if big else 0.035) * SPEC_ROWS
            ax.plot([e["t"], e["t"]], [SPEC_ROWS - h, SPEC_ROWS], color="white", linewidth=1.0 if big else 0.6,
                    alpha=0.9 if big else 0.55)
    ax.set_ylim(0, SPEC_ROWS)


def waveform_title(m: np.ndarray) -> str:
    peak = float(np.max(np.abs(m))) if m.size else 0.0
    if peak == 0:
        return "waveform: digital silence"
    scale = max(float(np.percentile(np.abs(m), 99.9)), 1e-9)
    return f"waveform (full height {20 * np.log10(scale):.0f} dBFS, peak {20 * np.log10(peak):.1f} dBFS)"


def draw_waveform(cv: Canvas, ax, m: np.ndarray) -> None:
    n = m.size
    cols = max(1, min(cv.geo.plot_px, n))
    edges = np.linspace(0, n, cols + 1).astype(int)
    scale = max(float(np.percentile(np.abs(m), 99.9)) if n else 0.0, 1e-9)
    lo = np.array([m[a:b].min() if b > a else 0.0 for a, b in zip(edges[:-1], edges[1:])]) / scale
    hi = np.array([m[a:b].max() if b > a else 0.0 for a, b in zip(edges[:-1], edges[1:])]) / scale
    t = cv.t0 + (edges[:-1] + edges[1:]) / 2 / max(n, 1) * cv.span
    ax.fill_between(t, np.clip(lo, -1, 1), np.clip(hi, -1, 1), color="#5a6b7d", linewidth=0)
    ax.set_ylim(-1.05, 1.05)
    ax.set_yticks([])
    cv.titles["waveform"] = waveform_title(m)


def _curve(block: dict | None) -> tuple[np.ndarray, np.ndarray]:
    if not block or not block.get("lufs"):
        return np.zeros(0), np.zeros(0)
    v = np.maximum(np.asarray(block["lufs"], dtype=float), -60)
    return block["t0"] + block["dt"] * np.arange(v.size), v


def draw_loudness(cv: Canvas, ax) -> None:
    L = cv.r["loudness"]
    for g in L.get("silences", []):
        ax.axvspan(g["start"], g["end"], color="#ffe14d", alpha=0.45, linewidth=0)
    tm, vm = _curve(L.get("momentary"))
    ts, vs = _curve(L.get("short_term"))
    if tm.size:
        ax.plot(tm, vm, color="#5b9bd5", linewidth=0.8, label="momentary")
    if ts.size:
        ax.plot(ts, vs, color="#1f3c88", linewidth=2.0, label="short-term")
    I = L.get("integrated_lufs")
    if I is not None and I > -60:
        ax.axhline(I, color="#d62728", linestyle="--", linewidth=1.0, label="integrated")
        ax.text(1.005, (I + 60) / 60, f"{I:.1f}", transform=ax.transAxes, ha="left", va="center",
                fontsize=0.72 * cv.geo.fs, color="#d62728")
    for c in L.get("clipping", {}).get("times", []):
        ax.plot([c, c], [-3, 0], color="#d62728", linewidth=1.2)
    ax.set_ylim(-60, 0)
    ax.set_yticks([-50, -40, -30, -20, -10])
    if tm.size or ts.size:
        ax.legend(loc="lower left", fontsize=0.65 * cv.geo.fs, frameon=False, ncol=3)


def draw_sync(cv: Canvas, ax) -> None:
    mu = cv.r["music"]
    ax.set_ylim(0, 1)
    ax.set_yticks([])
    if mu.get("beats") and mu.get("confidence") != "none":
        down = set(mu.get("downbeats", []))
        for b in mu["beats"]:
            if cv.t0 <= b <= cv.t1:
                tall = b in down
                ax.plot([b, b], [0, 0.45 if tall else 0.28], color="#111111" if tall else "#999999", linewidth=0.8)
    for kind, marker, y in (("cues", "v", 0.84), ("cuts", "D", 0.6)):
        for it in cv.r["sync"].get(kind, {}).get("items", []):
            ax.plot([it["t"]], [y], marker=marker, markersize=0.55 * cv.geo.fs, linestyle="none",
                    color=STATUS_COLOURS.get(it.get("status"), UNKNOWN), markeredgecolor="#333333",
                    markeredgewidth=0.4)


# ---------------------------------------------------------------- the sheet

def _audio(r: dict, path: str) -> np.ndarray:
    n = int(round((r["end"] - r["start"]) * 48000))
    if not r.get("has_audio", True):
        return np.zeros(n)
    from hark.core import audio

    return np.asarray(audio.load(path, mono=True, start=r["start"], end=r["end"]), dtype=np.float64)


def build_figure(r: dict, m: np.ndarray, width: int):
    """Draw every lane; returns (figure, counts)."""
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt

    geo = Geometry(width, bool(r.get("video")))
    lines = header_lines(r, geo.fs)
    head = header_height(lines)
    height_in = round((head + sum(h for _, h in geo.lanes) + 0.55) * 100) / 100
    fig = plt.figure(figsize=(geo.width / DPI, height_in), dpi=DPI, facecolor="white")
    cv = Canvas(fig, geo, r, r["start"], r["end"])
    cv.axes = _layout(fig, geo, head, height_in)
    _draw_header(fig, lines, height_in)
    counts = _draw_lanes(cv, m)
    draw_kind_chips(fig, counts.pop("kinds"), geo, height_in)
    names = [n for n, _ in geo.lanes]
    for name, ax in cv.axes.items():
        _style_axis(ax, name, cv, bottom=name == names[-1])
    _cut_lines(cv)
    return fig, dict(counts, width=geo.width, height=int(round(height_in * DPI)))


def _draw_lanes(cv: Canvas, m: np.ndarray) -> dict:
    ax = cv.axes
    thumbs = draw_frames(cv, ax["frames"]) if "frames" in ax else 0
    draw_sections(cv, ax["sections"])
    placed, tagged, kinds = draw_events(cv, ax["events"])
    draw_spectrogram(cv, ax["spectrogram"], m)
    draw_waveform(cv, ax["waveform"], m)
    draw_loudness(cv, ax["loudness"])
    draw_sync(cv, ax["sync"])
    return {"thumbnails": thumbs, "labelled_events": placed, "tagged_events": tagged, "kinds": kinds}


def default_png(path: str) -> str:
    return os.path.join(os.getcwd(), os.path.splitext(os.path.basename(path))[0] + "_sheet.png")


def _summary(png: str, c: dict, r: dict) -> str:
    from hark.timeline._dsp import ascii_safe

    return ascii_safe(
        f"Wrote {png} ({c['width']}x{c['height']} px) for {fmt_t(r['start'])}-{fmt_t(r['end'])}: "
        f"{c['thumbnails']} frames, {c['labelled_events']} of {c['tagged_events']} tagged events labelled, "
        f"{len(r['events'])} event ticks, {len(r['cuts'])} cut lines. Open it with an image viewer or a file "
        f"read tool to look at it.")


def sheet(path, out_png=None, video=None, cues=None, start=None, end=None, detail="normal", width=2000,
          tl=None) -> dict:
    """Draw one annotated PNG of a timeline result (run or reused) and return what it holds."""
    import matplotlib.pyplot as plt

    r = tl if tl is not None else timeline(path, video=video, cues=cues, start=start, end=end, detail=detail)
    png = os.path.abspath(out_png or default_png(path))
    fig, counts = build_figure(r, _audio(r, path), width)
    os.makedirs(os.path.dirname(png), exist_ok=True)
    fig.savefig(png, dpi=DPI, facecolor="white")
    plt.close(fig)
    summary = _summary(png, counts, r)
    return {"png": png, "width": counts["width"], "height": counts["height"], "start": r["start"], "end": r["end"],
            "thumbnails": counts["thumbnails"], "labelled_events": counts["labelled_events"],
            "tagged_events": counts["tagged_events"], "events": r["events"], "cuts": r["cuts"],
            "timeline_summary": r["summary"], "summary": summary,
            "text": summary + "\n\nTimeline: " + r["summary"]}


# ---------------------------------------------------------------- CLI

def _handler(args: argparse.Namespace) -> int:
    from hark.core.report import emit
    from hark.models import require_ml

    require_ml("sheet")
    res = sheet(args.file, args.png, video=args.video, cues=args.cues, start=args.start, end=args.end,
                detail=args.detail, width=args.width)
    emit(res, args)
    return 0


def add_cli(sub) -> None:
    """Register `hark sheet`."""
    from hark.core.report import add_common

    p = sub.add_parser("sheet", help="one annotated PNG of the timeline, for agents that can look at images")
    add_window_flags(p)
    p.add_argument("-o", dest="png", help="output PNG (default <file stem>_sheet.png in the current folder)")
    p.add_argument("--width", type=int, default=2000, help="image width in px, 1000-2400 (default 2000)")
    add_common(p)
    p.set_defaults(func=_handler)
