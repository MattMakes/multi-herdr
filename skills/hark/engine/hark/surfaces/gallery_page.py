"""Gallery page: asset URLs, embedding, lossy previews and the HTML itself."""

from __future__ import annotations

import base64
import hashlib
import html
import io
import math
import os
import shutil
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any
from urllib.parse import quote

ENCODE_TIMEOUT_S = 120
ENCODE_WORKERS = 3
JPEG_QUALITY = 90
WORDS_CAP = 220
LUFS_CLAMP = (-30.0, -14.0)
LUFS_PERCENTILE = 10
ERR_TAIL = 300

esc = lambda v: html.escape(str(v), quote=True)  # noqa: E731


def _lossy() -> dict[str, tuple]:
    from hark.surfaces.gallery import LOSSY
    return LOSSY


# ---------------------------------------------------------------- media


class Media:
    """URLs or data URIs for the page's audio and images, with identical audio embedded once."""

    def __init__(self, html_path: str, assets: str, embed: bool, codec: str, kbps: int | None) -> None:
        self.page_dir = os.path.dirname(html_path)
        self.assets, self.embed, self.codec, self.kbps = assets, embed, codec, kbps
        self.lossy = embed and codec in _lossy()
        self.previews: dict[str, str] = {}
        self.enc_used: set[str] = set()
        self.encoded = 0
        self.ids: dict[str, str] = {}
        self.dedup = 0
        self.audio_bytes = 0
        self.image_bytes = 0

    # previews ---------------------------------------------------------
    def _preview_path(self, path: str) -> str:
        with open(path, "rb") as fh:
            sha = hashlib.sha1(fh.read()).hexdigest()[:10]
        stem = os.path.splitext(os.path.basename(path))[0]
        ext = _lossy()[self.codec][3]
        return os.path.join(self.assets, "enc", f"{stem}_{sha}_{self.kbps}k{ext}")

    def _encode(self, src: str, dst: str) -> bool:
        if os.path.isfile(dst) and os.path.getsize(dst) > 0:
            return False
        encoder, fmt = _lossy()[self.codec][0], _lossy()[self.codec][1]
        tmp = dst + ".part"
        cmd = ["ffmpeg", "-nostdin", "-v", "error", "-y", "-i", src, "-vn", "-map_metadata", "-1",
               "-ar", "48000", "-c:a", encoder, "-b:a", f"{self.kbps}k", "-f", fmt, tmp]
        if self.codec == "mp3":
            cmd[-1:-1] = ["-write_xing", "1"]
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=ENCODE_TIMEOUT_S)
        if proc.returncode != 0 or not os.path.isfile(tmp) or os.path.getsize(tmp) == 0:
            raise RuntimeError(f"ffmpeg could not encode {src}: {(proc.stderr or '')[-ERR_TAIL:]}")
        os.replace(tmp, dst)
        return True

    def prepare(self, paths: list[str]) -> None:
        """Encode every needed lossy preview before the page is written (3 worker threads)."""
        if not self.lossy:
            return
        if shutil.which("ffmpeg") is None:
            raise RuntimeError("lossy previews need ffmpeg on PATH; use --embed for lossless embedding")
        os.makedirs(os.path.join(self.assets, "enc"), exist_ok=True)
        jobs = {p: self._preview_path(p) for p in dict.fromkeys(paths)}
        with ThreadPoolExecutor(ENCODE_WORKERS) as pool:
            made = list(pool.map(lambda kv: self._encode(*kv), jobs.items()))
        self.encoded = sum(made)
        self.previews = jobs
        self.enc_used = {os.path.basename(v) for v in jobs.values()}

    # urls -------------------------------------------------------------
    def url(self, path: str) -> str:
        try:
            rel = os.path.relpath(path, self.page_dir)
        except ValueError:
            return Path(path).resolve().as_uri()
        return quote(rel.replace("\\", "/"), safe="/._-~")

    def _audio_data(self, path: str) -> tuple[bytes, str]:
        if self.lossy:
            with open(self.previews[path], "rb") as fh:
                return fh.read(), _lossy()[self.codec][2]
        if self.codec == "wav":
            mime = "audio/flac" if path.lower().endswith(".flac") else "audio/wav"
            with open(path, "rb") as fh:
                return fh.read(), mime
        return _flac_bytes(path), "audio/flac"

    def audio_attrs(self, path: str, sha1: str) -> str:
        if not self.embed:
            return f' src="{esc(self.url(path))}"'
        if sha1 in self.ids:
            self.dedup += 1
            return f' data-src-of="{self.ids[sha1]}"'
        data, mime = self._audio_data(path)
        self.audio_bytes += len(data)
        self.ids[sha1] = f"a{len(self.ids) + 1}"
        return f' id="{self.ids[sha1]}" src="data:{mime};base64,{base64.b64encode(data).decode()}"'

    def image_src(self, png: str) -> str:
        if not self.embed:
            return self.url(png)
        with open(png, "rb") as fh:
            data, mime = fh.read(), "image/png"
        if self.lossy:
            data, mime = _maybe_jpeg(data, mime)
        self.image_bytes += len(data)
        return f"data:{mime};base64,{base64.b64encode(data).decode()}"


def _flac_bytes(path: str) -> bytes:
    import soundfile as sf
    x, rate = sf.read(path, dtype="float32", always_2d=True)
    buf = io.BytesIO()
    sf.write(buf, x, rate, format="FLAC", subtype="PCM_24")
    return buf.getvalue()


def _maybe_jpeg(data: bytes, mime: str) -> tuple[bytes, str]:
    try:
        from PIL import Image
    except ImportError:
        return data, mime
    buf = io.BytesIO()
    with Image.open(io.BytesIO(data)) as im:
        im.convert("RGB").save(buf, "JPEG", quality=JPEG_QUALITY, optimize=True, progressive=True, subsampling=0)
    jpg = buf.getvalue()
    return (jpg, "image/jpeg") if len(jpg) < len(data) else (data, mime)


# ---------------------------------------------------------------- formats


def _finite(v: Any) -> bool:
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def f1(v: Any) -> str:
    return f"{v:.1f}" if _finite(v) else "-"


def length_text(d: Any) -> str:
    return "-" if not _finite(d) else (f"{d:.2f} s" if d < 10 else f"{d:.1f} s")


def t40_text(ms: Any) -> str:
    return "-" if not _finite(ms) else (f"{ms:.0f} ms" if ms < 1000 else f"{ms / 1000:.2f} s")


def _yes_no(v: bool) -> str:
    return "yes" if v else "no"


def _param_float(v: float) -> str:
    return str(v) if not math.isfinite(v) else (f"{v:.0f}" if abs(v) >= 1000 else f"{v:.4g}")


def _param_list(v: list | tuple) -> str:
    return ", ".join(param_text(x) for x in v[:8]) + (" ..." if len(v) > 8 else "")


def _param_dict(v: dict) -> str:
    return "{" + ", ".join(f"{k}: {param_text(x)}" for k, x in list(v.items())[:6]) + "}"


_PARAM_FORMATS: tuple[tuple[Any, Any], ...] = (
    (bool, _yes_no), (int, str), (float, _param_float), ((list, tuple), _param_list), (dict, _param_dict))


def param_text(v: Any) -> str:
    """Parameter values: yes/no, ints as is, floats to 4 significant digits, short lists and dicts."""
    fn = next((f for kind, f in _PARAM_FORMATS if isinstance(v, kind)), str)
    return fn(v)


def fmt_num(v: Any, fmt: str) -> str:
    return fmt.format(v) if _finite(v) else "-"


def known_passes(sounds: list[dict]) -> list[dict]:
    """The sounds whose critic pass value is known."""
    return [s for s in sounds if s["critic"] and s["critic"].get("passes") is not None]


def passed(known: list[dict]) -> int:
    return sum(1 for s in known if s["critic"]["passes"])


def all_same(v: dict) -> bool:
    """Every sound of the version is the same file as in an earlier version."""
    return bool(v["sounds"]) and all(s["same_as"] for s in v["sounds"])


def shown_name(name: str, variant: str | None) -> str:
    """Only the trailing digits of ``name_07`` when the sound has a variant name."""
    tail = name.rsplit("_", 1)
    return tail[1] if variant and len(tail) == 2 and tail[1].isdigit() else name


def pills(s: dict) -> str:
    same = (f'<span class="pill same" title="same file as in that version">= {esc(s["same_as"])}</span>'
            if s.get("same_as") else "")
    passes = (s.get("critic") or {}).get("passes")
    word = {True: "pass", False: "no pass"}.get(passes)
    cls = "pass" if passes else "nopass"
    return same + (f'<span class="pill {cls}">{word}</span>' if word else "")


def phrase_cells(b: dict) -> list[tuple[str, str, str, bool]]:
    return [("phrase z", fmt_num(b.get("phrase_z"), "{:.2f}"), "benchmark phrase z", True),
            ("AudioSet", b.get("audioset") or "-", "top class", True),
            ("stereo r", fmt_num(b.get("stereo_corr"), "{:.2f}"), "stereo correlation", True)]


def bench_value_cells(b: dict) -> list[tuple[str, str, str, bool]]:
    inside = f"{b['inside']:g} / {b['n_checks']}" if _finite(b.get("inside")) and b.get("n_checks") else "-"
    return [("bench tone", fmt_num(b.get("tone_db"), "{:.1f} dB"), "distance to the bench median", True),
            ("bench tail", fmt_num(b.get("t40_ratio"), "{:.2f} x"), "tail over bench median", True),
            ("inside", inside, "checks inside the real spread", True)]


def param_head(sounds: list[dict]) -> str:
    cells = "".join(f"<th>{esc(s['name'].rsplit('_', 1)[-1])} {esc(s.get('variant') or '')}</th>" for s in sounds)
    return f"<tr><th>parameter</th>{cells}</tr>"


def seed_row(sounds: list[dict]) -> str:
    cells = "".join(f"<td>{esc(s['seed'] if s['seed'] is not None else '-')}</td>" for s in sounds)
    return f"<tr><td>seed</td>{cells}</tr>"


def param_row(key: str, sounds: list[dict]) -> str:
    cells = "".join(f"<td>{esc(param_text(s['params'][key]) if key in s['params'] else '-')}</td>" for s in sounds)
    return f"<tr><td>{esc(key)}</td>{cells}</tr>"


SPAN_HTML = ('<span title="time span of every picture on this card; log frequency 30 Hz to 20 kHz, 72 dB range">'
             'span {:.2f} s</span>')


def loudness_target(lufs: list[float]) -> float | None:
    """10th percentile of the made sounds' LUFS, clamped to [-30, -14]."""
    import numpy as np
    vals = [v for v in lufs if _finite(v)]
    if not vals:
        return None
    t = float(np.percentile(vals, LUFS_PERCENTILE))
    return round(min(LUFS_CLAMP[1], max(LUFS_CLAMP[0], t)), 1)


# ---------------------------------------------------------------- page

CSS = """
*{box-sizing:border-box}body{margin:0;background:#f5f5f3;color:#14181f;font:14px/1.45 system-ui,sans-serif}
main{max-width:1440px;margin:0 auto;padding:18px}h1{margin:.2em 0}.lede{max-width:70em}
.meta{color:#555;font-size:13px}.top{display:flex;flex-wrap:wrap;gap:24px;align-items:flex-start}
nav{position:sticky;top:0;z-index:5;background:#f5f5f3ee;padding:8px 0;display:flex;flex-wrap:wrap;gap:14px;align-items:center}
nav a{color:#27466b;text-decoration:none}nav .sw{margin-left:auto}
.card{background:#fff;border:1px solid #ddd;border-radius:10px;padding:14px;margin:14px 0}
.card h3{display:inline;margin:0}.slug{font-family:ui-monospace,monospace;color:#777;margin-left:8px}
.badge{background:#27466b;color:#fff;border-radius:4px;padding:0 5px;font-size:11px;margin-left:6px}
.row{display:grid;grid-template-columns:110px 1fr;gap:10px;margin-top:10px}.row.solo{grid-template-columns:1fr}
.lab small{display:block;color:#777}.tiles{display:grid;gap:8px;overflow-x:auto}
.tile{border:1px solid #e3e3e3;border-radius:8px;padding:6px;min-width:176px}.tile.empty,.tile.ref{border-style:dashed}
.tile img,.tile .noimg{width:100%;height:84px;display:block;background:#fbfbfa}
.name{display:flex;gap:4px;align-items:center;font-size:12px}.name .v{color:#888}.pill{margin-left:auto;border-radius:9px;padding:0 6px;font-size:11px}
.pass{color:#1d7048;background:#e7f2ec}.nopass{color:#666;background:#eee}.same{color:#27466b;background:#e8eef6}
audio{width:100%;height:30px}.kv{display:grid;grid-template-columns:repeat(3,1fr);gap:2px;font-size:11px;font-variant-numeric:tabular-nums}
.kv span{color:#888;display:block}.kv .b{color:#27466b;border-top:1px solid #27466b33}
.flag{color:#9b3b2a;background:#f8ebe7;font-size:11px;padding:1px 4px}.words{font-size:11px;color:#555}
table{border-collapse:collapse;font-variant-numeric:tabular-nums}td,th{padding:2px 8px;text-align:left;border-bottom:1px solid #eee}
.bench{display:flex;flex-wrap:wrap;gap:16px;margin-top:8px}@media(max-width:760px){.row{grid-template-columns:1fr}
.hint{display:none}.tile{min-width:200px;scroll-snap-align:start}.tiles{scroll-snap-type:x mandatory}}
"""

SCRIPT = """
document.querySelectorAll('audio[data-src-of]').forEach(function(a){var s=document.getElementById(a.dataset.srcOf);if(s){a.src=s.src;}});
document.addEventListener('play',function(e){document.querySelectorAll('audio').forEach(function(a){if(a!==e.target){a.pause();}});},true);
(function(){var sw=document.getElementById('match');if(!sw){return;}var T=parseFloat(sw.dataset.target);
function set(){document.querySelectorAll('audio').forEach(function(a){var l=parseFloat(a.dataset.lufs);
a.volume=(sw.checked&&isFinite(l))?Math.min(1,Math.pow(10,(T-l)/20)):1;});}sw.addEventListener('change',set);set();})();
"""

LEGEND = (
    ("critic", "the score and pass result of the forge critic, measured against the forge references only, never "
     "the bench. Scores rank the variants of 1 category; they do not compare across categories, because each "
     "category has its own references and mode."),
    ("tone / tail t40", "critic numbers: the 1/3-octave spectral distance from the median of the forge references, "
     "and the time the sound takes to drop 40 dB."),
    ("LUFS", "the highest momentary loudness, K-weighted, over a 400 ms window."),
)
LEGEND_BENCH = (
    ("bench", "recordings kept away from the forge; the columns are made, forge refs and bench leave-one-out."),
    ("inside", "how many benchmark checks fall within the spread of the real recordings."),
    ("-", "no value: the category has no bench split, or the file changed after the benchmark (see the notes)."),
)
SPECTRO_TEXT = ("frequency on a log axis from 30 Hz at the bottom to 20 kHz at the top, with faint guide lines at "
                "100 Hz, 1 kHz and 10 kHz, a 72 dB range, drawn from the onset, with 1 time span per card so "
                "lengths compare.")


class Page:
    """The HTML of 1 gallery page."""

    def __init__(self, cards: list[dict], labels: list[str], results: list[dict], media: Media, title: str,
                 warnings: list[str], families: tuple) -> None:
        self.cards, self.labels, self.results, self.media = cards, labels, results, media
        self.title, self.warnings, self.families = title, warnings, families
        self.multi = len(labels) > 1
        self.n_refs = sum(1 for c in cards for r in c["refs"] if r.get("path"))

    # top ----------------------------------------------------------------
    def _made(self) -> list[dict]:
        return [s for c in self.cards for v in c["versions"] if v for s in v["sounds"]]

    def _lede(self) -> str:
        text = ("Each sound here comes from a code recipe and holds no recording. The forge tuned it by listening "
                "to its renders and comparing them with reference recordings.")
        text += (" Each version is 1 row and equal variants share a column, so you compare old and new vertically."
                 if self.multi else " Each category shows its variants in 1 row.")
        return f'<p class="lede">{esc(text)}</p>'

    def _label_sounds(self, label: str) -> list[dict]:
        versions = [v for c in self.cards for v in c["versions"] if v and v["label"] == label]
        return [s for v in versions for s in v["sounds"]]

    def _pass_part(self, label: str) -> str:
        known = known_passes(self._label_sounds(label))
        return f"{label + ' ' if self.multi else ''}{passed(known)} / {len(known)}"

    def _pass_text(self) -> str:
        return "critic pass " + ", ".join(self._pass_part(lab) for lab in self.labels)

    def _meta(self) -> str:
        items = [f"{len(self.cards)} categories", f"{len(self._made())} sounds", self._pass_text()]
        items += [f"benchmark [{r['label']}] {r['tag']} ({r['created']}, {os.path.basename(r['path'])})"
                  for r in self.results]
        if self.n_refs:
            items.append(f"{self.n_refs} forge references (licensed recordings: keep this page private)")
        if self.media.lossy:
            items.append(f'<span title="every number and picture is measured on the original files">audio '
                         f'{esc(self.media.codec.upper())} {self.media.kbps} kbps previews (numbers from the WAVs)</span>')
        items.append(f"built {time.strftime('%Y-%m-%d %H:%M')}")
        return '<p class="meta">' + " · ".join(i if i.startswith("<span") else esc(i) for i in items) + "</p>"

    def _legend(self) -> str:
        rows = list(LEGEND) + (list(LEGEND_BENCH) if self.results else [])
        if self.multi:
            rows.append((f"= {self.labels[0]}", "this row or sound has the same files as that version: the same "
                         "bytes, or untouched at the path the benchmark measured."))
        rows.append(("spectrogram", SPECTRO_TEXT))
        body = "".join(f"<dt>{esc(k)}</dt><dd>{esc(v)}</dd>" for k, v in rows)
        return f"<div><h4>How to read it</h4><dl>{body}</dl></div>"

    # overview -------------------------------------------------------------
    def _bench_cols(self) -> set[str]:
        return {v["label"] for c in self.cards for v in c["versions"] if v and v["bench_summary"]}

    def _overview_head(self, bench: set[str]) -> str:
        cells = ["category"]
        for lab in self.labels:
            pre = f"{lab} " if self.multi else ""
            cells += [f"{pre}sounds", f"{pre}critic pass"]
            cells += ["variety (refs)", "tail", "tone dB", "inside"] if lab in bench else []
        return "<tr>" + "".join(f"<th>{esc(c)}</th>" for c in cells) + "</tr>"

    def _count_cells(self, v: dict) -> list[str]:
        n = len(v["sounds"])
        known = known_passes(v["sounds"])
        same = f"= {v['sounds'][0]['same_as']} " if all_same(v) else ""
        return [f"{same}{n}", f"{passed(known)} / {n}" if known else "-"]

    def _overview_cells(self, v: dict | None, bench: set[str], lab: str) -> list[str]:
        cells = self._count_cells(v) if v else ["-", "-"]
        more = (self._bench_values(v) if v else ["-"] * 4) if lab in bench else []
        return cells + more

    @staticmethod
    def _bench_value(rows: dict[str, dict], name: str, refs: bool = False) -> str:
        r = rows.get(name)
        extra = f" ({r['forge']})" if r and refs and r["forge"] != "-" else ""
        return r["made"] + extra if r else "-"

    def _bench_values(self, v: dict) -> list[str]:
        rows = {r["label"]: r for r in (v["bench_summary"] or {}).get("rows", [])}
        return [self._bench_value(rows, "variety", True), self._bench_value(rows, "tail t40"),
                self._bench_value(rows, "tone distance"), self._bench_value(rows, "inside")]

    def _overview(self) -> str:
        bench = self._bench_cols()
        width = 1 + sum(2 + (4 if lab in bench else 0) for lab in self.labels)
        rows = [self._overview_head(bench)]
        for key, title, _, cards in self._grouped():
            rows.append(f'<tr><td colspan="{width}"><a href="#fam-{key}"><b>{esc(title)}</b></a></td></tr>')
            for c in cards:
                cells = [f'<a href="#{esc(c["cat"])}">{esc(c["cat"])}</a>']
                for lab, v in zip(self.labels, c["versions"]):
                    cells += [esc(x) for x in self._overview_cells(v, bench, lab)]
                rows.append("<tr>" + "".join(f"<td>{x}</td>" for x in cells) + "</tr>")
        return "<table>" + "".join(rows) + "</table>"

    def _grouped(self) -> list[tuple[str, str, str, list[dict]]]:
        out = []
        for key, title, line, _ in self.families:
            cards = [c for c in self.cards if c["family"] == key]
            if cards:
                out.append((key, title, line, cards))
        return out

    # nav --------------------------------------------------------------------
    def _switch(self) -> str:
        t = loudness_target([s["lufs"] for s in self._made()])
        if t is None:
            return ""
        tip = (f"louder sounds are turned down to {t:.0f} LUFS momentary max; quieter ones play as they are")
        return (f'<label class="sw" title="{esc(tip)}"><input type="checkbox" id="match" checked '
                f'data-target="{t:.1f}"> match loudness</label>')

    def _nav(self) -> str:
        links = "".join(f'<a href="#fam-{k}">{esc(t)} ({len(c)})</a>' for k, t, _, c in self._grouped())
        return f"<nav>{links}{self._switch()}</nav>"

    # tiles ----------------------------------------------------------------
    def _img(self, s: dict) -> str:
        if not s.get("thumb"):
            return '<div class="noimg"></div>'
        return f'<img loading="lazy" alt="spectrogram of {esc(s["name"])}" src="{esc(self.media.image_src(s["thumb"]))}">'

    def _player(self, s: dict) -> str:
        lufs = f' data-lufs="{s["lufs"]:.1f}"' if _finite(s.get("lufs")) else ""
        return f'<audio controls preload="none"{lufs}{self.media.audio_attrs(s["path"], s["sha1"])}></audio>'

    def _base_cells(self, s: dict) -> list[tuple[str, str, str, bool]]:
        return [("length", length_text(s["dur_s"]), "length of the file", False),
                ("peak", f1(s["peak_dbfs"]), "sample peak, dBFS", False),
                ("LUFS", f1(s["lufs"]), "momentary-max loudness, 400 ms", False)]

    def _critic_cells(self, c: dict) -> list[tuple[str, str, str, bool]]:
        total = f"{c['total']:.2f}" if _finite(c.get("total")) else "-"
        tone = f"{c['tone_db']:.1f} dB" if _finite(c.get("tone_db")) else "-"
        return [("critic", total, "critic total score", False), ("tone", tone, "spectral distance to forge refs", False),
                ("tail t40", t40_text(c.get("t40_ms")), "time to drop 40 dB", False)]

    def _bench_cells(self, b: dict) -> list[tuple[str, str, str, bool]]:
        plain = b.get("tone_db") is None and b.get("t40_ratio") is None
        return phrase_cells(b) if plain else bench_value_cells(b)

    def _kv(self, cells: list[tuple[str, str, str, bool]]) -> str:
        return '<div class="kv">' + "".join(
            f'<div class="{"b" if b else ""}" title="{esc(tip)}"><span>{esc(k)}</span>{esc(v)}</div>'
            for k, v, tip, b in cells) + "</div>"

    def _name_line(self, s: dict) -> str:
        name, var = s["name"], s.get("variant")
        v = f'<span class="v">{esc(var)}</span>' if var else ""
        tip = name + (f" ({var})" if var else "")
        return f'<div class="name" title="{esc(tip)}">{esc(shown_name(name, var))} {v}{pills(s)}</div>'

    def _words(self, c: dict) -> str:
        words = str(c.get("words") or "")
        if words and not words.lower().startswith("clean"):
            return f'<div class="words">{esc(words[:WORDS_CAP])}</div>'
        like = c.get("sounds_like") or []
        return f'<div class="words">sounds like: {esc(", ".join(like[:2]))}</div>' if like else ""

    def tile(self, s: dict | None) -> str:
        if s is None:
            return '<div class="tile empty"></div>'
        cells = self._base_cells(s) + (self._critic_cells(s["critic"]) if s["critic"] else [])
        cells += self._bench_cells(s["bench"]) if s.get("bench") else []
        art = (s.get("bench") or {}).get("artifacts")
        flag = f'<div class="flag">artifact: {esc(", ".join(art))}</div>' if art else ""
        words = self._words(s["critic"]) if s["critic"] else ""
        return (f'<div class="tile">{self._img(s)}{self._name_line(s)}{self._player(s)}'
                f'{self._kv(cells)}{flag}{words}</div>')

    def ref_tile(self, r: dict) -> str:
        if not r.get("path"):
            return '<div class="tile empty"></div>'
        return (f'<div class="tile ref">{self._img(r)}<div class="name">{esc(os.path.basename(r["path"]))}</div>'
                f'{self._player(r)}{self._kv(self._base_cells(r))}</div>')

    # card -----------------------------------------------------------------
    def _newest(self, card: dict) -> dict | None:
        return next((v for v in reversed(card["versions"]) if v and v["sounds"]), None)

    def _target_line(self, card: dict) -> str:
        v = self._newest(card)
        target = v["report"].get("target") if v else None
        if not target:
            return ""
        mode = v["report"].get("mode")
        return f'<p class="meta">critic target "{esc(target)}"{esc(" · " + mode + " mode") if mode else ""}</p>'

    def _links(self, v: dict) -> str:
        if self.media.embed:
            return ""
        items = [(os.path.join(v["folder"], "report.md"), "report.md"),
                 (os.path.join(v["folder"], "compare.png"), "compare sheet"),
                 (os.path.join(os.path.dirname(v["folder"]), "LOG.md"), "LOG.md")]
        return "".join(f' <a href="{esc(self.media.url(p))}">{esc(t)}</a>' for p, t in items if os.path.isfile(p))

    def _meta_part(self, lab: str, v: dict | None) -> str:
        pre = f"{lab} " if self.multi else ""
        sounds = v["sounds"] if v else []
        known = known_passes(sounds)
        text = f"{pre}critic pass {passed(known)} / {len(sounds)}" if known else f"{pre}{len(sounds)} sounds"
        return esc(text if v else f"{lab}: -")

    def _card_meta(self, card: dict) -> str:
        parts = [self._meta_part(lab, v) for lab, v in zip(self.labels, card["versions"])]
        newest = self._newest(card)
        links = self._links(newest) if newest else ""
        return f'<p class="meta">{" · ".join(parts)}{links} · {SPAN_HTML.format(card["span"])}</p>'

    def _bench_block(self, card: dict) -> str:
        blocks = [self._bench_one(v) for v in card["versions"] if v]
        blocks = [b for b in blocks if b]
        return f'<div class="bench">{"".join(blocks)}</div>' if blocks else ""

    def _bench_one(self, v: dict) -> str:
        pre = f"{v['label']}: " if self.multi else ""
        if v["bench_stale"]:
            return f'<p class="meta">{esc(pre)}benchmark numbers withheld: these files changed after the run (see the notes at the end)</p>'
        summ = v["bench_summary"]
        if not summ or not summ["rows"]:
            return ""
        head = "held-out bench" + (f" ({summ['n_bench']} bench refs)" if summ.get("n_bench") is not None else "")
        rows = "".join(f'<tr><td>{esc(r["label"])}</td><td><b>{esc(r["made"])}</b></td><td>{esc(r["forge"])}</td>'
                       f'<td>{esc(r["loo"])}</td><td class="hint">{esc(r["hint"])}</td></tr>' for r in summ["rows"])
        label = ""
        if self.multi:
            borrowed = f" (files unchanged since the {v['bench_from']} benchmark run)" if v.get("bench_from") else ""
            label = f"<div>{esc(v['label'] + borrowed)}</div>"
        return (f"<div>{label}<table><tr><th>{esc(head)}</th><th>made</th><th>forge refs</th><th>bench LOO</th>"
                f'<th class="hint"></th></tr>{rows}</table></div>')

    def _cols(self, card: dict) -> int:
        lens = [len(v["order"]) for v in card["versions"] if v] + [len(card["refs"]), 1]
        return max(lens)

    def _row(self, label: str, sub: str, tiles: str, cols: int, labelled: bool) -> str:
        grid = f'<div class="tiles" style="grid-template-columns:repeat({cols},minmax(176px,1fr))">{tiles}</div>'
        if not labelled:
            return f'<div class="row solo">{grid}</div>'
        return f'<div class="row"><div class="lab">{esc(label)}<small>{esc(sub)}</small></div>{grid}</div>'

    def _version_row(self, lab: str, v: dict | None, cols: int, labelled: bool) -> str:
        if not v:
            return self._row(lab, "", f'<p class="meta">no {esc(lab)} sounds for this category</p>', cols, labelled)
        sub = "unchanged" if all_same(v) else f"{len(v['sounds'])} sounds"
        tiles = "".join(self.tile(s) for s in v["order"])
        return self._row(lab if self.multi else "made", sub, tiles, cols, labelled)

    def _rows(self, card: dict) -> str:
        cols, labelled = self._cols(card), self.multi or bool(card["refs"])
        out = [self._version_row(lab, v, cols, labelled) for lab, v in zip(self.labels, card["versions"])]
        refs = "".join(self.ref_tile(r) for r in card["refs"])
        out += [self._row("ref", "nearest forge ref", refs, cols, True)] if card["refs"] else []
        return "".join(out)

    def _audition(self, card: dict) -> str:
        reel = card.get("audition")
        if not reel:
            return ""
        from hark.surfaces import gallery_sound as gs
        sha = gs.decode(reel).sha1
        return (f'<p class="meta">audition (made, nearest ref, alternating) '
                f'<audio controls preload="none"{self.media.audio_attrs(reel, sha)}></audio></p>')

    def _params(self, card: dict) -> str:
        versions = [v for v in card["versions"] if v and v["sounds"]]
        n = sum(1 for v in versions for s in v["sounds"] if s["params"])
        if not n:
            return ""
        body = "".join(self._param_table(v) for v in versions)
        return f"<details><summary>Recipe and parameters ({n} sounds)</summary>{body}</details>"

    def _method_line(self, v: dict) -> str:
        rep = v["report"]
        pre = f"{v['label']}: " if self.multi else ""
        method = esc(". " + rep["method"]) if rep.get("method") else ""
        return f"<p><b>{esc(pre)}recipe {esc(rep.get('recipe') or '?')}</b>{method}</p>"

    def _param_table(self, v: dict) -> str:
        sounds = v["sounds"]
        keys = list(dict.fromkeys(k for s in sounds for k in s["params"]))
        label = f'<tr><td colspan="{len(sounds) + 1}"><b>{esc(v["label"])}</b></td></tr>' if self.multi else ""
        rows = "".join(param_row(k, sounds) for k in keys)
        return f"{self._method_line(v)}<table>{label}{param_head(sounds)}{seed_row(sounds)}{rows}</table>"

    def card(self, card: dict) -> str:
        pro = '<span class="badge" title="modelled on the designed effects of reference launch films">pro</span>' \
            if card["cat"].startswith("pro_") else ""
        desc = f"<p>{esc(card['desc'])}</p>" if card.get("desc") else ""
        return (f'<article class="card" id="{esc(card["cat"])}"><div><h3>{esc(card["display"])}</h3>{pro}'
                f'<span class="slug">{esc(card["cat"])}</span></div>{desc}{self._target_line(card)}'
                f"{self._card_meta(card)}{self._bench_block(card)}{self._rows(card)}{self._audition(card)}"
                f"{self._params(card)}</article>")

    # whole page -------------------------------------------------------------
    def _sections(self) -> str:
        return "".join(f'<section id="fam-{k}"><h2>{esc(t)}</h2><p class="meta">{esc(line)}</p>'
                       + "".join(self.card(c) for c in cards) + "</section>" for k, t, line, cards in self._grouped())

    def _footer(self) -> str:
        text = "Made with hark gallery. The audio is computed by forge recipes."
        if self.n_refs:
            text += " Licensed forge-side reference recordings are included for A/B."
        if self.media.lossy:
            text += f" The players use {self.media.codec.upper()} {self.media.kbps} kbps previews of the WAV files."
        return f'<footer class="meta">{esc(text + " Bench references are never shown.")}</footer>'

    def _notes(self) -> str:
        if not self.warnings:
            return ""
        items = "".join(f"<li>{esc(w)}</li>" for w in self.warnings)
        return f"<details><summary>{len(self.warnings)} note(s) from the build</summary><ul>{items}</ul></details>"

    def render(self) -> str:
        if self.media.embed and self.n_refs:
            self.warnings.insert(0, "this file carries licensed reference recordings and must stay private; "
                                    "build without --refs to share it")
        body = self._sections()
        head = (f'<header><h1>{esc(self.title)}</h1>{self._lede()}{self._meta()}'
                f'<div class="top">{self._overview()}{self._legend()}</div></header>')
        return ('<!doctype html><html lang="en"><head><meta charset="utf-8">'
                '<meta name="viewport" content="width=device-width, initial-scale=1">'
                f"<title>{esc(self.title)}</title><style>{CSS}</style></head><body><main>"
                f"{head}{self._nav()}{body}{self._notes()}{self._footer()}</main><script>{SCRIPT}</script></body></html>")
