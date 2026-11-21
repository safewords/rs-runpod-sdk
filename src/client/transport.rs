//! Sending a request: auth, retries, rate limits, and error answers.

use std::hash::{BuildHasher, Hasher};
use std::time::Duration;

use reqwest::header::{HeaderMap, RETRY_AFTER};

use super::Client;
use super::request::{Request, Response, Retry, method_name};
use crate::api::ErrorResponse;
use crate::error::{ApiError, Error};
use crate::rate_limit;

/// Never sleep longer than this for one `Retry-After`; past it, give up and
/// return the 429 so the caller can decide.
const MAX_RETRY_AFTER: Duration = Duration::from_secs(120);

impl Client {
    /// Send a request, retrying as its [`Retry`] class allows, and return
    /// the body of a 2xx.
    pub(crate) async fn execute(&self, req: Request) -> Result<Response, Error> {
        let method = method_name(&req.method);
        let path = req.path.clone();
        let response = self.send(req, false).await?;
        let status = response.status().as_u16();
        let body = response
            .bytes()
            .await
            .map_err(|source| Error::Transport {
                method,
                path: path.clone(),
                source,
            })?
            .to_vec();
        Ok(Response {
            status,
            body,
            method,
            path,
        })
    }

    /// Send a request and hand back the open response, for streaming.
    pub(crate) async fn execute_stream(&self, req: Request) -> Result<reqwest::Response, Error> {
        self.send(req, true).await
    }

    async fn send(&self, req: Request, streaming: bool) -> Result<reqwest::Response, Error> {
        let inner = &self.inner;
        let method = method_name(&req.method);
        if req.auth && inner.api_key.is_none() {
            return Err(Error::MissingApiKey);
        }
        let mut attempt = 0;
        loop {
            attempt += 1;
            let last = attempt >= inner.max_attempts;
            self.wait_for_quota().await;

            let response = match self.build(&req, streaming).send().await {
                Ok(r) => r,
                Err(source) => {
                    // A request that never connected was never sent, so it
                    // is safe to repeat whatever it is.
                    let safe = req.retry == Retry::Idempotent || source.is_connect();
                    if safe && !last {
                        let wait = backoff(attempt);
                        tracing::debug!(error = %source, attempt, wait_ms = wait.as_millis() as u64, "Runpod call failed; retrying");
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    return Err(Error::Transport {
                        method,
                        path: req.path,
                        source,
                    });
                }
            };

            self.observe(response.headers());
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(response);
            }

            let retry_after = retry_after(response.headers());
            // A 429 was refused before anything happened: any method may be
            // repeated. A 5xx may have happened: only idempotent ones.
            let retryable = status == 429 || (status >= 500 && req.retry == Retry::Idempotent);
            let wait = match retry_after {
                Some(d) if d > MAX_RETRY_AFTER => None,
                Some(d) => Some(d + jitter(Duration::from_millis(500))),
                None => Some(backoff(attempt)),
            };
            match wait {
                Some(wait) if retryable && !last => {
                    tracing::debug!(
                        status,
                        attempt,
                        wait_ms = wait.as_millis() as u64,
                        "Runpod call refused; retrying"
                    );
                    tokio::time::sleep(wait).await;
                }
                _ => return Err(api_error(method, req.path, status, retry_after, response).await),
            }
        }
    }

    fn build(&self, req: &Request, streaming: bool) -> reqwest::RequestBuilder {
        let inner = &self.inner;
        let mut rb = inner
            .http
            .request(req.method.clone(), format!("{}{}", inner.base, req.path));
        if !req.query.is_empty() {
            rb = rb.query(&req.query);
        }
        for (k, v) in &req.headers {
            rb = rb.header(*k, v);
        }
        if req.auth {
            rb = rb.bearer_auth(inner.api_key.as_deref().unwrap_or_default());
        }
        if let Some(team) = &inner.team_id {
            rb = rb.header("X-Team-Id", team);
        }
        if streaming {
            rb = rb.header("Accept", "text/event-stream");
        } else {
            rb = rb
                .header(
                    "Accept",
                    "application/json, application/problem+json, text/markdown",
                )
                .timeout(req.timeout.unwrap_or(inner.timeout).max(inner.timeout));
        }
        if let Some(body) = &req.body {
            rb = rb.json(body);
        }
        rb
    }

    /// When Runpod last said a quota window is spent, sleep until it resets
    /// rather than spend a request on a 429.
    async fn wait_for_quota(&self) {
        if !self.inner.respect_rate_limit {
            return;
        }
        if let Some(wait) = self.rate_limit().and_then(|r| r.wait()) {
            tracing::debug!(
                wait_ms = wait.as_millis() as u64,
                "Runpod quota exhausted; waiting for the window to reset"
            );
            tokio::time::sleep(wait + jitter(Duration::from_millis(250))).await;
        }
    }

    fn observe(&self, headers: &HeaderMap) {
        let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        if let Some(rl) = rate_limit::parse(get("ratelimit"), get("ratelimit-policy")) {
            *self.inner.rate_limit.lock().unwrap() = Some(rl);
        }
    }
}

async fn api_error(
    method: &'static str,
    path: String,
    status: u16,
    retry_after: Option<Duration>,
    response: reqwest::Response,
) -> Error {
    let body = response.text().await.unwrap_or_default();
    let problem: Option<ErrorResponse> = serde_json::from_str(&body).ok();
    Error::Api(Box::new(ApiError {
        method,
        path,
        status,
        title: problem.as_ref().map(|p| p.title.clone()),
        detail: problem.as_ref().map(|p| p.detail.clone()),
        errors: problem.and_then(|p| p.errors).unwrap_or_default(),
        body,
        retry_after,
    }))
}

fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let v = headers.get(RETRY_AFTER)?.to_str().ok()?;
    v.trim().parse::<u64>().ok().map(Duration::from_secs)
}

fn backoff(attempt: u32) -> Duration {
    let base = Duration::from_millis(500 * 2u64.pow(attempt.saturating_sub(1).min(6)));
    base + jitter(base / 2)
}

/// Up to `max` of random delay, so clients refused together do not retry
/// together. Randomness from the std hasher's per-process keys.
fn jitter(max: Duration) -> Duration {
    let ms = max.as_millis() as u64;
    if ms == 0 {
        return Duration::ZERO;
    }
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
    );
    Duration::from_millis(h.finish() % ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_in_range() {
        for _ in 0..100 {
            assert!(jitter(Duration::from_millis(10)) < Duration::from_millis(10));
        }
        assert_eq!(jitter(Duration::ZERO), Duration::ZERO);
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert!(backoff(1) < Duration::from_millis(750));
        assert!(backoff(3) >= Duration::from_secs(2));
        assert!(backoff(50) < Duration::from_secs(48));
    }
}
