"""Role orchestration (spec 6.2, 6.5-6.8; T-20..T-25).

No test starts a model session or a real `horch spawn`: SKILL_INTAKE_HORCH
points at a stub that records its argv and prints a `spawned ...` line.
"""

from __future__ import annotations

import io
import json
import os
import runpy
import sys
import time
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

from base import INTAKE, REPO, TESTS, IntakeTestCase

import fixtures
from intakelib import index, roles
from intakelib import task as tasklib
from intakelib.task import read_json, write_json

SOURCE = "wordtally-pack"  # the local source folder name: a source name (spec 3.8)
STUB = """#!{python} -I
import json, os, sys, time
log = os.environ.get("STUB_LOG")
if log:
    with open(log, "a") as f:
        f.write(json.dumps(sys.argv[1:]) + "\\n")
if sys.argv[1:] == ["--version"]:
    if os.environ.get("STUB_HANG"):
        time.sleep(30)
    sys.exit(int(os.environ.get("STUB_VERSION_EXIT", "0")))
if sys.argv[1:2] == ["spawn"]:
    code = int(os.environ.get("STUB_SPAWN_EXIT", "0"))
    n = sum(1 for _ in open(log)) if log else 1
    if code == 0:
        sys.stderr.write(f"spawned {{sys.argv[2]}}-{{n}} (pane p{{n}}): none none, new session\\n")
    else:
        sys.stderr.write("horch spawn: no such teammate\\n")
    sys.exit(code)
"""
FLEET = {"HORCH_ROLE": "skill-importer-1", "HORCH_RECORD_ID": "rec-1", "HERDR_PANE_ID": "p0"}


def expected_argv(teammate: str, brief: str) -> list[str]:
    shape = read_json(TESTS / "fixtures" / "spawn-argv.json")["argv"]
    return [a.replace("{teammate}", teammate).replace("{brief}", brief) for a in shape]


class RolesCase(IntakeTestCase):
    def setUp(self):
        super().setUp()
        self.stub = self.tmp / "bin" / "horch"
        self.stub.parent.mkdir()
        self.stub.write_text(STUB.format(python=sys.executable))
        self.stub.chmod(0o755)
        self.log = self.tmp / "horch-argv.jsonl"
        os.environ["STUB_LOG"] = str(self.log)
        os.environ["SKILL_INTAKE_HORCH"] = str(self.stub)

    def task(self) -> tasklib.Task:
        """acquire through triage (GATE 1 pass) on F-clean, then cr index."""
        src = fixtures.build(self.tmp / SOURCE, fixtures.f_clean())
        r = self.run_intake("run", str(src), "--base", str(self.tmp / "base"))
        c = Path(json.loads(r.stdout)["task_dir"])
        (c / "sweep" / "audit.jsonl").write_text(
            json.dumps({"kind": "purpose", "skill": "clean-fixture", "purpose": "Counts words."}) + "\n")
        self.assertEqual(self.run_intake("triage", "--task", str(c)).returncode, 0)
        t = tasklib.load(str(c))
        index.build(t)
        return t

    def fleet(self, *args: str, **env: str):
        return self.run_intake(*args, env={**FLEET, "SKILL_INTAKE_MODE": "fleet", **env})

    def calls(self) -> list[list[str]]:
        if not self.log.is_file():
            return []
        return [json.loads(ln) for ln in self.log.read_text().splitlines()]

    def spawn(self, t: tasklib.Task, role: str, *extra: str, expect: int = 0, **env: str) -> dict | None:
        r = self.fleet("spawn", role, "--task", str(t.c), *extra, **env)
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        return json.loads(r.stdout) if expect == 0 else None

    def status(self, path: str, **doc) -> Path:
        p = Path(path)
        write_json(p, doc)
        return p

    def result(self, status: Path, expect: int = 0):
        r = self.run_intake("role-result", str(status))
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        return r

    def roles(self, t: tasklib.Task, *extra: str) -> list[dict]:
        r = self.run_intake("roles", "--task", str(t.c), *extra)
        self.assertEqual(r.returncode, 0, r.stderr)
        return [json.loads(ln) for ln in r.stdout.splitlines()]


class ModeTest(RolesCase):
    """T-20."""

    def mode(self, **env: str) -> str:
        r = self.run_intake("mode", env=env)
        self.assertEqual(r.returncode, 0, r.stderr)
        return r.stdout.strip()

    def test_all_vars_and_a_working_horch_is_fleet(self):
        self.assertEqual(self.mode(**FLEET), "fleet")
        self.assertEqual(self.calls(), [["--version"]])

    def test_a_missing_or_empty_var_is_plain(self):
        for var in FLEET:
            missing = {k: v for k, v in FLEET.items() if k != var}
            self.assertEqual(self.mode(**missing), "plain", var)
            self.assertEqual(self.mode(**{**FLEET, var: ""}), "plain", var)

    def test_a_failing_or_absent_horch_is_plain(self):
        self.assertEqual(self.mode(**FLEET, STUB_VERSION_EXIT="1"), "plain")
        self.assertEqual(self.mode(**FLEET, SKILL_INTAKE_HORCH=str(self.tmp / "no-horch")), "plain")

    def test_a_horch_that_hangs_past_5_s_is_plain(self):
        start = time.monotonic()
        self.assertEqual(self.mode(**FLEET, STUB_HANG="1"), "plain")
        self.assertLess(time.monotonic() - start, 15)

    def test_the_override(self):
        self.assertEqual(self.mode(**FLEET, SKILL_INTAKE_MODE="plain"), "plain")
        self.assertEqual(self.mode(SKILL_INTAKE_MODE="fleet"), "fleet")
        self.assertEqual(self.run_intake("mode", env={"SKILL_INTAKE_MODE": "x"}).returncode, 2)


class BriefTest(RolesCase):
    """T-22."""

    def brief(self, t, role: str, *extra: str, expect: int = 0) -> dict | None:
        r = self.fleet("brief", role, "--task", str(t.c), *extra)
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        return json.loads(r.stdout) if expect == 0 else None

    def test_each_brief_is_in_its_side_root(self):
        t = self.task()
        # The analyst reads its brief in C and writes its status file in D (T-34).
        for role, root, status in (("auditor", t.c, t.c), ("analyst", t.c, t.d), ("sanitizer", t.d, t.d),
                                   ("writer", t.k, t.k), ("reviewer", t.k, t.k)):
            out = self.brief(t, role)
            self.assertEqual(out["brief"], str(root / "briefs" / f"{role}-1.md"))
            self.assertEqual(out["status_file"], str(status / "status" / f"{role}-1.json"))
            text = Path(out["brief"]).read_text()
            self.assertIn("Controller: skill-importer-1", text)
            self.assertIn(out["status_file"], text)
        self.assertEqual(self.brief(t, "writer")["brief"], str(t.k / "briefs" / "writer-2.md"))

    def test_a_clean_brief_names_only_k_paths_and_passes_cr_scan(self):
        t = self.task()
        out = self.brief(t, "writer", "--skill", "clean-fixture", "--unit", "u1")
        text = Path(out["brief"]).read_text()
        self.assertIn(str(t.k / "out" / "clean-fixture"), text)
        for d in (str(t.c), str(t.d), SOURCE):
            self.assertNotIn(d, text)
        scan = self.run_intake("cr", "scan", "--task", str(t.c), out["brief"])
        self.assertEqual(scan.returncode, 0, scan.stdout)

    def test_a_source_name_or_identifier_fails_a_clean_brief(self):
        t = self.task()
        self.assertIn("test_count", read_json(t.c / "contaminated" / "fingerprints.json")["identifiers"])
        for role in ("writer", "reviewer"):
            self.brief(t, role, "--skill", SOURCE, expect=roles.BRIEF_LEAK)
            self.brief(t, role, "--unit", "test_count", expect=roles.BRIEF_LEAK)
        self.assertEqual(list((t.k / "briefs").iterdir()), [])

    def test_a_value_that_is_not_a_plain_name_is_refused(self):
        t = self.task()
        for value in ("../x", "a b", "/abs", ""):
            self.brief(t, "writer", "--skill", value, expect=2)

    def test_task_ids_are_opaque(self):
        t = self.task()
        self.assertRegex(t.id, r"^\d{8}-\d{6}-[0-9a-f]{6}$")
        self.assertNotIn(SOURCE, self.brief(t, "writer")["brief"])


FIXES = {"fixes": [
    {"file": "out/clean-fixture/SKILL.md", "lines": [7, 7], "kind": "rule", "rule": "INJ-01"},
    {"file": "persona.md", "lines": [4, 5], "kind": "overlap", "rule": None},
]}


class FixBriefTest(RolesCase):
    """T-37: `--fix` gives the writer a brief that names K/fixes.json, the fix list
    that intake verify wrote. It still takes no free text."""

    def brief(self, t, role: str, *extra: str, expect: int = 0) -> dict | None:
        r = self.fleet("brief", role, "--task", str(t.c), *extra)
        self.assertEqual(r.returncode, expect, r.stdout + r.stderr)
        return json.loads(r.stdout) if expect == 0 else None

    def test_the_fix_brief_names_the_fix_list_and_only_k_paths(self):
        t = self.task()
        write_json(t.k / "fixes.json", FIXES)
        out = self.brief(t, "writer", "--fix")
        text = Path(out["brief"]).read_text()
        self.assertIn("## Fix list", text)
        self.assertIn(str(t.k / "fixes.json"), text)
        self.assertIn("Rewrite only the lines it names", text)
        for d in (str(t.c), str(t.d), SOURCE, "INJ-01", "persona.md\""):
            self.assertNotIn(d, text)
        paths = [p for p in text.split() if p.startswith("/")]
        self.assertTrue(paths)
        for p in paths:
            self.assertTrue(tasklib.is_within(Path(p), t.k), p)
        scan = self.run_intake("cr", "scan", "--task", str(t.c), out["brief"])
        self.assertEqual(scan.returncode, 0, scan.stdout)

    def test_a_plain_brief_has_no_fix_section(self):
        t = self.task()
        write_json(t.k / "fixes.json", FIXES)
        text = Path(self.brief(t, "writer")["brief"]).read_text()
        self.assertNotIn("## Fix list", text)

    def test_a_fix_brief_without_a_fix_list_exits_non_zero(self):
        t = self.task()
        self.brief(t, "writer", "--fix", expect=2)
        write_json(t.k / "fixes.json", {"fixes": []})
        self.brief(t, "writer", "--fix", expect=2)
        write_json(t.k / "fixes.json", {"fixes": [{"file": "/etc/passwd", "lines": [1, 1], "kind": "rule",
                                                   "rule": "INJ-01"}]})
        self.brief(t, "writer", "--fix", expect=2)
        (t.k / "fixes.json").write_text("not json")
        self.brief(t, "writer", "--fix", expect=2)
        self.assertFalse((t.k / "briefs").exists() and list((t.k / "briefs").iterdir()))

    def test_only_the_writer_takes_a_fix_brief(self):
        t = self.task()
        write_json(t.k / "fixes.json", FIXES)
        for role in ("auditor", "analyst", "sanitizer", "reviewer"):
            self.brief(t, role, "--fix", expect=2)

    def test_spawn_with_fix_starts_the_writer_on_the_fix_brief(self):
        t = self.task()
        write_json(t.k / "fixes.json", FIXES)
        self.spawn(t, "writer", "--fix")
        brief = t.k / "briefs" / "writer-1.md"
        self.assertEqual(self.calls(), [expected_argv("intake-writer", str(brief))])
        self.assertIn("## Fix list", brief.read_text())
        self.spawn(t, "writer", "--fix", expect=roles.ROLE_ACTIVE)


class SpawnTest(RolesCase):
    """T-21."""

    def test_spawn_runs_the_exact_argv_and_records_the_start(self):
        t = self.task()
        out = self.spawn(t, "analyst", "--unit", "u1")
        brief = str(t.c / "briefs" / "analyst-1.md")
        self.assertEqual(self.calls(), [expected_argv("intake-analyst", brief)])
        self.assertEqual(self.calls()[0], ["spawn", "intake-analyst", "--exact", f"Read and follow {brief} exactly."])
        rec = [json.loads(ln) for ln in (t.c / "roles.jsonl").read_text().splitlines()]
        self.assertEqual(len(rec), 1)
        self.assertEqual({k: rec[0][k] for k in ("kind", "role", "worker", "brief", "status_file", "attempt")},
                         {"kind": "start", "role": "analyst", "worker": "intake-analyst-1", "brief": brief,
                          "status_file": str(t.d / "status" / "analyst-1.json"), "attempt": 1})
        self.assertTrue(rec[0]["started"])
        self.assertEqual(out["worker"], "intake-analyst-1")
        snap = read_json(Path(rec[0]["snapshot"]))
        self.assertIn("C/briefs/analyst-1.md", snap["files"])

    def test_sanitizer_brief_is_under_d_and_the_teammate_seam_works(self):
        t = self.task()
        self.spawn(t, "sanitizer", SKILL_INTAKE_ROLE_TEAMMATE="smoke")
        self.assertEqual(self.calls(), [expected_argv("smoke", str(t.d / "briefs" / "sanitizer-1.md"))])

    def test_plain_mode_refuses(self):
        t = self.task()
        r = self.run_intake("spawn", "analyst", "--task", str(t.c), env={"SKILL_INTAKE_MODE": "plain"})
        self.assertEqual(r.returncode, roles.WRONG_MODE, r.stderr)
        self.assertEqual(self.calls(), [])
        self.assertFalse((t.c / "roles.jsonl").exists())

    def test_a_failed_horch_spawn_exits_11_and_records_nothing(self):
        t = self.task()
        self.spawn(t, "analyst", expect=roles.SPAWN_FAILED, STUB_SPAWN_EXIT="1")
        self.assertFalse((t.c / "roles.jsonl").exists())
        self.spawn(t, "analyst")  # nothing is active

    def test_one_role_at_a_time(self):
        t = self.task()
        first = self.spawn(t, "auditor")
        self.spawn(t, "analyst", expect=roles.ROLE_ACTIVE)
        self.assertEqual(len(self.calls()), 1)
        (t.c / "sweep" / "audit.jsonl").write_text("{}\n")
        self.result(self.status(first["status_file"], role="auditor", status="ok",
                                outputs=[str(t.c / "sweep" / "audit.jsonl")], error=None))
        self.spawn(t, "analyst")
        self.assertEqual([w["state"] for w in self.roles(t)], ["done", "running"])


class RoleResultTest(RolesCase):
    """T-23."""

    def started(self, role: str = "writer") -> tuple[tasklib.Task, dict]:
        t = self.task()
        return t, self.spawn(t, role, "--skill", "clean-fixture")

    def test_a_valid_status_file_passes(self):
        t, w = self.started()
        out_file = t.k / "out" / "clean-fixture" / "SKILL.md"
        fixtures.build(t.k / "out", {"clean-fixture/SKILL.md": "x\n"})
        r = self.result(self.status(w["status_file"], role="writer", status="ok", outputs=[str(out_file)], error=None))
        self.assertEqual(json.loads(r.stdout)["outputs"], [str(out_file)])
        self.assertEqual(self.roles(t)[0]["state"], "done")
        self.assertEqual(self.roles(t, "--pending"), [])

    def test_an_error_status_with_1_line_passes_as_error(self):
        t, w = self.started()
        r = self.result(self.status(w["status_file"], role="writer", status="error", outputs=[], error="spec unclear"))
        self.assertEqual(json.loads(r.stdout)["error"], "spec unclear")
        self.assertEqual(self.roles(t)[0]["state"], "error")

    def test_bad_status_files_fail(self):
        bad = [
            {"role": "writer", "status": "ok", "outputs": [], "error": None, "note": "extra"},
            {"role": "writer", "status": "error", "outputs": [], "error": "line 1\nline 2"},
            {"role": "writer", "status": "error", "outputs": [], "error": "x" * 500},
            {"role": "writer", "status": "ok", "outputs": ["/etc/hosts"], "error": None},
            {"role": "writer", "status": "ok", "outputs": ["relative/path"], "error": None},
            {"role": "auditor", "status": "ok", "outputs": [], "error": None},
            {"role": "writer", "status": "done", "outputs": [], "error": None},
            {"role": "writer", "status": "ok", "outputs": []},
        ]
        for doc in bad:
            t, w = self.started()
            if doc.get("outputs") == ["/etc/hosts"]:
                doc["outputs"] = [str(t.d / "drafts" / "x.json")]  # in the task, outside K
            self.result(self.status(w["status_file"], **doc), expect=roles.BAD_RESULT)
            self.assertEqual(self.roles(t)[0]["state"], "error", doc)

    def test_a_role_that_wrote_outside_its_domain_fails_with_an_incident(self):
        t, w = self.started("analyst")
        (t.k / "out" / "planted.md").write_text("x\n")
        r = self.result(self.status(w["status_file"], role="analyst", status="ok", outputs=[], error=None),
                        expect=roles.BAD_RESULT)
        self.assertIn("domain incident", r.stderr)
        incidents = list((t.c / "contaminated" / "incidents").glob("inc-*.json"))
        self.assertEqual(len(incidents), 1)
        self.assertIn("K/out/planted.md", read_json(incidents[0])["notes"])

    def test_a_missing_or_unknown_status_file_fails(self):
        t, w = self.started()
        self.result(Path(w["status_file"]), expect=roles.BAD_RESULT)
        self.assertEqual(self.roles(t)[0]["state"], "running")  # the role may still be working
        self.result(self.status(str(t.k / "status" / "writer-9.json"), role="writer", status="ok",
                                outputs=[], error=None), expect=roles.BAD_RESULT)
        self.result(self.status(str(self.tmp / "status.json"), role="writer"), expect=roles.BAD_RESULT)


class LostRoleTest(RolesCase):
    """T-24."""

    def test_a_lost_role_is_spawned_again_once_then_aborts(self):
        t = self.task()
        first = self.spawn(t, "analyst")
        r = self.fleet("roles", "--task", str(t.c), "--lost", first["worker"])
        self.assertEqual(r.returncode, 0, r.stderr)
        brief = str(t.c / "briefs" / "analyst-1.md")
        self.assertEqual(self.calls(), [expected_argv("intake-analyst", brief)] * 2)
        states = [(w["state"], w["attempt"], w["brief"]) for w in self.roles(t)]
        self.assertEqual(states, [("lost", 1, brief), ("running", 2, brief)])

        r = self.fleet("roles", "--task", str(t.c), "--lost", "intake-analyst-2")
        self.assertEqual(r.returncode, roles.LOST_TWICE, r.stderr)
        self.assertEqual(len(self.calls()), 2)
        self.assertEqual(self.roles(t, "--pending"), [])
        self.spawn(t, "writer", expect=roles.LOST_TWICE)

        rep = self.run_intake("report", "--task", str(t.c))
        self.assertEqual(rep.returncode, 0, rep.stderr)
        self.assertIn("role abort: brief analyst-1.md was lost 2 times", (t.c / "report.md").read_text())
        self.assertIn("analyst-1.md was lost 2 times", rep.stdout)

    def test_lost_needs_fleet_mode_and_a_running_worker(self):
        t = self.task()
        w = self.spawn(t, "analyst")
        self.assertEqual(self.run_intake("roles", "--task", str(t.c), "--lost", w["worker"],
                                         env={"SKILL_INTAKE_MODE": "plain"}).returncode, roles.WRONG_MODE)
        self.assertEqual(self.fleet("roles", "--task", str(t.c), "--lost", "nobody").returncode, 2)


class PlainTest(RolesCase):
    """T-25."""

    def prompt(self, role: str, brief: str) -> tuple[int, str]:
        cli = __import__("intake")
        buf = io.StringIO()
        with redirect_stdout(buf), redirect_stderr(io.StringIO()):
            code = cli.main(["subagent-prompt", role, "--brief", brief])
        return code, buf.getvalue()

    def brief(self, t, role: str) -> dict:
        r = self.run_intake("brief", role, "--task", str(t.c))
        self.assertEqual(r.returncode, 0, r.stderr)
        return json.loads(r.stdout)

    def test_prompt_is_the_role_file_plus_1_brief_line_inside_the_guard_and_snapshot(self):
        os.environ["SKILL_INTAKE_MODE"] = "plain"
        t = self.task()
        b = self.brief(t, "writer")
        self.assertIn("Controller: none (plain session)", Path(b["brief"]).read_text())
        self.assertNotIn("horch", Path(b["brief"]).read_text())
        code, out = self.prompt("writer", b["brief"])
        self.assertEqual(code, 0)
        role_text = (roles.ROLES_DIR / "writer.md").read_text().rstrip("\n")
        self.assertTrue(role_text.startswith("# Role: writer"))
        self.assertEqual(out, f"{role_text}\n\n"
                              f"Your brief is `{b['brief']}`. Read it and do only what it says.\n")
        active = read_json(tasklib.default_base() / "active-role.json")
        self.assertEqual((active["role"], active["task_dir"]), ("writer", str(t.c)))
        self.assertEqual([(w["mode"], w["worker"], w["state"]) for w in self.roles(t)], [("plain", "subagent", "running")])

        other = self.brief(t, "reviewer")
        self.assertEqual(self.prompt("reviewer", other["brief"])[0], roles.ROLE_ACTIVE)
        self.result(self.status(b["status_file"], role="writer", status="ok", outputs=[], error=None))
        self.assertFalse((tasklib.default_base() / "active-role.json").exists())
        self.assertEqual(self.prompt("reviewer", other["brief"])[0], 0)
        self.assertEqual(self.calls(), [])  # plain mode never calls horch

    def test_plain_snapshot_catches_a_subagent_outside_its_domain(self):
        os.environ["SKILL_INTAKE_MODE"] = "plain"
        t = self.task()
        b = self.brief(t, "sanitizer")
        self.assertEqual(self.prompt("sanitizer", b["brief"])[0], 0)
        (t.c / "sweep" / "planted.md").write_text("x\n")
        self.result(self.status(b["status_file"], role="sanitizer", status="ok", outputs=[], error=None),
                    expect=roles.BAD_RESULT)

    def test_fleet_mode_refuses_the_prompt_and_plain_mode_refuses_spawn(self):
        t = self.task()
        b = self.brief(t, "writer")
        r = self.fleet("subagent-prompt", "writer", "--brief", b["brief"])
        self.assertEqual(r.returncode, roles.WRONG_MODE, r.stderr)
        self.assertEqual(r.stdout, "")
        r = self.run_intake("spawn", "writer", "--task", str(t.c), env={"SKILL_INTAKE_MODE": "plain"})
        self.assertEqual(r.returncode, roles.WRONG_MODE)

    def test_a_wrong_brief_is_refused(self):
        os.environ["SKILL_INTAKE_MODE"] = "plain"
        t = self.task()
        b = self.brief(t, "writer")
        self.assertEqual(self.prompt("reviewer", b["brief"])[0], 2)
        stray = self.tmp / "briefs" / "writer-1.md"
        stray.parent.mkdir()
        stray.write_text("x\n")
        self.assertEqual(self.prompt("writer", str(stray))[0], 2)


SKILL_NAME = "clean-fixture"


class RealWritePathsTest(RolesCase):
    """T-29: role-result checks each role's outputs at the paths `_io` gives that role."""

    def outputs(self, t: tasklib.Task, role: str) -> list[Path]:
        """A correct output at each write path of `role` (roles._io), written to disk."""
        c, d, k = t.c, t.d, t.k
        paths = {
            "auditor": [c / "sweep" / "audit.jsonl", c / "sweep" / "opaque" / "a.md"],
            "analyst": [d / "drafts" / "unit-1.json"],
            "sanitizer": [d / "drafts" / "unit-1.json", d / "quarantine-cr" / "note.md"],
            "writer": [k / "out" / SKILL_NAME / "SKILL.md", k / "report.json", k / "persona.md"],
            "reviewer": [k / "out" / SKILL_NAME / "SKILL.md", k / "review.json"],
        }[role]
        for p in paths:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text("{}\n" if p.suffix in (".json", ".jsonl") else "x\n")
        return paths

    def start(self, t: tasklib.Task, role: str, plain: bool) -> str:
        """Start `role` in fleet or plain mode; return its status file path."""
        extra = ("--skill", SKILL_NAME) if role in ("writer", "reviewer") else ()
        if not plain:
            return self.spawn(t, role, *extra)["status_file"]
        env = {"SKILL_INTAKE_MODE": "plain"}
        r = self.run_intake("brief", role, "--task", str(t.c), *extra, env=env)
        self.assertEqual(r.returncode, 0, r.stderr)
        b = json.loads(r.stdout)
        r = self.run_intake("subagent-prompt", role, "--brief", b["brief"], env=env)
        self.assertEqual(r.returncode, 0, r.stderr)
        return b["status_file"]

    def check(self, role: str, plain: bool):
        t = self.task()
        status = self.start(t, role, plain)
        outs = self.outputs(t, role)
        self.status(status, role=role, status="ok", outputs=[str(p) for p in outs], error=None)
        env = {"SKILL_INTAKE_MODE": "plain"} if plain else {}
        r = self.run_intake("role-result", status, env=env)
        self.assertEqual(r.returncode, 0, f"{role} plain={plain}: {r.stdout}{r.stderr}")
        self.assertEqual(json.loads(r.stdout)["outputs"], [str(p) for p in outs])
        self.assertEqual(self.roles(t)[0]["state"], "done")

    def test_each_role_result_passes_in_fleet_mode(self):
        for role in roles.ROLES:
            with self.subTest(role=role):
                self.check(role, plain=False)

    def test_each_role_result_passes_in_plain_mode(self):
        for role in roles.ROLES:
            with self.subTest(role=role):
                self.check(role, plain=True)

    def test_an_output_outside_the_role_io_paths_fails(self):
        for role in roles.ROLES:
            for plain in (False, True):
                with self.subTest(role=role, plain=plain):
                    t = self.task()
                    status = self.start(t, role, plain)
                    outs = [str(p) for p in self.outputs(t, role)] + [str(t.c / "task.json")]
                    self.status(status, role=role, status="ok", outputs=outs, error=None)
                    env = {"SKILL_INTAKE_MODE": "plain"} if plain else {}
                    r = self.run_intake("role-result", status, env=env)
                    self.assertEqual(r.returncode, roles.BAD_RESULT, r.stdout + r.stderr)
                    self.assertIn("output outside the", r.stderr)


    # T-32: a role may list its own status file in outputs, and only that status file.

    def result_with(self, role: str, plain: bool, extra):
        t = self.task()
        status = self.start(t, role, plain)
        outs = [str(p) for p in self.outputs(t, role)] + [e(t, status) for e in extra]
        self.status(status, role=role, status="ok", outputs=outs, error=None)
        env = {"SKILL_INTAKE_MODE": "plain"} if plain else {}
        return self.run_intake("role-result", status, env=env), t, status

    def test_each_role_may_list_its_own_status_file(self):
        for role in roles.ROLES:
            for plain in (False, True):
                with self.subTest(role=role, plain=plain):
                    r, t, status = self.result_with(role, plain, [lambda t, s: s])
                    self.assertEqual(r.returncode, 0, f"{r.stdout}{r.stderr}")
                    self.assertIn(status, json.loads(r.stdout)["outputs"])
                    self.assertEqual(self.roles(t)[0]["state"], "done")

    def test_another_roles_status_file_fails(self):
        for role in roles.ROLES:
            other = next(o for o in roles.ROLES if o != role)
            for plain in (False, True):
                with self.subTest(role=role, other=other, plain=plain):
                    r, _, _ = self.result_with(
                        role, plain, [lambda t, s: str(roles.status_path(t, other, f"{other}-1"))])
                    self.assertEqual(r.returncode, roles.BAD_RESULT, r.stdout + r.stderr)
                    self.assertIn("output outside the", r.stderr)

    def test_a_same_role_other_numbered_status_file_fails(self):
        for role in roles.ROLES:
            for plain in (False, True):
                with self.subTest(role=role, plain=plain):
                    r, _, _ = self.result_with(
                        role, plain, [lambda t, s: str(Path(s).with_name(f"{role}-9.json"))])
                    self.assertEqual(r.returncode, roles.BAD_RESULT, r.stdout + r.stderr)
                    self.assertIn("output outside the", r.stderr)

    def test_a_different_file_under_status_fails(self):
        for role in roles.ROLES:
            for plain in (False, True):
                with self.subTest(role=role, plain=plain):
                    r, _, _ = self.result_with(role, plain, [lambda t, s: str(Path(s).with_name("notes.json"))])
                    self.assertEqual(r.returncode, roles.BAD_RESULT, r.stdout + r.stderr)
                    self.assertIn("output outside the", r.stderr)


def sandbox_paths(teammate: Path, home: Path) -> dict[str, list[Path]]:
    """{"allowRead", "allowWrite", "denyRead"} of the sandbox.filesystem block in the
    frontmatter of `teammate`, with "~" expanded to `home`."""
    front = teammate.read_text(encoding="utf-8").split("\n---", 1)[0].splitlines()
    out: dict[str, list[Path]] = {}
    key, in_fs = None, False
    for ln in front:
        if ln.startswith("  filesystem:"):
            in_fs = True
        elif in_fs and ln.startswith("    ") and ln.strip().endswith(":"):
            key = ln.strip()[:-1]
            out[key] = []
        elif in_fs and key and ln.startswith("      - "):
            raw = ln.strip()[2:].strip().strip('"')
            out[key].append(home / raw[2:] if raw.startswith("~/") else Path(raw))
        elif not ln.startswith("    "):
            in_fs, key = False, None
    return out


class TeammateSandboxTest(RolesCase):
    """T-34: each role's brief, status file and write paths fit the real
    teammates/intake-<role>.md sandbox block and the domain write set."""

    def test_each_role_paths_fit_its_teammate_sandbox_and_write_set(self):
        from intakelib import domain
        os.environ.update({**FLEET, "SKILL_INTAKE_MODE": "fleet"})
        t = tasklib.create()
        self.assertEqual(t.base, self.home / ".local" / "state" / "skill-intake")
        for role in roles.ROLES:
            with self.subTest(role=role):
                skill = SKILL_NAME if role in ("writer", "reviewer") else None
                box = sandbox_paths(REPO / "teammates" / f"intake-{role}.md", self.home)
                self.assertTrue(box.get("allowRead") and box.get("allowWrite"), box)

                def inside(p, key: str) -> bool:
                    return any(tasklib.is_within(Path(p), a) for a in box[key])

                out = roles.brief(t, role, skill=skill)
                reads, writes = roles._io(t, role, skill)
                self.assertTrue(inside(out["brief"], "allowRead"), f"brief {out['brief']}")
                self.assertTrue(inside(out["status_file"], "allowWrite"), f"status {out['status_file']}")
                for p in reads:
                    self.assertTrue(inside(p, "allowRead"), f"read path {p}")
                    self.assertFalse(inside(p, "denyRead"), f"read path {p}")
                for p in writes:
                    self.assertTrue(inside(p, "allowWrite"), f"write path {p}")
                # The domain write set names the status file and only sides the role can write.
                side = next(s for s in tasklib.SIDES if tasklib.is_within(Path(out["status_file"]), t.root(s)))
                rel = domain.SHORT[side] + "/" + str(Path(out["status_file"]).relative_to(t.root(side)))
                self.assertTrue(domain._allowed(rel, domain.WRITE_SETS[role]), f"{rel} not in the write set")
                short = {v: k for k, v in domain.SHORT.items()}
                for pat in domain.WRITE_SETS[role]:
                    self.assertTrue(inside(t.root(short[pat.split("/", 1)[0]]), "allowWrite"), pat)


class PythonVersionTest(RolesCase):
    """T-29: intake needs Python 3.11 or later."""

    def run_old(self) -> tuple[int, str]:
        err = io.StringIO()
        old = (3, 10, 4, "final", 0)
        with mock.patch.object(sys, "version_info", old), redirect_stderr(err):
            with self.assertRaises(SystemExit) as cm:
                runpy.run_path(str(INTAKE), run_name="intake_old_python")
        return cm.exception.code, err.getvalue()

    def test_python_older_than_3_11_exits_2(self):
        code, err = self.run_old()
        self.assertEqual(code, 2)
        self.assertEqual(err.strip(), "intake needs Python 3.11 or later; found 3.10.4")
