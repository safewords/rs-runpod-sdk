//! The [`Client`] and how it talks to Runpod.
//!
//! - `builder.rs`: [`ClientBuilder`] and the defaults.
//! - `transport.rs`: sending a request: auth, retries, rate limits, errors.
//! - `request.rs`: the request and response types the generated operations
//!   in `crate::api` are written against.

mod builder;
mod request;
mod transport;

use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use builder::ClientBuilder;
pub(crate) use request::{Request, Retry, encode_segment};

use crate::error::Error;
use crate::rate_limit::RateLimit;

/// The management API host. Operation paths carry the `/v2` prefix.
pub const DEFAULT_BASE_URL: &str = "https://api.runpod.io";

/// A Runpod API v2 client. Cheap to clone; clones share the connection pool
/// and the rate-limit state, which is what Runpod asks for (quotas are per
/// user, not per client).
///
/// Its methods are the API's operations, defined area by area under
/// `src/api/`, plus the log streams in `src/logs/`.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    base: String,
    api_key: Option<String>,
    team_id: Option<String>,
    timeout: Duration,
    max_attempts: u32,
    respect_rate_limit: bool,
    rate_limit: Mutex<Option<RateLimit>>,
}

impl std::fmt::Debug for Client {
    // Never print the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base", &self.inner.base)
            .field("has_api_key", &self.inner.api_key.is_some())
            .field("team_id", &self.inner.team_id)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// A client for `https://api.runpod.io` with this API key.
    pub fn new(api_key: impl Into<String>) -> Result<Self, Error> {
        let builder = Self::builder().api_key(api_key);
        if !builder.has_api_key() {
            return Err(Error::MissingApiKey);
        }
        builder.build()
    }

    /// A client with no API key, for the signup operations
    /// ([`Client::create_authentication_attempt`] and friends), which are
    /// how you get one.
    pub fn anonymous() -> Result<Self, Error> {
        Self::builder().build()
    }

    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    /// The quota Runpod reported on the most recent response, if it sent
    /// `RateLimit` headers.
    pub fn rate_limit(&self) -> Option<RateLimit> {
        self.inner.rate_limit.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_key_is_refused() {
        assert!(matches!(Client::new("  "), Err(Error::MissingApiKey)));
    }

    #[test]
    fn debug_never_prints_the_key() {
        let c = Client::new("secret-key-value").unwrap();
        assert!(!format!("{c:?}").contains("secret-key-value"));
    }
}
