"""Operations to `impl Client` methods."""

from __future__ import annotations

import re
from dataclasses import dataclass

from .naming import doc, field_ident, pascal, rust_str, snake
from .schemas import Gen
from .spec import METHODS, SCHEMAS, SSE_OPS, deref, flatten, param, response, spec, type_list


@dataclass
class Operation:
    tag: str
    method: str
    path: str
    operation_id: str
    source: str
    # Rust type of the JSON request body, and of each JSON 2xx answer by
    # status, for the generated spec-example tests.
    request_type: str | None
    response_types: dict[str, str]


def gen_ops(g: Gen) -> list[Operation]:
    out = []
    for path, item in spec["paths"].items():
        path_params = [param(p) for p in item.get("parameters", [])]
        for method in METHODS:
            op = item.get(method)
            if not op or op["operationId"] in SSE_OPS:
                continue
            source, request_type, response_types = gen_op(g, path, method, op, path_params)
            out.append(
                Operation(
                    tag=op["tags"][0],
                    method=method.upper(),
                    path=path,
                    operation_id=op["operationId"],
                    source=source,
                    request_type=request_type,
                    response_types=response_types,
                )
            )
    return out


def gen_op(g: Gen, path: str, method: str, op: dict, path_params: list) -> tuple[str, str | None, dict[str, str]]:
    opid = op["operationId"]
    fname = snake(opid)
    params = path_params + [param(p) for p in op.get("parameters", [])]
    by_name = {}
    for p in params:
        by_name[(p["in"], p["name"])] = p
    params = list(by_name.values())
    pparams = [p for p in params if p["in"] == "path"]
    order = re.findall(r"\{(\w+)\}", path)
    pparams.sort(key=lambda p: order.index(p["name"]))
    others = [p for p in params if p["in"] in ("query", "header")]
    req_others = [p for p in others if p.get("required")]
    opt_others = [p for p in others if not p.get("required")]

    args = ["&self"]
    pre = []
    fmt_path = path
    for p in pparams:
        ident = field_ident(p["name"])
        sch = p.get("schema", {})
        if "$ref" in sch and "enum" in deref(sch):
            tname = sch["$ref"].split("/")[-1]
            g.schema(tname)
            args.append(f"{ident}: &{tname}")
            pre.append(f"let {snake(p['name'])}_seg = encode_segment({ident}.as_str());")
        else:
            args.append(f"{ident}: &str")
            pre.append(f"let {snake(p['name'])}_seg = encode_segment({ident});")
        fmt_path = fmt_path.replace("{" + p["name"] + "}", "{" + snake(p["name"]) + "_seg}")
    # Base URL already ends at the host; spec paths start with /v2.
    query_lines = []
    header_lines = []

    def emit_param(p, expr_owner: str, is_opt: bool):
        name = p["name"]
        ident = field_ident(name)
        access = f"{expr_owner}{ident}" if expr_owner else ident
        sch = p.get("schema")
        target = header_lines if p["in"] == "header" else query_lines
        if "content" in p:  # JSON-encoded query parameter
            conv = "serde_json::to_string(v).map_err(|source| Error::Encode { source })?"
        else:
            s = deref(sch) if "$ref" in sch else sch
            t = type_list(s)
            if "array" in t:
                explode = p.get("explode", p.get("style", "form") == "form")
                item_conv = "x.to_string()"
                if explode:
                    if is_opt:
                        target.append(f"if let Some(v) = &{access} {{ for x in v {{ {target_name(p)}.push(({rust_str(name)}, {item_conv})); }} }}")
                    else:
                        target.append(f"for x in {access} {{ {target_name(p)}.push(({rust_str(name)}, {item_conv})); }}")
                    return
                conv = f"v.iter().map(|x| {item_conv}).collect::<Vec<_>>().join(\",\")"
            else:
                conv = "v.to_string()"
        if is_opt:
            target.append(f"if let Some(v) = &{access} {{ {target_name(p)}.push(({rust_str(name)}, {conv})); }}")
        else:
            target.append(f"{{ let v = &{access}; {target_name(p)}.push(({rust_str(name)}, {conv})); }}")

    def target_name(p):
        return "headers" if p["in"] == "header" else "query"

    params_struct = None
    for p in req_others:
        ident = field_ident(p["name"])
        t, _ = g.ty(p.get("schema", {}), pascal(opid) + pascal(p["name"]))
        args.append(f"{ident}: {'&str' if t == 'String' else '&' + t}")
        emit_param(p, "", False)
    if opt_others:
        params_struct = g.fresh(pascal(opid) + "Params")
        src = f"/// Optional parameters for [`Client::{fname}`](crate::Client::{fname}).\n"
        fields = []
        for p in opt_others:
            if "content" in p:
                sch = next(iter(p["content"].values()))["schema"]
            else:
                sch = p.get("schema", {})
            t, _ = g.ty(sch, params_struct + pascal(p["name"]))
            fields.append((p, t))
        src += "#[derive(Debug, Clone, Default, PartialEq)]\n"
        src += f"pub struct {params_struct} {{\n"
        for p, t in fields:
            src += doc(p.get("description"), "    ")
            src += f"    pub {field_ident(p['name'])}: Option<{t}>,\n"
        src += "}\n"
        g.emit(params_struct, src)
        args.append(f"params: &{params_struct}")
        for p in opt_others:
            emit_param(p, "params.", True)

    body_line = "None"
    request_type = None
    rb = op.get("requestBody")
    if rb:
        content = rb.get("content", {})
        sch = content.get("application/json", {}).get("schema")
        if sch is not None:
            if "$ref" in sch:
                tname = sch["$ref"].split("/")[-1]
                g.schema(tname)
            else:
                tname = g.fresh(pascal(opid) + "Request")
                g.struct(tname, sch)
            request_type = tname
            if rb.get("required", True):
                args.append(f"body: &{tname}")
                body_line = "Some(serde_json::to_value(body).map_err(|source| Error::Encode { source })?)"
            else:
                args.append(f"body: Option<&{tname}>")
                body_line = "body.map(serde_json::to_value).transpose().map_err(|source| Error::Encode { source })?"

    # Responses
    oks = []
    for code, r in op["responses"].items():
        if not code.startswith("2"):
            continue
        r = response(r)
        content = r.get("content") or {}
        if not content:
            oks.append((code, None, None, r.get("description")))
            continue
        ctype, c = next(iter(content.items()))
        sch = c.get("schema", {})
        if "json" in ctype:
            if "$ref" in sch:
                tname = sch["$ref"].split("/")[-1]
                g.schema(tname)
            else:
                tname = g.fresh(pascal(opid) + "Response")
                if is_object_like(sch):
                    g.struct(tname, sch)
                else:
                    t, _ = g.ty(sch, tname)
                    tname = t
            oks.append((code, "json", tname, r.get("description")))
        else:
            oks.append((code, "text", "String", r.get("description")))

    retry = "Retry::Idempotent"
    if method == "post":
        retry = "Retry::Idempotent" if any(p["name"] == "Idempotency-Key" for p in others) else "Retry::RateLimitOnly"
    auth = "false" if op.get("security") == [] else "true"

    with_body = [o for o in oks if o[1] is not None]
    no_body = [o for o in oks if o[1] is None]
    if len(with_body) == 0:
        ret = "()"
        tail = "let _ = response;\n        Ok(())"
    elif len(with_body) == 1 and not no_body:
        code, kind, tname, _ = with_body[0]
        ret = tname
        tail = "response.json()" if kind == "json" else "response.text()"
    elif len(with_body) == 1 and no_body:
        code, kind, tname, _ = with_body[0]
        ret = f"Option<{tname}>"
        conv = "response.json().map(Some)" if kind == "json" else "response.text().map(Some)"
        tail = f"if response.status == {no_body[0][0]} {{ return Ok(None); }}\n        {conv}"
    else:
        ename = g.fresh(pascal(opid) + "Response")
        src = f"/// What [`Client::{fname}`](crate::Client::{fname}) answered; which variant depends on the status code.\n"
        src += "#[derive(Debug, Clone, PartialEq)]\n"
        src += f"pub enum {ename} {{\n"
        arms = []
        for code, kind, tname, desc in oks:
            vn = {"200": "Ok", "201": "Created", "202": "Accepted", "204": "NoContent"}.get(code, f"Status{code}")
            src += doc(f"`{code}`: {desc}" if desc else f"`{code}`", "    ")
            if tname:
                src += f"    {vn}({tname}),\n"
                arms.append(f"{code} => Ok({ename}::{vn}(response.json()?)),")
            else:
                src += f"    {vn},\n"
                arms.append(f"{code} => Ok({ename}::{vn}),")
        src += "}\n"
        g.emit(ename, src)
        ret = ename
        tail = "match response.status {\n            " + "\n            ".join(arms)
        tail += "\n            _ => Err(response.unexpected_status()),\n        }"
        g.defaultable[ename] = False

    lines = []
    summary = op.get("summary", "")
    desc = op.get("description", "")
    d = f"{summary}\n\n" if summary else ""
    d += desc
    d += f"\n\n`{method.upper()} {path}`"
    lines.append(doc(d, "    ").rstrip("\n"))
    lines.append(f"    pub async fn {fname}({', '.join(args)}) -> Result<{ret}, Error> {{")
    for l in pre:
        lines.append("        " + l)
    for var, pushed in (("query", query_lines), ("headers", header_lines)):
        mut = "mut " if pushed else ""
        lines.append(f"        let {mut}{var}: Vec<(&'static str, String)> = Vec::new();")
    for l in query_lines + header_lines:
        lines.append("        " + l)
    lines.append("        let response = self")
    lines.append("            .execute(Request {")
    lines.append(f"                method: Method::{method.upper()},")
    lines.append(f"                path: format!({rust_str(fmt_path)}),")
    lines.append("                query,")
    lines.append("                headers,")
    lines.append(f"                body: {body_line},")
    lines.append(f"                retry: {retry},")
    lines.append(f"                auth: {auth},")
    # exec waits for the command: give the HTTP call the command's own budget.
    if opid == "execPod":
        lines.append("                timeout: body.timeout_seconds.map(|s| std::time::Duration::from_secs(s.max(0) as u64 + 30)),")
    else:
        lines.append("                timeout: None,")
    lines.append("            })")
    lines.append("            .await?;")
    lines.append(f"        {tail}")
    lines.append("    }")

    # Paginated: an *_all helper that walks nextCursor.
    if params_struct and any(p["name"] == "cursor" for p in opt_others) and len(with_body) == 1:
        tname = with_body[0][2]
        rs = SCHEMAS.get(tname)
        if rs is not None:
            props, _, _ = flatten(rs)
            arrays = [k for k, v in props.items() if k != "pagination" and "array" in type_list(deref(v) if "$ref" in v else v)]
            if "pagination" in props and len(arrays) == 1:
                arr = arrays[0]
                vec_t = g.field_types[(tname, arr)]
                assert vec_t.startswith("Vec<"), vec_t
                item_t = vec_t[4:-1]
                pargs = [a for a in args if not a.startswith("params:")]
                call_args = [a.split(":")[0] for a in pargs[1:]]
                lines.append("")
                lines.append(f"    /// [`Client::{fname}`], following `nextCursor` until the last page, and")
                lines.append(f"    /// returning every `{arr}` entry. `params.cursor` is the starting point.")
                lines.append(f"    pub async fn {fname}_all({', '.join(args)}) -> Result<Vec<{item_t}>, Error> {{")
                lines.append("        let mut params = params.clone();")
                lines.append("        let mut out = Vec::new();")
                lines.append("        loop {")
                lines.append(f"            let page = self.{fname}({', '.join(call_args + ['&params'])}).await?;")
                lines.append(f"            out.extend(page.{field_ident(arr)});")
                lines.append("            match page.pagination.next_cursor {")
                lines.append("                Some(next) if page.pagination.has_next_page => params.cursor = Some(next),")
                lines.append("                _ => return Ok(out),")
                lines.append("            }")
                lines.append("        }")
                lines.append("    }")
    response_types = {code: tname for code, kind, tname, _ in oks if kind == "json"}
    return "\n".join(lines) + "\n", request_type, response_types
