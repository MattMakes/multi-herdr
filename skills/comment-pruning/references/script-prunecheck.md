# Script: prunecheck

A small Python 3.10+ program (standard library only) that does the mechanical
part of the audit. It never edits a file. It has 3 subcommands:

| Subcommand | Input | Answers |
| --- | --- | --- |
| `scope [--base BRANCH]` | the current repository | which files differ from the base branch (default `main`), with uncommitted and untracked work included |
| `check [--scope PATH ...]` | a unified diff on standard input | did the pruning pass change anything other than comments? |
| `suppressions FILE ...` | files of the scope | where are lint, type-checker and formatter directives? |

Every subcommand prints JSON. Exit codes: `0` fine, `1` the diff is to be
rejected (`check` only), `2` wrong usage or a `git` failure.

`check` is a heuristic. It knows the comment markers of common languages by
file suffix. A line it cannot judge is reported as `unclear`, and the verdict
is then `reject`: read those lines yourself before you decide. A `clean`
verdict proves only the comment-only rule and the scope rule. Whether each
deleted comment deserved deletion stays your judgement.

Finding kinds of `check`:

| Kind | Meaning |
| --- | --- |
| `code-change` | a removed or added line that is not a comment |
| `comment-added` | a new comment line appeared (a rewrite, which is forbidden) |
| `out-of-scope` | the diff touches a file that the scope does not name |
| `protected-header` | a removed comment mentions a licence, copyright or SPDX tag |
| `formatter-directive` | a removed comment was a formatter on/off/skip directive |
| `file-added`, `file-removed`, `renamed`, `binary`, `mode-change` | the pass did more than edit comments |
| `unclear` | unknown file type, or a line the heuristic cannot classify |

## Use

Extract the first fenced block below to a temporary folder, then run it:

```sh
work="$(mktemp -d)"
awk '/^```python$/ && !done {on=1; next} /^```$/ && on {on=0; done=1} on' \
  references/script-prunecheck.md > "$work/prunecheck.py"
python3 "$work/prunecheck.py" scope --base main
git diff | python3 "$work/prunecheck.py" check --scope src/billing.py
python3 "$work/prunecheck.py" suppressions src/billing.py
```

## Program

```python
#!/usr/bin/env python3
"""prunecheck: mechanical checks around a comment-pruning pass."""

import argparse
import json
import os
import re
import subprocess
import sys

HASH = {"line": ["#"], "block": None, "quotes": "\"'"}
C_LIKE = {"line": ["//"], "block": ("/*", "*/"), "quotes": "\"'"}
C_BACKTICK = {"line": ["//"], "block": ("/*", "*/"), "quotes": "\"'`"}
RUST = {"line": ["//"], "block": ("/*", "*/"), "quotes": "\""}
DASHES = {"line": ["--"], "block": ("/*", "*/"), "quotes": "'\""}
LUA = {"line": ["--"], "block": None, "quotes": "\"'"}
PHP = {"line": ["//", "#"], "block": ("/*", "*/"), "quotes": "\"'"}
MARKUP = {"line": [], "block": ("<!--", "-->"), "quotes": ""}

SYNTAX_BY_SUFFIX = {}
for _suffixes, _syntax in (
    (".py .pyi .sh .bash .zsh .rb .pl .r .yml .yaml .toml .tf .ex .exs", HASH),
    (".c .h .cc .cpp .hpp .java .cs .swift .kt .kts .scala .dart .m", C_LIKE),
    (".js .jsx .mjs .cjs .ts .tsx .go", C_BACKTICK),
    (".rs", RUST),
    (".sql", DASHES),
    (".lua .hs", LUA),
    (".php", PHP),
    (".html .htm .xml .md .vue .svelte", MARKUP),
):
    for _suffix in _suffixes.split():
        SYNTAX_BY_SUFFIX[_suffix] = _syntax

DIRECTIVES = (
    ("formatter", re.compile(
        r"fmt:\s*(off|on|skip)|prettier-ignore|rustfmt::skip"
        r"|clang-format (off|on)|yapf:\s*disable")),
    ("type-checker", re.compile(
        r"type:\s*ignore|pyright:\s*ignore|mypy:\s*ignore"
        r"|@ts-ignore|@ts-expect-error|@ts-nocheck|\$FlowFixMe")),
    ("linter", re.compile(
        r"\bnoqa\b|pylint:\s*disable|eslint-disable|#!?\[(allow|expect)\("
        r"|\bnolint\b|\bNOLINT|@SuppressWarnings|#pragma warning disable"
        r"|rubocop:disable|shellcheck disable|\bnosec\b")),
)
ATTRIBUTE_FORM = re.compile(
    r"^(#!?\[(allow|expect)\(.*\)\]|#\[rustfmt::skip\]"
    r"|@SuppressWarnings\(.*\)|#pragma warning disable.*)$")
PROTECTED = re.compile(r"licen[cs]e|copyright|spdx", re.IGNORECASE)
HUNK = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def syntax_for(path):
    return SYNTAX_BY_SUFFIX.get(os.path.splitext(path)[1].lower())


def directive_kind(text):
    for kind, pattern in DIRECTIVES:
        if pattern.search(text):
            return kind
    return None


def split_trailing_comment(line, syntax):
    """Return (code, True) when a comment starts outside a string literal."""
    quote = None
    index = 0
    while index < len(line):
        char = line[index]
        if quote:
            if char == "\\":
                index += 2
                continue
            if char == quote:
                quote = None
        elif char in syntax["quotes"]:
            quote = char
        else:
            for marker in syntax["line"]:
                if line.startswith(marker, index):
                    return line[:index], True
            if syntax["block"]:
                opener, closer = syntax["block"]
                if line.startswith(opener, index):
                    rest = line[index + len(opener):].rstrip()
                    if rest.endswith(closer) and rest.count(closer) == 1:
                        return line[:index], True
        index += 1
    return line, False


def classify_whole_lines(lines, syntax):
    """Give each removed line a kind: comment, blank, suppression, code, unclear."""
    kinds = []
    inside_block = False
    opener, closer = syntax["block"] or (None, None)
    for text in lines:
        stripped = text.strip()
        if inside_block:
            if closer in stripped:
                inside_block = False
                tail = stripped.split(closer, 1)[1].strip()
                kinds.append("code" if tail else "comment")
            else:
                kinds.append("comment")
        elif not stripped:
            kinds.append("blank")
        elif ATTRIBUTE_FORM.match(stripped):
            kinds.append("suppression")
        elif "#" in syntax["line"] and stripped.startswith("#!"):
            kinds.append("code")
        elif any(stripped.startswith(marker) for marker in syntax["line"]):
            kinds.append("comment")
        elif opener and stripped.startswith(opener):
            rest = stripped[len(opener):]
            if closer in rest:
                tail = rest.split(closer, 1)[1].strip()
                kinds.append("code" if tail else "comment")
            else:
                inside_block = True
                kinds.append("comment")
        elif opener == "/*" and stripped.startswith("*"):
            kinds.append("unclear")
        else:
            kinds.append("code")
    return kinds


def clean_path(raw):
    raw = raw.split("\t", 1)[0].strip()
    if raw == "/dev/null":
        return None
    if raw.startswith(("a/", "b/")):
        return raw[2:]
    return raw


def read_diff(text):
    """Parse a unified diff into file entries that hold change groups."""
    entries = []
    current = None
    group = None
    old_left = new_left = old_number = 0

    def new_entry(path):
        entry = {"path": path, "notes": [], "groups": [], "headers": False}
        entries.append(entry)
        return entry

    for raw in text.splitlines():
        if old_left > 0 or new_left > 0:
            tag, body = raw[:1], raw[1:]
            if tag == "\\":
                continue
            if tag in "-+" and tag:
                if group is None:
                    group = {"removed": [], "added": []}
                    current["groups"].append(group)
                if tag == "-":
                    group["removed"].append((old_number, body))
                    old_number += 1
                    old_left -= 1
                else:
                    group["added"].append(body)
                    new_left -= 1
            else:
                group = None
                old_number += 1
                old_left -= 1
                new_left -= 1
            continue
        group = None
        if raw.startswith("diff --git "):
            current = new_entry(raw.rsplit(" b/", 1)[-1])
        elif raw.startswith("--- "):
            if current is None or current["headers"]:
                current = new_entry(None)
            current["headers"] = True
            old_path = clean_path(raw[4:])
            if old_path is None:
                current["notes"].append("file-added")
            elif current["path"] is None:
                current["path"] = old_path
        elif raw.startswith("+++ ") and current is not None:
            new_path = clean_path(raw[4:])
            if new_path is None:
                current["notes"].append("file-removed")
            else:
                current["path"] = new_path
        elif current is not None and raw.startswith(("rename from ", "rename to ")):
            if "renamed" not in current["notes"]:
                current["notes"].append("renamed")
        elif current is not None and raw.startswith("Binary files "):
            current["notes"].append("binary")
        elif current is not None and raw.startswith(("old mode ", "new mode ")):
            if "mode-change" not in current["notes"]:
                current["notes"].append("mode-change")
        else:
            match = HUNK.match(raw)
            if match and current is not None:
                old_number = int(match.group(1))
                old_left = int(match.group(2)) if match.group(2) is not None else 1
                new_left = int(match.group(4)) if match.group(4) is not None else 1
    return entries


def in_scope(path, scope):
    for allowed in scope:
        allowed = allowed.rstrip("/")
        if allowed.startswith("./"):
            allowed = allowed[2:]
        if path == allowed or path.startswith(allowed + "/"):
            return True
    return False


def judge_group(path, group, syntax, report):
    removed = group["removed"]

    def finding(kind, number, text):
        report["findings"].append(
            {"file": path, "kind": kind, "old_line": number, "text": text.strip()})

    if syntax is None:
        for number, text in removed:
            finding("unclear", number, text)
        for text in group["added"]:
            finding("unclear", None, text)
        return

    outcome = [None] * len(removed)
    for new_text in group["added"]:
        partner = None
        for index, (_, old_text) in enumerate(removed):
            if outcome[index] is not None:
                continue
            code, had_comment = split_trailing_comment(old_text, syntax)
            if had_comment and code.strip() and code.rstrip() == new_text.rstrip():
                partner = index
                break
        if partner is not None:
            outcome[partner] = "comment"
            continue
        whole = classify_whole_lines([new_text], syntax)[0]
        finding("comment-added" if whole == "comment" else "code-change", None, new_text)

    pending = [index for index, kind in enumerate(outcome) if kind is None]
    kinds = classify_whole_lines([removed[index][1] for index in pending], syntax)
    for index, kind in zip(pending, kinds):
        outcome[index] = kind

    run_open = False
    for (number, text), kind in zip(removed, outcome):
        if kind == "blank":
            report["blank_lines_removed"] += 1
            continue
        if kind in ("comment", "suppression"):
            directive = directive_kind(text)
            if directive == "formatter":
                finding("formatter-directive", number, text)
            elif directive or kind == "suppression":
                report["suppressions_removed"].append(
                    {"file": path, "old_line": number, "text": text.strip()})
            elif PROTECTED.search(text):
                finding("protected-header", number, text)
            report["comment_lines_removed"] += 1
            if not run_open:
                report["comment_groups_removed"] += 1
            run_open = True
            continue
        run_open = False
        finding("code-change" if kind == "code" else "unclear", number, text)


def check_diff(text, scope=None):
    report = {
        "verdict": "clean",
        "files": [],
        "comment_lines_removed": 0,
        "comment_groups_removed": 0,
        "blank_lines_removed": 0,
        "suppressions_removed": [],
        "findings": [],
    }
    for entry in read_diff(text):
        path = entry["path"] or "(unknown)"
        report["files"].append(path)
        for note in entry["notes"]:
            report["findings"].append(
                {"file": path, "kind": note, "old_line": None, "text": ""})
        if scope is not None and not in_scope(path, scope):
            report["findings"].append(
                {"file": path, "kind": "out-of-scope", "old_line": None, "text": ""})
        syntax = syntax_for(path)
        for group in entry["groups"]:
            judge_group(path, group, syntax, report)
    if report["findings"]:
        report["verdict"] = "reject"
    return report


def list_suppressions(paths):
    found = []
    for path in paths:
        with open(path, encoding="utf-8", errors="replace") as handle:
            for number, text in enumerate(handle, start=1):
                kind = directive_kind(text)
                if kind:
                    found.append(
                        {"file": path, "line": number, "kind": kind, "text": text.strip()})
    return found


def run_git(arguments, cwd):
    done = subprocess.run(
        ["git", *arguments], cwd=cwd, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        raise RuntimeError(done.stderr.strip() or "git " + " ".join(arguments) + " failed")
    return done.stdout


def find_scope(base="main", cwd=None):
    """Files that differ from the base branch, plus uncommitted and untracked ones."""
    fork_point = run_git(["merge-base", base, "HEAD"], cwd).strip()
    names = set(run_git(["diff", "--name-only", fork_point], cwd).splitlines())
    names.update(run_git(["ls-files", "--others", "--exclude-standard"], cwd).splitlines())
    names.discard("")
    return {"base": base, "merge_base": fork_point, "files": sorted(names)}


def main(argv=None):
    parser = argparse.ArgumentParser(prog="prunecheck", description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    scope_cmd = commands.add_parser("scope", help="list the files of the default scope")
    scope_cmd.add_argument("--base", default="main")
    check_cmd = commands.add_parser("check", help="judge a unified diff from stdin")
    check_cmd.add_argument("--scope", nargs="*", default=None)
    supp_cmd = commands.add_parser("suppressions", help="list directives in files")
    supp_cmd.add_argument("files", nargs="+")
    options = parser.parse_args(argv)

    try:
        if options.command == "scope":
            result, code = find_scope(options.base), 0
        elif options.command == "check":
            result = check_diff(sys.stdin.read(), options.scope)
            code = 0 if result["verdict"] == "clean" else 1
        else:
            result, code = list_suppressions(options.files), 0
    except (RuntimeError, OSError) as problem:
        print("prunecheck: " + str(problem), file=sys.stderr)
        return 2
    json.dump(result, sys.stdout, indent=2)
    sys.stdout.write("\n")
    return code


if __name__ == "__main__":
    sys.exit(main())
```

## Test

The test file imports `prunecheck.py` from its own folder. Run it from the
folder that holds the skill folder. The command below extracts both fenced
Python blocks of this file to a new temporary folder and runs the tests:

```sh
python3 -c 'import os,re,subprocess,sys,tempfile
text=open(sys.argv[1],encoding="utf-8").read()
blocks=re.findall(r"^`{3}python\n(.*?)^`{3}$",text,re.S|re.M)
folder=tempfile.mkdtemp()
for name,body in zip(("prunecheck.py","test_prunecheck.py"),blocks):
    open(os.path.join(folder,name),"w",encoding="utf-8").write(body)
sys.exit(subprocess.call([sys.executable,os.path.join(folder,"test_prunecheck.py")]))' \
  comment-pruning/references/script-prunecheck.md
```

```python
#!/usr/bin/env python3
"""Tests for prunecheck. Scenario ids (t1, t3, t4, t5) refer to scenarios.md."""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import prunecheck  # noqa: E402


def diff_for(path, hunk_lines, old_count, new_count):
    head = [
        "diff --git a/%s b/%s" % (path, path),
        "--- a/%s" % path,
        "+++ b/%s" % path,
        "@@ -1,%d +1,%d @@" % (old_count, new_count),
    ]
    return "\n".join(head + hunk_lines) + "\n"


class CheckDiff(unittest.TestCase):
    def test_t1_narration_and_dead_code_go_header_stays(self):
        text = diff_for("billing.py", [
            " # Licence: operator terms",
            " import decimal",
            "-# add up the invoice lines",
            " def total(lines):",
            "     return sum(lines)",
            "-# ==================",
            " def tax(amount):",
            "-    # rate = 0.2",
            "-    # old = amount * rate",
            "-    # return old",
            "     return amount",
        ], 11, 6)
        report = prunecheck.check_diff(text, ["billing.py"])
        self.assertEqual(report["verdict"], "clean")
        self.assertEqual(report["files"], ["billing.py"])
        self.assertEqual(report["comment_groups_removed"], 3)
        self.assertEqual(report["comment_lines_removed"], 5)
        self.assertEqual(report["findings"], [])

    def test_trailing_comment_removal_is_clean(self):
        text = diff_for("pay.py", [
            "-amount = cents(raw)  # convert it",
            "+amount = cents(raw)",
            " label = '# not a comment'",
        ], 2, 2)
        report = prunecheck.check_diff(text)
        self.assertEqual(report["verdict"], "clean")
        self.assertEqual(report["comment_lines_removed"], 1)

    def test_hash_inside_string_is_code(self):
        text = diff_for("pay.py", [
            "-label = '# old'",
            "+label = ''",
        ], 1, 1)
        report = prunecheck.check_diff(text)
        self.assertEqual(report["verdict"], "reject")
        kinds = {item["kind"] for item in report["findings"]}
        self.assertEqual(kinds, {"code-change"})

    def test_t4_renamed_function_is_rejected(self):
        text = diff_for("billing.py", [
            "-def total(lines):",
            "+def invoice_total(lines):",
            "     return sum(lines)",
        ], 2, 2)
        report = prunecheck.check_diff(text, ["billing.py"])
        self.assertEqual(report["verdict"], "reject")
        self.assertTrue(all(f["kind"] == "code-change" for f in report["findings"]))
        self.assertEqual(len(report["findings"]), 2)

    def test_rewritten_comment_is_rejected(self):
        text = diff_for("billing.py", [
            "-# this long story explains the hack below",
            "+# hack",
            " x = 1",
        ], 2, 2)
        report = prunecheck.check_diff(text)
        self.assertIn("comment-added", [f["kind"] for f in report["findings"]])

    def test_out_of_scope_file_is_rejected(self):
        text = diff_for("shared/dates.py", ["-# narration", " x = 1"], 2, 1)
        report = prunecheck.check_diff(text, ["billing.py"])
        self.assertEqual([f["kind"] for f in report["findings"]], ["out-of-scope"])

    def test_directory_scope_covers_its_files(self):
        text = diff_for("src/a.py", ["-# narration", " x = 1"], 2, 1)
        self.assertEqual(prunecheck.check_diff(text, ["src/"])["verdict"], "clean")

    def test_removed_licence_header_is_rejected(self):
        text = diff_for("billing.py", ["-# Copyright the operator", " x = 1"], 2, 1)
        report = prunecheck.check_diff(text)
        self.assertEqual([f["kind"] for f in report["findings"]], ["protected-header"])

    def test_removed_formatter_directive_is_rejected(self):
        text = diff_for("table.py", ["-# fmt: off", " x = [1,  2]"], 2, 1)
        report = prunecheck.check_diff(text)
        self.assertEqual([f["kind"] for f in report["findings"]], ["formatter-directive"])

    def test_t3_removed_type_suppression_is_recorded_not_rejected(self):
        text = diff_for("orders.py", [
            "-charge(customer)  # type: ignore[arg-type]",
            "+charge(customer)",
            " ship(order)  # noqa: E501",
        ], 2, 2)
        report = prunecheck.check_diff(text)
        self.assertEqual(report["verdict"], "clean")
        self.assertEqual(len(report["suppressions_removed"]), 1)

    def test_rust_allow_attribute_counts_as_suppression(self):
        text = diff_for("lib.rs", ["-#[allow(unused_must_use)]", " fn run() {}"], 2, 1)
        report = prunecheck.check_diff(text)
        self.assertEqual(report["verdict"], "clean")
        self.assertEqual(len(report["suppressions_removed"]), 1)

    def test_block_comment_removal_is_clean(self):
        text = diff_for("app.js", [
            "-/*",
            "- * walks the list",
            "- */",
            " function walk() {}",
            "-const a = 1; /* one */",
            "+const a = 1;",
        ], 5, 2)
        report = prunecheck.check_diff(text)
        self.assertEqual(report["verdict"], "clean")
        self.assertEqual(report["comment_groups_removed"], 2)

    def test_sql_comment_is_not_taken_for_a_file_header(self):
        text = diff_for("q.sql", ["--- old query", " select 1;"], 2, 1)
        report = prunecheck.check_diff(text)
        self.assertEqual(report["verdict"], "clean")
        self.assertEqual(report["files"], ["q.sql"])

    def test_unknown_suffix_is_unclear(self):
        text = diff_for("data.xyz", ["-# maybe", " x"], 2, 1)
        report = prunecheck.check_diff(text)
        self.assertEqual([f["kind"] for f in report["findings"]], ["unclear"])

    def test_new_file_is_rejected(self):
        text = "\n".join([
            "diff --git a/new.py b/new.py",
            "--- /dev/null",
            "+++ b/new.py",
            "@@ -0,0 +1,1 @@",
            "+x = 1",
        ]) + "\n"
        kinds = [f["kind"] for f in prunecheck.check_diff(text)["findings"]]
        self.assertIn("file-added", kinds)

    def test_empty_diff_is_clean(self):
        report = prunecheck.check_diff("")
        self.assertEqual(report["verdict"], "clean")
        self.assertEqual(report["files"], [])


class Suppressions(unittest.TestCase):
    def test_t3_lists_type_and_style_directives(self):
        folder = tempfile.mkdtemp()
        path = os.path.join(folder, "orders.py")
        with open(path, "w", encoding="utf-8") as handle:
            handle.write("charge(customer)  # type: ignore[arg-type]\n")
            handle.write("ship(order)  # noqa: E501\n")
            handle.write("# fmt: off\n")
            handle.write("plain = 1\n")
        found = prunecheck.list_suppressions([path])
        self.assertEqual([item["kind"] for item in found],
                         ["type-checker", "linter", "formatter"])
        self.assertEqual([item["line"] for item in found], [1, 2, 3])
        shutil.rmtree(folder)


@unittest.skipUnless(shutil.which("git"), "git is not installed")
class Scope(unittest.TestCase):
    def setUp(self):
        self.repo = tempfile.mkdtemp()
        self.env = dict(os.environ, GIT_CONFIG_GLOBAL=os.devnull,
                        GIT_CONFIG_NOSYSTEM="1")

    def tearDown(self):
        shutil.rmtree(self.repo, ignore_errors=True)

    def git(self, *arguments):
        subprocess.run(
            ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid",
             "-c", "commit.gpgsign=false", *arguments],
            cwd=self.repo, env=self.env, check=True, capture_output=True)

    def write(self, name, body):
        with open(os.path.join(self.repo, name), "w", encoding="utf-8") as handle:
            handle.write(body)

    def test_t5_default_scope_has_committed_and_uncommitted_files(self):
        self.git("init")
        self.git("symbolic-ref", "HEAD", "refs/heads/main")
        for name in ("a.py", "b.py", "c.py", "d.py"):
            self.write(name, "x = 1\n")
        self.git("add", ".")
        self.git("commit", "-m", "base")
        self.git("checkout", "-b", "feature")
        self.write("a.py", "x = 2\n")
        self.write("b.py", "x = 2\n")
        self.git("commit", "-am", "work")
        self.write("c.py", "x = 3\n")
        result = prunecheck.find_scope("main", self.repo)
        self.assertEqual(result["files"], ["a.py", "b.py", "c.py"])

    def test_untracked_file_is_in_scope(self):
        self.git("init")
        self.git("symbolic-ref", "HEAD", "refs/heads/main")
        self.write("a.py", "x = 1\n")
        self.git("add", ".")
        self.git("commit", "-m", "base")
        self.write("fresh.py", "y = 1\n")
        self.assertEqual(prunecheck.find_scope("main", self.repo)["files"], ["fresh.py"])

    def test_missing_base_branch_is_an_error(self):
        self.git("init")
        self.git("symbolic-ref", "HEAD", "refs/heads/trunk")
        self.write("a.py", "x = 1\n")
        self.git("add", ".")
        self.git("commit", "-m", "base")
        with self.assertRaises(RuntimeError):
            prunecheck.find_scope("main", self.repo)


if __name__ == "__main__":
    unittest.main()
```
