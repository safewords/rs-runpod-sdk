"""Component schemas to Rust types."""

from __future__ import annotations

from .naming import doc, enum_variant, field_ident, pascal, rust_str, snake
from .spec import SCHEMAS, deref, flatten, is_null, is_object_like, type_list


def is_sensitive(struct: str, field: str) -> bool:
    """Whether a field holds a credential. A struct with one gets a hand-rolled
    Debug printing `<redacted>` in its place, so `{:?}` of a request or answer
    never leaks a password, token or secret into a log. `env` is included
    because that is where people put tokens."""
    if field in ("password", "apiKey", "secret", "token", "codeVerifier", "env"):
        return True
    if field == "value":
        return "Secret" in struct
    if field == "code":
        return "Verify" in struct
    return False


class Gen:
    def __init__(self):
        self.items: dict[str, str] = {}  # type name -> rust source
        self.defaultable: dict[str, bool] = {}
        self.pending_default: dict[str, list] = {}
        # schema name -> discriminator property to drop (variants of a tagged enum)
        self.drop_prop: dict[str, str] = {}
        for name, s in SCHEMAS.items():
            d = s.get("discriminator")
            if d and "oneOf" in s:
                for v in s["oneOf"]:
                    vn = v["$ref"].split("/")[-1]
                    self.drop_prop[vn] = d["propertyName"]
        self.names_taken = set(SCHEMAS)
        self.field_types: dict[tuple[str, str], str] = {}

    def fresh(self, base: str) -> str:
        name = base
        i = 2
        while name in self.names_taken:
            name = f"{base}{i}"
            i += 1
        self.names_taken.add(name)
        return name

    def emit(self, name: str, src: str):
        if name in self.items:
            return
        self.items[name] = src

    # Returns (rust type, optional) where optional means "null is a valid value".
    def ty(self, s: dict, ctx: str) -> tuple[str, bool]:
        if "$ref" in s and len(set(s) - {"$ref", "description", "examples", "example", "default", "deprecated"}) == 0:
            name = s["$ref"].split("/")[-1]
            self.schema(name)
            return name, False
        for key in ("anyOf", "oneOf"):
            if key in s:
                opts = s[key]
                non_null = [o for o in opts if not is_null(o)]
                nullable = len(non_null) != len(opts)
                if len(non_null) == 1:
                    t, n = self.ty(non_null[0], ctx)
                    return t, n or nullable
                name = self.fresh(ctx)
                self.union(name, {**s, key: non_null})
                return name, nullable
        if "allOf" in s:
            parts = [p for p in s["allOf"] if not set(p) <= {"if", "then", "else", "not"}]
            if len(parts) == 1 and not s.get("properties"):
                return self.ty(parts[0], ctx)
            name = self.fresh(ctx)
            self.struct(name, s)
            return name, False
        types = type_list(s)
        nullable = "null" in types
        types = [t for t in types if t != "null"]
        t = types[0] if types else None
        if "const" in s and t is None:
            t = "string" if isinstance(s["const"], str) else None
        if t == "string":
            if "enum" in s:
                name = self.fresh(ctx)
                self.enum(name, s)
                return name, nullable
            return "String", nullable
        if t == "integer":
            return "i64", nullable
        if t == "number":
            return "f64", nullable
        if t == "boolean":
            return "bool", nullable
        if t == "array":
            items = s.get("items") or {}
            it, inull = self.ty(items, ctx + "Item") if items else ("serde_json::Value", False)
            if inull:
                it = f"Option<{it}>"
            return f"Vec<{it}>", nullable
        if t == "object" or "properties" in s:
            if s.get("properties"):
                name = self.fresh(ctx)
                self.struct(name, s)
                return name, nullable
            ap = s.get("additionalProperties")
            if isinstance(ap, dict) and ap:
                vt, vnull = self.ty(ap, ctx + "Value")
                if vnull:
                    vt = f"Option<{vt}>"
                return f"std::collections::BTreeMap<String, {vt}>", nullable
            return "serde_json::Map<String, serde_json::Value>", nullable
        return "serde_json::Value", nullable

    def schema(self, name: str):
        if name in self.items or name in self.pending_default:
            return
        s = SCHEMAS[name]
        self.pending_default[name] = []
        if "oneOf" in s or ("anyOf" in s and not any(is_null(o) for o in s["anyOf"])):
            self.union(name, s)
        elif type_list(s) == ["string"] and "enum" in s:
            self.enum(name, s)
        elif is_object_like(s):
            self.struct(name, s)
        else:
            t, n = self.ty(s, name + "Inner")
            if n:
                t = f"Option<{t}>"
            self.defaultable[name] = default_ok(t, self.defaultable)
            self.emit(name, doc(s.get("description")) + f"pub type {name} = {t};\n")

    def enum(self, name: str, s: dict):
        values = [str(v) for v in s["enum"]]
        self.defaultable[name] = False
        variants = []
        used = set()
        for v in values:
            vn = enum_variant(v)
            while vn in used or vn == "Other":
                vn += "_"
            used.add(vn)
            variants.append((vn, v))
        src = doc(s.get("description"))
        src += "#[derive(Debug, Clone, PartialEq, Eq, Hash)]\npub enum " + name + " {\n"
        for vn, v in variants:
            src += f"    /// `{v}`\n    {vn},\n"
        src += "    /// A value this crate does not know yet, kept verbatim.\n    Other(String),\n}\n\n"
        src += f"impl {name} {{\n"
        src += "    /// Every value the API documented when this crate was generated.\n"
        src += f"    pub const ALL: &'static [{name}] = &[" + ", ".join(f"{name}::{vn}" for vn, _ in variants) + "];\n\n"
        src += "    /// The wire value.\n    pub fn as_str(&self) -> &str {\n        match self {\n"
        for vn, v in variants:
            src += f"            {name}::{vn} => {rust_str(v)},\n"
        src += f"            {name}::Other(s) => s,\n        }}\n    }}\n}}\n\n"
        src += f"impl From<&str> for {name} {{\n    fn from(s: &str) -> Self {{\n        match s {{\n"
        for vn, v in variants:
            src += f"            {rust_str(v)} => {name}::{vn},\n"
        src += f"            other => {name}::Other(other.to_string()),\n        }}\n    }}\n}}\n\n"
        src += f"impl std::fmt::Display for {name} {{\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n        f.write_str(self.as_str())\n    }}\n}}\n\n"
        src += f"impl serde::Serialize for {name} {{\n    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {{\n        s.serialize_str(self.as_str())\n    }}\n}}\n\n"
        src += f"impl<'de> serde::Deserialize<'de> for {name} {{\n    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {{\n        let s = std::borrow::Cow::<'de, str>::deserialize(d)?;\n        Ok({name}::from(s.as_ref()))\n    }}\n}}\n"
        self.emit(name, src)

    def union(self, name: str, s: dict):
        self.defaultable[name] = False
        key = "oneOf" if "oneOf" in s else "anyOf"
        opts = s[key]
        disc = s.get("discriminator")
        src = doc(s.get("description"))
        variants = []
        if disc:
            mapping = disc.get("mapping") or {}
            inv = {v.split("/")[-1]: k for k, v in mapping.items()}
            for o in opts:
                vn_schema = o["$ref"].split("/")[-1]
                self.schema(vn_schema)
                tag = inv.get(vn_schema)
                if tag is None:
                    tag = deref(o)["properties"][disc["propertyName"]].get("enum", [vn_schema])[0]
                variants.append((enum_variant(tag), vn_schema, tag))
            src += "#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]\n"
            src += f"#[serde(tag = {rust_str(disc['propertyName'])})]\npub enum {name} {{\n"
            for vn, ty, tag in variants:
                src += f"    #[serde(rename = {rust_str(tag)})]\n    {vn}({ty}),\n"
            src += "}\n"
        else:
            used = set()
            for i, o in enumerate(opts):
                t, n = self.ty(o, f"{name}Variant{i}")
                if n:
                    t = f"Option<{t}>"
                base = o["$ref"].split("/")[-1] if "$ref" in o else f"V{i}"
                # Strip the union's own name from variant names where it is a suffix/prefix.
                vn = base
                for affix in (name,):
                    if vn.endswith(affix) and vn != affix:
                        vn = vn[: -len(affix)]
                while vn in used:
                    vn += "_"
                used.add(vn)
                variants.append((vn, t))
            src += "#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]\n"
            src += f"#[serde(untagged)]\npub enum {name} {{\n"
            for vn, t in variants:
                src += f"    {vn}({t}),\n"
            src += "}\n"
        self.emit(name, src)

    def struct(self, name: str, s: dict):
        props, required, _ = flatten(s)
        drop = self.drop_prop.get(name)
        fields = []
        for pname, ps in props.items():
            if pname == drop:
                continue
            t, nullable = self.ty(ps, name + pascal(pname))
            optional = nullable or pname not in required
            fields.append((pname, ps, t, optional))
        sensitive = [pname for pname, *_ in fields if is_sensitive(name, pname)]
        for pname, _, t, optional in fields:
            self.field_types[(name, pname)] = t
        derives = ["Clone", "PartialEq", "serde::Serialize", "serde::Deserialize"]
        if not sensitive:
            derives.insert(0, "Debug")
        src = doc(s.get("description"))
        src += f"#[derive({', '.join(derives)})]\n__DEFAULT__{name}__\npub struct {name} {{\n"
        for pname, ps, t, optional in fields:
            ident = field_ident(pname)
            desc = ps.get("description")
            if ps.get("deprecated"):
                desc = (desc or "") + "\n\nDeprecated by Runpod."
            src += doc(desc, "    ")
            attrs = []
            if snake(pname) != pname or ident.startswith("r#"):
                attrs.append(f"rename = {rust_str(pname)}")
            if optional:
                attrs.append("default")
                attrs.append('skip_serializing_if = "Option::is_none"')
                ft = f"Option<{t}>"
            else:
                ft = t
            if attrs:
                src += f"    #[serde({', '.join(attrs)})]\n"
            src += f"    pub {ident}: {ft},\n"
        src += "}\n"
        if sensitive:
            src += f"\nimpl std::fmt::Debug for {name} {{\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{\n"
            src += f"        f.debug_struct({rust_str(name)})\n"
            for pname, ps, t, optional in fields:
                ident = field_ident(pname)
                if pname in sensitive:
                    src += f'            .field({rust_str(snake(pname))}, &"<redacted>")\n'
                else:
                    src += f"            .field({rust_str(snake(pname))}, &self.{ident})\n"
            src += "            .finish()\n    }\n}\n"
        self.pending_default[name] = [(t, optional) for _, _, t, optional in fields]
        self.emit(name, src)

    def finish_defaults(self):
        # Fixpoint: a struct derives Default when every field can.
        changed = True
        for n, fields in self.pending_default.items():
            if fields is not None and n not in self.defaultable:
                self.defaultable[n] = True
        while changed:
            changed = False
            for n, fields in self.pending_default.items():
                if not self.defaultable.get(n, False):
                    continue
                if n not in self.items or not self.items[n].count("__DEFAULT__"):
                    continue
                ok = all(opt or default_ok(t, self.defaultable) for t, opt in fields)
                if not ok:
                    self.defaultable[n] = False
                    changed = True
        for n in list(self.items):
            marker = f"__DEFAULT__{n}__\n"
            if marker in self.items[n]:
                if self.defaultable.get(n):
                    src = self.items[n].replace(marker, "")
                    src = src.replace("#[derive(", "#[derive(Default, ", 1)
                else:
                    src = self.items[n].replace(marker, "")
                self.items[n] = src


def default_ok(t: str, known: dict) -> bool:
    if t.startswith(("Option<", "Vec<", "std::collections::BTreeMap<", "serde_json::")):
        return True
    if t in ("String", "i64", "f64", "bool"):
        return True
    return known.get(t, False)
