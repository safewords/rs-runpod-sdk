#!/usr/bin/env python3
"""Generate the Runpod v2 types and operations from the OpenAPI snapshot.

    python3 codegen/generate.py            # writes src/api/
    python3 codegen/generate.py --check    # fails if src/api/ is stale

Input is `openapi/runpod-v2.json`, a verbatim copy of
`https://api.runpod.io/v2/openapi.json`. To pick up API changes, replace that
file, run this, then `cargo test --all-targets`. Needs `rustfmt` on PATH and
nothing outside the standard library.

The work is split across `runpod_codegen/`:

* `spec.py` loads the document and walks `$ref`, `allOf` and parameters.
* `naming.py` turns OpenAPI names into Rust ones and descriptions into rustdoc.
* `schemas.py` turns each schema into a type. `allOf` is flattened into one
  struct, a discriminated `oneOf` becomes an internally tagged enum, any other
  `oneOf` an untagged one, and every string enum gets an `Other(String)`
  fallback so a value Runpod adds later does not break decoding.
* `operations.py` turns each operation into an `async fn` on `Client`;
  paginated lists also get an `*_all` variant that follows `nextCursor`. The
  two Server-Sent Events operations are hand-written in `src/logs/`.
* `layout.py` puts each type in the API area that uses it, or `common`.
* `examples.py` writes `tests/spec_examples.rs`, which round-trips every
  example in the spec through its type.
"""

from __future__ import annotations

import shutil
import sys

from runpod_codegen import examples
from runpod_codegen.layout import API_DIR, render, rustfmt
from runpod_codegen.operations import gen_ops
from runpod_codegen.schemas import Gen
from runpod_codegen.spec import ROOT, SCHEMAS


def main() -> None:
    check = "--check" in sys.argv
    g = Gen()
    ops = gen_ops(g)
    for name in SCHEMAS:
        g.schema(name)
    g.finish_defaults()
    files = render(g.items, ops)
    examples_files = {p: rustfmt(src) for p, src in examples.render(ops).items()}

    if check:
        existing = {p for p in API_DIR.rglob("*.rs")}
        stale = [p for p, s in (files | examples_files).items() if not p.exists() or p.read_text() != s]
        stale += sorted(existing - set(files))
        if stale:
            print("stale:", *(p.relative_to(ROOT) for p in stale), sep="\n  ")
            sys.exit(1)
        return

    if API_DIR.exists():
        shutil.rmtree(API_DIR)
    for path, src in (files | examples_files).items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(src)
    (ROOT / "codegen" / "operations.txt").write_text(
        "".join(f"{op.method} {op.path} {op.operation_id}\n" for op in ops)
    )
    print(f"{len(g.items)} types, {len(ops)} operations, {len(files)} files")


if __name__ == "__main__":
    main()
