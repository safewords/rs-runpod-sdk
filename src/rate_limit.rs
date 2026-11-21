//! Runpod's `RateLimit` and `RateLimit-Policy` headers (IETF
//! ratelimit-headers draft), e.g.
//!
//! ```text
//! RateLimit-Policy: "minute";q=60;w=60, "hour";q=3000;w=3600
//! RateLimit: "minute";r=0;t=12, "hour";r=2800;t=1812
//! ```

use std::time::{Duration, Instant};

/// The quota state Runpod last reported, from [`crate::Client::rate_limit`].
#[derive(Debug, Clone, PartialEq)]
pub struct RateLimit {
    /// Live state of each window.
    pub windows: Vec<RateLimitWindow>,
    /// The quota behind each window, when Runpod sent `RateLimit-Policy`.
    pub policies: Vec<RateLimitPolicy>,
    /// When the headers were read. `reset` is relative to this.
    pub observed_at: Instant,
}

/// One window of the `RateLimit` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitWindow {
    /// `minute`, `hour`, `day`.
    pub name: String,
    /// Requests left in the window.
    pub remaining: u64,
    /// Until the window resets, from `observed_at`.
    pub reset: Duration,
}

/// One window of the `RateLimit-Policy` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitPolicy {
    pub name: String,
    /// Requests allowed per window.
    pub quota: u64,
    pub window: Duration,
}

impl RateLimit {
    /// How long to wait before the next request, if a window is exhausted
    /// and has not reset yet.
    pub fn wait(&self) -> Option<Duration> {
        let elapsed = self.observed_at.elapsed();
        self.windows
            .iter()
            .filter(|w| w.remaining == 0)
            .filter_map(|w| w.reset.checked_sub(elapsed))
            .filter(|d| !d.is_zero())
            .max()
    }
}

/// Parse the items of a structured-field list: `"name";k=v;k=v, ...`.
fn items(header: &str) -> impl Iterator<Item = (String, Vec<(&str, u64)>)> {
    header.split(',').filter_map(|item| {
        let mut parts = item.trim().split(';');
        let name = parts.next()?.trim().trim_matches('"').to_string();
        if name.is_empty() {
            return None;
        }
        let params = parts
            .filter_map(|kv| {
                let (k, v) = kv.trim().split_once('=')?;
                Some((k.trim(), v.trim().parse().ok()?))
            })
            .collect();
        Some((name, params))
    })
}

fn get(params: &[(&str, u64)], key: &str) -> Option<u64> {
    params.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

pub(crate) fn parse(state: Option<&str>, policy: Option<&str>) -> Option<RateLimit> {
    let windows: Vec<_> = items(state?)
        .filter_map(|(name, p)| {
            Some(RateLimitWindow {
                name,
                remaining: get(&p, "r")?,
                reset: Duration::from_secs(get(&p, "t").unwrap_or(0)),
            })
        })
        .collect();
    if windows.is_empty() {
        return None;
    }
    let policies = policy
        .map(|h| {
            items(h)
                .filter_map(|(name, p)| {
                    Some(RateLimitPolicy {
                        name,
                        quota: get(&p, "q")?,
                        window: Duration::from_secs(get(&p, "w")?),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(RateLimit {
        windows,
        policies,
        observed_at: Instant::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_example() {
        let rl = parse(
            Some(r#""minute";r=0;t=12, "hour";r=2800;t=1812, "day";r=49500;t=45012"#),
            Some(r#""minute";q=60;w=60, "hour";q=3000;w=3600, "day";q=50000;w=86400"#),
        )
        .unwrap();
        assert_eq!(rl.windows.len(), 3);
        assert_eq!(rl.windows[0].name, "minute");
        assert_eq!(rl.windows[0].remaining, 0);
        assert_eq!(rl.windows[0].reset, Duration::from_secs(12));
        assert_eq!(rl.policies[2].quota, 50000);
        let wait = rl.wait().unwrap();
        assert!(wait <= Duration::from_secs(12) && wait > Duration::from_secs(11));
    }

    #[test]
    fn no_wait_while_quota_remains() {
        let rl = parse(Some(r#""minute";r=5;t=12"#), None).unwrap();
        assert_eq!(rl.wait(), None);
    }

    #[test]
    fn garbage_is_ignored() {
        assert!(parse(Some("nonsense"), None).is_none());
        assert!(parse(None, None).is_none());
    }
}
