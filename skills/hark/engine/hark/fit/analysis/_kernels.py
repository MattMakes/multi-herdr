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
    fq = rfftfreq(int(nfft), 1.0 / sr)
    lfv = np.log2(np.maximum(fq, 1.0))
    lcv = np.log2(np.asarray(centers, dtype=float))
    eye = np.eye(len(lcv))
    return np.array([np.interp(lfv, lcv, eye[bv]) for bv in range(len(lcv))])


def finish(y, sr=SR, fade_s=0.002, peak_db=-1.0, limit=True):
    """Half-cosine power fade-in, then a gain down to ``peak_db`` when over."""
    y = np.array(y, dtype=float, copy=True)
    ix = int(round(fade_s * sr))
    if ix > 1 and len(y):
        ix = min(ix, len(y))
        ramp = np.sin(np.pi * np.arange(ix) / (2.0 * ix)) ** 2
        y[:ix] = (y[:ix].T * ramp).T
    if limit and y.size:
        pkv = float(np.max(np.abs(y)))
        capv = 10.0 ** (peak_db / 20.0)
        if pkv > capv:
            y = y * (capv / pkv)
    return y


def raised_cos(k):
    """Rising raised cosine of ``k`` samples (empty for k < 1)."""
    if k < 1:
        return np.ones(0)
    return 0.5 - 0.5 * np.cos(np.pi * np.arange(k) / k)


def side_from_gains(mid, bands, pan, dif, sign=1.0, sr=SR):
    """Side signal from a mid signal and per-sample band gains (B, n)."""
    mid = np.asarray(mid, dtype=float)
    cnt = len(mid)
    acc = np.zeros(cnt)
    if cnt == 0:
        return acc
    fft_n = next_fast_len(cnt + 8192)
    specv = rfft(mid, fft_n)
    specv[0] = 0.0
    mask_l = tri_masks(bands, fft_n, sr)
    for bv in range(len(bands)):
        bnd = specv * mask_l[bv]
        if np.any(np.abs(pan[bv]) > 1e-4):
            acc += pan[bv] * irfft(bnd, fft_n)[:cnt]
        if np.any(dif[bv] > 1e-4):
            acc += dif[bv] * irfft(bnd * (-1j * sign), fft_n)[:cnt]
    return acc


def shaped_side(mid, bands, times, pan, diffuse, sign=1, sr=SR):
    """A side signal made from ``mid``: pan gains on the band, diffuse gains on its quadrature."""
    mid = np.asarray(mid, dtype=float)
    cnt = len(mid)
    if cnt == 0:
        return np.zeros(0)
    tv = np.arange(cnt) / sr
    pan_s = np.array([np.interp(tv, times, pv) for pv in pan])
    dif_s = np.array([np.interp(tv, times, dv) for dv in diffuse])
    return side_from_gains(mid, bands, pan_s, dif_s, float(sign), sr)


# ---------------------------------------------------------------- sines kernels

def synth_partials(partials, n, sr=SR, cents=None):
    """Sum of partials with log2-linear frequency and dB-linear amplitude breakpoints."""
    yv = np.zeros(n)
    for pv in partials:
        ii0 = max(0, int(np.floor(pv["t0"] * sr)))
        ii1 = min(n, int(np.ceil((pv["t1"] + pv["rel"]) * sr)) + 1)
        if ii1 <= ii0:
            continue
        yv[ii0:ii1] += _wave_of_partial(pv, ii0, ii1, sr, cents)
    return yv


def _freq_of_partial(pv, tv, cent_l, ii0, ii1):
    frv = np.asarray(pv["freq"], dtype=float)
    fq = 2.0 ** np.interp(tv, frv[:, 0], np.log2(np.maximum(frv[:, 1], 1e-6)))
    if cent_l is not None:
        fq = fq * 2.0 ** (cent_l[ii0:ii1] / 1200.0)
    return fq


def _amp_of_partial(pv, tv):
    amv = np.asarray(pv["amp"], dtype=float)
    av = 10.0 ** (np.interp(tv, amv[:, 0], amv[:, 1]) / 20.0)
    risev = max(float(pv.get("rise", 0.0002)), 1e-9)
    relv = max(float(pv.get("rel", 0.0)), 1e-9)
    stop_i = pv["t1"] + pv["rel"]
    return av * np.clip((tv - pv["t0"]) / risev, 0.0, 1.0) * np.clip((stop_i - tv) / relv, 0.0, 1.0)


def _wave_of_partial(pv, ii0, ii1, rate, cent_l):
    tv = np.arange(ii0, ii1) / rate
    fq = _freq_of_partial(pv, tv, cent_l, ii0, ii1)
    av = _amp_of_partial(pv, tv)
    av[fq >= 0.49 * rate] = 0.0
    phz = pv["ph"] + TWO_PI * (np.cumsum(fq) - fq[0]) / rate + TWO_PI * fq[0] * (ii0 / rate - pv["t0"])
    return av * np.cos(phz)


def synth_atoms(transients, n, sr=SR):
    """Strike atoms: damped cosines that start at full amplitude."""
    yv = np.zeros(n)
    for trv in transients:
        sv = int(round(trv["t"] * sr))
        for fq, tauv, av, phz in trv["atoms"]:
            kn_v = min(int(7 * tauv * sr) + 1, n - max(sv, 0))
            if kn_v <= 0 or tauv <= 0:
                continue
            ix = np.arange(kn_v)
            if sv < 0:
                ix = ix - sv
            wave = av * np.exp(-ix / (tauv * sr)) * np.cos(TWO_PI * fq * ix / sr + phz)
            yv[max(sv, 0):max(sv, 0) + kn_v] += wave
    return yv


def _band_of_noise(specv, shp, kept, fft_n, padv, cnt, tv, tvals, lvl):
    bnd = irfft(specv * shp, fft_n)[padv:padv + cnt]
    rmsv = np.sqrt(np.mean(bnd ** 2))
    if rmsv <= 0:
        return 0.0
    return bnd * (kept / rmsv) * 10.0 ** (np.interp(tv, tvals, lvl) / 20.0)


def synth_noise(bands, times, levels, n, seed=0, sr=SR, pad=4096, hf=None):
    """Band noise with dB-linear level curves per band."""
    yv = np.zeros(max(n, 0))
    if n > 0:
        rng = np.random.default_rng(int(seed) & 0xFFFFFFFF)
        fft_n = 1 << int(np.ceil(np.log2(n + 2 * pad)))
        specv = rfft(rng.standard_normal(fft_n))
        shp, kept = _shape_hf(tri_masks(bands, fft_n, sr), fft_n, sr, hf)
        tv = np.arange(n) / sr
        for bv, lvl in enumerate(levels):
            if max(lvl) >= -140:
                yv = yv + _band_of_noise(specv, shp[bv], kept[bv], fft_n, pad, n, tv, times, lvl)
    return yv


def _shape_hf(mask_l, fft_n, rate, hfv):
    if not hfv:
        return mask_l, np.ones(len(mask_l))
    fq = rfftfreq(fft_n, 1.0 / rate)
    hfv = np.asarray(hfv, dtype=float)
    gain = np.where(fq > hfv[0, 0], 10.0 ** (np.interp(fq, hfv[:, 0], hfv[:, 1]) / 20.0), 1.0)
    shp = mask_l * gain
    kept = np.sqrt(np.sum(shp ** 2, axis=1) / np.maximum(np.sum(mask_l ** 2, axis=1), 1e-30))
    return shp, kept


def sines_fade_in(model):
    """Fade-in length (s) of a sines render."""
    if not model.get("coherent"):
        return 0.0
    return float(np.clip(model.get("onset", 0.0) - 0.0005, 0.00025, 0.002))


def sines_identity(model, seed=0):
    """Mono render of a sines model at its identity knobs."""
    rate = SR
    cnt = int(round(model["duration"] * rate))
    resv = model["residual"]
    yv = synth_partials(model["partials"], cnt, rate)
    yv = yv + synth_atoms(model.get("transients", []), cnt, rate)
    yv = yv + synth_noise(resv["bands"], resv["times"], resv["db"], cnt, seed, rate, hf=resv.get("hf"))
    return sines_finish(model, yv)


def sines_finish(model, y):
    """The finish of a sines render: fade-in and, for a full model, the peak gain."""
    if not model.get("coherent"):
        return y
    lim = model.get("layer") != "deterministic"
    return finish(y, SR, sines_fade_in(model), -1.0, lim)


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
    uv = erb_rate(rfftfreq(int(n_fft), 1.0 / sr))
    sv = [np.clip((uv - erb_rate(ev) + h) / (2 * h), 0.0, 1.0) for ev in edges]
    nbv = len(edges) + 1
    acc = np.ones((nbv, len(uv)))
    for bv in range(nbv):
        if bv > 0:
            acc[bv] *= np.sin(np.pi * sv[bv - 1] / 2)
        if bv < nbv - 1:
            acc[bv] *= np.cos(np.pi * sv[bv] / 2)
    return acc


def erb_centres(edges=NOISE_EDGES, top=22000.0):
    ev = np.asarray(edges, dtype=float)
    mids = np.sqrt(ev[:-1] * ev[1:])
    return np.concatenate([[ev[0] / np.sqrt(2)], mids, [np.sqrt(ev[-1] * top)]])


def modal_gens(seed):
    sv = int(seed) & 0xFFFFFFFF
    return {ix: np.random.default_rng([sv, ix]) for ix in (1, 2, 3, 5)}


def vary_fields(r, n_fields, parts=6, scale=np.sqrt(1.0 / 3.0)):
    """Smooth random functions of log frequency, drawn in order."""
    acc = []
    for _ in range(n_fields):
        cv = r.uniform(0.25, 1.25, parts)
        pv = r.uniform(0.0, TWO_PI, parts)
        acc.append((cv, pv, scale))
    return acc


def field_at(field, f):
    cv, pv, scl = field
    lfv = np.log2(np.maximum(np.asarray(f, dtype=float), 20.0))
    return scl * np.sum(np.cos(TWO_PI * np.multiply.outer(lfv, cv) + pv), axis=-1)


def modal_plan(model, seed, size=1.0, damping=1.0, brightness=0.0, hardness=1.0,
               vary=0.0, spacing=1.0):
    """Strike times and mode rows after the knobs."""
    jit = modal_gens(seed)[1]
    strikes = model["strikes"]
    coh = bool(model.get("coherent"))
    t_old = [sv["t"] for sv in strikes]
    state = {"size": size, "stretch": max(size / damping, 0.001), "hardness": hardness,
           "tilt": brightness + 4.0 * np.log2(max(hardness, 0.001)), "vary": vary, "rj": jit,
           "told": t_old, "tnew": _times_of_strike(t_old, spacing, vary, coh, jit)}
    state["fields"] = vary_fields(jit, 3) if coh else None
    state["move"] = coh and (vary > 0 or size != 1 or spacing != 1)
    plan_d = [_strike_plan(ii, sv, state) for ii, sv in enumerate(strikes)]
    return plan_d, state["stretch"]


def _strike_plan(ii, sv, state):
    rws = _modes_plan(sv["modes"], state["size"], state["stretch"], state["tilt"], state["vary"], state["fields"],
                       state["rj"])
    t_new, t_old = state["tnew"], state["told"]
    if state["move"] and ii > 0 and len(rws):
        fq0 = np.asarray([mv[0] for mv in sv["modes"]], dtype=float)
        rws[:, 3] += TWO_PI * (rws[:, 0] * (t_new[ii] - t_new[0]) - fq0 * (t_old[ii] - t_old[0]))
    rws = rws[rws[:, 0] < 0.47 * SR] if len(rws) else rws
    att = _contact_plan(sv.get("attack", []), state["size"], state["hardness"], state["tilt"])
    return {"t": t_new[ii], "t_old": t_old[ii], "ramp_ms": sv.get("ramp_ms", 0.0) / max(state["hardness"], 1e-3),
            "modes": rws, "attack": att}


def _times_of_strike(t_old, spc, vary, coh, jit):
    if not t_old:
        return []
    acc = [t_old[0]]
    for ii in range(1, len(t_old)):
        gapv = (t_old[ii] - t_old[ii - 1]) * spc
        if not coh:
            gapv = max(gapv + vary * 0.002 * jit.standard_normal(), 0.003)
        acc.append(acc[-1] + gapv)
    return acc


def _modes_plan(modes, size, strch, tiltv, vary, fields, jit):
    if not modes:
        return np.zeros((0, 4))
    mv = np.array(modes, dtype=float)
    fq0 = mv[:, 0]
    if fields is not None:
        zf, zt, za = (field_at(fd, fq0) for fd in fields)
    else:
        zf, zt, za = (jit.standard_normal(len(fq0)) for _ in range(3))
    fq = fq0 / size * (1.0 + 0.006 * vary * zf)
    tauv = mv[:, 1] * strch * np.exp(0.12 * vary * zt)
    av = mv[:, 2] * 10.0 ** ((tiltv * np.log2(np.maximum(fq, 20.0) / 1000.0) + 1.5 * vary * za) / 20.0)
    return np.column_stack([fq, tauv, av, mv[:, 3].copy()])


def _contact_plan(attack, size, hard, tiltv):
    if not attack:
        return np.zeros((0, 4))
    mv = np.array(attack, dtype=float)
    fa = mv[:, 0] / size
    ta = mv[:, 1] * size / np.sqrt(max(hard, 1e-3))
    aa = mv[:, 2] * 10.0 ** (tiltv * np.log2(np.maximum(fa, 20.0) / 1000.0) / 20.0)
    rws = np.column_stack([fa, ta, aa, mv[:, 3]])
    return rws[rws[:, 0] < 0.47 * SR]


def _ring_tail_power(plan_d, len_n, thr):
    stop_i, pow_v = 0.0, 0.0
    for pv in plan_d:
        for fq, tauv, av, _ in pv["modes"]:
            if av > thr:
                stop_i = max(stop_i, pv["t"] + tauv * np.log(av / thr))
            if len_n > pv["t"]:
                pow_v += (av * np.exp(-(len_n - pv["t"]) / tauv)) ** 2
    return stop_i, pow_v


def _ring_length(stop_i, pow_v, refv, len_n, first_i):
    level = 10.0 * np.log10(max(pow_v, 1e-300) / refv ** 2)
    lo = 0.2 if level > -50 else 0.06
    return float(min(max(stop_i - len_n, lo), max(lo, 0.5 * (len_n - first_i))))


def ring_extension(plan, length):
    """Seconds to add after ``length`` so the modes ring 60 dB down."""
    sums = [np.sqrt(np.sum(pv["modes"][:, 2] ** 2)) for pv in plan if len(pv["modes"])]
    refv = max(sums) if sums else 0.0
    extv = 0.0
    if refv > 0:
        stop_i, pow_v = _ring_tail_power(plan, length, refv * 1e-3)
        if stop_i > length + 0.001:
            extv = _ring_length(stop_i, pow_v, refv, length, plan[0]["t"])
    return extv


def mode_wave(rows, n, sr=SR):
    """Re(sum C z^k) for k = 0 .. n-1, in blocks."""
    yv = np.zeros(n)
    if not len(rows) or n <= 0:
        return yv
    z = np.exp(-1.0 / (rows[:, 1] * sr) + 2j * np.pi * rows[:, 0] / sr)
    cv = rows[:, 2] * np.exp(1j * rows[:, 3])
    topv = min(n, int(14 * np.max(rows[:, 1]) * sr) + 1)
    for bv0 in range(0, topv, 4096):
        b1 = min(topv, bv0 + 4096)
        ix = np.arange(bv0, b1)
        yv[bv0:b1] = np.real(np.exp(np.multiply.outer(ix, np.log(z))) @ cv)
    return yv


def strike_wave(p, n, first, coherent, sr=SR):
    """One strike's modes (with ramp) plus contact, from its onset sample to n."""
    sv = int(round(p["t"] * sr))
    if sv >= n:
        return sv, np.zeros(0)
    sv0 = max(sv, 0)
    mv = n - sv0
    wv = mode_wave(p["modes"], mv, sr)
    rv = int(round(p["ramp_ms"] * sr / 1000.0))
    if rv > 0:
        ix = min(rv, mv)
        wv[:ix] *= raised_cos(rv)[:ix]
    wv = wv + mode_wave(p["attack"], mv, sr)
    if not first and coherent:
        ix = min(LATER_RAMP, mv)
        wv[:ix] *= raised_cos(LATER_RAMP)[:ix]
    return sv0, wv


def modal_noise_env(model, plan, n_grid, stretch, k, rj):
    """Noise band levels (dB) on the new 0.5 ms grid, shape (bands, n_grid). k holds the render knobs."""
    nz = model["noise"]
    tgv = np.arange(n_grid) * GRID_S
    oldv = _times_before(plan, tgv, stretch)
    cutv = _cut_segments(model, plan, tgv, stretch, k["ring"])
    envl = np.array([_level_of_band(bv, oldv, cutv) for bv in nz["env"]])
    cent_v = erb_centres(nz["edges"])
    envl = _shift_by_size(envl, cent_v, k["size"]) + (k["brightness"] * np.log2(cent_v / 1000.0))[:, None] + k["noise"]
    return _env_vary(_env_hard(envl, cent_v, plan, tgv, k["hardness"]), cent_v, k["vary"], rj)


def _env_hard(envl, cent_v, plan_d, tgv, hard):
    if hard != 1:
        envl = envl + np.outer(4 * np.log2(hard) * np.log2(cent_v / 1000.0), _window_hard(plan_d, tgv))
    return envl


def _env_vary(envl, cent_v, vary, jit):
    if vary:
        envl = envl + (1.5 * vary * jit.standard_normal(len(cent_v)))[:, None]
    return envl


def _shift_by_size(envl, cent_v, size):
    if size == 1:
        return envl
    lcv = np.log2(cent_v)
    return np.array([np.interp(lcv + np.log2(size), lcv, envl[:, ix]) for ix in range(envl.shape[1])]).T


def _index_of_seg(plan_d, tgv):
    tnv = np.array([pv["t"] for pv in plan_d]) if plan_d else np.zeros(1)
    return np.clip(np.searchsorted(tnv, tgv, side="right") - 1, 0, len(tnv) - 1), tnv


def _times_before(plan_d, tgv, strch):
    if not plan_d:
        return tgv.copy()
    chunk, tnv = _index_of_seg(plan_d, tgv)
    tov = np.array([pv["t_old"] for pv in plan_d])
    relv = tgv - tnv[chunk]
    orel = np.where(relv < 0.002, relv, 0.002 + (relv - 0.002) / strch)
    return tov[chunk] + orel


def _cut_segment(ii, plan_d, tnv, tov, stop_i, strch):
    last = ii + 1 >= len(plan_d)
    cv = stop_i if last else tov[ii + 1]
    old_len = cv - tov[ii]
    new_len = old_len * max(1.0, strch) if last else tnv[ii + 1] - tnv[ii]
    if strch != 1 or abs(new_len - old_len) > 1e-12:
        cv -= GRID_S
    return cv


def _cut_segments(model, plan_d, tgv, strch, ringv):
    len_n = model["length_s"]
    if not plan_d:
        return np.full(len(tgv), len_n)
    chunk, tnv = _index_of_seg(plan_d, tgv)
    tov = [pv["t_old"] for pv in plan_d]
    stop_i = len_n - (model.get("fade_out_s", 0.0) if (ringv and model.get("coherent")) else 0.0)
    cuts = [_cut_segment(ii, plan_d, tnv, tov, stop_i, strch) for ii in range(len(plan_d))]
    return np.asarray(cuts)[chunk]


def _level_of_band(points, oldv, cutv):
    pt_l = np.asarray(points, dtype=float)
    lvl = np.interp(np.minimum(oldv, cutv), pt_l[:, 0], pt_l[:, 1])
    at_cut = np.interp(cutv, pt_l[:, 0], pt_l[:, 1])
    slope = np.clip((at_cut - np.interp(cutv - 0.01, pt_l[:, 0], pt_l[:, 1])) / 0.01, -3000.0, 0.0)
    overv = oldv > cutv
    lvl[overv] = at_cut[overv] + slope[overv] * (oldv[overv] - cutv[overv])
    return lvl


def _window_hard(plan_d, tgv):
    wv = np.zeros(len(tgv))
    for pv in plan_d:
        dv = tgv - pv["t"]
        risev = np.clip((dv + 0.001) / 0.001, 0.0, 1.0)
        fall = np.where(dv <= 0.004, 1.0, np.clip((0.008 - dv) / 0.004, 0.0, 1.0))
        wv = np.maximum(wv, risev * fall * (dv >= -0.001))
    return wv


def modal_noise(env, n, edges, r):
    """Band noise from the dB envelope (bands, grid)."""
    fft_n = next_fast_len(n + 16384)
    mask_l = erb_masks(fft_n, edges)
    tv = np.arange(n) / SR
    tgv = np.arange(env.shape[1]) * GRID_S
    total = np.zeros(fft_n // 2 + 1, dtype=complex)
    for bv in range(env.shape[0]):
        x = r.standard_normal(n)
        lvl = 10.0 ** (np.interp(tv, tgv, env[bv]) / 20.0) / np.sqrt(np.mean(mask_l[bv] ** 2))
        total += rfft(x * lvl, fft_n) * mask_l[bv]
    return irfft(total, fft_n)[:n]


def _side_time(stv, midv, plan_d, strch, size, sgn):
    band_l = np.asarray(stv["bands"], dtype=float) / size
    oldv = _times_before(plan_d, np.arange(len(midv)) / SR, strch)
    panv = np.array([np.interp(oldv, stv["times"], pv) for pv in stv["pan"]])
    diffv = np.array([np.interp(oldv, stv["times"], dv) for dv in stv["diffuse"]])
    return side_from_gains(midv, band_l, panv, diffv, sgn)


def _side_corr(rho, midv, edg, sgn):
    cnt = len(midv)
    fft_n = next_fast_len(cnt + 16384)
    shape = np.sqrt(np.sum((np.asarray(rho)[:, None] * erb_masks(fft_n, edg)) ** 2, axis=0))
    return irfft(rfft(midv, fft_n) * (-1j * sgn) * shape, fft_n)[:cnt]


def modal_side(model, mid, plan, stretch, size, width, sign):
    """The side channel of a stereo modal render."""
    side_v = np.zeros(len(mid))
    if width > 0 and model.get("side_t"):
        side_v = _side_time(model["side_t"], mid, plan, stretch, size, sign) * width
    elif width > 0 and model.get("side"):
        side_v = _side_corr(model["side"], mid, model["noise"]["edges"], sign) * width
    return side_v


def room_ir(r, size):
    rt60 = 0.25 + 0.15 * size
    mv = int(np.ceil(rt60 * SR))
    tv = np.arange(mv) / SR
    hv = lfilter([0.35], [1.0, -0.65], r.standard_normal(mv) * np.exp(-6.9 * tv / rt60))
    hv[:int(0.004 * SR)] = 0.0
    ev = np.sqrt(np.sum(hv ** 2))
    return hv / ev if ev > 0 else hv


def add_room(chans, room, size, r):
    from scipy.signal import fftconvolve
    acc = []
    for cv in chans:
        hv = room_ir(r, size)
        acc.append(cv + room * fftconvolve(cv, hv)[:len(cv)])
    return acc


MODAL_KNOBS = {"size": 1.0, "damping": 1.0, "brightness": 0.0, "hardness": 1.0, "noise": 0.0, "width": 1.0,
               "room": 0.0, "vary": 0.0, "spacing": 1.0, "gain": 0.0, "stereo": None, "ring": None}


def render_model(model, seed=0, **knobs):
    """Render a modal model: float64 at 48 kHz, (n,) or (n, 2). Knobs and defaults: MODAL_KNOBS."""
    ix = _knobs_modal(model, knobs)
    return _knobs_render(model, seed, ix) * 10.0 ** (ix["gain"] / 20.0)


def _knobs_modal(model, knobs):
    bad = sorted(set(knobs) - set(MODAL_KNOBS))
    if bad:
        raise TypeError(f"unknown modal knob(s) {bad}; known: {sorted(MODAL_KNOBS)}")
    ix = dict(MODAL_KNOBS, **knobs)
    ix["ring"] = ix["vary"] > 0 if ix["ring"] is None else ix["ring"]
    ix["stereo"] = int(model.get("channels", 1)) == 2 if ix["stereo"] is None else ix["stereo"]
    return ix


def _knobs_render(model, seed, ix):
    gen_l = modal_gens(seed)
    plan_d, strch = modal_plan(model, seed, ix["size"], ix["damping"], ix["brightness"], ix["hardness"],
                               ix["vary"], ix["spacing"])
    cnt, nom_n = _lengths_render(model, plan_d, strch, ix["ring"])
    envl = modal_noise_env(model, plan_d, int(np.ceil(cnt / (GRID_S * SR))) + 2, strch, ix, gen_l[1])
    midv = _mid_modal(model, plan_d, envl, cnt, nom_n, ix["ring"], gen_l[2])
    side_v = None
    if ix["stereo"]:
        sgn = -1.0 if gen_l[3].uniform() < 0.5 else 1.0
        side_v = modal_side(model, midv, plan_d, strch, ix["size"], ix["width"], sgn)
    return _fade_end(_out_modal(midv, side_v, ix["room"], ix["size"], gen_l[5]), cnt)


def _out_modal(midv, side_v, roomv, size, rv):
    chan_l = [midv] if side_v is None else [midv + side_v, midv - side_v]
    if roomv > 0:
        chan_l = add_room(chan_l, roomv, size, rv)
    return chan_l[0] if side_v is None else np.column_stack(chan_l)


def _lengths_render(model, plan_d, strch, ringv):
    len_n = _length_new(model, plan_d, strch)
    extv = ring_extension(plan_d, len_n) if ringv else 0.0
    cnt = max(1, int(round((len_n + extv) * SR)))
    return cnt, min(cnt, int(round(len_n * SR)))


def _mid_modal(model, plan_d, envl, cnt, nom_n, ringv, rv):
    midv = _tonal_modal(model, plan_d, cnt, nom_n, ringv)
    if envl.size and np.max(envl) > -200.0:
        midv = midv + modal_noise(envl, cnt, model["noise"]["edges"], rv)
    return _fade_ring(midv, nom_n, ringv)


def _fade_ring(midv, nom_n, ringv):
    """Fade the ring extension after n_nom by 60 dB."""
    cnt = len(midv)
    if ringv and cnt > nom_n:
        midv[nom_n:] *= 10.0 ** (-3.0 * np.arange(cnt - nom_n) / (cnt - nom_n))
    return midv


def _fade_end(yv, cnt):
    ix = min(cnt, int(0.001 * SR))
    if ix > 0:
        yv[cnt - ix:] = (yv[cnt - ix:].T * np.linspace(1.0, 0.0, ix)).T
    return yv


def _length_new(model, plan_d, strch):
    if not plan_d:
        return model["length_s"]
    return plan_d[-1]["t"] + (model["length_s"] - plan_d[-1]["t_old"]) * max(1.0, strch)


def _sub_add(yv, subv, cnt):
    if subv and subv.get("x"):
        up = resample_poly(np.asarray(subv["x"], dtype=float), SUB_DEC, 1)
        mv = min(cnt, len(up))
        yv[:mv] += up[:mv]


def _fade_apply(yv, fo, nom_n):
    nf = int(round(fo * SR))
    if nf > 0:
        ix = min(nf, nom_n)
        yv[nom_n - ix:nom_n] *= np.linspace(1.0, 0.0, ix)
        yv[nom_n:] = 0.0


def _tonal_modal(model, plan_d, cnt, nom_n, ringv):
    yv = np.zeros(cnt)
    coh = bool(model.get("coherent"))
    for ii, pv in enumerate(plan_d):
        sv0, wv = strike_wave(pv, cnt, ii == 0, coh)
        yv[sv0:sv0 + len(wv)] += wv
    _sub_add(yv, model.get("sub"), cnt)
    _fade_apply(yv, 0.0 if ringv else float(model.get("fade_out_s", 0.0)), nom_n)
    return yv
