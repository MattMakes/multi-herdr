"""Minimal JSON Schema validator (subset of draft 2020-12) so we need no jsonschema package.

Ported from clean-room crlib/schema.py. The schemas live in
skills/skill-intake/schemas/ (ported and extended by ticket 08).

Supported: type, required, properties, additionalProperties (bool or schema), items,
enum, const, pattern, minLength, maxLength, minItems, minimum, maximum, format: date-time,
$ref to "#/$defs/<name>". Anything else in a schema raises, so an unsupported keyword is
never silently ignored.
"""

from __future__ import annotations

import re
from pathlib import Path

from .task import read_json

SCHEMA_DIR = Path(__file__).resolve().parent.parent.parent / "schemas"

KNOWN = {
    "$schema", "$id", "$defs", "title", "description", "type", "required", "properties",
    "additionalProperties", "items", "enum", "const", "pattern", "minLength", "maxLength",
    "minItems", "minimum", "maximum", "format", "$ref",
}
DATE_TIME = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$")
TYPES = {
    "object": lambda v: isinstance(v, dict),
    "array": lambda v: isinstance(v, list),
    "string": lambda v: isinstance(v, str),
    "integer": lambda v: isinstance(v, int) and not isinstance(v, bool),
    "number": lambda v: isinstance(v, (int, float)) and not isinstance(v, bool),
    "boolean": lambda v: isinstance(v, bool),
    "null": lambda v: v is None,
}


def kinds() -> list[str]:
    return sorted(p.name[: -len(".schema.json")] for p in SCHEMA_DIR.glob("*.schema.json"))


def load_schema(kind: str) -> dict:
    path = SCHEMA_DIR / f"{kind}.schema.json"
    if not path.is_file():
        raise ValueError(f"unknown artifact kind '{kind}'; known: {', '.join(kinds())}")
    return read_json(path)


def validate(instance, schema: dict) -> list[str]:
    errors: list[str] = []
    _check(instance, schema, schema, "$", errors)
    return errors


def _check(value, schema: dict, root: dict, at: str, errors: list[str]) -> None:
    unknown = set(schema) - KNOWN
    if unknown:
        raise ValueError(f"schema uses unsupported keywords at {at}: {sorted(unknown)}")

    if "$ref" in schema:
        ref = schema["$ref"]
        if not ref.startswith("#/$defs/"):
            raise ValueError(f"only local #/$defs refs are supported: {ref}")
        _check(value, root["$defs"][ref[len("#/$defs/"):]], root, at, errors)
        return

    if "type" in schema:
        allowed = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        if not any(TYPES[t](value) for t in allowed):
            errors.append(f"{at}: expected {'/'.join(allowed)}, got {type(value).__name__}")
            return

    if "const" in schema and value != schema["const"]:
        errors.append(f"{at}: must equal {schema['const']!r}")
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{at}: {value!r} not one of {schema['enum']}")

    if isinstance(value, str):
        if len(value) < schema.get("minLength", 0):
            errors.append(f"{at}: shorter than {schema['minLength']}")
        if "maxLength" in schema and len(value) > schema["maxLength"]:
            errors.append(f"{at}: longer than {schema['maxLength']}")
        if "pattern" in schema and not re.search(schema["pattern"], value):
            errors.append(f"{at}: does not match {schema['pattern']}")
        if schema.get("format") == "date-time" and not DATE_TIME.match(value):
            errors.append(f"{at}: not an ISO 8601 date-time")

    if TYPES["number"](value):
        if "minimum" in schema and value < schema["minimum"]:
            errors.append(f"{at}: below minimum {schema['minimum']}")
        if "maximum" in schema and value > schema["maximum"]:
            errors.append(f"{at}: above maximum {schema['maximum']}")

    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            errors.append(f"{at}: needs at least {schema['minItems']} item(s)")
        if "items" in schema:
            for i, item in enumerate(value):
                _check(item, schema["items"], root, f"{at}[{i}]", errors)

    if isinstance(value, dict):
        for key in schema.get("required", []):
            if key not in value:
                errors.append(f"{at}: missing required '{key}'")
        props = schema.get("properties", {})
        extra = schema.get("additionalProperties", True)
        for key, item in value.items():
            if key in props:
                _check(item, props[key], root, f"{at}.{key}", errors)
            elif extra is False:
                errors.append(f"{at}: unexpected property '{key}'")
            elif isinstance(extra, dict):
                _check(item, extra, root, f"{at}.{key}", errors)
