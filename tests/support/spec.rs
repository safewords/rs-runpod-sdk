//! Reading the examples out of `openapi/runpod-v2.json`, and round-tripping
//! them through the generated types.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

const SPEC: &str = include_str!("../../openapi/runpod-v2.json");

/// Examples that disagree with their own schema in Runpod's document, with
/// why. The types follow the schema; these are skipped, not fixed.
const KNOWN_SPEC_DEFECTS: &[(&str, &str)] = &[(
    "get /v2/catalog/templates 200 templates",
    "the example has a `pagination` block, but the response schema (TemplateList) has none and the operation takes no cursor",
)];

pub struct Examples {
    spec: Value,
    checked: usize,
    failures: Vec<String>,
}

impl Examples {
    pub fn load() -> Self {
        Self {
            spec: serde_json::from_str(SPEC).unwrap(),
            checked: 0,
            failures: Vec::new(),
        }
    }

    fn resolve<'a>(&'a self, v: &'a Value) -> &'a Value {
        match v.get("$ref").and_then(Value::as_str) {
            Some(r) => {
                let pointer = r.trim_start_matches('#');
                self.spec.pointer(pointer).unwrap_or(v)
            }
            None => v,
        }
    }

    /// Example values under a `content` map, from its JSON media type.
    fn content_examples(&self, content: Option<&Value>) -> Vec<(String, Value)> {
        let Some(content) = content.and_then(Value::as_object) else {
            return vec![];
        };
        let mut out = vec![];
        for (ctype, media) in content {
            if !ctype.contains("json") {
                continue;
            }
            if let Some(map) = media.get("examples").and_then(Value::as_object) {
                for (name, ex) in map {
                    if let Some(v) = self.resolve(ex).get("value") {
                        out.push((name.clone(), v.clone()));
                    }
                }
            }
            if let Some(v) = media.get("example") {
                out.push(("example".into(), v.clone()));
            }
        }
        out
    }

    fn operation(&self, path: &str, method: &str) -> &Value {
        let p = format!(
            "/paths/{}/{method}",
            path.replace('~', "~0").replace('/', "~1")
        );
        self.spec
            .pointer(&p)
            .unwrap_or_else(|| panic!("no {method} {path} in the spec"))
    }

    fn response_examples(&self, path: &str, method: &str, status: &str) -> Vec<(String, Value)> {
        let r = &self.operation(path, method)["responses"][status];
        self.content_examples(self.resolve(r).get("content"))
    }

    pub fn first_response(&self, path: &str, method: &str, status: &str) -> Value {
        self.response_examples(path, method, status)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no example for {status} {method} {path}"))
            .1
    }

    pub fn response<T: DeserializeOwned + Serialize>(
        &mut self,
        path: &str,
        method: &str,
        status: &str,
    ) {
        for (name, v) in self.response_examples(path, method, status) {
            self.round_trip::<T>(&format!("{method} {path} {status} {name}"), v);
        }
    }

    pub fn request<T: DeserializeOwned + Serialize>(&mut self, path: &str, method: &str) {
        let rb = self.operation(path, method).get("requestBody").cloned();
        let examples = self.content_examples(rb.as_ref().and_then(|r| r.get("content")));
        for (name, v) in examples {
            self.round_trip::<T>(&format!("{method} {path} request {name}"), v);
        }
    }

    fn round_trip<T: DeserializeOwned + Serialize>(&mut self, what: &str, v: Value) {
        if KNOWN_SPEC_DEFECTS.iter().any(|(w, _)| *w == what) {
            return;
        }
        self.checked += 1;
        let typed: T = match serde_json::from_value(v.clone()) {
            Ok(t) => t,
            Err(e) => {
                self.failures.push(format!(
                    "{what} ({}): does not decode: {e}",
                    std::any::type_name::<T>()
                ));
                return;
            }
        };
        let back = serde_json::to_value(&typed).unwrap();
        if let Some(diff) = difference(&strip_nulls(v), &strip_nulls(back), "") {
            self.failures.push(format!(
                "{what} ({}): changed on re-encoding at {diff}",
                std::any::type_name::<T>()
            ));
        }
    }

    pub fn finish(self) {
        assert!(
            self.checked > 50,
            "only {} examples found; is the spec loaded?",
            self.checked
        );
        assert!(
            self.failures.is_empty(),
            "{} of {} spec examples failed:\n{}",
            self.failures.len(),
            self.checked,
            self.failures.join("\n")
        );
    }
}

fn strip_nulls(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k, strip_nulls(v)))
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(strip_nulls).collect()),
        v => v,
    }
}

/// Where two JSON values differ, if they do. Numbers compare by value, so
/// `5` and `5.0` are the same.
fn difference(a: &Value, b: &Value, at: &str) -> Option<String> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            (x.as_f64() != y.as_f64()).then(|| format!("{at}: {x} vs {y}"))
        }
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => {
                        if let Some(d) = difference(p, q, &format!("{at}.{k}")) {
                            return Some(d);
                        }
                    }
                    (Some(_), None) => {
                        return Some(format!("{at}.{k}: dropped (not in the type?)"));
                    }
                    (None, Some(_)) => return Some(format!("{at}.{k}: added")),
                    (None, None) => unreachable!(),
                }
            }
            None
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
            .iter()
            .zip(y)
            .enumerate()
            .find_map(|(i, (p, q))| difference(p, q, &format!("{at}[{i}]"))),
        _ => (a != b).then(|| format!("{at}: {a} vs {b}")),
    }
}
