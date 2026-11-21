//! Pod and Serverless worker logs, streamed as Server-Sent Events.
//!
//! `GET /v2/pods/{id}/logs` and `GET /v2/serverless/{id}/workers/{workerId}/logs`
//! answer `text/event-stream`. Each event has an `id:` line and a JSON
//! `data:` line with `ts`, `source` and `line`. To resume after a dropped
//! connection, pass the last event's [`LogEvent::id`] back as
//! [`LogStreamParams::last_event_id`].

use std::pin::Pin;
use std::task::{Context, Poll};

use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::Method;
use serde::Deserialize;

use crate::api::LogSource;
use crate::client::{Client, Request, Retry, encode_segment};
use crate::error::Error;

mod parser;

use parser::Parser;

/// Optional parameters for a log stream.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogStreamParams {
    /// Only this source: the container's stdout/stderr, or the host's
    /// lifecycle log. Both when `None`.
    pub source: Option<LogSource>,
    /// How many past lines to send before following, 0–5000; Runpod's
    /// default is 100.
    pub tail: Option<u32>,
    /// Only lines at or after this RFC 3339 time.
    pub since: Option<String>,
    /// Resume after this event: the [`LogEvent::id`] last received.
    pub last_event_id: Option<String>,
}

/// One log line.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct LogEvent {
    /// The SSE event id, to resume from. `None` if Runpod sent none.
    #[serde(skip)]
    pub id: Option<String>,
    /// When the line was written, RFC 3339.
    pub ts: String,
    pub source: LogSource,
    pub line: String,
}

/// A live log stream. Ends when Runpod closes it; drop it to stop.
pub struct LogStream {
    inner: Pin<Box<dyn Stream<Item = Result<LogEvent, Error>> + Send>>,
}

impl std::fmt::Debug for LogStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogStream").finish_non_exhaustive()
    }
}

impl Stream for LogStream {
    type Item = Result<LogEvent, Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.inner.as_mut().poll_next(cx)
    }
}

impl Client {
    /// Stream a pod's container and system logs as they are written.
    ///
    /// `GET /v2/pods/{id}/logs`
    pub async fn stream_pod_logs(
        &self,
        id: &str,
        params: &LogStreamParams,
    ) -> Result<LogStream, Error> {
        let path = format!("/v2/pods/{}/logs", encode_segment(id));
        self.log_stream(path, params).await
    }

    /// Stream one Serverless worker's logs as they are written.
    ///
    /// `GET /v2/serverless/{id}/workers/{workerId}/logs`
    pub async fn stream_worker_logs(
        &self,
        endpoint_id: &str,
        worker_id: &str,
        params: &LogStreamParams,
    ) -> Result<LogStream, Error> {
        let path = format!(
            "/v2/serverless/{}/workers/{}/logs",
            encode_segment(endpoint_id),
            encode_segment(worker_id)
        );
        self.log_stream(path, params).await
    }

    async fn log_stream(&self, path: String, params: &LogStreamParams) -> Result<LogStream, Error> {
        let mut query = Vec::new();
        if let Some(s) = &params.source {
            query.push(("source", s.to_string()));
        }
        if let Some(t) = params.tail {
            query.push(("tail", t.to_string()));
        }
        if let Some(s) = &params.since {
            query.push(("since", s.clone()));
        }
        let mut headers = Vec::new();
        if let Some(id) = &params.last_event_id {
            headers.push(("Last-Event-ID", id.clone()));
        }
        let response = self
            .execute_stream(Request {
                method: Method::GET,
                path: path.clone(),
                query,
                headers,
                body: None,
                retry: Retry::Idempotent,
                auth: true,
                timeout: None,
            })
            .await?;
        let bytes = response.bytes_stream().boxed();
        let state = (bytes, Parser::default(), path);
        let stream =
            futures_util::stream::unfold(state, |(mut bytes, mut parser, path)| async move {
                loop {
                    if let Some(event) = parser.next_event() {
                        let item = event.map_err(|source| Error::Decode {
                            method: "GET",
                            path: path.clone(),
                            source,
                        });
                        return Some((item, (bytes, parser, path)));
                    }
                    match bytes.next().await {
                        Some(Ok(chunk)) => parser.push(&chunk),
                        Some(Err(source)) => {
                            let e = Error::Transport {
                                method: "GET",
                                path: path.clone(),
                                source,
                            };
                            // End after reporting the error: the connection is gone.
                            parser.closed = true;
                            return Some((Err(e), (bytes, parser, path)));
                        }
                        None => return None,
                    }
                    if parser.closed {
                        return None;
                    }
                }
            });
        Ok(LogStream {
            inner: Box::pin(stream),
        })
    }
}
