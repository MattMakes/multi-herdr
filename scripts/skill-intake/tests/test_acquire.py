"""`intake acquire` (spec 3.1-3.5, 3.8; T-01..T-03, F-archive, F-licence)."""

from __future__ import annotations

import hashlib
import json
import os
import socket
import unittest
from pathlib import Path

from base import IntakeTestCase

import fixtures
from intakelib import acquire
from intakelib.acquire import BadSource


class SourceFormsTest(IntakeTestCase):
    """T-01: every accepted form resolves; anything else is BadSource (exit 2)."""

    def test_github_forms_normalize(self):
        for text in ("https://github.com/o/r", "https://github.com/o/r.git", "git@github.com:o/r.git", "o/r"):
            s = acquire.parse(text)
            self.assertEqual((s.kind, s.url, s.ref, s.subdir), ("github", "https://github.com/o/r.git", None, None), text)

    def test_at_ref_and_ref_flag(self):
        self.assertEqual(acquire.parse("o/r@v1.2").ref, "v1.2")
        self.assertEqual(acquire.parse("git@github.com:o/r.git@main").ref, "main")
        self.assertEqual(acquire.parse("https://github.com/o/r", ref="feature/x").ref, "feature/x")
        with self.assertRaises(BadSource):
            acquire.parse("o/r@v1", ref="v2")

    def test_tree_url_gives_ref_and_subdir(self):
        s = acquire.parse("https://github.com/o/r/tree/main/skills/pdf")
        self.assertEqual((s.url, s.ref, s.subdir), ("https://github.com/o/r.git", "main", "skills/pdf"))

    def test_local_forms(self):
        d = self.tmp / "dir"
        d.mkdir()
        z = fixtures.write_zip(self.tmp / "a.skill", [("x/SKILL.md", b"x", None)])
        self.assertEqual(acquire.parse(str(d)).kind, "local")
        self.assertEqual(acquire.parse(str(z)).kind, "archive")

    def test_short_form_is_a_local_path_when_it_exists(self):
        (self.tmp / "work" / "o" / "r").mkdir(parents=True)
        self.assertEqual(acquire.parse("o/r").kind, "local")

    def test_bad_forms(self):
        f = self.tmp / "work" / "notes.txt"
        f.write_text("x")
        for text in ("", "a/b/c", "https://gitlab.com/o/r", "-o/r", "o/r@--upload-pack=x", "o/r@a..b",
                     "https://github.com/o/r/tree/main/../x", str(f), "./nope"):
            with self.assertRaises(BadSource, msg=text):
                acquire.parse(text)

    def test_cli_bad_form_exits_2_and_lists_forms(self):
        r = self.run_intake("acquire", "https://gitlab.com/o/r", "--base", str(self.tmp / "b"))
        self.assertEqual(r.returncode, 2, r.stderr)
        self.assertIn("accepted source forms", r.stderr)
        self.assertEqual(r.stdout, "")
        self.assertFalse((self.tmp / "b").exists())


class AcquireCase(IntakeTestCase):
    def acquire(self, *args: str, env: dict | None = None):
        r = self.run_intake("acquire", *args, "--base", str(self.tmp / "base"), env=env)
        out = json.loads(r.stdout) if r.stdout.strip() else {}
        return r, Path(out["task_dir"]) if "task_dir" in out else None

    def prov(self, c: Path) -> dict:
        return json.loads((c / "provenance.json").read_text())

    def findings(self, c: Path) -> list[dict]:
        text = (c / "sweep" / "acquire.jsonl").read_text()
        return [json.loads(line) for line in text.splitlines()]

    def src_files(self, c: Path) -> list[str]:
        q = c / "quarantine" / "src"
        return sorted(p.relative_to(q).as_posix() for p in q.rglob("*") if p.is_file())


class GitHubCloneTest(AcquireCase):
    """T-02: the clone runs against a local file:// repo through the test seam."""

    def setUp(self):
        super().setUp()
        self.sentinel = self.tmp / "hook-ran"
        self.gh = self.tmp / "gh"
        self.repo = fixtures.git_repo(self.gh / "o" / "r.git", {
            "skills/pdf/SKILL.md": fixtures.skill_md("pdf"), "LICENSE": fixtures.LICENCE_TEXTS["mit"],
        }, sentinel=self.sentinel)
        self.env = {acquire.BASE_ENV: f"file://{self.gh}"}
        # A global hooks folder that git would use on checkout: the clone's
        # core.hooksPath=/dev/null must win over it.
        hooks = self.tmp / "global-hooks"
        hooks.mkdir()
        (hooks / "post-checkout").write_text(f"#!/bin/sh\ntouch '{self.sentinel}'\n")
        (hooks / "post-checkout").chmod(0o755)
        (self.home / ".gitconfig").write_text(f"[core]\n\thooksPath = {hooks}\n")

    def test_clone_records_commit_removes_git_and_runs_no_hook(self):
        r, c = self.acquire("o/r", env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        head = fixtures.git(self.repo, "rev-parse", "HEAD")
        p = self.prov(c)
        self.assertEqual(p["source"]["commit"], head)
        self.assertEqual(p["source"]["url"], "https://github.com/o/r.git")
        self.assertEqual(p["licence"]["class"], "permissive")
        self.assertEqual(self.src_files(c), ["LICENSE", "skills/pdf/SKILL.md"])
        self.assertFalse((c / "quarantine" / "src" / ".git").exists())
        self.assertFalse((c / "quarantine" / ".clone").exists())
        self.assertFalse(self.sentinel.exists())

    def test_sha_ref_uses_shallow_fetch(self):
        first = fixtures.git(self.repo, "rev-parse", "HEAD")
        (self.repo / "later.md").write_text("later\n")
        fixtures.git(self.repo, "add", "-A")
        fixtures.git(self.repo, "commit", "-q", "-m", "later")
        r, c = self.acquire(f"o/r@{first}", env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.prov(c)["source"]["commit"], first)
        self.assertNotIn("later.md", self.src_files(c))
        self.assertFalse(self.sentinel.exists())

    def test_branch_ref_and_subdir(self):
        r, c = self.acquire("https://github.com/o/r/tree/main/skills", env=self.env)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.src_files(c), ["pdf/SKILL.md"])
        p = self.prov(c)
        self.assertEqual((p["source"]["ref"], p["source"]["subdir"]), ("main", "skills"))
        # The repo-root licence still counts for a subfolder.
        self.assertEqual(p["licence"]["class"], "permissive")

    def test_file_protocol_is_never_allowed_without_the_seam(self):
        cfg = acquire._git_config(False)
        self.assertIn("protocol.file.allow=never", cfg)
        for item in ("core.hooksPath=/dev/null", "core.symlinks=false", "submodule.recurse=false"):
            self.assertIn(item, cfg)

    def test_git_child_gets_a_minimal_env(self):
        os.environ["INTAKE_FIXTURE_TOKEN"] = "x"
        env = acquire._git_env()
        self.assertEqual(set(env) - {"PATH", "HOME", "LANG"},
                         {"GIT_LFS_SKIP_SMUDGE", "GIT_TERMINAL_PROMPT", "GIT_CONFIG_NOSYSTEM"})

    def test_git_child_keeps_only_the_sandbox_network_variables(self):
        allowed = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "http_proxy", "https_proxy",
                   "all_proxy", "no_proxy", "SSL_CERT_FILE", "SSL_CERT_DIR", "GIT_SSL_CAINFO"]
        saved = {k: os.environ.get(k) for k in [*allowed, "UNRELATED_VAR", "FOO_API_KEY"]}
        try:
            for k in allowed:
                os.environ[k] = "v-" + k
            os.environ["UNRELATED_VAR"] = "x"
            os.environ["FOO_API_KEY"] = "secret"
            env = acquire._git_env()
        finally:
            for k, v in saved.items():
                if v is None:
                    os.environ.pop(k, None)
                else:
                    os.environ[k] = v
        for k in allowed:
            self.assertEqual(env.get(k), "v-" + k)
        self.assertNotIn("UNRELATED_VAR", env)
        self.assertNotIn("FOO_API_KEY", env)

    def test_seam_must_be_file_url(self):
        r, c = self.acquire("o/r", env={acquire.BASE_ENV: "https://evil.invalid/"})
        self.assertEqual(r.returncode, 2)
        self.assertIsNone(c)


class FolderTest(AcquireCase):
    """T-03: a folder copies regular files only; symlinks and special files are recorded."""

    def test_symlinks_and_fifos_are_skipped_and_recorded(self):
        src = fixtures.build(self.tmp / "src", {"s/SKILL.md": fixtures.skill_md("s"), "s/a.md": "a"})
        os.symlink("/etc/hosts", src / "s" / "hosts")
        os.symlink(str(src / "s"), src / "loop")
        os.mkfifo(src / "s" / "pipe")
        sock = socket.socket(socket.AF_UNIX)
        self.addCleanup(sock.close)
        here = os.getcwd()
        os.chdir(src / "s")  # a relative bind: AF_UNIX paths are short on macOS
        sock.bind("sock")
        os.chdir(here)
        r, c = self.acquire(str(src))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.src_files(c), ["s/SKILL.md", "s/a.md"])
        rules = sorted((f["rule"], f["file"]) for f in self.findings(c))
        self.assertEqual(rules, [("ACQ-02", "loop"), ("ACQ-02", "s/hosts"), ("ACQ-03", "s/pipe"), ("ACQ-03", "s/sock")])
        self.assertEqual(self.prov(c)["source"]["kind"], "local")

    def test_quarantine_is_read_only_and_hash_matches(self):
        src = fixtures.build(self.tmp / "src", {"s/SKILL.md": fixtures.skill_md("s"), "s/b/c.txt": "c"})
        r, c = self.acquire(str(src))
        self.assertEqual(r.returncode, 0, r.stderr)
        q = c / "quarantine" / "src"
        for p in [q, *q.rglob("*")]:
            self.assertEqual(p.stat().st_mode & 0o222, 0, str(p))
        lines = sorted(
            f"{rel}\0{hashlib.sha256((src / rel).read_bytes()).hexdigest()}\n" for rel in ("s/SKILL.md", "s/b/c.txt"))
        self.assertEqual(self.prov(c)["content_sha256"], hashlib.sha256("".join(lines).encode()).hexdigest())
        self.assertIsNone(self.prov(c)["archive_sha256"])

    def test_caps_refuse_with_acq04_and_leave_nothing(self):
        src = fixtures.build(self.tmp / "src", {"s/SKILL.md": fixtures.skill_md("s"), "s/a/b/c.md": "x" * 100})
        for flag, value in (("--max-files", "1"), ("--max-file-bytes", "50"), ("--max-total-bytes", "120"),
                            ("--max-depth", "3")):
            r, c = self.acquire(str(src), flag, value)
            self.assertEqual(r.returncode, 3, f"{flag}: {r.stderr}")
            self.assertEqual(self.findings(c)[0]["rule"], "ACQ-04", flag)
            self.assertEqual(self.src_files(c), [], flag)

    def test_source_inside_base_is_refused(self):
        base = self.tmp / "base"
        src = fixtures.build(base / "src", {"s/SKILL.md": fixtures.skill_md("s")})
        r = self.run_intake("acquire", str(src), "--base", str(base))
        self.assertEqual(r.returncode, 2, r.stderr)


class ArchiveTest(AcquireCase):
    """T-03 / F-archive: refuse the whole archive; a valid .skill extracts."""

    def test_hostile_archives_are_refused_whole(self):
        arch = fixtures.f_archive(self.tmp / "arch")
        for kind in ("dotdot", "symlink", "bomb"):
            r, c = self.acquire(str(arch[kind]))
            self.assertEqual(r.returncode, 3, f"{kind}: {r.stderr}")
            self.assertEqual(self.findings(c)[0]["rule"], "ACQ-01", kind)
            self.assertEqual(self.src_files(c), [], kind)
            self.assertEqual(json.loads(r.stdout)["refused"], "ACQ-01")
        self.assertFalse((self.tmp / "x").exists())

    def test_absolute_and_drive_letter_entries_are_refused(self):
        for name in ("/abs/x", "C:/x", "a\\..\\x"):
            z = fixtures.write_zip(self.tmp / "z.zip", [(name, b"x", None)])
            with self.assertRaises(acquire.Refused, msg=name) as cm:
                acquire.extract(z, self.tmp / "out", acquire.Caps())
            self.assertEqual(cm.exception.rule, "ACQ-01")

    def test_duplicate_entries_are_refused(self):
        z = fixtures.write_zip(self.tmp / "d.zip", [("s/a", b"1", None), ("s/a", b"2", None)])
        r, c = self.acquire(str(z))
        self.assertEqual(r.returncode, 3, r.stderr)
        self.assertEqual(self.src_files(c), [])

    def test_skill_at_the_archive_root_is_moved_into_a_folder(self):
        z = fixtures.write_zip(self.tmp / "Root_Skill.skill", [("SKILL.md", fixtures.skill_md("x").encode(), None)])
        r, c = self.acquire(str(z))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.src_files(c), ["root-skill/SKILL.md"])

    def test_refused_run_writes_no_provenance(self):
        arch = fixtures.f_archive(self.tmp / "arch")
        r, c = self.acquire(str(arch["dotdot"]))
        self.assertEqual(r.returncode, 3, r.stderr)
        self.assertFalse((c / "provenance.json").exists())

    def test_total_size_over_cap_is_refused(self):
        z = fixtures.write_zip(self.tmp / "z.zip", [("a", b"a" * 100, None), ("b", b"b" * 100, None)])
        r, c = self.acquire(str(z), "--max-total-bytes", "150")
        self.assertEqual(r.returncode, 3, r.stderr)
        self.assertEqual(self.src_files(c), [])

    def test_not_a_zip_is_refused(self):
        z = self.tmp / "fake.zip"
        z.write_bytes(b"not a zip")
        r, _ = self.acquire(str(z))
        self.assertEqual(r.returncode, 3, r.stderr)

    def test_valid_skill_extracts(self):
        arch = fixtures.f_archive(self.tmp / "arch")
        r, c = self.acquire(str(arch["valid"]))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(self.src_files(c), ["zip-skill/SKILL.md", "zip-skill/references/notes.md"])
        p = self.prov(c)
        self.assertEqual(p["source"]["kind"], "archive")
        self.assertEqual(p["archive_sha256"], hashlib.sha256(arch["valid"].read_bytes()).hexdigest())
        self.assertRegex(p["content_sha256"], r"^[0-9a-f]{64}$")


class LicenceTest(AcquireCase):
    """F-licence: none -> flag, MIT -> permissive, GPL-3.0 -> copyleft + flag, no-RE -> restrictive."""

    def test_classes(self):
        want = {
            "none": ("none", "no-licence", None),
            "mit": ("permissive", None, "MIT"),
            "gpl3": ("copyleft", "copyleft", "GPL-3.0"),
            "no-re": ("restrictive", "restrictive", None),
        }
        for kind, (cls, flag, spdx) in want.items():
            src = fixtures.f_licence(self.tmp / f"lic-{kind}", kind)
            r, c = self.acquire(str(src))
            self.assertEqual(r.returncode, 0, r.stderr)
            lic = self.prov(c)["licence"]
            self.assertEqual((lic["class"], lic["flag"], lic["spdx"]), (cls, flag, spdx), kind)
            self.assertEqual(lic["files"], [] if kind == "none" else ["LICENSE"], kind)

    def test_manifest_and_frontmatter_fields(self):
        from intakelib import licensing

        src = fixtures.build(self.tmp / "m", {
            "package.json": json.dumps({"license": "Apache-2.0"}),
            "s/SKILL.md": "---\nname: s\nlicense: AGPL-3.0\n---\n",
        })
        found = licensing.detect(src, [src, src / "s"])
        self.assertEqual((found["class"], found["spdx"]), ("copyleft", "AGPL-3.0"))
        self.assertEqual(found["files"], ["package.json", "s/SKILL.md"])
        self.assertEqual(licensing.classify_spdx("CC-BY-ND-4.0"), "restrictive")
        self.assertEqual(licensing.classify_text("Copyright (c) X. All rights reserved.\nRedistribution and use "
                                               "in source and binary forms, with or without modification"),
                         ("permissive", "BSD"))


if __name__ == "__main__":
    unittest.main()
