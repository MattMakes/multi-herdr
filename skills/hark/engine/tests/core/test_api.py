"""Package root and the flat API: version, lazy names, no heavy imports (core-io 2.5; ARCH 7)."""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

import pytest

ENGINE = Path(__file__).resolve().parents[2]

API_NAMES = sorted(["describe", "match", "rank", "find", "compare", "ab", "realness", "realness_many", "profile",
                    "timeline", "sheet", "critique", "measure", "doctor", "llm_critique", "sfx_list",
                    "sfx_render", "sfx_check", "kit_init", "kit_render", "kit_check", "kit_audition"])


def _fresh(code: str, *args: str) -> subprocess.CompletedProcess:
    env = {"PYTHONPATH": str(ENGINE), "PATH": "/usr/bin:/bin:/opt/homebrew/bin:/usr/local/bin"}
    return subprocess.run([sys.executable, "-c", code, *args], env=env, capture_output=True, text=True,
                          stdin=subprocess.DEVNULL, timeout=300)


def test_at_14_versions_agree():
    import hark

    meta = tomllib.loads((ENGINE / "pyproject.toml").read_text(encoding="utf-8"))["project"]["version"]
    changelog = (ENGINE / "CHANGELOG.md").read_text(encoding="utf-8")
    newest = re.search(r"^## (\S+)", changelog, re.MULTILINE).group(1)
    assert hark.__version__ == meta == newest == "0.1.0"


def test_at_16_api_names():
    from hark.core import api

    assert api.__all__ == API_NAMES and len(api.__all__) == 22
    with pytest.raises(AttributeError):
        api.no_such_name  # noqa: B018
    from hark.core import features

    assert api.measure is features.measure


def test_api_targets_follow_the_architecture_table():
    from hark.core import api

    assert api.target("doctor") == ("hark.doctor", "check")
    assert api.target("kit_audition") == ("hark.forge.bank.kit", "audition")
    assert api.target("llm_critique") == ("hark.surfaces.llm", "critique")


def test_at_15_measure_from_a_fresh_process():
    code = ("import sys, numpy as np, hark\n"
            "assert 'torch' not in sys.modules\n"
            "r = hark.measure(0.5 * np.sin(np.arange(48000) / 10.0), light=True)\n"
            "print(r['duration'], 'torch' in sys.modules, 'librosa' in sys.modules)")
    proc = _fresh(code)
    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.split() == ["1.0", "False", "False"]


_LIGHT = """
import json, sys
import hark, hark.cli
heavy = ('numpy', 'scipy', 'librosa', 'torch', 'transformers', 'huggingface_hub')
print(json.dumps(sorted(m for m in heavy if m in sys.modules)))
"""


def test_import_hark_is_light():
    proc = _fresh(_LIGHT)
    assert proc.returncode == 0, proc.stderr
    assert json.loads(proc.stdout) == []


def test_unknown_root_name_raises():
    import hark

    with pytest.raises(AttributeError):
        hark.no_such_thing  # noqa: B018


@pytest.mark.ml
def test_at_15_describe_after_submodule_import(tmp_path):
    pytest.importorskip("hark.listen.describe")
    import numpy as np
    import soundfile as sf

    path = tmp_path / "t.wav"
    sf.write(str(path), (0.3 * np.sin(np.arange(48000) / 8.0)).astype(np.float32), 48000)
    code = ("import sys, hark\nimport hark.listen.describe\nr = hark.describe(sys.argv[1])\n"
            "assert isinstance(r, dict) and 'summary' in r\nimport hark.timeline\nprint('ok')")
    proc = _fresh(code, str(path))
    assert proc.returncode == 0, proc.stderr
