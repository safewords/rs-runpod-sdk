use std::time::Duration;

/// Everything a call can fail with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The request got no answer: DNS, TLS, a timeout, a dropped connection.
    ///
    /// For a request that creates something (a pod, an endpoint, a secret) a
    /// timeout does **not** mean nothing was created: Runpod may have acted
    /// and the answer been lost. List before you retry one of those.
    #[error("Runpod request {method} {path} failed: {source}")]
    Transport {
        method: &'static str,
        path: String,
        #[source]
        source: reqwest::Error,
    },

    /// Runpod answered with an error status.
    #[error("Runpod answered {} to {} {}: {}", .0.status, .0.method, .0.path, .0.message())]
    Api(Box<ApiError>),

    /// A 2xx whose status this crate did not expect for the operation.
    #[error("Runpod answered an unexpected {status} to {method} {path}")]
    UnexpectedStatus {
        method: &'static str,
        path: String,
        status: u16,
    },

    /// The body was not the shape this crate expects.
    #[error("Runpod sent an unexpected body for {method} {path}: {source}")]
    Decode {
        method: &'static str,
        path: String,
        #[source]
        source: serde_json::Error,
    },

    /// A request value could not be serialised. Practically unreachable.
    #[error("could not encode the request: {source}")]
    Encode {
        #[source]
        source: serde_json::Error,
    },

    /// The client has no API key and the operation needs one. Only the
    /// signup operations work without one.
    #[error("the Runpod API key is missing or empty")]
    MissingApiKey,
}

/// An error answer: Runpod's `application/problem+json` body, plus the
/// request it was for.
#[derive(Debug, Clone)]
pub struct ApiError {
    pub method: &'static str,
    pub path: String,
    pub status: u16,
    /// The problem's `title`, e.g. `Not Found`.
    pub title: Option<String>,
    /// The problem's `detail`: the explanation, when there is one.
    pub detail: Option<String>,
    /// Field-level validation messages, on a 422.
    pub errors: Vec<String>,
    /// The raw body, for when it was not problem+json.
    pub body: String,
    /// `Retry-After`, on a 429 (and sometimes a 503).
    pub retry_after: Option<Duration>,
}

impl ApiError {
    /// The most specific explanation available.
    pub fn message(&self) -> String {
        let mut m = self
            .detail
            .clone()
            .or_else(|| self.title.clone())
            .unwrap_or_else(|| self.body.clone());
        if !self.errors.is_empty() {
            m.push_str(" (");
            m.push_str(&self.errors.join("; "));
            m.push(')');
        }
        m
    }
}

impl Error {
    /// Whether the same request could succeed later unchanged: rate
    /// limiting, a server error, or the network.
    ///
    /// The client has already retried idempotent requests by the time you see
    /// this. See [`Error::Transport`] before retrying a create yourself.
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Transport { .. } => true,
            Error::Api(e) => e.status == 429 || e.status >= 500,
            _ => false,
        }
    }

    /// The HTTP status, when Runpod answered at all.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api(e) => Some(e.status),
            Error::UnexpectedStatus { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// A 404: the resource does not exist, or not on this account.
    pub fn is_not_found(&self) -> bool {
        self.status() == Some(404)
    }

    /// A 409: the resource is in a state that does not allow this, or a
    /// name is already taken.
    pub fn is_conflict(&self) -> bool {
        self.status() == Some(409)
    }

    /// A 429 that outlasted the client's retries.
    pub fn is_rate_limited(&self) -> bool {
        self.status() == Some(429)
    }

    /// The problem details, when Runpod answered with an error status.
    pub fn api(&self) -> Option<&ApiError> {
        match self {
            Error::Api(e) => Some(e),
            _ => None,
        }
    }
}
