"""The OpenAPI document and helpers for walking it."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
SPEC = ROOT / "openapi" / "runpod-v2.json"

METHODS = ("get", "post", "put", "patch", "delete")
# Server-Sent Events: written by hand in src/logs/.
SSE_OPS = {"getPodLogs", "getWorkerLogs"}

spec = json.loads(SPEC.read_text())
SCHEMAS: dict = spec["components"]["schemas"]
PARAMS: dict = spec["components"].get("parameters", {})
RESPONSES: dict = spec["components"].get("responses", {})


def resolve_ref(ref: str) -> tuple[str, dict]:
    name = ref.split("/")[-1]
    return name, SCHEMAS[name]


def deref(s: dict) -> dict:
    while "$ref" in s:
        _, s = resolve_ref(s["$ref"])
    return s


def is_null(s: dict) -> bool:
    return s.get("type") == "null"


def type_list(s: dict) -> list[str]:
    t = s.get("type")
    if t is None:
        return []
    return t if isinstance(t, list) else [t]


def is_object_like(s: dict) -> bool:
    s = deref(s)
    if "allOf" in s:
        return all(is_object_like(p) for p in s["allOf"] if not set(p) <= {"if", "then", "else", "not"})
    return "properties" in s or "object" in type_list(s)


def flatten(s: dict) -> tuple[dict, set, list]:
    """Merge an object schema and its allOf parts.
    Returns (properties, required, descriptions)."""
    props: dict = {}
    required: set = set()
    descs: list = []
    if "$ref" in s:
        name, target = resolve_ref(s["$ref"])
        return flatten(target)
    for part in s.get("allOf", []):
        if set(part) <= {"if", "then", "else", "not"}:
            continue
        p, r, _ = flatten(part)
        props.update(p)
        required |= r
    props.update(s.get("properties", {}))
    required |= set(s.get("required", []))
    return props, required, descs


def param(p: dict) -> dict:
    if "$ref" in p:
        return PARAMS[p["$ref"].split("/")[-1]]
    return p


def response(r: dict) -> dict:
    if "$ref" in r:
        return RESPONSES[r["$ref"].split("/")[-1]]
    return r
