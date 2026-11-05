"""T-10: the clean-room `tests/test_cr.py` behaviors, rewritten for `intake cr` and the 3 side roots."""

from __future__ import annotations

import copy
import io
import json
import os
import sys
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

from base import IntakeTestCase

import fixtures
from intakelib import guard, handoff, index, sandbox, scan, schema
from intakelib import task as tasklib
from intakelib.cmd_cr import preflight
from intakelib.task import read_json, write_json

SOURCE_FILE = '''
import json
from collections import OrderedDict

def parse_zephyr_manifest(raw_text, strict_mode=False):
    """Parse the manifest; reject duplicate keys when strict."""
    seen_keys = OrderedDict()
    for line_number, line in enumerate(raw_text.splitlines()):
        key, _, value = line.partition("=")
        if strict_mode and key in seen_keys:
            raise ValueError("duplicate key on line %d" % line_number)
        seen_keys[key.strip()] = value.strip()
    return dict(seen_keys)

class ZephyrCacheStore:
    def get_or_compute(self, cache_key, compute_fn):
        return compute_fn(cache_key)
'''

SKILL_MD = "---\nname: manifest-tool\ndescription: Parses manifests.\n---\nUse the parser script.\n"
BIG_SCRIPT = "".join(f"step_value_{i} = {i}\n" for i in range(320))

PROVENANCE = {
    "task_id": "", "fetched_at": "2026-11-05T00:00:00+00:00",
    "source": {"kind": "github", "input": "acme-labs/zephyrlib@v1", "url": "https://github.com/acme-labs/zephyrlib.git",
               "ref": "v1", "commit": "0" * 40, "subdir": None},
    "content_sha256": "a" * 64, "archive_sha256": None,
    "licence": {"spdx": "MIT", "files": ["LICENSE"], "class": "permissive", "flag": None},
}

GOOD_SPEC = {
    "spec_id": "spec-manifest-parse",
    "title": "Parse key/value manifests",
    "summary": "Turns text made of key=value lines into a mapping, with an optional strict mode.",
    "skill": {"name": "manifest-tool", "purpose": "Reads manifest text for the agent.",
              "triggers": ["the user asks to read a manifest"], "outputs": ["a mapping of keys to values"]},
    "capabilities": {"reads": ["the current repo"], "runs": ["python3"]},
    "replace_or_remove": [{"id": "R-1", "finding": "SUP-04#1", "action": "replace",
                           "behavior": "Reads the file with the standard library in place of the package."}],
    "public_contract": [{"kind": "function", "name": "parse_zephyr_manifest",
                         "description": "Entry point; name must match for drop-in compatibility."}],
    "behaviors": [{"id": "b1", "given": "text with one pair per line", "when": "parsed",
                   "then": "each key maps to its value with surrounding whitespace removed"},
                  {"id": "b2", "given": "strict mode and a repeated key", "when": "parsed",
                   "then": "an error names the offending line position"}],
    "test_scenarios": [{"id": "t1", "description": "two pairs", "input": "a=1 and b=2 on separate lines",
                        "expected": "a mapping with both pairs"}],
    "provenance": {"unit_id": "u1"},
    "review": {"reviewer": "sanitizer", "status": "approved"},
}


def quiet(fn, *args, **kw):
    with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
        return fn(*args, **kw)


class Base(IntakeTestCase):
    """A task with a quarantined source of 1 skill, skills.json, provenance and an index."""

    def setUp(self):
        super().setUp()
        self.task = tasklib.create(str(self.tmp / "base"))
        self.src = index.source_root(self.task)
        fixtures.build(self.src, {
            "skills/manifest-tool/SKILL.md": SKILL_MD,
            "skills/manifest-tool/scripts/manifest.py": SOURCE_FILE,
            "skills/manifest-tool/scripts/steps.py": BIG_SCRIPT,
            "skills/manifest-tool/secret.txt": "source-only\n",
            "tools/unused.sh": "echo hi\n",
        })
        write_json(self.task.c / "skills.json", {"skills": [
            {"name": "manifest-tool", "rel_path": "skills/manifest-tool", "description": {"untrusted": "x"},
             "files": 4, "bytes": 1, "dropped_variants": []}]})
        write_json(self.task.c / "provenance.json", dict(PROVENANCE, task_id=self.task.id))
        write_json(self.task.c / "contaminated" / "preflight.json", preflight(self.task))
        index.build(self.task)

    def draft(self, spec, name=None) -> Path:
        p = self.task.d / "drafts" / (name or f"{spec['spec_id']}.json")
        write_json(p, spec)
        return p


class SchemaTests(Base):
    def pre(self):
        return read_json(self.task.c / "contaminated" / "preflight.json")

    def test_valid_preflight(self):
        self.assertEqual(schema.validate(self.pre(), schema.load_schema("preflight")), [])

    def test_missing_and_extra_fields(self):
        bad = dict(self.pre(), sneaky="x")
        del bad["goal"]
        errs = schema.validate(bad, schema.load_schema("preflight"))
        self.assertTrue(any("goal" in e for e in errs))
        self.assertTrue(any("sneaky" in e for e in errs))

    def test_authorization_must_be_true(self):
        bad = self.pre()
        bad["authorization"]["confirmed"] = False
        self.assertTrue(schema.validate(bad, schema.load_schema("preflight")))

    def test_unsupported_keyword_is_not_ignored(self):
        with self.assertRaises(ValueError):
            schema.validate({}, {"type": "object", "oneOf": []})

    def test_all_schemas_load(self):
        self.assertEqual(set(schema.kinds()), {
            "behavior-spec", "findings", "handoff", "incident", "mitigation", "preflight", "provenance",
            "report", "verify"})
        for k in schema.kinds():
            schema.validate({}, schema.load_schema(k))  # must not raise on keywords

    def test_behavior_spec_6_9_fields(self):
        bs = schema.load_schema("behavior-spec")
        self.assertEqual(schema.validate(GOOD_SPEC, bs), [])
        no_skill = {k: v for k, v in GOOD_SPEC.items() if k != "skill"}
        self.assertTrue(any("skill" in e for e in schema.validate(no_skill, bs)))
        bad = copy.deepcopy(GOOD_SPEC)
        bad["replace_or_remove"][0]["action"] = "keep"
        self.assertTrue(schema.validate(bad, bs))
        bad = copy.deepcopy(GOOD_SPEC)
        del bad["replace_or_remove"][0]["behavior"]
        self.assertTrue(schema.validate(bad, bs))
        bad = copy.deepcopy(GOOD_SPEC)
        bad["replace_or_remove"][0]["finding"] = "INJ-01#1"  # INJ findings never appear in a spec
        self.assertTrue(schema.validate(bad, bs))
        bad = copy.deepcopy(GOOD_SPEC)
        bad["capabilities"]["shell"] = ["anything"]
        self.assertTrue(schema.validate(bad, bs))

    def test_provenance_fixture_is_valid(self):
        self.assertEqual(schema.validate(read_json(self.task.c / "provenance.json"),
                                         schema.load_schema("provenance")), [])


class InitTests(Base):
    def test_preflight_is_unattended_and_names_source_and_licence(self):
        r = self.run_intake("cr", "init", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 0, r.stderr)
        pre = read_json(Path(json.loads(r.stdout)["preflight"]))
        self.assertEqual(pre["mode"], "unattended")
        self.assertIs(pre["confirmed_by_user"], True)
        self.assertEqual(pre["open_questions"], [])
        self.assertEqual(pre["max_iterations"], 3)
        self.assertEqual(pre["policies"]["dependencies"], "stdlib-only")
        self.assertEqual(pre["policies"]["compatibility"], "behavioral")
        self.assertEqual(pre["authorization"]["basis"],
                         "operator invoked skill-intake on acme-labs/zephyrlib@v1; licence MIT/permissive")

    def test_init_without_provenance_exits_2(self):
        (self.task.c / "provenance.json").unlink()
        r = self.run_intake("cr", "init", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 2)
        self.assertIn("provenance.json", r.stderr)


class IndexTests(Base):
    def idx(self):
        return read_json(self.task.c / "contaminated" / "source-index.json")

    def test_index_contents(self):
        files = {f["path"]: f for f in self.idx()["files"]}
        f = files["skills/manifest-tool/scripts/manifest.py"]
        self.assertEqual(f["language"], "python")
        self.assertEqual(f["skill"], "manifest-tool")
        self.assertIn("parse_zephyr_manifest", f["symbols"])
        self.assertIn("collections", f["imports"])
        self.assertEqual(self.idx()["out_of_scope"], ["tools/unused.sh"])

    def test_one_unit_per_skill_plus_one_per_long_script(self):
        units = self.idx()["units"]
        self.assertEqual([(u["unit_id"], u["kind"]) for u in units], [("u1", "skill"), ("u2", "script")])
        self.assertEqual(units[1]["files"], ["skills/manifest-tool/scripts/steps.py"])
        self.assertNotIn("skills/manifest-tool/scripts/steps.py", units[0]["files"])
        self.assertIn("skills/manifest-tool/scripts/manifest.py", units[0]["files"])

    def test_fingerprints(self):
        fp = read_json(self.task.c / "contaminated" / "fingerprints.json")
        self.assertIn("ZephyrCacheStore", fp["identifiers"])
        self.assertIn("seen_keys", fp["identifiers"])
        self.assertNotIn("return", fp["identifiers"])
        self.assertTrue(fp["shingles"])

    def test_index_needs_skills_json(self):
        (self.task.c / "skills.json").unlink()
        r = self.run_intake("cr", "index", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 2)
        self.assertIn("skills.json", r.stderr)

    def test_index_refuses_rel_path_outside_quarantine(self):
        (self.task.c / "elsewhere").mkdir()
        (self.src / "skills" / "link").symlink_to(self.task.c / "elsewhere")
        for rel in ("../../elsewhere", "/etc", "skills/../..", ".", "skills/./manifest-tool", "skills/link",
                    "skills/man ifest", "", "skills/missing"):
            skills = read_json(self.task.c / "skills.json")
            skills["skills"][0]["rel_path"] = rel
            write_json(self.task.c / "skills.json", skills)
            with self.assertRaises(tasklib.TaskError, msg=rel):
                index.build(self.task)
            r = self.run_intake("cr", "index", "--task", str(self.task.c))
            self.assertEqual(r.returncode, 2, rel)
            self.assertIn("refused skills.json rel_path", r.stderr, rel)

    def test_cli_index_prints_units(self):
        r = self.run_intake("cr", "index", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(len(json.loads(r.stdout)["units"]), 2)


class ScanTests(Base):
    def check(self, spec):
        return [f.check for f in scan.scan_file(self.draft(spec), self.task) if f.severity == "block"]

    def test_clean_spec_passes(self):
        self.assertEqual(self.check(GOOD_SPEC), [])

    def test_public_contract_name_is_allowed_but_others_are_not(self):
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] += " Uses a ZephyrCacheStore internally."
        self.assertIn("identifiers", self.check(spec))

    def test_code_fence_and_code_lines(self):
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = "Like this:\n```\nx = 1\n```"
        self.assertIn("code-fence", self.check(spec))
        spec["summary"] = "def handle(raw):"
        self.assertIn("code-line", self.check(spec))

    def test_prose_is_not_code(self):
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = "Public methods return a mapping. Class of errors: value errors. Import is lazy."
        self.assertEqual(self.check(spec), [])

    def test_verbatim_run_detected_without_identifiers(self):
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = 'key, _, value = line.partition("=") then raise ValueError("duplicate key on line %d" % n)'
        self.assertIn("verbatim", self.check(spec))

    def test_source_name_and_path(self):
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = f"Port of zephyrlib by acme-labs found at {self.src}"
        checks = self.check(spec)
        self.assertIn("source-name", checks)
        self.assertIn("source-path", checks)

    def test_local_source_path_from_provenance(self):
        prov = read_json(self.task.c / "provenance.json")
        prov["source"] = {"kind": "local", "input": "/opt/work/blueskill"}
        write_json(self.task.c / "provenance.json", prov)
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = "Copied from /opt/work/blueskill today."
        checks = self.check(spec)
        self.assertIn("source-path", checks)
        self.assertIn("source-name", checks)

    def test_skill_name_may_cross_the_wall(self):
        prov = read_json(self.task.c / "provenance.json")
        prov["source"] = {"kind": "local", "input": "/opt/work/manifest-tool"}
        write_json(self.task.c / "provenance.json", prov)
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = "The manifest-tool skill turns key=value text into a mapping."
        self.assertEqual(self.check(spec), [])

    def test_decompiler_marker(self):
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = "Behavior observed in the decompiled build."
        self.assertIn("decompiler", self.check(spec))


class HandoffTests(Base):
    def test_approved_clean_spec_crosses_and_is_stripped(self):
        done, failures = handoff.handoff(self.task, [str(self.draft(GOOD_SPEC))])
        self.assertEqual((done, failures), (["spec-manifest-parse"], []))
        out = read_json(self.task.k / "handoff" / "spec-manifest-parse.json")
        self.assertNotIn("provenance", out)
        self.assertNotIn("review", out)
        self.assertEqual(out["replace_or_remove"], GOOD_SPEC["replace_or_remove"])
        self.assertEqual(handoff.verify_package(self.task), [])
        self.assertEqual(schema.validate(read_json(self.task.k / "handoff" / "package.json"),
                                         schema.load_schema("handoff")), [])

    def test_unapproved_is_rejected(self):
        spec = copy.deepcopy(GOOD_SPEC)
        del spec["review"]
        done, failures = handoff.handoff(self.task, [str(self.draft(spec))])
        self.assertEqual(done, [])
        self.assertIn("not approved", failures[0])

    def test_leaking_spec_is_quarantined_with_incident(self):
        spec = copy.deepcopy(GOOD_SPEC)
        spec["summary"] = "Wraps ZephyrCacheStore."
        done, failures = handoff.handoff(self.task, [str(self.draft(spec))])
        self.assertEqual(done, [])
        self.assertIn("quarantined", failures[0])
        self.assertFalse((self.task.k / "handoff" / "spec-manifest-parse.json").exists())
        incidents = list(handoff.incidents_dir(self.task).glob("inc-*.json"))
        self.assertEqual(len(incidents), 1)
        self.assertEqual(schema.validate(read_json(incidents[0]), schema.load_schema("incident")), [])
        self.assertTrue(list((self.task.d / "quarantine-cr").glob("inc-*")))
        self.assertEqual(list(self.task.k.rglob("inc-*")), [])

    def test_draft_outside_drafts_dir_is_refused(self):
        for p in (self.tmp / "elsewhere.json", self.task.c / "contaminated" / "spec-x.json",
                  self.task.k / "spec-x.json"):
            write_json(p, GOOD_SPEC)
            _, failures = handoff.handoff(self.task, [str(p)])
            self.assertIn("drafts must be files", failures[0], str(p))

    def test_tampering_after_handoff_is_detected(self):
        handoff.handoff(self.task, [str(self.draft(GOOD_SPEC))])
        f = self.task.k / "handoff" / "spec-manifest-parse.json"
        f.write_text(f.read_text().replace("mapping", "dict"))
        self.assertIn("changed after handoff", handoff.verify_package(self.task)[0])

    def test_context_has_no_source_paths(self):
        handoff.handoff(self.task, [str(self.draft(GOOD_SPEC))])
        path = handoff.write_context(self.task)
        self.assertTrue(tasklib.is_within(path, self.task.k))
        text = path.read_text()
        self.assertNotIn(str(self.task.c), text)
        self.assertNotIn(str(self.task.d), text)
        self.assertNotIn("zephyrlib", text)
        self.assertNotIn("acme-labs", text)
        self.assertEqual(read_json(path)["specs"], ["spec-manifest-parse.json"])

    def test_context_requires_confirmed_preflight(self):
        p = self.task.c / "contaminated" / "preflight.json"
        write_json(p, dict(read_json(p), confirmed_by_user=False))
        with self.assertRaises(ValueError):
            handoff.write_context(self.task)


class VerifyTests(Base):
    def run_py(self, code):
        return quiet(sandbox.run, self.task, [sys.executable, "-I", "-c", code], 60)[0]

    def test_no_sandbox_runs_nothing_and_marks_unverified(self):
        os.environ["SKILL_INTAKE_NO_SANDBOX"] = "1"
        target = self.task.k / "out" / "ran.txt"
        r = self.run_intake("cr", "verify", "--task", str(self.task.c), "--",
                            sys.executable, "-I", "-c", f"open({str(target)!r},'w').write('x')")
        self.assertEqual(r.returncode, sandbox.UNVERIFIED, r.stderr)
        self.assertEqual(json.loads(r.stdout)["status"], "unverified")
        self.assertFalse(target.exists())
        log = read_json(self.task.k / "verify-log" / "log.json")
        self.assertEqual(log["runs"][-1]["status"], "unverified")

    def test_argv_into_contaminated_or_drafts_is_refused(self):
        os.environ["SKILL_INTAKE_NO_SANDBOX"] = "1"
        for path in (self.src / "skills" / "manifest-tool" / "scripts" / "manifest.py", self.task.d / "drafts" / "x.py"):
            r = self.run_intake("cr", "verify", "--task", str(self.task.c), "--", sys.executable, str(path))
            self.assertEqual(r.returncode, 2, str(path))
        self.assertFalse((self.task.k / "verify-log" / "log.json").exists())


@unittest.skipUnless(sandbox.kind(), "no usable sandbox-exec or bwrap on this host")
class SandboxTests(Base):
    def run_py(self, code):
        return quiet(sandbox.run, self.task, [sys.executable, "-I", "-c", code], 60)[0]

    def test_write_inside_out_ok_and_exit_code_propagates(self):
        self.assertEqual(self.run_py("open('ok.txt','w').write('x')"), 0)
        self.assertTrue((self.task.k / "out" / "ok.txt").exists())
        self.assertEqual(self.run_py("raise SystemExit(3)"), 3)

    def test_cannot_read_source(self):
        secret = self.src / "skills" / "manifest-tool" / "secret.txt"
        self.assertNotEqual(self.run_py(f"open({str(secret)!r}).read()"), 0)

    def test_cannot_read_contaminated_or_drafts(self):
        self.assertNotEqual(self.run_py(f"open({str(self.task.c / 'task.json')!r}).read()"), 0)
        draft = self.draft(GOOD_SPEC)
        self.assertNotEqual(self.run_py(f"open({str(draft)!r}).read()"), 0)

    def test_cannot_write_outside_out(self):
        for target in (self.tmp / "escape.txt", self.task.k / "handoff" / "x.json"):
            self.assertNotEqual(self.run_py(f"open({str(target)!r},'w').write('x')"), 0)
            self.assertFalse(target.exists())

    def test_no_network(self):
        """A local listener is reachable outside the sandbox and not inside it (no real network used)."""
        import socket
        srv = socket.socket()
        self.addCleanup(srv.close)
        srv.bind(("127.0.0.1", 0))
        srv.listen(1)
        code = f"import socket; socket.create_connection(('127.0.0.1', {srv.getsockname()[1]}), timeout=5)"
        self.assertNotEqual(self.run_py(code), 0)

    def test_run_is_logged(self):
        self.run_py("pass")
        log = read_json(self.task.k / "verify-log" / "log.json")
        self.assertEqual(log["runs"][-1]["exit_code"], 0)
        self.assertEqual(log["runs"][-1]["status"], "pass")


class GuardTests(Base):
    def decide(self, role, tool, agent=True, **tool_input):
        state = {"task_dir": str(self.task.c), "role": role} if role else None
        event = {"tool_name": tool, "tool_input": tool_input, "cwd": str(self.task.k / "out")}
        if agent:
            event["agent_id"] = "a1"
        return guard.decide(event, state)

    def test_state_is_under_the_base_not_claude(self):
        self.assertTrue(tasklib.is_within(guard.state_path(), self.state))

    def test_inactive_or_main_session_is_unrestricted(self):
        secret = str(self.src / "skills" / "manifest-tool" / "secret.txt")
        self.assertIsNone(self.decide(None, "Read", file_path=secret))
        self.assertIsNone(self.decide("writer", "Read", agent=False, file_path=secret))

    def test_roles_cannot_read_other_side_roots(self):
        secret = str(self.src / "skills" / "manifest-tool" / "secret.txt")
        for role in ("writer", "reviewer", "sanitizer"):
            self.assertIsNotNone(self.decide(role, "Read", file_path=secret), role)
        self.assertIsNotNone(self.decide("writer", "Grep", path=str(self.task.base)))
        self.assertIsNotNone(self.decide("writer", "Read", file_path=str(self.task.d / "drafts" / "spec-a.json")))
        self.assertIsNotNone(self.decide("sanitizer", "Read", file_path=str(self.task.k / "context.json")))
        self.assertIsNotNone(self.decide("analyst", "Read", file_path=str(self.task.k / "context.json")))
        self.assertIsNone(self.decide("analyst", "Read", file_path=secret))
        self.assertIsNone(self.decide("auditor", "Grep", path=str(self.src)))
        self.assertIsNone(self.decide("sanitizer", "Read", file_path=str(self.task.d / "drafts" / "spec-a.json")))

    def test_clean_roles_read_clean(self):
        self.assertIsNone(self.decide("writer", "Read", file_path=str(self.task.k / "context.json")))
        self.assertIsNone(self.decide("reviewer", "Glob", path=str(self.task.k / "out")))
        self.assertIsNone(self.decide("writer", "Read", file_path="notes.md"))  # relative to K/out cwd

    def test_write_domains(self):
        self.assertIsNotNone(self.decide("analyst", "Write", file_path=str(self.task.k / "x.json")))
        self.assertIsNone(self.decide("analyst", "Write", file_path=str(self.task.d / "drafts" / "spec-a.json")))
        self.assertIsNotNone(self.decide("analyst", "Write", file_path=str(self.task.c / "skills.json")))
        self.assertIsNone(self.decide("auditor", "Write", file_path=str(self.task.c / "sweep" / "audit.jsonl")))
        self.assertIsNotNone(self.decide("auditor", "Write", file_path=str(self.task.d / "drafts" / "spec-a.json")))
        self.assertIsNone(self.decide("sanitizer", "Write", file_path=str(self.task.d / "quarantine-cr" / "x.json")))
        self.assertIsNone(self.decide("writer", "Edit", file_path=str(self.task.k / "out" / "s" / "SKILL.md")))
        self.assertIsNone(self.decide("writer", "Write", file_path=str(self.task.k / "report.json")))
        self.assertIsNotNone(self.decide("writer", "Write", file_path=str(self.task.k / "review.json")))
        self.assertIsNotNone(self.decide("writer", "Write", file_path=str(self.task.k / "handoff" / "package.json")))
        self.assertIsNone(self.decide("reviewer", "Write", file_path=str(self.task.k / "review.json")))

    def test_shell(self):
        verify = f"python3 -I {guard.INTAKE} cr verify --task {self.task.c} -- python3 -I test_skill.py"
        self.assertIsNone(self.decide("writer", "Bash", command=verify))
        self.assertIsNone(self.decide("reviewer", "Bash", command=verify))
        self.assertIsNotNone(self.decide("sanitizer", "Bash", command=verify))
        self.assertIsNotNone(self.decide("writer", "Bash", command="cat /etc/passwd"))
        self.assertIsNotNone(self.decide("writer", "Bash", command=verify + " ; cat x"))
        self.assertIsNotNone(self.decide("writer", "Bash", command=f"python3 -I {guard.INTAKE} cr role clear"))

    def test_web_agents_mcp_denied(self):
        for role in guard.ROLES:
            self.assertIsNotNone(self.decide(role, "WebFetch", url="https://x.invalid"), role)
            self.assertIsNotNone(self.decide(role, "Agent", prompt="x"), role)
            self.assertIsNotNone(self.decide(role, "mcp__fs__read", path="/"), role)
        self.assertIsNone(self.decide("writer", "TodoWrite"))

    def test_hook_blocks_with_exit_2(self):
        r = self.run_intake("cr", "role", "set", "writer", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 0, r.stderr)
        event = {"tool_name": "Read", "agent_id": "a1", "cwd": "/",
                 "tool_input": {"file_path": str(self.src / "skills" / "manifest-tool" / "SKILL.md")}}
        old_stdin = sys.stdin
        sys.stdin = io.StringIO(json.dumps(event))
        try:
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()) as err:
                code = guard.main()
        finally:
            sys.stdin = old_stdin
        self.assertEqual(code, 2)
        self.assertIn("another side root", err.getvalue())
        self.assertEqual(self.run_intake("cr", "role", "clear").returncode, 0)
        self.assertFalse(guard.state_path().exists())

    def test_hook_config_names_intake_guard(self):
        r = self.run_intake("hook-config")
        self.assertEqual(r.returncode, 0, r.stderr)
        cmd = json.loads(r.stdout)["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
        self.assertEqual(cmd, f"python3 -I {guard.INTAKE} guard")

    def test_guard_is_refused_in_fleet_mode(self):
        env = {"SKILL_INTAKE_MODE": "fleet"}
        self.assertEqual(self.run_intake("hook-config", env=env).returncode, 10)
        r = self.run_intake("cr", "role", "set", "writer", "--task", str(self.task.c), env=env)
        self.assertEqual(r.returncode, 10)
        self.assertFalse(guard.state_path().exists())


class CliTests(Base):
    def test_status_and_validate(self):
        r = self.run_intake("cr", "status", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 0, r.stderr)
        st = json.loads(r.stdout)
        self.assertEqual(st["artifacts"]["preflight"], "ok")
        self.assertEqual(st["artifacts"]["source index"], "ok")
        self.assertIn("preflight              ok", r.stderr)
        pre = str(self.task.c / "contaminated" / "preflight.json")
        self.assertEqual(self.run_intake("cr", "validate", pre).returncode, 0)
        self.assertEqual(self.run_intake("cr", "validate", "--task", str(self.task.c), pre).returncode, 0)
        other = self.tmp / "preflight.json"
        write_json(other, read_json(Path(pre)))
        self.assertEqual(self.run_intake("cr", "validate", "--task", str(self.task.c), str(other)).returncode, 1)

    def test_status_reports_tampered_handoff(self):
        handoff.handoff(self.task, [str(self.draft(GOOD_SPEC))])
        (self.task.k / "handoff" / "spec-manifest-parse.json").write_text("{}\n")
        r = self.run_intake("cr", "status", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 1)
        self.assertIn("changed after handoff", json.loads(r.stdout)["handoff_integrity"][0])

    def test_scan_exit_codes(self):
        good = self.draft(GOOD_SPEC)
        bad = self.draft(dict(GOOD_SPEC, spec_id="spec-bad", summary="```code```"), "spec-bad.json")
        self.assertEqual(self.run_intake("cr", "scan", "--task", str(self.task.c), str(good)).returncode, 0)
        r = self.run_intake("cr", "scan", "--task", str(self.task.c), str(bad))
        self.assertEqual(r.returncode, 1)
        self.assertFalse(json.loads(r.stdout)["results"][0]["passed"])

    def test_scan_refuses_contaminated_paths(self):
        r = self.run_intake("cr", "scan", "--task", str(self.task.c), str(self.task.c / "skills.json"))
        self.assertEqual(r.returncode, 2)

    def test_handoff_and_context_cli(self):
        r = self.run_intake("cr", "handoff", "--task", str(self.task.c), str(self.draft(GOOD_SPEC)))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(json.loads(r.stdout)["handed_off"], ["spec-manifest-parse"])
        r = self.run_intake("cr", "context", "--task", str(self.task.c))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertTrue(Path(json.loads(r.stdout)["context"]).is_file())


if __name__ == "__main__":
    unittest.main()
