use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Client, DEFAULT_BASE_URL, Inner};
use crate::error::Error;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_ATTEMPTS: u32 = 4;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Builds a [`Client`] with non-default settings.
#[derive(Clone)]
pub struct ClientBuilder {
    api_key: Option<String>,
    base: String,
    team_id: Option<String>,
    timeout: Duration,
    max_attempts: u32,
    respect_rate_limit: bool,
}

impl std::fmt::Debug for ClientBuilder {
    // Never print the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientBuilder")
            .field("base", &self.base)
            .field("has_api_key", &self.api_key.is_some())
            .field("team_id", &self.team_id)
            .field("timeout", &self.timeout)
            .field("max_attempts", &self.max_attempts)
            .field("respect_rate_limit", &self.respect_rate_limit)
            .finish()
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            api_key: None,
            base: DEFAULT_BASE_URL.to_string(),
            team_id: None,
            timeout: DEFAULT_TIMEOUT,
            max_attempts: DEFAULT_ATTEMPTS,
            respect_rate_limit: true,
        }
    }
}

impl ClientBuilder {
    /// The API key. Without one only the signup operations work.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        let key = key.into().trim().to_string();
        self.api_key = (!key.is_empty()).then_some(key);
        self
    }

    pub(super) fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Somewhere other than `https://api.runpod.io`, e.g. a test server.
    pub fn base_url(mut self, base: impl Into<String>) -> Self {
        self.base = base.into().trim_end_matches('/').to_string();
        self
    }

    /// Send `X-Team-Id`, acting for a team. Only console sessions need this;
    /// an API key created inside a team already acts for that team.
    pub fn team_id(mut self, team_id: impl Into<String>) -> Self {
        self.team_id = Some(team_id.into());
        self
    }

    /// Per-request timeout, 60 s by default. Log streams are exempt.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Attempts per request, including the first, before an error is
    /// returned. 4 by default; 1 disables retries.
    pub fn max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = attempts.max(1);
        self
    }

    /// Whether to pause before a request when Runpod last said a quota
    /// window is exhausted, rather than spend a request on a 429. On by
    /// default.
    pub fn respect_rate_limit(mut self, on: bool) -> Self {
        self.respect_rate_limit = on;
        self
    }

    pub fn build(self) -> Result<Client, Error> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("rs-runpod-sdk/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|source| Error::Transport {
                method: "BUILD",
                path: String::new(),
                source,
            })?;
        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base: self.base,
                api_key: self.api_key,
                team_id: self.team_id,
                timeout: self.timeout,
                max_attempts: self.max_attempts,
                respect_rate_limit: self.respect_rate_limit,
                rate_limit: Mutex::new(None),
            }),
        })
    }
}
