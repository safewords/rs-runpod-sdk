use std::time::Duration;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::Error;

/// How a request may be repeated after a failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Retry {
    /// GET, PUT, PATCH, DELETE, and POSTs with an idempotency key: on the
    /// network, 429 and 5xx.
    Idempotent,
    /// A POST that creates or acts: only on 429 and refused connections,
    /// since anything else might already have happened.
    RateLimitOnly,
}

/// One API call, as a generated operation describes it.
pub(crate) struct Request {
    pub method: Method,
    /// Starting `/v2/`, path segments already encoded.
    pub path: String,
    pub query: Vec<(&'static str, String)>,
    pub headers: Vec<(&'static str, String)>,
    pub body: Option<Value>,
    pub retry: Retry,
    /// Whether to send the API key. False only for signup.
    pub auth: bool,
    /// Overrides the client's timeout upwards, for calls that wait (exec).
    pub timeout: Option<Duration>,
}

/// A 2xx answer, read in full.
pub(crate) struct Response {
    pub status: u16,
    pub(super) body: Vec<u8>,
    pub(super) method: &'static str,
    pub(super) path: String,
}

impl Response {
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, Error> {
        serde_json::from_slice(&self.body).map_err(|source| Error::Decode {
            method: self.method,
            path: self.path.clone(),
            source,
        })
    }

    pub fn text(&self) -> Result<String, Error> {
        Ok(String::from_utf8_lossy(&self.body).into_owned())
    }

    pub fn unexpected_status(&self) -> Error {
        Error::UnexpectedStatus {
            method: self.method,
            path: self.path.clone(),
            status: self.status,
        }
    }
}

/// Percent-encode one path segment, so an id can never change the path.
pub(crate) fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(super) fn method_name(m: &Method) -> &'static str {
    match *m {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cannot_escape_the_path() {
        assert_eq!(encode_segment("abc-123_x.y~"), "abc-123_x.y~");
        assert_eq!(encode_segment("../pods"), "..%2Fpods");
        assert_eq!(encode_segment("a b?c"), "a%20b%3Fc");
    }
}
