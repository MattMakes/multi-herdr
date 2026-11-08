"""Acceptance tests A-5 .. A-14 of fit.md at the pipeline level, plus engine contracts."""

from __future__ import annotations

import copy
import json
import math
import subprocess
import sys

import numpy as np
import pytest

from hark.fit.pipeline import auto, codegen, quality as Q
from hark.fit.pipeline.render import render_full

SR = 48000
KINDS = ("auto", "modal", "sines", "spectral", "gesture", "hybrid")
LIM = 10 ** (-1 / 20)


def _changed_knob(model: dict) -> dict:
    """One knob moved off its identity, inside its range."""
    for k, (rng, ident, _d) in auto.knobs_of(model).items():
        if k in ("vary", "morph"):
            continue
        lo, hi = rng[0], rng[1]
        return {k: ident + 0.5 * (hi - ident) if hi != ident else lo}
    return {}


def _rms_db(x: np.ndarray) -> float:
    return 10 * math.log10(max(float(np.mean(np.square(x))), 1e-20))


def _strip_seconds(o):
    if isinstance(o, dict):
        return {k: _strip_seconds(v) for k, v in o.items() if k not in ("seconds", "fit_s")}
    if isinstance(o, list):
        return [_strip_seconds(v) for v in o]
    return o


def _mean_freq(model: dict) -> float:
    """Energy-weighted mean frequency over every partial's breakpoints."""
    num = den = 0.0
    for p in model["partials"]:
        fr, am = np.asarray(p["freq"]), np.asarray(p["amp"])
        e = 10 ** (np.interp(fr[:, 0], am[:, 0], am[:, 1]) / 10)
        num += float(np.sum(np.log2(fr[:, 1]) * e))
        den += float(np.sum(e))
    return float(2 ** (num / den))


@pytest.mark.parametrize("name", ["S1", "S2", "S3", "S4"])
def test_a_5_recipe_renders_as_engine(name, sounds, tmp_path):
    for kind in KINDS:
        m = auto.fit(sounds[name], kind=kind)
        rec = codegen.load_recipe(codegen.write_recipe(str(tmp_path / f"{name}_{kind}.py"), m))
        for knobs in ({}, _changed_knob(m)):
            for s in (0, 1, 5):
                a, b = rec.render_seed(s, **knobs), auto.render(m, seed=s, **knobs)
                assert a.dtype == b.dtype and np.array_equal(a, b), (name, kind, knobs, s)


def test_a_6_renders_and_fits_are_deterministic(sounds, tmp_path):
    m = auto.fit(sounds["S3"], kind="spectral")
    path = str(tmp_path / "m.json")
    auto.save(m, path)
    code = (
        "import sys\nimport numpy as np\nfrom hark.fit.pipeline import auto\n"
        f"m = auto.load({path!r})\nnp.save(sys.argv[1], auto.render(m, seed=3, width=1.2))\n")
    outs = []
    for i in range(2):
        out = str(tmp_path / f"r{i}.npy")
        subprocess.run([sys.executable, "-c", code, out], check=True, env=_child_env())
        outs.append(np.load(out))
    assert np.array_equal(outs[0], outs[1])
    assert np.array_equal(outs[0], auto.render(auto.load(path), seed=3, width=1.2))
    a, b = auto.fit(sounds["S1"]), auto.fit(sounds["S1"])
    ja, jb = json.loads(json.dumps(_strip_seconds(a))), json.loads(json.dumps(_strip_seconds(b)))
    assert ja == jb


def _child_env() -> dict:
    import os

    env = dict(os.environ)
    root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    env["PYTHONPATH"] = root + os.pathsep + env.get("PYTHONPATH", "")
    return env


def test_a_7_seed_changes_noise_only(sounds):
    m = auto.fit(sounds["S3"])
    y0, y1 = auto.render(m, seed=0), auto.render(m, seed=1)
    assert not np.array_equal(y0, y1)
    assert Q.quality(y0, y1)["mel_db"] > 0
    det = auto.deterministic(auto.fit_kind(sounds["S1"], "modal"))
    r = [auto.render(det, seed=s) for s in (0, 1, 2)]
    assert np.array_equal(r[0], r[1]) and np.array_equal(r[0], r[2])


def _s2_up(semitones: float) -> np.ndarray:
    n = int(0.2 * SR)
    t = np.arange(n) / SR
    k = 2 ** (semitones / 12)
    f = np.where(t < 0.06, 900 * k * (500 / 900) ** (t / 0.06), 500.0 * k)
    env = np.minimum(t / 0.003, 1) * np.exp(-np.maximum(t - 0.003, 0) / 0.04)
    return 0.6 * env * np.sin(2 * np.pi * np.cumsum(f) / SR)


def test_a_8_morph_ends_and_middle(sounds):
    a = auto.fit(sounds["S2"], kind="sines")
    b = auto.fit(_s2_up(3), kind="sines")
    strip = lambda m: {k: v for k, v in m.items() if k != "fit_engine"}  # noqa: E731
    assert np.array_equal(auto.render(auto.morph(a, b, 0)), auto.render(strip(a)))
    assert np.array_equal(auto.render(auto.morph(a, b, 1)), auto.render(strip(b)))
    mid = auto.morph(a, b, 0.5)
    fa, fb, fm = _mean_freq(a), _mean_freq(b), _mean_freq(mid)
    assert min(fa, fb) < fm < max(fa, fb)
    assert fm == pytest.approx(math.sqrt(fa * fb), rel=0.01)
    with pytest.raises(ValueError):
        auto.morph(auto.fit(sounds["S1"], kind="modal"), a, 0.5)


@pytest.mark.parametrize("kind", ["modal", "hybrid"])
def test_a_9_takes_ring_out_without_dc(kind, sounds):
    x = sounds["S1"]
    m = auto.fit(x, kind=kind)
    for s in (1, 2, 3, 4):
        y = auto.render(m, seed=s, take=True, vary=0.3)
        assert abs(float(np.mean(y))) < 0.005 * float(np.max(np.abs(y)))
        assert len(y) >= len(x)


def _fade_ok(y: np.ndarray, raw: np.ndarray) -> bool:
    ym, rm = (y if y.ndim == 1 else y.mean(axis=1)), (raw if raw.ndim == 1 else raw.mean(axis=1))
    if ym[0] != 0:
        return False
    k = 96
    g = float(np.sum(ym[k:] * rm[k:]) / max(float(np.sum(rm[k:] ** 2)), 1e-30))
    ratio = np.divide(ym[:k], rm[:k] * g, out=np.ones(k), where=np.abs(rm[:k]) > 1e-9)
    return bool(np.all(ratio <= 1 + 1e-9))


def test_a_10_finish_caps_the_peak_and_fades_in(sounds):
    loud = sounds["S1"] * (10 ** (6 / 20) / np.max(np.abs(sounds["S1"])))
    recs = [(sounds[n], k) for n in ("S1", "S2", "S3", "S4") for k in ("auto", "modal", "sines")]
    recs.append((loud, "modal"))
    for x, kind in recs:
        m = auto.fit(x, kind=kind)
        if not auto.finished(m):
            continue
        has_vary = "vary" in auto.knobs_of(m)
        for s, take in ((0, None), (1, True), (2, True), (3, True), (4, True)):
            kn = {"vary": 0.3} if take and has_vary else {}
            y = auto.render(m, seed=s, take=take, **kn)
            raw = render_full(m, s, take=take, final=False, **kn)
            assert float(np.max(np.abs(y))) <= LIM + 1e-9
            if float(np.max(np.abs(raw))) < LIM:
                assert float(np.max(np.abs(y))) == pytest.approx(float(np.max(np.abs(raw))), rel=0.05)
            assert _fade_ok(y, raw)


def test_a_11_gate_cuts_after_each_pulse(sounds):
    x = sounds["S4"]
    m = auto.fit(x)
    assert m["post"].get("gate"), m["fit_engine"]["chosen"]
    y = auto.render(m)
    open_ = copy.deepcopy(m)
    open_["post"].pop("gate")
    y_open = auto.render(open_)
    for i in range(4):
        s, e = int(i * 0.08 * SR), int((i * 0.08 + 0.03) * SR)
        peak = _rms_db(y[s: s + 480])
        after = _rms_db(y[e + 480: e + 960])
        assert after <= peak - 20, (i, peak, after)
        if i:
            # the gate is open again at each pulse onset
            assert abs(_rms_db(y[s: s + 240]) - _rms_db(y_open[s: s + 240])) <= 1.0, i
            assert abs(_rms_db(y[s: s + 240]) - _rms_db(x[s: s + 240])) <= 1.0, i


def test_a_12_bed_strip_and_check(sounds, bed_lead):
    x = sounds["S5"]
    m = auto.fit(x)
    bed = m["fit_engine"].get("bed")
    assert bed is not None
    assert abs(bed["t_on"] - bed_lead) <= 0.003
    assert abs(bed["level_db"] + 25) <= 3
    assert isinstance(bed["kept"], bool)
    if bed["kept"]:
        a, b = int(0.01 * SR), int((bed_lead - 0.01) * SR)
        assert _rms_db(auto.render(m, bed=False)[a:b]) <= _rms_db(auto.render(m)[a:b]) - 15
    plain = auto.fit(x, bed=False)
    assert "bed" not in plain["fit_engine"]
    assert "bed" not in (plain.get("post") or {})


def test_a_13_auto_chooses_by_character(sounds):
    s1, s3 = auto.fit(sounds["S1"]), auto.fit(sounds["S3"])
    k1 = auto.kind_of(s1)
    # Spec test adjustment (orchestrator approved): a gesture whose tones layer is sines or modal also passes.
    tonal = ("modal", "sines")
    assert k1 in tonal or (k1 == "hybrid" and any(
        auto.kind_of(layer["model"]) in tonal for layer in s1["layers"])) or (
        k1 == "gesture" and bool(s1.get("tones")) and auto.kind_of(s1["tones"]) in tonal)
    k3 = auto.kind_of(s3)
    if k3 == "hybrid":
        parts = [np.sum(render_full(layer["model"], 0, take=False, final=False) ** 2) for layer in s3["layers"]]
        assert s3["layers"][int(np.argmax(parts))]["role"] == "air"
    else:
        assert k3 in ("spectral", "gesture")
    for name in ("S1", "S2", "S3"):
        m = auto.fit(sounds[name])
        q = Q.quality(sounds[name], auto.render(m))
        fl = Q.floor(lambda s: auto.render(m, seed=s))
        assert Q.score(q) <= Q.score(fl) + 3, name


def test_a_14_stereo_space_and_mono_shapes(sounds):
    m = auto.fit(sounds["S3"])
    y = auto.render(m)
    assert y.ndim == 2 and y.shape[1] == 2
    q = Q.quality(sounds["S3"], y)
    assert abs(q["side_db"][1] - q["side_db"][0]) <= 6
    mono = auto.fit(sounds["S1"])
    assert auto.render(mono).ndim == 1
    assert "space" not in mono["post"]


# ---------------------------------------------------------------- engine contracts


def test_kind_of_and_errors(sounds):
    assert auto.kind_of({"kind": "spectral_noise"}) == "spectral"
    assert auto.kind_of({"kind": "hybrid"}) == "hybrid" and auto.kind_of({"kind": "morph"}) == "morph"
    with pytest.raises(ValueError):
        auto.kind_of({"kind": "space"})
    with pytest.raises(ValueError):
        auto.fit_kind(sounds["S1"], "hybrid")
    with pytest.raises(ValueError):
        auto.fit_hybrid(sounds["S1"], ("spectral", "sines"))
    m = auto.fit(sounds["S1"], kind="modal")
    with pytest.raises(ValueError):
        auto.render(m, nosuchknob=1)


def test_every_failure_raises_runtime_error(sounds, monkeypatch):
    from hark.fit.pipeline import _layers

    def boom(*a, **k):
        raise ArithmeticError("no fit")

    monkeypatch.setattr(_layers, "fit_kind", boom)
    monkeypatch.setattr(auto, "fit_kind", boom)
    monkeypatch.setattr(auto, "fit_hybrid", boom)
    with pytest.raises(RuntimeError, match="no fit"):
        auto.fit(sounds["S1"], bed=False)


def test_a_failing_candidate_is_an_error_row(sounds, monkeypatch):
    real = auto.fit_kind

    def some(x, kind, *a, **k):
        if kind == "sines":
            raise ArithmeticError("sines broke")
        return real(x, kind, *a, **k)

    monkeypatch.setattr(auto, "fit_kind", some)
    m = auto.fit(sounds["S1"], candidates=["sines", "modal"])
    rows = {r["candidate"]: r for r in m["fit_engine"]["candidates"]}
    assert "sines broke" in rows["sines"]["error"] and rows["sines"]["score"] is None
    assert m["fit_engine"]["chosen"] == "modal"


def test_plan_orders_and_limits():
    short = {"class": "glide", "dur_s": 0.3}
    p = auto.plan(short)
    assert p[:4] == ["sines", "hybrid:sines+modal+spectral", "hybrid:sines+spectral", "modal"]
    assert len(p) == 7 and p.index("gesture") > p.index("sines")
    long = auto.plan({"class": "gesture", "dur_s": 2.0})
    assert long == ["sines", "gesture", "modal", "hybrid:sines+spectral"]


def test_vary_scale_and_identity(sounds):
    m = auto.fit(sounds["S1"], kind="modal")
    assert m["vary_scale"] == 0.5
    ident = auto.identity(m)
    assert np.array_equal(auto.render(m, **ident), auto.render(m))
    assert "fit_engine" in m and m["fit_engine"]["kind"] == "modal"


def test_morph_model_knob(sounds):
    a = auto.fit(sounds["S2"], kind="sines")
    b = auto.fit(_s2_up(2), kind="sines")
    mm = auto.morph_model(a, b, 0.5)
    table = auto.knobs_of(mm)
    assert table["morph"][0] == (0.2, 0.8) and table["morph"][1] == 0.5
    assert np.array_equal(auto.render(mm, morph=0), auto.render({k: v for k, v in a.items() if k != "fit_engine"}))


def test_save_and_load_round_trip(sounds, tmp_path):
    m = auto.fit(sounds["S1"], kind="hybrid")
    path = str(tmp_path / "h.json")
    auto.save(m, path)
    assert " " not in open(path).read(200)
    m2 = auto.load(path)
    assert np.array_equal(auto.render(m2, seed=2), auto.render(m, seed=2))


def test_package_api_is_lazy():
    import hark.fit as fit

    assert {"fit", "render", "morph", "to_recipe", "run_fit", "run_morph", "kind_of", "load", "save"} <= set(fit.__all__)
    assert fit.kind_of({"kind": "modal"}) == "modal"
    with pytest.raises(AttributeError):
        fit.nosuchname  # noqa: B018
