"""Synthesis kernels shared by the fit analysis modules.

This file imports only numpy and scipy. The standalone scripts that
``modal.to_code`` and ``sinusoidal.to_code`` write hold a verbatim copy of it,
so a script renders the same samples as the package.
"""

import numpy as np
from scipy.fft import irfft, next_fast_len, rfft, rfftfreq
from scipy.signal import lfilter, resample_poly

SR = 48000
TWO_PI = 2.0 * np.pi


# ---------------------------------------------------------------- generic

def tri_masks(centers, nfft, sr=SR):
    """Log-frequency triangles at ``centers`` on the rfft bins; they sum to 1."""
    f = rfftfreq(int(nfft), 1.0 / sr)
    lf = np.log2(np.maximum(f, 1.0))
    lc = np.log2(np.asarray(centers, dtype=float))
    eye = np.eye(len(lc))
    return np.array([np.interp(lf, lc, eye[b]) for b in range(len(lc))])


def finish(y, sr=SR, fade_s=0.002, peak_db=-1.0, limit=True):
    """Half-cosine power fade-in, then a gain down to ``peak_db`` when over."""
    y = np.array(y, dtype=float, copy=True)
    k = int(round(fade_s * sr))
    if k > 1 and len(y):
        k = min(k, len(y))
        ramp = np.sin(np.pi * np.arange(k) / (2.0 * k)) ** 2
        y[:k] = (y[:k].T * ramp).T
    if limit and y.size:
        peak = float(np.max(np.abs(y)))
        cap = 10.0 ** (peak_db / 20.0)
        if peak > cap:
            y = y * (cap / peak)
    return y


def raised_cos(k):
    """Rising raised cosine of ``k`` samples (empty for k < 1)."""
    if k < 1:
        return np.ones(0)
    return 0.5 - 0.5 * np.cos(np.pi * np.arange(k) / k)


def side_from_gains(mid, bands, pan, dif, sign=1.0, sr=SR):
    """Side signal from a mid signal and per-sample band gains (B, n)."""
    mid = np.asarray(mid, dtype=float)
    n = len(mid)
    out = np.zeros(n)
    if n == 0:
        return out
    nfft = next_fast_len(n + 8192)
    spec = rfft(mid, nfft)
    spec[0] = 0.0
    masks = tri_masks(bands, nfft, sr)
    for b in range(len(bands)):
        band = spec * masks[b]
        if np.any(np.abs(pan[b]) > 1e-4):
            out += pan[b] * irfft(band, nfft)[:n]
        if np.any(dif[b] > 1e-4):
            out += dif[b] * irfft(band * (-1j * sign), nfft)[:n]
    return out


def shaped_side(mid, bands, times, pan, diffuse, sign=1, sr=SR):
    """A side signal made from ``mid``: pan gains on the band, diffuse gains on its quadrature."""
    mid = np.asarray(mid, dtype=float)
    n = len(mid)
    if n == 0:
        return np.zeros(0)
    t = np.arange(n) / sr
    pan_s = np.array([np.interp(t, times, p) for p in pan])
    dif_s = np.array([np.interp(t, times, d) for d in diffuse])
    return side_from_gains(mid, bands, pan_s, dif_s, float(sign), sr)


# ---------------------------------------------------------------- sines kernels

def synth_partials(partials, n, sr=SR, cents=None):
    """Sum of partials with log2-linear frequency and dB-linear amplitude breakpoints."""
    y = np.zeros(n)
    for p in partials:
        i0 = max(0, int(np.floor(p["t0"] * sr)))
        i1 = min(n, int(np.ceil((p["t1"] + p["rel"]) * sr)) + 1)
        if i1 <= i0:
            continue
        y[i0:i1] += _partial_wave(p, i0, i1, sr, cents)
    return y


def _partial_wave(p, i0, i1, sr, cents):
    t = np.arange(i0, i1) / sr
    fr = np.asarray(p["freq"], dtype=float)
    am = np.asarray(p["amp"], dtype=float)
    f = 2.0 ** np.interp(t, fr[:, 0], np.log2(np.maximum(fr[:, 1], 1e-6)))
    if cents is not None:
        f = f * 2.0 ** (cents[i0:i1] / 1200.0)
    a = 10.0 ** (np.interp(t, am[:, 0], am[:, 1]) / 20.0)
    rise = max(float(p.get("rise", 0.0002)), 1e-9)
    rel = max(float(p.get("rel", 0.0)), 1e-9)
    end = p["t1"] + p["rel"]
    a = a * np.clip((t - p["t0"]) / rise, 0.0, 1.0) * np.clip((end - t) / rel, 0.0, 1.0)
    a[f >= 0.49 * sr] = 0.0
    ph = p["ph"] + TWO_PI * (np.cumsum(f) - f[0]) / sr + TWO_PI * f[0] * (i0 / sr - p["t0"])
    return a * np.cos(ph)


def synth_atoms(transients, n, sr=SR):
    """Strike atoms: damped cosines that start at full amplitude."""
    y = np.zeros(n)
    for tr in transients:
        s = int(round(tr["t"] * sr))
        for f, tau, a, ph in tr["atoms"]:
            k_n = min(int(7 * tau * sr) + 1, n - max(s, 0))
            if k_n <= 0 or tau <= 0:
                continue
            k = np.arange(k_n)
            if s < 0:
                k = k - s
            wave = a * np.exp(-k / (tau * sr)) * np.cos(TWO_PI * f * k / sr + ph)
            y[max(s, 0):max(s, 0) + k_n] += wave
    return y


def synth_noise(bands, times, levels, n, seed=0, sr=SR, pad=4096, hf=None):
    """Band noise with dB-linear level curves per band."""
    y = np.zeros(n)
    if n <= 0:
        return y
    rng = np.random.default_rng(int(seed) & 0xFFFFFFFF)
    nfft = 1 << int(np.ceil(np.log2(n + 2 * pad)))
    spec = rfft(rng.standard_normal(nfft))
    masks = tri_masks(bands, nfft, sr)
    shaped, keep = _hf_shape(masks, nfft, sr, hf)
    t = np.arange(n) / sr
    for b, lev in enumerate(levels):
        if max(lev) < -140:
            continue
        band = irfft(spec * shaped[b], nfft)[pad:pad + n]
        rms = np.sqrt(np.mean(band ** 2))
        if rms <= 0:
            continue
        y += band * (keep[b] / rms) * 10.0 ** (np.interp(t, times, lev) / 20.0)
    return y


def _hf_shape(masks, nfft, sr, hf):
    if not hf:
        return masks, np.ones(len(masks))
    f = rfftfreq(nfft, 1.0 / sr)
    hf = np.asarray(hf, dtype=float)
    gain = np.where(f > hf[0, 0], 10.0 ** (np.interp(f, hf[:, 0], hf[:, 1]) / 20.0), 1.0)
    shaped = masks * gain
    keep = np.sqrt(np.sum(shaped ** 2, axis=1) / np.maximum(np.sum(masks ** 2, axis=1), 1e-30))
    return shaped, keep


def sines_fade_in(model):
    """Fade-in length (s) of a sines render."""
    if not model.get("coherent"):
        return 0.0
    return float(np.clip(model.get("onset", 0.0) - 0.0005, 0.00025, 0.002))


def sines_identity(model, seed=0):
    """Mono render of a sines model at its identity knobs."""
    sr = SR
    n = int(round(model["duration"] * sr))
    res = model["residual"]
    y = synth_partials(model["partials"], n, sr)
    y = y + synth_atoms(model.get("transients", []), n, sr)
    y = y + synth_noise(res["bands"], res["times"], res["db"], n, seed, sr, hf=res.get("hf"))
    return sines_finish(model, y)


def sines_finish(model, y):
    """The finish of a sines render: fade-in and, for a full model, the peak gain."""
    if not model.get("coherent"):
        return y
    limit = model.get("layer") != "deterministic"
    return finish(y, SR, sines_fade_in(model), -1.0, limit)


# ---------------------------------------------------------------- modal kernels

NOISE_EDGES = (50, 100, 200, 300, 400, 510, 630, 770, 920, 1080, 1270, 1480, 1720, 2000, 2320,
               2700, 3150, 3700, 4400, 5300, 6400, 7700, 9500, 12000, 14000, 16000, 18500)
GRID_S = 0.0005
SUB_DEC = 480
LATER_RAMP = 7


def erb_rate(f):
    return 21.4 * np.log10(1.0 + 0.00437 * np.asarray(f, dtype=float))


def erb_masks(n_fft, edges=NOISE_EDGES, h=0.4, sr=SR):
    """Power-complementary ERB-rate crossover masks on the rfft bins (bands, bins)."""
    u = erb_rate(rfftfreq(int(n_fft), 1.0 / sr))
    s = [np.clip((u - erb_rate(e) + h) / (2 * h), 0.0, 1.0) for e in edges]
    nb = len(edges) + 1
    out = np.ones((nb, len(u)))
    for b in range(nb):
        if b > 0:
            out[b] *= np.sin(np.pi * s[b - 1] / 2)
        if b < nb - 1:
            out[b] *= np.cos(np.pi * s[b] / 2)
    return out


def erb_centres(edges=NOISE_EDGES, top=22000.0):
    e = np.asarray(edges, dtype=float)
    mids = np.sqrt(e[:-1] * e[1:])
    return np.concatenate([[e[0] / np.sqrt(2)], mids, [np.sqrt(e[-1] * top)]])


def modal_gens(seed):
    s = int(seed) & 0xFFFFFFFF
    return {k: np.random.default_rng([s, k]) for k in (1, 2, 3, 5)}


def vary_fields(r, n_fields, parts=6, scale=np.sqrt(1.0 / 3.0)):
    """Smooth random functions of log frequency, drawn in order."""
    out = []
    for _ in range(n_fields):
        c = r.uniform(0.25, 1.25, parts)
        p = r.uniform(0.0, TWO_PI, parts)
        out.append((c, p, scale))
    return out


def field_at(field, f):
    c, p, scale = field
    lf = np.log2(np.maximum(np.asarray(f, dtype=float), 20.0))
    return scale * np.sum(np.cos(TWO_PI * np.multiply.outer(lf, c) + p), axis=-1)


def modal_plan(model, seed, size=1.0, damping=1.0, brightness=0.0, hardness=1.0,
               vary=0.0, spacing=1.0):
    """Strike times and mode rows after the knobs."""
    gens = modal_gens(seed)
    rj = gens[1]
    strikes = model["strikes"]
    coherent = bool(model.get("coherent"))
    told = [s["t"] for s in strikes]
    tnew = _strike_times(told, spacing, vary, coherent, rj)
    stretch = max(size / damping, 0.001)
    tilt = brightness + 4.0 * np.log2(max(hardness, 0.001))
    fields = vary_fields(rj, 3) if coherent else None
    move = coherent and (vary > 0 or size != 1 or spacing != 1)
    plan = []
    for i, s in enumerate(strikes):
        rows = _plan_modes(s["modes"], size, stretch, tilt, vary, fields, rj)
        if move and i > 0 and len(rows):
            f0 = np.asarray([m[0] for m in s["modes"]], dtype=float)
            rows[:, 3] += TWO_PI * (rows[:, 0] * (tnew[i] - tnew[0]) - f0 * (told[i] - told[0]))
        rows = rows[rows[:, 0] < 0.47 * SR] if len(rows) else rows
        att = _plan_contact(s.get("attack", []), size, hardness, tilt)
        plan.append({"t": tnew[i], "t_old": told[i], "ramp_ms": s.get("ramp_ms", 0.0) / max(hardness, 1e-3),
                     "modes": rows, "attack": att})
    return plan, stretch


def _strike_times(told, spacing, vary, coherent, rj):
    if not told:
        return []
    out = [told[0]]
    for i in range(1, len(told)):
        gap = (told[i] - told[i - 1]) * spacing
        if not coherent:
            gap = max(gap + vary * 0.002 * rj.standard_normal(), 0.003)
        out.append(out[-1] + gap)
    return out


def _plan_modes(modes, size, stretch, tilt, vary, fields, rj):
    if not modes:
        return np.zeros((0, 4))
    m = np.array(modes, dtype=float)
    f0 = m[:, 0]
    if fields is not None:
        zf, zt, za = (field_at(fd, f0) for fd in fields)
    else:
        zf, zt, za = (rj.standard_normal(len(f0)) for _ in range(3))
    f = f0 / size * (1.0 + 0.006 * vary * zf)
    tau = m[:, 1] * stretch * np.exp(0.12 * vary * zt)
    a = m[:, 2] * 10.0 ** ((tilt * np.log2(np.maximum(f, 20.0) / 1000.0) + 1.5 * vary * za) / 20.0)
    return np.column_stack([f, tau, a, m[:, 3].copy()])


def _plan_contact(attack, size, hardness, tilt):
    if not attack:
        return np.zeros((0, 4))
    m = np.array(attack, dtype=float)
    fa = m[:, 0] / size
    ta = m[:, 1] * size / np.sqrt(max(hardness, 1e-3))
    aa = m[:, 2] * 10.0 ** (tilt * np.log2(np.maximum(fa, 20.0) / 1000.0) / 20.0)
    rows = np.column_stack([fa, ta, aa, m[:, 3]])
    return rows[rows[:, 0] < 0.47 * SR]


def ring_extension(plan, length):
    """Seconds to add after ``length`` so the modes ring 60 dB down."""
    sums = [np.sqrt(np.sum(p["modes"][:, 2] ** 2)) for p in plan if len(p["modes"])]
    if not sums or max(sums) <= 0:
        return 0.0
    ref = max(sums)
    thr = ref * 1e-3
    end, power = 0.0, 0.0
    for p in plan:
        for f, tau, a, _ in p["modes"]:
            if a > thr:
                end = max(end, p["t"] + tau * np.log(a / thr))
            if length > p["t"]:
                power += (a * np.exp(-(length - p["t"]) / tau)) ** 2
    if end <= length + 0.001:
        return 0.0
    level = 10.0 * np.log10(max(power, 1e-300) / ref ** 2)
    lo = 0.2 if level > -50 else 0.06
    first = plan[0]["t"]
    return float(min(max(end - length, lo), max(lo, 0.5 * (length - first))))


def mode_wave(rows, n, sr=SR):
    """Re(sum C z^k) for k = 0 .. n-1, in blocks."""
    y = np.zeros(n)
    if not len(rows) or n <= 0:
        return y
    z = np.exp(-1.0 / (rows[:, 1] * sr) + 2j * np.pi * rows[:, 0] / sr)
    c = rows[:, 2] * np.exp(1j * rows[:, 3])
    top = min(n, int(14 * np.max(rows[:, 1]) * sr) + 1)
    for b0 in range(0, top, 4096):
        b1 = min(top, b0 + 4096)
        k = np.arange(b0, b1)
        y[b0:b1] = np.real(np.exp(np.multiply.outer(k, np.log(z))) @ c)
    return y


def strike_wave(p, n, first, coherent, sr=SR):
    """One strike's modes (with ramp) plus contact, from its onset sample to n."""
    s = int(round(p["t"] * sr))
    if s >= n:
        return s, np.zeros(0)
    s0 = max(s, 0)
    m = n - s0
    w = mode_wave(p["modes"], m, sr)
    r = int(round(p["ramp_ms"] * sr / 1000.0))
    if r > 0:
        k = min(r, m)
        w[:k] *= raised_cos(r)[:k]
    w = w + mode_wave(p["attack"], m, sr)
    if not first and coherent:
        k = min(LATER_RAMP, m)
        w[:k] *= raised_cos(LATER_RAMP)[:k]
    return s0, w


def modal_noise_env(model, plan, n_grid, stretch, size, brightness, hardness, noise, vary,
                    ring, rj):
    """Noise band levels (dB) on the new 0.5 ms grid, shape (bands, n_grid)."""
    nz = model["noise"]
    tg = np.arange(n_grid) * GRID_S
    old = _old_times(plan, tg, stretch)
    cut = _segment_cuts(model, plan, tg, stretch, ring)
    env = np.array([_band_level(b, old, cut) for b in nz["env"]])
    cent = erb_centres(nz["edges"])
    lc = np.log2(cent)
    if size != 1:
        env = np.array([np.interp(lc + np.log2(size), lc, env[:, k]) for k in range(n_grid)]).T
    env = env + (brightness * np.log2(cent / 1000.0))[:, None] + noise
    if hardness != 1:
        env = env + np.outer(4 * np.log2(hardness) * np.log2(cent / 1000.0), _hard_window(plan, tg))
    if vary:
        env = env + (1.5 * vary * rj.standard_normal(len(cent)))[:, None]
    return env


def _seg_index(plan, tg):
    tn = np.array([p["t"] for p in plan]) if plan else np.zeros(1)
    return np.clip(np.searchsorted(tn, tg, side="right") - 1, 0, len(tn) - 1), tn


def _old_times(plan, tg, stretch):
    if not plan:
        return tg.copy()
    seg, tn = _seg_index(plan, tg)
    to = np.array([p["t_old"] for p in plan])
    rel = tg - tn[seg]
    orel = np.where(rel < 0.002, rel, 0.002 + (rel - 0.002) / stretch)
    return to[seg] + orel


def _segment_cuts(model, plan, tg, stretch, ring):
    length = model["length_s"]
    if not plan:
        return np.full(len(tg), length)
    seg, tn = _seg_index(plan, tg)
    to = [p["t_old"] for p in plan]
    end = length - (model.get("fade_out_s", 0.0) if (ring and model.get("coherent")) else 0.0)
    cuts = []
    for i in range(len(plan)):
        c = to[i + 1] if i + 1 < len(plan) else end
        old_len = c - to[i]
        new_len = (tn[i + 1] - tn[i]) if i + 1 < len(plan) else old_len * max(1.0, stretch)
        if stretch != 1 or abs(new_len - old_len) > 1e-12:
            c -= GRID_S
        cuts.append(c)
    return np.asarray(cuts)[seg]


def _band_level(points, old, cut):
    pts = np.asarray(points, dtype=float)
    lev = np.interp(np.minimum(old, cut), pts[:, 0], pts[:, 1])
    at_cut = np.interp(cut, pts[:, 0], pts[:, 1])
    slope = np.clip((at_cut - np.interp(cut - 0.01, pts[:, 0], pts[:, 1])) / 0.01, -3000.0, 0.0)
    over = old > cut
    lev[over] = at_cut[over] + slope[over] * (old[over] - cut[over])
    return lev


def _hard_window(plan, tg):
    w = np.zeros(len(tg))
    for p in plan:
        d = tg - p["t"]
        rise = np.clip((d + 0.001) / 0.001, 0.0, 1.0)
        fall = np.where(d <= 0.004, 1.0, np.clip((0.008 - d) / 0.004, 0.0, 1.0))
        w = np.maximum(w, rise * fall * (d >= -0.001))
    return w


def modal_noise(env, n, edges, r):
    """Band noise from the dB envelope (bands, grid)."""
    nfft = next_fast_len(n + 16384)
    masks = erb_masks(nfft, edges)
    t = np.arange(n) / SR
    tg = np.arange(env.shape[1]) * GRID_S
    total = np.zeros(nfft // 2 + 1, dtype=complex)
    for b in range(env.shape[0]):
        x = r.standard_normal(n)
        lev = 10.0 ** (np.interp(t, tg, env[b]) / 20.0) / np.sqrt(np.mean(masks[b] ** 2))
        total += rfft(x * lev, nfft) * masks[b]
    return irfft(total, nfft)[:n]


def modal_side(model, mid, plan, stretch, size, width, sign):
    """The side channel of a stereo modal render."""
    n = len(mid)
    st = model.get("side_t")
    if st and width > 0:
        bands = np.asarray(st["bands"], dtype=float) / size
        tg = np.arange(n) / SR
        old = _old_times(plan, tg, stretch)
        pan = np.array([np.interp(old, st["times"], p) for p in st["pan"]])
        dif = np.array([np.interp(old, st["times"], d) for d in st["diffuse"]])
        return side_from_gains(mid, bands, pan, dif, sign) * width
    rho = model.get("side")
    if rho and width > 0:
        nfft = next_fast_len(n + 16384)
        masks = erb_masks(nfft, model["noise"]["edges"])
        shape = np.sqrt(np.sum((np.asarray(rho)[:, None] * masks) ** 2, axis=0))
        q = rfft(mid, nfft) * (-1j * sign) * shape
        return irfft(q, nfft)[:n] * width
    return np.zeros(n)


def room_ir(r, size):
    rt60 = 0.25 + 0.15 * size
    m = int(np.ceil(rt60 * SR))
    t = np.arange(m) / SR
    h = lfilter([0.35], [1.0, -0.65], r.standard_normal(m) * np.exp(-6.9 * t / rt60))
    h[:int(0.004 * SR)] = 0.0
    e = np.sqrt(np.sum(h ** 2))
    return h / e if e > 0 else h


def add_room(chans, room, size, r):
    from scipy.signal import fftconvolve
    out = []
    for c in chans:
        h = room_ir(r, size)
        out.append(c + room * fftconvolve(c, h)[:len(c)])
    return out


def render_model(model, seed=0, size=1.0, damping=1.0, brightness=0.0, hardness=1.0, noise=0.0,
                 width=1.0, room=0.0, vary=0.0, spacing=1.0, gain=0.0, stereo=None, ring=None):
    """Render a modal model: float64 at 48 kHz, (n,) or (n, 2)."""
    gens = modal_gens(seed)
    if ring is None:
        ring = vary > 0
    if stereo is None:
        stereo = int(model.get("channels", 1)) == 2
    plan, stretch = modal_plan(model, seed, size, damping, brightness, hardness, vary, spacing)
    length = _new_length(model, plan, stretch)
    ext = ring_extension(plan, length) if ring else 0.0
    n = max(1, int(round((length + ext) * SR)))
    n_nom = min(n, int(round(length * SR)))
    det = _modal_det(model, plan, n, n_nom, ring)
    n_grid = int(np.ceil(n / (GRID_S * SR))) + 2
    env = modal_noise_env(model, plan, n_grid, stretch, size, brightness, hardness, noise, vary,
                          ring, gens[1])
    mid = det
    if env.size and np.max(env) > -200.0:
        mid = det + modal_noise(env, n, model["noise"]["edges"], gens[2])
    if ring and n > n_nom:
        mid[n_nom:] *= 10.0 ** (-3.0 * np.arange(n - n_nom) / (n - n_nom))
    chans = [mid]
    if stereo:
        sign = -1.0 if gens[3].uniform() < 0.5 else 1.0
        side = modal_side(model, mid, plan, stretch, size, width, sign)
        chans = [mid + side, mid - side]
    if room > 0:
        chans = add_room(chans, room, size, gens[5])
    y = np.column_stack(chans) if stereo else chans[0]
    k = min(n, int(0.001 * SR))
    if k > 0:
        y[n - k:] = (y[n - k:].T * np.linspace(1.0, 0.0, k)).T
    return y * 10.0 ** (gain / 20.0)


def _new_length(model, plan, stretch):
    if not plan:
        return model["length_s"]
    return plan[-1]["t"] + (model["length_s"] - plan[-1]["t_old"]) * max(1.0, stretch)


def _modal_det(model, plan, n, n_nom, ring):
    y = np.zeros(n)
    coherent = bool(model.get("coherent"))
    for i, p in enumerate(plan):
        s0, w = strike_wave(p, n, i == 0, coherent)
        y[s0:s0 + len(w)] += w
    sub = model.get("sub")
    if sub and sub.get("x"):
        up = resample_poly(np.asarray(sub["x"], dtype=float), SUB_DEC, 1)
        m = min(n, len(up))
        y[:m] += up[:m]
    fo = 0.0 if ring else float(model.get("fade_out_s", 0.0))
    nf = int(round(fo * SR))
    if nf > 0:
        k = min(nf, n_nom)
        y[n_nom - k:n_nom] *= np.linspace(1.0, 0.0, k)
        y[n_nom:] = 0.0
    return y
