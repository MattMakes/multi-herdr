"""Tests of the comment ban (scripts/comments/comment_lint.py)."""

from __future__ import annotations

import contextlib
import io
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import comment_lint as cl


def found(name: str, src: str) -> list[str]:
    _, comments = cl.scan(Path(name), src)
    return [c.text for c in comments if not c.exempt]


def exempt(name: str, src: str) -> list[tuple[str, str]]:
    _, comments = cl.scan(Path(name), src)
    return [(c.exempt, c.text) for c in comments if c.exempt]


def stripped(name: str, src: str) -> str:
    _, comments = cl.scan(Path(name), src)
    return cl.strip_source(src, comments)


class Rust(unittest.TestCase):
    def test_line_doc_and_block_comments_are_found(self):
        src = "//! mod\n/// doc\nfn a() {} // tail\n/* block\n more */\n"
        self.assertEqual(
            found("a.rs", src), ["//! mod", "/// doc", "// tail", "/* block\n more */"]
        )

    def test_comment_markers_inside_literals_are_code(self):
        src = (
            'let a = "// not";\n'
            'let b = r#"/* not " */"#;\n'
            "let c = '\"'; // yes\n"
            "let d = b'/';\n"
            'let e = "\\"//";\n'
            "fn f<'a>(x: &'a str) {}\n"
        )
        self.assertEqual(found("a.rs", src), ["// yes"])

    def test_block_comments_nest(self):
        self.assertEqual(found("a.rs", "/* a /* b */ c */ fn x() {}\n"), ["/* a /* b */ c */"])

    def test_a_safety_note_is_exempt_with_its_following_lines(self):
        src = "// SAFETY: the buffer is zeroed\n// and of the right size.\nunsafe { go() }\n// plain\n"
        self.assertEqual(found("a.rs", src), ["// plain"])
        self.assertEqual([kind for kind, _ in exempt("a.rs", src)], ["directive", "directive"])

    def test_doc_comments_that_clap_prints_are_exempt(self):
        src = (
            "/// Top help.\n"
            "#[derive(Parser)]\n"
            "struct Cli {\n"
            "    /// Flag help.\n"
            "    #[arg(long)]\n"
            "    flag: bool,\n"
            "    // plain\n"
            "}\n"
            "/// Not help.\n"
            "struct Other;\n"
        )
        self.assertEqual(found("a.rs", src), ["// plain", "/// Not help."])
        self.assertEqual(
            exempt("a.rs", src), [("clap-help", "/// Top help."), ("clap-help", "/// Flag help.")]
        )

    def test_strip_deletes_whole_lines_and_trims_trailing_comments(self):
        src = "fn a() {\n    // gone\n    let x = 1; // gone too\n    x\n}\n"
        self.assertEqual(stripped("a.rs", src), "fn a() {\n    let x = 1;\n    x\n}\n")

    def test_strip_does_not_stack_blank_lines(self):
        src = "use a;\n\n// banner\n\nfn b() {}\n"
        self.assertEqual(stripped("a.rs", src), "use a;\n\nfn b() {}\n")

    def test_strip_keeps_exempt_comments(self):
        src = "// SAFETY: sound\nunsafe { go() } // why\n"
        self.assertEqual(stripped("a.rs", src), "// SAFETY: sound\nunsafe { go() }\n")


class Python(unittest.TestCase):
    def test_a_docstring_and_a_hash_in_a_string_are_not_comments(self):
        src = '"""Doc # not."""\nx = "# not"  # yes\ny = f"{x} # not"\n'
        self.assertEqual(found("a.py", src), ["# yes"])

    def test_shebang_and_tool_directives_are_exempt(self):
        src = "#!/usr/bin/env python3\nimport x  # noqa: E402\ny = 1  # type: ignore\n# plain\n"
        self.assertEqual(found("a.py", src), ["# plain"])

    def test_a_comment_after_a_directive_line_is_still_counted(self):
        src = "import x  # noqa: E402\n# plain\n"
        self.assertEqual(found("a.py", src), ["# plain"])

    def test_strip_leaves_the_code_identical(self):
        src = "def a():\n    # gone\n    return 1  # gone\n"
        self.assertEqual(stripped("a.py", src), "def a():\n    return 1\n")


class Shell(unittest.TestCase):
    def test_only_a_hash_that_starts_a_word_is_a_comment(self):
        src = 'n=$#\nlen=${#arr[@]}\necho "a # b" \'c # d\' e#f # yes\n'
        self.assertEqual(found("a.sh", src), ["# yes"])

    def test_a_heredoc_body_is_data(self):
        src = "cat <<'EOF'\n# not a comment\nEOF\n# yes\ncat <<<\"$x\" # also\n"
        self.assertEqual(found("a.sh", src), ["# yes", "# also"])

    def test_shellcheck_directive_and_shebang_are_exempt(self):
        src = "#!/usr/bin/env bash\n# shellcheck disable=SC2086\nx=$y\n"
        self.assertEqual(found("a.sh", src), [])

    def test_a_script_without_extension_is_found_by_its_shebang(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "tool"
            path.write_text("#!/usr/bin/env bash\n# plain\n")
            self.assertEqual(cl.language_of(path), "shell")
            src, comments = cl.scan(path)
            self.assertEqual(cl.comment_lines(src, comments), {2})


class JavaScript(unittest.TestCase):
    def test_strings_templates_and_regexes_are_code(self):
        src = (
            'const a = "// not";\n'
            "const b = `/* not ${c /* yes1 */} // not`;\n"
            "const r = /\\/\\/ not/g; // yes2\n"
            "const d = e / f; // yes3\n"
        )
        self.assertEqual(found("a.mjs", src), ["// yes2", "// yes3"])

    def test_checker_directives_are_exempt(self):
        self.assertEqual(found("a.ts", "// @ts-expect-error\nx();\n// eslint-disable-next-line\n"), [])


class CSharp(unittest.TestCase):
    def test_line_and_block_comments_are_found_outside_strings(self):
        src = (
            'var a = "// not"; // yes1\n'
            'var b = @"C:\\dir\\ ""/* not */"""; /* yes2 */\n'
            'var c = $"{d} // not";\n'
            'var e = """\n// not\n""";\n'
            "var f = '/'; /// yes3\n"
        )
        self.assertEqual(found("A.cs", src), ["// yes1", "/* yes2 */", "/// yes3"])

    def test_strip_keeps_the_code(self):
        src = "class A {\n    // gone\n    int b = 1; /* gone */\n}\n"
        self.assertEqual(stripped("A.cs", src), "class A {\n    int b = 1;\n}\n")


class Config(unittest.TestCase):
    def test_gdscript(self):
        src = '## doc\nvar a = "# not" # yes\n#region Folded\n'
        self.assertEqual(found("a.gd", src), ["## doc", "# yes"])

    def test_toml(self):
        src = "# top\nname = \"a # not\"\npath = 'b # not' # yes\n"
        self.assertEqual(found("Cargo.toml", src), ["# top", "# yes"])

    def test_yaml_block_scalars_are_data_and_pinned_versions_are_exempt(self):
        src = (
            "# top\n"
            "steps:\n"
            "  - uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0\n"
            "  - run: |\n"
            "      # shell text, not YAML\n"
            "      echo hi\n"
            "  - name: 'a # not' # yes\n"
        )
        self.assertEqual(found("ci.yml", src), ["# top", "# yes"])
        self.assertEqual(exempt("ci.yml", src), [("pinned-version", "# v4.4.0")])

    def test_just_keeps_the_line_that_just_list_prints(self):
        src = "# about this file\n\n# Long story.\n# Build it.\nbuild ARG='x':\n    cargo build # yes\n"
        self.assertEqual(found("justfile", src), ["# about this file", "# Long story.", "# yes"])


class Ratchet(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.baseline = self.root / "baseline.json"
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.write("a.rs", "// one\n// two\nfn a() {}\n")

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, rel: str, text: str) -> None:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        subprocess.run(["git", "-C", str(self.root), "add", rel], check=True)

    def run_lint(self, *args: str) -> tuple[int, str]:
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = cl.main(["--root", str(self.root), *args])
        return code, out.getvalue()

    def check(self, *args: str) -> tuple[int, str]:
        return self.run_lint("check", "--baseline", str(self.baseline), *args)

    def test_a_file_at_its_baseline_passes(self):
        self.baseline.write_text(json.dumps({"a.rs": 2}))
        self.assertEqual(self.check()[0], 0)

    def test_a_new_comment_fails_and_is_named(self):
        self.baseline.write_text(json.dumps({"a.rs": 1}))
        code, out = self.check()
        self.assertEqual(code, 1)
        self.assertIn("a.rs:1: // one", out)

    def test_a_file_without_a_baseline_may_hold_none(self):
        self.baseline.write_text("{}")
        self.assertEqual(self.check()[0], 1)

    def test_a_count_below_the_baseline_passes_and_asks_to_lower_it(self):
        self.baseline.write_text(json.dumps({"a.rs": 5}))
        code, out = self.check()
        self.assertEqual(code, 0)
        self.assertIn("--update-baseline", out)

    def test_update_lowers_and_never_raises(self):
        self.write("b.rs", "// new\n")
        self.baseline.write_text(json.dumps({"a.rs": 5, "gone.rs": 3}))
        code, _ = self.check("--update-baseline")
        self.assertEqual(code, 1)
        self.assertEqual(json.loads(self.baseline.read_text()), {"a.rs": 2})

    def test_the_first_baseline_records_todays_counts(self):
        self.assertEqual(self.check("--update-baseline")[0], 0)
        self.assertEqual(json.loads(self.baseline.read_text()), {"a.rs": 2})

    def test_copied_files_and_untracked_files_are_out_of_scope(self):
        self.write("skills/copied.json", json.dumps({"skills": [{"name": "s", "copied_files": ["x.py"]}]}))
        self.write("skills/s/x.py", "# copied\n")
        (self.root / "draft.rs").write_text("// untracked\n")
        self.baseline.write_text(json.dumps({"a.rs": 2}))
        self.assertEqual(self.check()[0], 0)

    def test_strip_then_check_passes_with_an_empty_baseline(self):
        self.run_lint("strip", "a.rs")
        self.assertEqual((self.root / "a.rs").read_text(), "fn a() {}\n")
        self.baseline.write_text("{}")
        self.assertEqual(self.check()[0], 0)

    def commit(self) -> None:
        git = ["git", "-C", str(self.root), "-c", "user.name=t", "-c", "user.email=t@example.invalid"]
        subprocess.run([*git, "commit", "-q", "-m", "base"], check=True)

    def test_verify_passes_when_only_comments_left(self):
        self.commit()
        self.run_lint("strip", "a.rs")
        code, out = self.run_lint("verify")
        self.assertEqual(code, 0, out)
        self.assertIn("1 changed files", out)

    def test_verify_fails_when_code_changed_too(self):
        self.commit()
        (self.root / "a.rs").write_text("fn b() {}\n")
        code, out = self.run_lint("verify")
        self.assertEqual(code, 1)
        self.assertIn("was: fn a() {}", out)

    def test_list_prints_one_json_line_per_comment(self):
        _, out = self.run_lint("list", "a.rs")
        rows = [json.loads(line) for line in out.splitlines()]
        self.assertEqual([(r["file"], r["line"], r["text"]) for r in rows], [("a.rs", 1, "// one"), ("a.rs", 2, "// two")])


class ThisRepository(unittest.TestCase):
    def test_the_lint_and_its_tests_hold_no_comments(self):
        here = Path(__file__).resolve()
        for path in (here, here.parents[1] / "comment_lint.py"):
            src, comments = cl.scan(path)
            self.assertEqual(cl.comment_lines(src, comments), set(), path)


if __name__ == "__main__":
    unittest.main()
