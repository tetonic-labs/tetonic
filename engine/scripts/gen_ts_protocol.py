#!/usr/bin/env python3
"""Generate the editor's TypeScript protocol types from the daemon's schema.

The Rust types in `lokai-rpc` are the single source of truth. `lokaid
--print-schema` dumps them as a JSON Schema bundle; this script turns that
bundle into a `protocol.ts` the editor's JSON-RPC client imports. Because both
sides are derived from the same Rust definitions, they cannot drift.

Usage (from the `engine/` directory):

    cargo run -q -p lokaid -- --print-schema | python scripts/gen_ts_protocol.py
    # or, against an already-dumped file:
    python scripts/gen_ts_protocol.py --in schema.json --out clients/ts/protocol.ts

With no --out, writes to engine/clients/ts/protocol.ts (the home for the
future editor client). The output is deterministic, so re-running it produces a
clean diff or none at all; CI can fail if the checked-in file is stale.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

SCALARS = {
    "string": "string",
    "integer": "number",
    "number": "number",
    "boolean": "boolean",
    "null": "null",
}


def ref_name(schema: dict) -> str:
    return schema["$ref"].rsplit("/", 1)[-1]


def needs_parens(t: str) -> bool:
    return "|" in t


def ts_type(schema) -> tuple[str, bool]:
    """Map a JSON-Schema node to (typescript_type, nullable)."""
    # serde_json::Value renders as the boolean schema `true` (anything).
    if schema is True or schema == {}:
        return "unknown", False
    if schema is False:
        return "never", False
    if not isinstance(schema, dict):
        return "unknown", False

    if "$ref" in schema:
        return ref_name(schema), False

    if "enum" in schema:
        return " | ".join(json.dumps(v) for v in schema["enum"]), False

    for combinator in ("anyOf", "oneOf", "allOf"):
        if combinator in schema:
            parts: list[str] = []
            nullable = False
            for sub in schema[combinator]:
                if isinstance(sub, dict) and sub.get("type") == "null":
                    nullable = True
                    continue
                t, n = ts_type(sub)
                nullable = nullable or n
                parts.append(t)
            uniq = list(dict.fromkeys(parts)) or ["unknown"]
            return " | ".join(uniq), nullable

    t = schema.get("type")
    if isinstance(t, list):
        parts = []
        nullable = False
        for member in t:
            if member == "null":
                nullable = True
            else:
                parts.append(SCALARS.get(member, "unknown"))
        uniq = list(dict.fromkeys(parts)) or ["unknown"]
        return " | ".join(uniq), nullable

    if t == "array":
        inner, _ = ts_type(schema.get("items", True))
        if needs_parens(inner):
            inner = f"({inner})"
        return f"{inner}[]", False

    if t == "object":
        # Nested inline objects are rare in this protocol; map open maps to a
        # Record and anything else to a safe `unknown`.
        if "additionalProperties" in schema and "properties" not in schema:
            value, _ = ts_type(schema["additionalProperties"])
            return f"Record<string, {value}>", False
        return "Record<string, unknown>", False

    if isinstance(t, str):
        return SCALARS.get(t, "unknown"), False

    return "unknown", False


def jsdoc(description: str | None, indent: str = "") -> list[str]:
    if not description:
        return []
    text = description.replace("*/", "*\u200b/").replace("\n", " ")
    return [f"{indent}/** {text} */"]


def emit_type(name: str, schema: dict) -> str:
    # String enum -> union alias.
    if "enum" in schema and "properties" not in schema:
        union, _ = ts_type(schema)
        lines = jsdoc(schema.get("description"))
        lines.append(f"export type {name} = {union};")
        return "\n".join(lines)

    if schema.get("type") == "object" or "properties" in schema:
        props: dict = schema.get("properties", {})
        required = set(schema.get("required", []))
        lines = jsdoc(schema.get("description"))
        lines.append(f"export interface {name} {{")
        for prop_name in props:  # schemars emits keys already sorted
            prop = props[prop_name]
            t, nullable = ts_type(prop)
            if nullable and "null" not in t.split(" | "):
                t = f"{t} | null"
            optional = "?" if prop_name not in required else ""
            lines.extend(jsdoc(prop.get("description") if isinstance(prop, dict) else None, "  "))
            lines.append(f"  {prop_name}{optional}: {t};")
        lines.append("}")
        return "\n".join(lines)

    # Fallback: alias whatever it resolves to.
    alias, _ = ts_type(schema)
    return f"export type {name} = {alias};"


def emit_const_object(name: str, entries: dict, value_is_string: bool) -> str:
    lines = [f"export const {name} = {{"]
    for key in entries:
        v = json.dumps(entries[key]) if value_is_string else entries[key]
        lines.append(f"  {key}: {v},")
    lines.append("} as const;")
    return "\n".join(lines)


def generate(bundle: dict) -> str:
    out: list[str] = []
    out.append("// Code generated from `lokaid --print-schema`. DO NOT EDIT.")
    out.append("// Source of truth: engine/crates/lokai-rpc (Rust). Regenerate with")
    out.append("// `cargo run -q -p lokaid -- --print-schema | python scripts/gen_ts_protocol.py`.")
    out.append("")
    out.append(f"export const PROTOCOL_VERSION = {bundle.get('protocol_version', 1)} as const;")
    out.append("")

    definitions: dict = bundle.get("definitions", {})
    for name in sorted(definitions):
        out.append(emit_type(name, definitions[name]))
        out.append("")

    if "methods" in bundle:
        out.append(emit_const_object("Methods", bundle["methods"], value_is_string=True))
        out.append("")
    if "events" in bundle:
        out.append(emit_const_object("Events", bundle["events"], value_is_string=True))
        out.append("")
    if "error_codes" in bundle:
        out.append(emit_const_object("ErrorCodes", bundle["error_codes"], value_is_string=False))
        out.append("")

    return "\n".join(out).rstrip() + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--in", dest="infile", help="schema JSON file (default: stdin)")
    parser.add_argument("--out", dest="outfile", help="output .ts file (default: engine/clients/ts/protocol.ts)")
    parser.add_argument("--check", action="store_true", help="exit non-zero if the output file is missing or stale")
    args = parser.parse_args()

    if args.infile:
        raw = Path(args.infile).read_text(encoding="utf-8")
    else:
        raw = sys.stdin.read()
    bundle = json.loads(raw)
    ts = generate(bundle)

    if args.outfile:
        out_path = Path(args.outfile)
    else:
        out_path = Path(__file__).resolve().parent.parent / "clients" / "ts" / "protocol.ts"

    if args.check:
        current = out_path.read_text(encoding="utf-8") if out_path.exists() else ""
        if current != ts:
            sys.stderr.write(f"stale: {out_path} is out of date; regenerate with gen_ts_protocol.py\n")
            return 1
        return 0

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(ts, encoding="utf-8", newline="\n")
    sys.stderr.write(f"wrote {out_path} ({len(bundle.get('definitions', {}))} types)\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
