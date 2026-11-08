"""``hark sheet``: render a timeline result as one annotated PNG.

The picture stacks horizontal lanes that share one time axis (frames, sections,
labelled events, spectrogram, waveform, loudness, sync marks) under a short text
header. It is meant for agents that can read images.
"""

from __future__ import annotations

import argparse
import os

import numpy

from hark.timeline.timeline import add_window_flags, timeline

__all__ = ["sheet", "stagger", "log_spectrogram", "tick_step", "add_cli", "KIND_COLOURS", "STATUS_COLOURS"]

KIND_COLOURS = {
    "WHOOSH": "#1f77b4", "RISER": "#17becf", "HIT": "#d62728", "KEY": "#2ca02c",
    "UI": "#8c564b", "STEP": "#bcbd22", "FOLEY": "#e377c2", "NOTE": "#9467bd",
    "DRUM": "#ff7f0e", "VOICE": "#0b6e4f", "-": "#7f7f7f",
}
STATUS_COLOURS = {
    "OK": "#2ca02c", "near": "#ff9f1c", "swell": "#1f77b4", "peak": "#9467bd",
    "off": "#d62728", "no sound": "#d62728",
}

_PX_PER_IN = 100
_FALLBACK_COLOUR = "#777777"
_INK_DARK = "#444444"
_INK_PALE = "#777777"
_RULE = "#bbbbbb"

# Lane name -> (height in inches, title). Order here is the order on the page.
_LANES = {
    "frames": (1.55, "frames"),
    "sections": (0.6, "sections"),
    "events": (1.55, "events"),
    "spectrogram": (3.6, "spectrogram"),
    "waveform": (0.95, "waveform"),
    "loudness": (1.75, "loudness"),
    "sync": (0.75, "sync /\nbeats"),
}
_TITLE_ON_PLOT = frozenset({"spectrogram", "waveform", "loudness"})

_TICK_CHOICES = (0.1, 0.25, 0.5, 1, 2, 5, 10, 15, 30, 60, 120, 300)
_MAX_TICKS = 24

_BANDS = 240
_BAND_LOW_HZ = 30.0
_BAND_HIGH_HZ = 16000.0
_SHOWN_DB = 75.0
_FFT = 4096
_RATE = 48000

_LABEL_ROWS = 4

_CUT_STYLE = {"color": "#00a6d6", "linestyle": (0, (4, 3)), "linewidth": 1.0}
_KEY_TEXT = ("markers: v cue, <> cut; colours: green OK, amber near, blue swell, violet level peak, "
             "red off or no sound")
_TAG_NOTE = "labels are kind + CLAP tag, hints only; pale with ? = under 12 dB over the bed"


def _clock(t) -> str:
    from hark.core.report import fmt_t

    return fmt_t(t)


# ---------------------------------------------------------------- time axis

def tick_step(span: float) -> float:
    """Smallest tick spacing that keeps the axis at 24 ticks or fewer (600 s past the table)."""
    return next((float(c) for c in _TICK_CHOICES if span / c <= _MAX_TICKS), 600.0)


def _tick_text(t: float, step: float) -> str:
    minutes, secs = divmod(t + 1e-9, 60)
    whole = int(minutes)
    if step >= 1:
        return f"{whole}:{int(secs):02d}"
    return f"{whole}:{secs:05.2f}" if step == 0.25 else f"{whole}:{secs:04.1f}"


def _tick_positions(low: float, high: float) -> tuple[numpy.ndarray, float]:
    step = tick_step(max(high - low, 1e-6))
    start = numpy.ceil(low / step - 1e-9) * step
    return numpy.arange(start, high + 1e-9, step), step


# ---------------------------------------------------------------- header text

def _pct(x) -> str:
    return f"{100 * x:.0f}%"


def _describe_sync(which: str, stats: dict) -> str:
    one_frame = "frame_ms" in stats
    suffix = "within_1_frame" if one_frame else "within_40ms"
    tol = "1 frame" if one_frame else "40 ms"
    parts = [f"{which}: {stats['n']}, OK within {tol} {_pct(stats['hit_' + suffix])} "
             f"(chance {_pct(stats.get('chance_' + suffix, 0))})"]
    if "accent_" + suffix in stats:
        parts.append(f"accents {_pct(stats['accent_' + suffix])} "
                     f"(chance {_pct(stats['accent_chance_' + suffix])})")
    only_peak = stats.get("answered_by_level_peak")
    if only_peak:
        parts.append(f"{only_peak} met only by a level peak")
    parts.append(f"{len(stats['no_sound'])} without a hit")
    return ", ".join(parts)


def _level_summary(res: dict) -> list[str]:
    lv = res["loudness"]
    if lv.get("integrated_lufs") is None:
        if not res.get("has_audio", True):
            return ["no audio stream"]
        return ["digital silence"] if lv.get("digital_silence") else []
    text = f"{lv['integrated_lufs']:.1f} LUFS   TP {lv['true_peak_dbtp']:.1f} dBTP"
    if lv.get("lra_lu") is not None:
        text += f"   LRA {lv['lra_lu']:.1f} LU"
    return [text]


def _tempo_summary(music: dict) -> list[str]:
    bpm = music.get("tempo")
    if bpm is None or music.get("confidence") == "none":
        return []
    other = music.get("tempo_alt")
    shown = f"{bpm} (or {other})" if other else f"{bpm}"
    return [f"~{shown} BPM ({music['confidence']})"]


def header_lines(r: dict, fs: float) -> list[tuple[str, float, str, str]]:
    """Header rows as (text, point size, weight, colour), top to bottom."""
    title = f"{os.path.basename(r['file'])}   {_clock(r['start'])}-{_clock(r['end'])}"
    facts = _level_summary(r) + _tempo_summary(r["music"]) + [f"{len(r['events'])} events"]
    rows = [(title, 1.25 * fs, "bold", "black"), ("    ".join(facts), 0.95 * fs, "normal", _INK_DARK)]
    for which in ("cuts", "cues"):
        block = r["sync"].get(which)
        if block and block["items"]:
            rows.append((_describe_sync(which, block["stats"]), 0.95 * fs, "normal", _INK_DARK))
    small = 0.72 * fs
    if r["sync"]:
        key = _KEY_TEXT + ("; dashed lines are picture cuts" if r["cuts"] else "")
        rows.append((key, small, "normal", _INK_PALE))
    if any(ev.get("label") for ev in r["events"]):
        rows.append((_TAG_NOTE, small, "normal", _INK_PALE))
    return rows


def _header_inches(rows) -> float:
    return 0.22 + sum(row[1] * 1.45 / 72 for row in rows)


# ---------------------------------------------------------------- label placement

def _fits(row_spans, low: float, high: float) -> bool:
    return all(high <= s or low >= e for s, e in row_spans)


def stagger(items, t0: float, t1: float, plot_px: float, char_px: float, rows: int = _LABEL_ROWS) -> list[tuple]:
    """Place labels greedily into rows, best priority first, without overlap.

    ``items`` holds (priority, t, text). The result holds (row, t, left edge, text);
    a label that fits no row is dropped.
    """
    px_per_s = plot_px / max(t1 - t0, 1e-9)
    used: list[list[tuple[float, float]]] = [[] for _ in range(rows)]
    out = []
    for _, at, text in sorted(items, key=lambda item: item[0]):
        span = (len(text) * char_px + 16) / px_per_s
        low, high = at - 0.15 * span, at + 0.85 * span
        if high > t1:
            low, high = t1 - span, t1
        low = max(low, t0)
        free = [k for k in range(rows) if _fits(used[k], low, high)]
        if free:
            used[free[0]].append((low, high))
            out.append((free[0], at, low, text))
    return out


def _label_candidates(res: dict) -> list[tuple]:
    hit_times = {it.get("event_t") for blk in res["sync"].values() for it in blk["items"]}
    hit_times.discard(None)
    cands = []
    for ev in res["events"]:
        if not ev.get("label"):
            continue
        mark = "" if ev.get("exposed") else "?"
        rank = (0 if ev["t"] in hit_times else 1, -ev.get("prominence_db", 0))
        cands.append((rank, ev["t"], f"{ev['kind']}{mark} {ev['label']}"))
    return cands


# ---------------------------------------------------------------- spectrogram maths

def _triangle_bank() -> tuple[numpy.ndarray, numpy.ndarray]:
    """Unit-area triangular filters on log-spaced centres, and those centres."""
    pts = numpy.geomspace(_BAND_LOW_HZ, _BAND_HIGH_HZ, _BANDS + 2)
    hz = numpy.fft.rfftfreq(_FFT, 1 / _RATE)
    bank = numpy.zeros((_BANDS, hz.size))
    for row, (low, mid, high) in enumerate(zip(pts[:-2], pts[1:-1], pts[2:])):
        rise = (hz - low) / (mid - low)
        fall = (high - hz) / (high - mid)
        weights = numpy.clip(numpy.minimum(rise, fall), 0, None)
        if not weights.any():
            weights[int(numpy.argmin(numpy.abs(hz - mid)))] = 1.0
        bank[row] = weights / weights.sum()
    return bank, pts[1:-1]


def log_spectrogram(m: numpy.ndarray, plot_px: int) -> tuple[numpy.ndarray, numpy.ndarray]:
    """Band power in dB, shape (240, columns), plus the band centre frequencies."""
    sig = numpy.asarray(m, dtype=numpy.float64)
    hop = max(64, int(len(sig) / max(plot_px, 1)))
    padded = numpy.pad(sig, _FFT // 2)
    n_cols = 1 + max(0, padded.size - _FFT) // hop
    taper = numpy.hanning(_FFT + 1)[:-1]
    bank, centres = _triangle_bank()
    offsets = numpy.arange(_FFT)[None, :]
    chunks = []
    for first in range(0, n_cols, 512):
        starts = numpy.arange(first, min(n_cols, first + 512)) * hop
        spec = numpy.fft.rfft(padded[starts[:, None] + offsets] * taper, axis=1)
        chunks.append(bank @ (spec.real ** 2 + spec.imag ** 2).T)
    return 10 * numpy.log10(numpy.concatenate(chunks, axis=1) + 1e-14), centres


def spec_scale(S: numpy.ndarray) -> tuple[float, float]:
    ceiling = max(float(numpy.percentile(S, 99.7)), -40.0)
    return ceiling - _SHOWN_DB, ceiling


def waveform_title(m: numpy.ndarray) -> str:
    peak = float(numpy.max(numpy.abs(m))) if m.size else 0.0
    if peak == 0:
        return "waveform: digital silence"
    full = max(float(numpy.percentile(numpy.abs(m), 99.9)), 1e-9)
    return f"waveform (full height {20 * numpy.log10(full):.0f} dBFS, peak {20 * numpy.log10(peak):.1f} dBFS)"


def _min_max_columns(sig: numpy.ndarray, n_cols: int):
    """Per-column min and max of ``sig`` over equal slices, and each slice centre."""
    cut = numpy.linspace(0, sig.size, n_cols + 1).astype(int)
    lows, highs = [], []
    for a, b in zip(cut[:-1], cut[1:]):
        piece = sig[a:b]
        lows.append(piece.min() if b > a else 0.0)
        highs.append(piece.max() if b > a else 0.0)
    return numpy.array(lows), numpy.array(highs), (cut[:-1] + cut[1:]) / 2


def _lufs_line(block: dict | None):
    if not block or not block.get("lufs"):
        return numpy.zeros(0), numpy.zeros(0)
    vals = numpy.maximum(numpy.asarray(block["lufs"], dtype=float), -60)
    return block["t0"] + block["dt"] * numpy.arange(vals.size), vals


_AT_LEFT = {"ha": "left", "va": "center"}
_AT_RIGHT = {"ha": "right", "va": "center"}
_AT_TOP_LEFT = {"ha": "left", "va": "top"}


def _bare_y(pane, low, high) -> None:
    """Fix the vertical range of a lane and hide its y ticks."""
    pane.set_ylim(low, high)
    pane.set_yticks([])


def _stroke(pane, x, y0, y1, **style) -> None:
    pane.plot([x, x], [y0, y1], **style)


# ---------------------------------------------------------------- the drawing

class _Page:
    """One sheet being drawn: page sizes, the result, and one axes per lane."""

    def __init__(self, res: dict, width: int):
        self.res = res
        self.begin, self.until = res["start"], res["end"]
        self.width = int(min(max(width, 1000), 2400))
        self.pt = max(11.0, self.width / 125)
        self.left = max(92, int(6.4 * self.pt) + 8)
        self.plot_px = self.width - self.left - 64
        self.label_pt = 0.85 * self.pt
        self.char_px = 0.602 * self.label_pt * 100 / 72
        names = [tally for tally in _LANES if tally != "frames" or res.get("video")]
        self.lanes = [(tally, _LANES[tally][0]) for tally in names]
        self.titles = {tally: t for tally, (_, t) in _LANES.items()}
        self.header = header_lines(res, self.pt)
        head_in = _header_inches(self.header)
        self.height_in = round((head_in + sum(h for _, h in self.lanes) + 0.55) * 100) / 100
        self.fig = None
        self.pane: dict = {}
        self._head_in = head_in

    @property
    def span(self) -> float:
        return max(self.until - self.begin, 1e-9)

    @property
    def px_per_s(self) -> float:
        return self.plot_px / self.span

    def open(self) -> None:
        import matplotlib

        matplotlib.use("Agg")
        import matplotlib.pyplot as plt

        self.fig = plt.figure(figsize=(self.width / _PX_PER_IN, self.height_in), dpi=_PX_PER_IN, facecolor="white")
        top = self.height_in - self._head_in
        for name, h in self.lanes:
            top -= h
            box = (self.left / self.width, top / self.height_in, self.plot_px / self.width,
                   (h - 0.06) / self.height_in)
            self.pane[name] = self.fig.add_axes(box)

    def write_header(self) -> None:
        y = 0.12
        for text, size, weight, colour in self.header:
            half = size * 0.725 / 72
            y += half
            self.fig.text(0.008, 1 - y / self.height_in, text, **_AT_LEFT, fontsize=size,
                          fontweight=weight, color=colour)
            y += half

    # ---- lanes

    def frames_lane(self, pane) -> int:
        from hark.timeline import video as vid

        src = self.res["video"]
        meta = vid.info(src)
        thumb_w = (_LANES["frames"][0] * _PX_PER_IN - 18) * (meta.get("width") or 16) / (meta.get("height") or 9)
        n_slots = max(1, int(self.plot_px // (thumb_w + 6)))
        _bare_y(pane, 0, 1)
        slot = self.span / n_slots
        shown = 0
        for k in range(n_slots):
            a, b = self.begin + k * slot, self.begin + (k + 1) * slot
            when = next((min(c + 0.15, b - 0.05) for c in self.res["cuts"] if a <= c < b - 0.2), (a + b) / 2)
            img = vid.frame(src, when, width=int(1.5 * thumb_w))
            _stroke(pane, when, 0, 0.12, color="#222222", linewidth=1.0)
            if img is not None:
                centre, half = (a + b) / 2, thumb_w / self.px_per_s / 2
                pane.imshow(numpy.asarray(img), extent=(centre - half, centre + half, 0.12, 1.0), aspect="auto",
                          zorder=2)
                shown += 1
        pane.set_xlim(self.begin, self.until)
        return shown

    def sections_lane(self, pane) -> None:
        from matplotlib.patches import Rectangle

        _bare_y(pane, 0, 1)
        glyph_px = 0.55 * self.pt * 0.9 * 100 / 72
        for k, sec in enumerate(self.res["sections"]):
            a, b = max(sec["start"], self.begin), min(sec["end"], self.until)
            if b <= a:
                continue
            fill = "#f3e9d2" if k % 2 else "#dbe8f5"
            pane.add_patch(Rectangle((a, 0.08), b - a, 0.84, facecolor=fill, edgecolor="#999999", linewidth=0.6))
            room = int((b - a) * self.px_per_s / glyph_px) - 1
            if room < 4:
                continue
            name = sec["label"]
            if len(name) > room:
                name = name[: room - 2] + ".."
            pane.text(a + 4 / self.px_per_s, 0.5, name, **_AT_LEFT, fontsize=0.9 * self.pt, clip_on=True)

    def events_lane(self, pane) -> tuple[int, int, list]:
        cands = _label_candidates(self.res)
        spots = stagger(cands, self.begin, self.until, self.plot_px, self.char_px)
        kind_at = {ev["t"]: ev["kind"] for ev in self.res["events"] if ev.get("label")}
        _bare_y(pane, 0, _LABEL_ROWS)
        for row, at, low, text in spots:
            colour = KIND_COLOURS.get(kind_at.get(at, "-"), _FALLBACK_COLOUR)
            y = _LABEL_ROWS - row - 0.5
            fade = 0.55 if text.split(" ", 1)[0].endswith("?") else 1.0
            _stroke(pane, at, 0, y, color=colour, linewidth=0.8, alpha=fade)
            pane.text(low + 8 / self.px_per_s, y, text, **_AT_LEFT, fontsize=self.label_pt,
                    family="monospace", color="white", clip_on=True,
                    bbox={"boxstyle": "square,pad=0.25", "fc": colour, "ec": "none", "alpha": fade})
        kinds = sorted({kind_at.get(spot[1], "-") for spot in spots})
        return len(spots), len(cands), kinds

    def spectrogram_lane(self, pane, sig: numpy.ndarray) -> None:
        power, centres = log_spectrogram(sig, self.plot_px)
        low, high = spec_scale(power)
        pane.imshow(power, origin="lower", aspect="auto", cmap="magma", vmin=low, vmax=high, interpolation="bilinear",
                  extent=(self.begin, self.until, 0, _BANDS))
        hz_marks = [50, 100, 200, 500, 1000, 2000, 5000, 10000]
        pane.set_yticks(numpy.interp(numpy.log(hz_marks), numpy.log(centres), numpy.arange(_BANDS) + 0.5))
        pane.set_yticklabels(["50", "100", "200", "500", "1k", "2k", "5k", "10k"])
        for ev in self.res["events"]:
            at = ev["t"]
            if not self.begin <= at <= self.until:
                continue
            strong = bool(ev.get("label"))
            tick = (0.07 if strong else 0.035) * _BANDS
            _stroke(pane, at, _BANDS - tick, _BANDS, color="white", linewidth=1.0 if strong else 0.6,
                    alpha=0.9 if strong else 0.55)
        pane.set_ylim(0, _BANDS)

    def waveform_lane(self, pane, sig: numpy.ndarray) -> None:
        count = sig.size
        full = max(float(numpy.percentile(numpy.abs(sig), 99.9)) if count else 0.0, 1e-9)
        lows, highs, mids = _min_max_columns(sig, max(1, min(self.plot_px, count)))
        xs = self.begin + mids / max(count, 1) * self.span
        pane.fill_between(xs, numpy.clip(lows / full, -1, 1), numpy.clip(highs / full, -1, 1), color="#5a6b7d", linewidth=0)
        _bare_y(pane, -1.05, 1.05)
        self.titles["waveform"] = waveform_title(sig)

    def loudness_lane(self, pane) -> None:
        lv = self.res["loudness"]
        for quiet in lv.get("silences", []):
            pane.axvspan(quiet["start"], quiet["end"], color="#ffe14d", alpha=0.45, linewidth=0)
        mt, mv = _lufs_line(lv.get("momentary"))
        st, sv = _lufs_line(lv.get("short_term"))
        if mt.size:
            pane.plot(mt, mv, color="#5b9bd5", linewidth=0.8, label="momentary")
        if st.size:
            pane.plot(st, sv, color="#1f3c88", linewidth=2.0, label="short-term")
        whole = lv.get("integrated_lufs")
        if whole is not None and whole > -60:
            pane.axhline(whole, color="#d62728", linestyle="--", linewidth=1.0, label="integrated")
            pane.text(1.005, (whole + 60) / 60, f"{whole:.1f}", transform=pane.transAxes, ha="left", va="center",
                    fontsize=0.72 * self.pt, color="#d62728")
        for clip_t in lv.get("clipping", {}).get("times", []):
            _stroke(pane, clip_t, -3, 0, color="#d62728", linewidth=1.2)
        pane.set_ylim(-60, 0)
        pane.set_yticks([-50, -40, -30, -20, -10])
        if mt.size or st.size:
            pane.legend(loc="lower left", fontsize=0.65 * self.pt, frameon=False, ncol=3)

    def sync_lane(self, pane) -> None:
        music = self.res["music"]
        _bare_y(pane, 0, 1)
        if music.get("beats") and music.get("confidence") != "none":
            bars = set(music.get("downbeats", []))
            for bt in music["beats"]:
                if not self.begin <= bt <= self.until:
                    continue
                strong = bt in bars
                _stroke(pane, bt, 0, 0.45 if strong else 0.28, color="#111111" if strong else "#999999",
                        linewidth=0.8)
        for which, glyph, y in (("cues", "v", 0.84), ("cuts", "D", 0.6)):
            for it in self.res["sync"].get(which, {}).get("items", []):
                pane.plot([it["t"]], [y], marker=glyph, markersize=0.55 * self.pt, linestyle="none",
                        color=STATUS_COLOURS.get(it.get("status"), _FALLBACK_COLOUR), markeredgecolor="#333333",
                        markeredgewidth=0.4)

    # ---- finishing touches

    def kind_chips(self, kinds) -> None:
        x = 1 - 8 / self.width
        small = 0.72 * self.pt
        for kind in reversed(kinds):
            self.fig.text(x, 1 - 0.2 / self.height_in, kind, **_AT_RIGHT, fontsize=small, color="white",
                          bbox={"boxstyle": "round,pad=0.3", "fc": KIND_COLOURS.get(kind, _FALLBACK_COLOUR),
                                "ec": "none"})
            x -= (len(kind) * 0.65 * 0.72 * self.pt * 100 / 72 + 18) / self.width

    def dress(self, name: str, pane, bottom: bool) -> None:
        where, step = _tick_positions(self.begin, self.until)
        pane.set_xlim(self.begin, self.until)
        pane.set_xticks(where)
        pane.set_xticklabels([_tick_text(t, step) for t in where] if bottom else [], fontsize=0.8 * self.pt)
        if bottom:
            pane.set_xlabel("time (m:ss)", fontsize=0.85 * self.pt)
        for spine in pane.spines.values():
            spine.set_color(_RULE)
            spine.set_linewidth(0.6)
        pane.tick_params(axis="both", labelsize=0.72 * self.pt, length=3, color=_RULE)
        title = self.titles[name]
        if name in _TITLE_ON_PLOT:
            pane.text(0.004, 0.96, title, transform=pane.transAxes, **_AT_TOP_LEFT, fontsize=0.8 * self.pt,
                    zorder=20, bbox={"boxstyle": "square,pad=0.2", "fc": "white", "ec": "none", "alpha": 0.75})
        else:
            pane.text(-0.004, 0.5, title, transform=pane.transAxes, **_AT_RIGHT, fontsize=0.72 * self.pt,
                    color="#333333")

    def cut_marks(self) -> None:
        inside = [c for c in self.res["cuts"] if self.begin <= c <= self.until]
        for name, pane in self.pane.items():
            if name == "events":
                continue
            depth = 0 if name == "frames" else 15
            for c in inside:
                pane.axvline(c, zorder=depth, **_CUT_STYLE)


def build_figure(r: dict, m: numpy.ndarray, width: int):
    """Draw the whole sheet; return (figure, counts with page size in px)."""
    page = _Page(r, width)
    page.open()
    page.write_header()
    pane = page.pane
    thumbs = page.frames_lane(pane["frames"]) if "frames" in pane else 0
    page.sections_lane(pane["sections"])
    placed, tagged, kinds = page.events_lane(pane["events"])
    page.spectrogram_lane(pane["spectrogram"], m)
    page.waveform_lane(pane["waveform"], m)
    page.loudness_lane(pane["loudness"])
    page.sync_lane(pane["sync"])
    page.kind_chips(kinds)
    bottom = page.lanes[-1][0]
    for name, lane_ax in pane.items():
        page.dress(name, lane_ax, name == bottom)
    page.cut_marks()
    counts = {"thumbnails": thumbs, "labelled_events": placed, "tagged_events": tagged,
              "width": page.width, "height": int(round(page.height_in * _PX_PER_IN))}
    return page.fig, counts


# ---------------------------------------------------------------- entry point

def _mono(res: dict, path: str) -> numpy.ndarray:
    if res.get("has_audio", True):
        from hark.core import audio

        return numpy.asarray(audio.load(path, mono=True, start=res["start"], end=res["end"]), dtype=numpy.float64)
    return numpy.zeros(int(round((res["end"] - res["start"]) * _RATE)))


def _png_path(path: str, chosen) -> str:
    if chosen:
        return os.path.abspath(chosen)
    stem = os.path.splitext(os.path.basename(path))[0]
    return os.path.abspath(os.path.join(os.getcwd(), stem + "_sheet.png"))


def _report(png: str, tally: dict, res: dict) -> str:
    from hark.timeline._dsp import ascii_safe

    window = f"{_clock(res['start'])}-{_clock(res['end'])}"
    return ascii_safe(
        f"Wrote {png} ({tally['width']}x{tally['height']} px) for {window}: "
        f"{tally['thumbnails']} frames, {tally['labelled_events']} of {tally['tagged_events']} tagged events labelled, "
        f"{len(res['events'])} event ticks, {len(res['cuts'])} cut lines. "
        "Open it with an image viewer or a file read tool to look at it.")


def sheet(path, out_png=None, video=None, cues=None, start=None, end=None, detail="normal", width=2000,
          tl=None) -> dict:
    """Render a timeline result (given as ``tl`` or computed now) to a PNG; describe what it shows."""
    import matplotlib.pyplot as plt

    res = tl if tl is not None else timeline(path, video=video, cues=cues, start=start, end=end, detail=detail)
    png = _png_path(path, out_png)
    fig, tally = build_figure(res, _mono(res, path), width)
    os.makedirs(os.path.dirname(png), exist_ok=True)
    fig.savefig(png, dpi=_PX_PER_IN, facecolor="white")
    plt.close(fig)
    line = _report(png, tally, res)
    out = {"png": png, "width": tally["width"], "height": tally["height"], "start": res["start"], "end": res["end"]}
    for key in ("thumbnails", "labelled_events", "tagged_events"):
        out[key] = tally[key]
    out.update(events=res["events"], cuts=res["cuts"], timeline_summary=res["summary"], summary=line,
               text=line + "\n\nTimeline: " + res["summary"])
    return out


# ---------------------------------------------------------------- CLI

def _run_cli(args: argparse.Namespace) -> int:
    from hark.core.report import emit
    from hark.models import require_ml

    require_ml("sheet")
    emit(sheet(args.file, args.png, video=args.video, cues=args.cues, start=args.start, end=args.end,
               detail=args.detail, width=args.width), args)
    return 0


def add_cli(sub) -> None:
    """Add the ``hark sheet`` subcommand."""
    from hark.core.report import add_common

    cmd = sub.add_parser("sheet", help="one annotated PNG of the timeline, for agents that can look at images")
    add_window_flags(cmd)
    cmd.add_argument("-o", dest="png", help="output PNG (default <file stem>_sheet.png in the current folder)")
    cmd.add_argument("--width", type=int, default=2000, help="image width in px, 1000-2400 (default 2000)")
    add_common(cmd)
    cmd.set_defaults(func=_run_cli)
