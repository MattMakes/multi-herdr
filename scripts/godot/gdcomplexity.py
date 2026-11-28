import argparse
import json
import sys
from pathlib import Path

from gdtoolkit.parser import parser
from lark import Token, Tree

DEFAULTS = {"cyclomaticComplexity": 10, "parameterCount": 7}

BRANCHES = {
    "if_branch": 1,
    "elif_branch": 1,
    "for_stmt": 1,
    "for_stmt_typed": 1,
    "while_stmt": 1,
    "match_branch": 1,
    "guarded_match_branch": 2,
    "pattern_guard": 1,
    "and_test": 1,
    "or_test": 1,
    "asless_and_test": 1,
    "asless_or_test": 1,
    "test_expr": 1,
    "asless_test_expr": 1,
    "ternary_expr": 1,
    "asless_ternary_expr": 1,
}

PARAMETERS = {"func_arg_regular", "func_arg_inf", "func_arg_typed"}


def load_config(path):
    text = Path(path).read_text()
    if path.endswith((".yaml", ".yml")) or Path(path).name in (
        ".static-analysisrc",
        "static-analysisrc",
    ):
        try:
            data = json.loads(text)
        except ValueError:
            import yaml

            data = yaml.safe_load(text)
    elif path.endswith(".json"):
        data = json.loads(text)
    else:
        return {}
    if Path(path).name == "package.json":
        data = data.get("static-analysis", {})
    return data or {}


def thresholds(config_path):
    limits = dict(DEFAULTS)
    if not config_path:
        return limits
    configured = load_config(config_path).get("thresholds", {})
    for metric in DEFAULTS:
        entry = configured.get(metric)
        if isinstance(entry, dict):
            value = entry.get("warn", entry.get("error"))
            if value is not None:
                limits[metric] = value
        elif isinstance(entry, (int, float)):
            limits[metric] = entry
    return limits


def end_line(node):
    last = node.meta.line
    for sub in node.iter_subtrees():
        if not any(isinstance(c, Tree) for c in sub.children) and not sub.meta.empty:
            last = max(last, sub.meta.end_line)
    for tok in node.scan_values(lambda v: isinstance(v, Token)):
        if tok.end_line is not None:
            last = max(last, tok.end_line)
    return last


def complexity(func):
    return 1 + sum(BRANCHES.get(sub.data, 0) for sub in func.iter_subtrees())


def parameter_count(func):
    header = func.children[0]
    args = next(c for c in header.children if isinstance(c, Tree) and c.data == "func_args")
    return sum(
        1
        for c in args.children
        if isinstance(c, Tree)
        and (c.data in PARAMETERS or c.data == "func_arg_variadic")
    )


def functions(tree, prefix=""):
    for child in tree.children:
        if not isinstance(child, Tree):
            continue
        if child.data == "static_func_def":
            child = child.children[0]
        if child.data == "func_def":
            yield prefix + str(child.children[0].children[0]), child
        elif child.data == "class_def":
            name = str(child.children[0])
            yield from functions(child, prefix + name + ".")


def has_content_after(lines, line):
    return any(
        text.strip() and not text.strip().startswith("#") for text in lines[line:]
    )


def analyse(path, limits):
    source = Path(path).read_text()
    tree = parser.parse(source, gather_metadata=True)
    lines = source.splitlines()
    found = []
    for name, func in functions(tree):
        found.append((name, func.meta.line, end_line(func), func))
    starts = {line for _, line, _, _ in found}
    entries = []
    violations = []
    for name, line, end, func in sorted(found, key=lambda f: f[1]):
        entries.append({"name": name, "line": line, "endLine": end})
        if end + 1 not in starts and has_content_after(lines, end):
            entries.append({"name": "", "line": end + 1, "endLine": end + 1})
        scores = {
            "cyclomaticComplexity": complexity(func),
            "parameterCount": parameter_count(func),
        }
        for metric, value in scores.items():
            if value > limits[metric]:
                violations.append(
                    {
                        "functionName": name,
                        "filePath": path,
                        "line": line,
                        "metric": metric,
                        "value": value,
                        "threshold": limits[metric],
                        "severity": "warn",
                    }
                )
    return {"filePath": path, "functions": entries}, violations


def main(argv):
    args = argparse.ArgumentParser(prog="gdcomplexity.py")
    args.add_argument("--format", choices=["json"], default="json")
    args.add_argument("--config")
    args.add_argument("--threshold-action", default="none")
    args.add_argument("files", nargs="*")
    opts = args.parse_args(argv)
    limits = thresholds(opts.config)
    report = {"files": [], "violations": []}
    for path in opts.files:
        entry, violations = analyse(path, limits)
        report["files"].append(entry)
        report["violations"].extend(violations)
    json.dump(report, sys.stdout)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
