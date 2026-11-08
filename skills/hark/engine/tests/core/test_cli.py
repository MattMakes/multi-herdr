"""The hark command shell: dispatch, threads, errors, exit codes, ML install path (core-io 2.3, 3.5; ARCH 4.4, 5.4, 6)."""

from __future__ import annotations

import argparse
import io
import json
import os
import sys
import types

import numpy as np
import pytest
import soundfile as sf

from hark import cli
from hark.models import MLMissing, install_command

SR = 48000


@pytest.fixture
def two_wavs(tmp_path):
    t = np.arange(SR) / SR
    paths = []
    for name, f in (("a.wav", 440.0), ("b.wav", 880.0)):
        path = tmp_path / name
        sf.write(str(path), (0.5 * np.sin(2 * np.pi * f * t)).astype(np.float32), SR, subtype="FLOAT")
        paths.append(str(path))
    return paths


@pytest.fixture
def fake_command(monkeypatch):
    """fake_command(handler, name="describe"): map a table command to a stand-in module built here."""

    def install(handler, name: str = "describe"):
        mod = types.ModuleType(f"hark_test_cmd_{name}")

        def add_cli(sub):
            p = sub.add_parser(name)
            p.add_argument("path", nargs="?")
            from hark.core import report

            report.add_common(p)
            p.set_defaults(func=handler)

        mod.add_cli = add_cli
        monkeypatch.setitem(sys.modules, mod.__name__, mod)
        monkeypatch.setitem(cli.MODULE_OF, name, mod.__name__)
        return mod

    return install


@pytest.fixture
def hide_ml(monkeypatch):
    """Make torch and transformers look absent (import spec missing)."""
    from hark import models

    monkeypatch.setattr(models, "_absent", lambda names: tuple(n for n in names if n in models.ML_MODULES))


def test_command_table_order():
    names = [c for c, _ in cli.COMMANDS]
    assert names == ["describe", "match", "rank", "find", "compare", "ab", "realness", "timeline", "sheet",
                     "critique", "profile", "forge", "fit", "sfx", "kit", "gallery", "measure", "llm", "mcp",
                     "doctor"]
    assert cli.MODULE_OF["measure"] == "hark.core.features" and cli.MODULE_OF["doctor"] == "hark.doctor"


def test_at_12_threads_and_error_line(two_wavs, monkeypatch, capsys):
    monkeypatch.setenv("HARK_THREADS", "3")
    assert cli.main(["measure", two_wavs[0], "--light", "--threads", "2"]) == 0
    assert os.environ["HARK_THREADS"] == "2"
    capsys.readouterr()
    assert cli.main(["measure", two_wavs[0] + ".nope"]) == 1
    err = capsys.readouterr().err.splitlines()
    assert len(err) == 1 and err[0].startswith("hark measure: error:")


_PROBE = ("import os, sys; from hark import cli; code = cli.main(sys.argv[1:]); "
          "print('BLAS=' + os.environ.get('OPENBLAS_NUM_THREADS', '')); sys.exit(code)")


@pytest.mark.parametrize("flags, env, expected", [
    (["--threads", "2"], {}, "2"),
    (["--threads=2"], {}, "2"),
    ([], {"HARK_THREADS": "3"}, "3"),
    (["--threads", "2"], {"OPENBLAS_NUM_THREADS": "1"}, "1"),
])
def test_at_13_blas_hold_in_a_fresh_process(two_wavs, flags, env, expected):
    proc = _run_probe(["measure", two_wavs[0], "--light", *flags], env)
    assert proc.returncode == 0, proc.stderr
    assert f"BLAS={expected}" in proc.stdout


def _run_probe(args, env):
    import subprocess

    base = {k: v for k, v in os.environ.items() if k not in ("OPENBLAS_NUM_THREADS", "HARK_THREADS")}
    full = {**base, "PYTHONPATH": str(cli_engine()), **env}
    return subprocess.run([sys.executable, "-c", _PROBE, *args], env=full, capture_output=True, text=True,
                          stdin=subprocess.DEVNULL, timeout=300)


def cli_engine():
    from pathlib import Path

    return Path(cli.__file__).resolve().parents[1]


def test_blas_hold_skips_when_numpy_loaded(monkeypatch):
    monkeypatch.delenv("OPENBLAS_NUM_THREADS", raising=False)
    cli.blas_hold(["--threads", "2"])
    assert "OPENBLAS_NUM_THREADS" not in os.environ


def test_e_c5_threads_abc(monkeypatch, capsys):
    assert cli._hold_count(["--threads", "abc"]) is None
    assert cli.main(["measure", "x.wav", "--threads", "abc"]) == 2
    assert "invalid int value" in capsys.readouterr().err


def test_e_c6_threads_zero_keeps_env(two_wavs, monkeypatch):
    monkeypatch.setenv("HARK_THREADS", "3")
    assert cli.main(["measure", two_wavs[0], "--light", "--threads", "0"]) == 0
    assert os.environ["HARK_THREADS"] == "3"
    assert cli._hold_count(["--threads", "0"]) == 3


def test_e_c12_later_threads_wins(two_wavs, monkeypatch):
    monkeypatch.delenv("HARK_THREADS", raising=False)
    assert cli.main(["--threads", "4", "measure", two_wavs[0], "--light", "--threads", "2"]) == 0
    assert os.environ["HARK_THREADS"] == "2"
    assert cli._hold_count(["--threads", "4", "measure", "--threads", "2"]) == 2
    assert cli.main(["--threads", "4", "measure", two_wavs[0], "--light"]) == 0
    assert os.environ["HARK_THREADS"] == "4"


def test_q9_find_command_skips_global_threads_value():
    assert cli.find_command(["--threads", "2", "measure", "f"]) == "measure"
    assert cli.find_command(["--threads=2", "doctor"]) == "doctor"
    assert cli.find_command(["-h"]) is None


def test_e_c1_no_command_prints_help(capsys):
    assert cli.main([]) == 0
    assert "measure" in capsys.readouterr().out


def test_e_c2_unknown_command(capsys):
    assert cli.main(["nosuch"]) == 2
    assert "invalid choice" in capsys.readouterr().err


def test_help_exits_0(capsys):
    assert cli.main(["-h"]) == 0
    assert cli.main(["measure", "-h"]) == 0
    assert "--light" in capsys.readouterr().out


def test_not_built_yet_exit_2(monkeypatch, capsys):
    monkeypatch.setitem(cli.MODULE_OF, "describe", "hark.no_such_unit.describe")
    assert cli.main(["describe", "x.wav"]) == 2
    err = capsys.readouterr().err.strip()
    assert err == 'hark: the "describe" command is not built yet (module hark.no_such_unit.describe is missing)'


def test_e_c3_broken_module_exit_2(monkeypatch, capsys, tmp_path):
    (tmp_path / "hark_test_broken.py").write_text("import json\nraise ValueError('bad\\nmodule')\n")
    monkeypatch.syspath_prepend(str(tmp_path))
    monkeypatch.setitem(cli.MODULE_OF, "describe", "hark_test_broken")
    assert cli.main(["describe", "x.wav"]) == 2
    assert capsys.readouterr().err.strip() == 'hark: the "describe" command is unavailable: ValueError: bad module'


def test_at_39_e_c4_help_with_a_broken_module(monkeypatch, capsys, tmp_path):
    (tmp_path / "hark_test_broken_help.py").write_text("raise ImportError('broken on purpose')\n")
    monkeypatch.syspath_prepend(str(tmp_path))
    table = tuple((c, "hark_test_broken_help" if c == "gallery" else m) for c, m in cli.COMMANDS)
    monkeypatch.setattr(cli, "COMMANDS", table)
    assert cli.main(["-h"]) == 0
    out, err = capsys.readouterr()
    assert "(hark: module hark_test_broken_help unavailable: ImportError: broken on purpose)" in err
    assert "measure" in out and "doctor" in out


def test_at_31_two_files_json(two_wavs, capsys):
    assert cli.main(["measure", *two_wavs, "--light", "--json"]) == 0
    result = json.loads(capsys.readouterr().out)
    assert result["summary"] == "2 files measured."
    assert len(result["files"]) == 2
    assert all("file" in f and "summary" in f for f in result["files"])


def test_measure_text_form_has_one_line_per_file(two_wavs, capsys):
    assert cli.main(["measure", *two_wavs, "--light"]) == 0
    lines = capsys.readouterr().out.splitlines()
    assert len(lines) == 2 and lines[0].startswith("a.wav: 1.000 s")


def test_at_32_out_equals_json(two_wavs, tmp_path, capsys, monkeypatch):
    monkeypatch.chdir(tmp_path)
    assert cli.main(["measure", two_wavs[0], "--light", "--json"]) == 0
    printed = json.loads(capsys.readouterr().out)
    assert cli.main(["measure", two_wavs[0], "--light", "--out", "sub/dir/r.json"]) == 0
    assert (tmp_path / "sub" / "dir").is_dir()
    assert json.loads((tmp_path / "sub/dir/r.json").read_text(encoding="utf-8")) == printed


def test_e_c10_debug_traceback(fake_command, monkeypatch):
    def handler(args):
        raise KeyError("deep")

    fake_command(handler)
    monkeypatch.setenv("HARK_DEBUG", "1")
    with pytest.raises(KeyError):
        cli.main(["describe"])


def test_handler_return_code_and_error_type(fake_command, capsys):
    fake_command(lambda args: 7)
    assert cli.main(["describe"]) == 7


def test_at_40_ml_command_without_torch_non_tty(fake_command, hide_ml, monkeypatch, capsys):
    from hark.models import require_ml

    def handler(args):
        require_ml("describe")
        return 0

    fake_command(handler)
    monkeypatch.setattr(sys, "stdin", io.StringIO(""))
    assert cli.main(["describe", "x.wav"]) == 3
    err = capsys.readouterr().err.splitlines()
    assert len(err) == 1
    assert err[0] == ("hark describe: error: MLMissing: describe needs the ml extras. "
                      f"Install them with: {install_command('ml')}")
    assert "[y/N]" not in err[0]


def test_install_command_text():
    cmd = install_command("ml")
    assert "--index-url https://download.pytorch.org/whl/cpu" in cmd and '"torch>=2.6"' in cmd
    assert cmd.split(" && ")[1].endswith('-m pip install "transformers>=4.40"')
    assert install_command("mcp").endswith('-m pip install "mcp>=2"')


def test_require_extra_mcp(monkeypatch):
    from hark import models

    monkeypatch.setattr(models, "_absent", lambda names: tuple(names))
    with pytest.raises(MLMissing) as err:
        models.require_extra("mcp", "mcp server")
    assert err.value.missing == ("mcp",) and err.value.extra == "mcp"
    with pytest.raises(ValueError):
        models.require_extra("gpu", "x")


def _tty_session(monkeypatch, answer: str):
    monkeypatch.setattr(cli, "_is_tty", lambda: True)
    monkeypatch.setattr(sys, "stdin", io.StringIO(answer + "\n"))


def test_tty_prompt_declined_exit_3(fake_command, hide_ml, monkeypatch, capsys):
    from hark.models import require_ml

    fake_command(lambda args: require_ml("describe"))
    _tty_session(monkeypatch, "n")
    monkeypatch.setattr(cli.subprocess, "run", lambda *a, **k: pytest.fail("must not install"))
    assert cli.main(["describe"]) == 3
    err = capsys.readouterr().err
    assert "hark describe: describe needs the ml extras (torch, transformers)." in err
    assert "Install them now? [y/N]" in err


def test_tty_prompt_accepted_installs_and_reruns(fake_command, monkeypatch):
    calls, runs = [], []
    state = {"missing": True}

    def handler(args):
        runs.append(1)
        if state["missing"]:
            raise MLMissing("describe", ("torch",), "py -m pip install a && py -m pip install 'b c'")
        return 0

    def fake_run(argv, check=False):
        calls.append(argv)
        state["missing"] = False
        return argparse.Namespace(returncode=0)

    fake_command(handler)
    _tty_session(monkeypatch, "YES")
    monkeypatch.setattr(cli.subprocess, "run", fake_run)
    assert cli.main(["describe"]) == 0
    assert calls == [["py", "-m", "pip", "install", "a"], ["py", "-m", "pip", "install", "b c"]]
    assert len(runs) == 2


def test_tty_install_failure_exit_1(fake_command, monkeypatch):
    def handler(args):
        raise MLMissing("describe", ("torch",), "py -m pip install a")

    fake_command(handler)
    _tty_session(monkeypatch, "y")
    monkeypatch.setattr(cli.subprocess, "run", lambda argv, check=False: argparse.Namespace(returncode=1))
    assert cli.main(["describe"]) == 1


_HIDE_TORCH = """
import importlib.abc, json, sys
class Hide(importlib.abc.MetaPathFinder):
    def find_spec(self, name, path=None, target=None):
        if name.split('.')[0] in ('torch', 'transformers', 'huggingface_hub'):
            raise ImportError('hidden: ' + name)
        return None
sys.meta_path.insert(0, Hide())
from hark import cli
codes = [cli.main(['measure', sys.argv[1], '--light', '--json'])]
for cmd in (['sfx', 'list', '--json'], ['forge', '--list', '--json']):
    mod = cli.MODULE_OF[cmd[0]]
    try:
        __import__(mod)
    except ModuleNotFoundError:
        continue
    codes.append(cli.main(cmd))
heavy = [m for m in ('torch', 'transformers', 'huggingface_hub', 'librosa') if m in sys.modules]
print('RESULT ' + json.dumps({'codes': codes, 'heavy': heavy}))
"""


def test_model_free_paths_run_with_torch_hidden(two_wavs):
    import subprocess

    env = {**os.environ, "PYTHONPATH": str(cli_engine())}
    proc = subprocess.run([sys.executable, "-c", _HIDE_TORCH, two_wavs[0]], env=env, capture_output=True, text=True,
                          stdin=subprocess.DEVNULL, timeout=300)
    assert proc.returncode == 0, proc.stderr
    line = [x for x in proc.stdout.splitlines() if x.startswith("RESULT ")][-1]
    result = json.loads(line[len("RESULT "):])
    assert result["codes"] and set(result["codes"]) == {0}
    assert result["heavy"] == []


def test_python_m_hark(run_hark):
    proc = run_hark("--version")
    assert proc.returncode == 0 and proc.stdout.strip() == "hark 0.1.0"
