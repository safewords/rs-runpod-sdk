"""Rust naming and rustdoc rendering."""

from __future__ import annotations

import json
import re

RUST_KEYWORDS = set(
    "as break const continue crate else enum extern false fn for if impl in let loop match mod move mut pub ref "
    "return self Self static struct super trait true type unsafe use where while async await dyn abstract become "
    "box do final macro override priv typeof unsized virtual yield try gen".split()
)


def pascal(s: str) -> str:
    parts = re.split(r"[^A-Za-z0-9]+", s)
    out = ""
    for p in parts:
        if not p:
            continue
        # Split camelCase runs so "nextCursor" -> "NextCursor".
        out += p[0].upper() + p[1:]
    if not out:
        out = "Empty"
    if out[0].isdigit():
        out = "V" + out
    return out


def enum_variant(value: str) -> str:
    v = str(value)
    if re.fullmatch(r"[A-Z0-9_]+", v):  # SCREAMING_CASE
        name = "".join(w.capitalize() for w in v.lower().split("_") if w)
    else:
        name = pascal(v)
    if not name:
        name = "Empty"
    if name[0].isdigit():
        # Durations like "1h", "15m": Interval15m reads better than V15m.
        name = "Every" + name if re.fullmatch(r"\d+[smhdw]", v) else "V" + name
    return name


def snake(s: str) -> str:
    s = re.sub(r"[^A-Za-z0-9]+", "_", s)
    s = re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", s)
    s = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", s)
    s = s.lower().strip("_")
    if s and s[0].isdigit():
        s = "n" + s
    return s


def field_ident(s: str) -> str:
    n = snake(s)
    if n in RUST_KEYWORDS:
        return "r#" + n
    return n


def doc(text: str | None, indent: str = "") -> str:
    """Render an OpenAPI description as rustdoc. Keeps it from turning into
    doctests: bare code fences become `text`, and deep indentation (which
    markdown would read as an indented code block) is flattened."""
    if not text:
        return ""
    lines = text.strip("\n").split("\n")
    out = []
    in_fence = False
    for line in lines:
        stripped = line.lstrip()
        if stripped.startswith("```"):
            if not in_fence:
                lang = stripped[3:].strip()
                if lang in ("", "rust", "rs"):
                    line = "```text"
                else:
                    line = "```" + lang
                in_fence = True
            else:
                line = "```"
                in_fence = False
        elif not in_fence:
            lead = len(line) - len(stripped)
            line = " " * min(lead, 2) + stripped
        line = line.rstrip()
        out.append(f"{indent}///{(' ' + line) if line else ''}")
    if in_fence:
        out.append(f"{indent}/// ```")
    return "\n".join(out) + "\n"


def rust_str(s: str) -> str:
    return json.dumps(s)
