"""fit.md acceptance tests at the analysis level (ARCHITECTURE 14.2, area fit_analysis)."""

from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys

import numpy as np
import pytest

from hark.fit.analysis import gesture, modal, module_for, sinusoidal, space, spectral

SR = 48000
ENGINE = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def _load_module(path):
    spec = importlib.util.spec_from_file_location(f"gen_{abs(hash(str(path)))}", str(path))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _strip(model):
    return {k: v for k, v in model.items() if k != "fit_engine"}


def _no_seconds(obj):
    if isinstance(obj, dict):
        return {k: _no_seconds(v) for k, v in obj.items() if k != "seconds"}
    if isinstance(obj, list):
        return [_no_seconds(v) for v in obj]
    return obj


def _mel(a, b):
    a = np.asarray(a, float)
    b = np.asarray(b, float)
    a = a.mean(axis=1) if a.ndim == 2 else a
    b = b.mean(axis=1) if b.ndim == 2 else b
    return modal.mel_distance(a, b)[0]


# ---------------------------------------------------------------- A-1 .. A-4

@pytest.mark.skip(reason="needs-data: the 12 validation references")
def test_a_1_validation_clap():
    pass


@pytest.mark.skip(reason="needs-data: the 12 validation references")
def test_a_2_validation_mel_gap():
    pass


@pytest.mark.skip(reason="needs-data: the 12 validation references and describe (ML)")
def test_a_3_validation_describe_tag():
    pass


@pytest.mark.skip(reason="needs-data: the replication benchmark set")
def test_a_4_replication_benchmark():
    pass


# ---------------------------------------------------------------- A-5

@pytest.mark.parametrize("knobs", [{}, {"size": 1.1}])
def test_a_5_modal_code_renders_fit(models, tmp_path, knobs):
    m = models["modal_s1"]
    path = tmp_path / "modal_s1.py"
    path.write_text(modal.to_code(m, "s1"))
    mod = _load_module(path)
    for s in (0, 1, 5):
        assert np.array_equal(mod.render(s, **knobs), modal.render(m, s, **knobs))


@pytest.mark.parametrize("knobs", [{}, {"tilt": 1.5}])
def test_a_5_spectral_code_renders_fit(models, tmp_path, knobs):
    m = models["spectral_s3"]
    path = tmp_path / "spectral_s3.py"
    path.write_text(spectral.to_code(m))
    mod = _load_module(path)
    for s in (0, 1, 5):
        assert np.array_equal(mod.render(s, **knobs), spectral.render(m, s, **knobs))


def test_a_5_sines_code_renders_fit(models, tmp_path):
    m = models["sines_s2"]
    path = tmp_path / "sines_s2.py"
    path.write_text(sinusoidal.to_code(m, "s2"))
    mod = _load_module(path)
    for s in (0, 1, 5):
        assert np.array_equal(mod.render(s), sinusoidal.render(m, s))


def test_a_5_standalone_scripts_do_not_import_hark(models):
    for src in (modal.to_code(models["modal_s1"]), sinusoidal.to_code(models["sines_s2"])):
        assert "import hark" not in src
        assert "from hark" not in src


def test_a_5_modal_script_writes_the_render(models, tmp_path):
    from scipy.io import wavfile
    m = models["modal_s1"]
    path = tmp_path / "s1.py"
    path.write_text(modal.to_code(m))
    out = tmp_path / "s1.wav"
    subprocess.run([sys.executable, str(path), "1", str(out)], check=True, cwd=tmp_path)
    sr, y = wavfile.read(out)
    assert sr == SR
    assert np.array_equal(y, modal.render(m, 1).astype(np.float32))


def test_a_5_every_kind_renders(models):
    for key, m in models.items():
        y = module_for(m["kind"]).render(m, 0)
        assert y.ndim in (1, 2) and len(y) > 0 and np.all(np.isfinite(y)), key


# ---------------------------------------------------------------- A-6

_RENDER_SCRIPT = """
import json, sys
import numpy as np
from hark.fit.analysis import module_for
m = json.load(open(sys.argv[1]))
np.save(sys.argv[2], module_for(m["kind"]).render(m, 3))
"""


@pytest.mark.parametrize("key", ["modal_s1", "sines_s2", "spectral_s3", "gesture_s3"])
def test_a_6_render_is_pure_across_processes(models, tmp_path, key):
    m = json.loads(json.dumps(models[key]))
    mp = tmp_path / "m.json"
    mp.write_text(json.dumps(m))
    script = tmp_path / "r.py"
    script.write_text(_RENDER_SCRIPT)
    env = dict(os.environ, PYTHONPATH=ENGINE)
    env.pop("ANTHROPIC_API_KEY", None)
    outs = []
    for i in range(2):
        out = tmp_path / f"y{i}.npy"
        subprocess.run([sys.executable, str(script), str(mp), str(out)], check=True, env=env,
                       cwd=os.path.dirname(os.path.abspath(__file__)))
        outs.append(np.load(out))
    assert np.array_equal(outs[0], outs[1])
    assert np.array_equal(outs[0], module_for(m["kind"]).render(m, 3))


@pytest.mark.parametrize("kind", ["modal", "sines", "spectral", "gesture"])
def test_a_6_fit_is_deterministic(s1, kind):
    mod = {"modal": modal, "sines": sinusoidal, "spectral": spectral, "gesture": gesture}[kind]
    a = json.loads(json.dumps(mod.fit(s1)))
    b = json.loads(json.dumps(mod.fit(s1)))
    assert _no_seconds(a) == _no_seconds(b)


# ---------------------------------------------------------------- A-7

def test_a_7_noise_layer_differs_per_seed(models):
    m = models["spectral_s3"]
    y0, y1 = spectral.render(m, 0), spectral.render(m, 1)
    assert not np.array_equal(y0, y1)
    assert _mel(y0, y1) > 0


def test_a_7_deterministic_model_is_seed_free(models):
    det = gesture._deterministic(models["modal_s1"], False, 0)
    ys = [modal.render(det, s) for s in (0, 1, 7)]
    assert np.array_equal(ys[0], ys[1]) and np.array_equal(ys[0], ys[2])


def test_a_7_modal_noise_differs_per_seed(models):
    m = models["modal_s1"]
    assert not np.array_equal(modal.render(m, 0), modal.render(m, 1))


# ---------------------------------------------------------------- A-8 (morph side)

def test_a_8_morph_side_ends_and_middle(make):
    a = np.column_stack([make.s3()[:, 0], 0.5 * make.s3()[:, 1]])
    b = make.s3(seed=9)
    sa = sinusoidal.side_model(a)
    sb = sinusoidal.side_model(b)
    m0 = sinusoidal.morph_side(sa, sb, 0.0)
    m1 = sinusoidal.morph_side(sa, sb, 1.0)
    mh = sinusoidal.morph_side(sa, sb, 0.5)
    t = np.asarray(sa["times"])
    assert np.allclose(np.interp(t, m0["times"], np.asarray(m0["pan"])[3]), np.asarray(sa["pan"])[3], atol=1e-3)
    tb = np.asarray(sb["times"])
    assert np.allclose(np.interp(tb, m1["times"], np.asarray(m1["pan"])[3]), np.asarray(sb["pan"])[3], atol=1e-3)
    assert mh["bands"] == sa["bands"]


def test_a_8_morph_side_band_mismatch_raises(make):
    sa = sinusoidal.side_model(make.s3())
    sb = dict(sa, bands=sa["bands"][:-1])
    with pytest.raises(ValueError):
        sinusoidal.morph_side(sa, sb, 0.5)


def test_a_8_sines_glide_fits_track_the_pitch(make):
    a = sinusoidal.fit(make.s2())
    b = sinusoidal.fit(make.s2(3.0))
    fa = a["summary"]["f_low"]
    fb = b["summary"]["f_low"]
    assert abs(np.log2(fb / fa) * 12 - 3.0) < 0.5


def test_a_8_kinds_do_not_mix(models):
    with pytest.raises(ValueError):
        modal.check(models["sines_s1"])
    with pytest.raises(ValueError):
        sinusoidal.check(models["modal_s1"])


# ---------------------------------------------------------------- A-9

@pytest.mark.parametrize("seed", [1, 2, 3, 4])
def test_a_9_modal_takes_ring_out(models, s1, seed):
    m = models["modal_s1"]
    y = modal.render(m, seed, vary=0.3)
    assert len(y) >= len(s1)
    assert abs(float(np.mean(y))) < 0.005 * float(np.max(np.abs(y)))
    tail = y[-int(0.01 * SR):]
    assert np.max(np.abs(tail)) < 0.05 * np.max(np.abs(y))


@pytest.mark.parametrize("seed", [1, 2, 3, 4])
def test_a_9_gesture_takes_ring_out(models, s1, seed):
    m = models["gesture_s1"]
    y = gesture.render(m, seed, vary=0.3, ring=True)
    assert len(y) >= len(s1)


# ---------------------------------------------------------------- A-14

def _side_db(x):
    x = np.asarray(x, float)
    mid = (x[:, 0] + x[:, 1]) / 2
    side = (x[:, 0] - x[:, 1]) / 2
    return 10 * np.log10(np.sum(side ** 2) / np.sum(mid ** 2))


def test_a_14_stereo_whoosh_keeps_its_side(models, s3):
    y = spectral.render(models["spectral_s3"], 0)
    assert y.ndim == 2 and y.shape[1] == 2
    assert abs(_side_db(y) - _side_db(s3)) <= 6.0
    g = gesture.render(models["gesture_s3"], 0)
    assert g.ndim == 2
    assert abs(_side_db(g) - _side_db(s3)) <= 6.0


def test_a_14_mono_struck_renders_mono(models, s1):
    assert modal.render(models["modal_s1"], 0).ndim == 1
    assert sinusoidal.render(models["sines_s1"], 0).ndim == 1
    assert space.fit_space(s1) is None
    assert not space.needs_space(s1)


# ---------------------------------------------------------------- A-16

def test_a_16_threads_do_not_change_the_model(s3, monkeypatch):
    out = []
    for v in ("1", "4", "bad"):
        monkeypatch.setenv("HARK_THREADS", v)
        out.append(_no_seconds(json.loads(json.dumps(spectral.fit(s3)))))
    assert out[0] == out[1] == out[2]
    from hark.fit.analysis._common import threads
    monkeypatch.setenv("HARK_THREADS", "bad")
    assert threads() == 2
    monkeypatch.setenv("HARK_THREADS", "0")
    assert threads() == 2
