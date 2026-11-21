//! An unofficial async client for the [Runpod](https://www.runpod.io) REST
//! API v2 (`https://api.runpod.io/v2`).
//!
//! Every operation in Runpod's published OpenAPI document is a method on
//! [`Client`]: Pods and queued Pods, Clusters, Serverless endpoints (with
//! workers, releases, builds and metrics), templates, network and global
//! volumes, container registries and ECR delegations, the catalog of GPU and
//! CPU types and data centers, cost centers, tags, audit logs, billing, the
//! account, its secrets and SSH keys, the organization, and signup. Method
//! names are Runpod's `operationId`s in snake case: `listPods` is
//! [`Client::list_pods`]. Paginated lists also have an `_all` variant that
//! follows the cursor to the end.
//!
//! ```no_run
//! # async fn demo() -> Result<(), runpod::Error> {
//! use runpod::{Client, CreatePodRequest, ListGpuTypesParams, PodAction, PodActionRequest};
//!
//! let runpod = Client::new(std::env::var("RUNPOD_API_KEY").unwrap())?;
//!
//! for gpu in runpod.list_gpu_types(&ListGpuTypesParams::default()).await?.gpus {
//!     println!("{} ({} GB)", gpu.id, gpu.memory);
//! }
//!
//! let pods = runpod.list_pods_all(&Default::default()).await?;
//! for pod in &pods {
//!     println!("{} {} {}", pod.id, pod.name, pod.status);
//! }
//! # Ok(()) }
//! ```
//!
//! # Retries and rate limits
//!
//! Reads and idempotent writes (`GET`, `PUT`, `PATCH`, `DELETE`) are retried
//! on network errors, 429 and 5xx with jittered backoff. A `POST` that
//! creates or acts is retried only on 429, which Runpod sends before doing
//! anything, and on refused connections: a retry after a lost answer could
//! create a second pod. `Retry-After` is honoured, and when the `RateLimit`
//! header says a window is spent the client waits for it to reset instead of
//! spending a request on a 429. [`Client::rate_limit`] shows the last quota
//! Runpod reported.
//!
//! # Forward compatibility
//!
//! Every enum has an `Other(String)` variant, so a status or type Runpod adds
//! later decodes rather than failing the whole call.
//!
//! # Secrets in logs
//!
//! Types that carry a credential (secret values, registry passwords,
//! verification codes, the API key signup returns, and `env`, where people
//! put tokens) print `<redacted>` for it under `{:?}`.
//!
//! # Layout
//!
//! - `client/`: the [`Client`], its builder, and the transport (auth,
//!   retries, rate limits).
//! - `api/`: generated from Runpod's OpenAPI document by
//!   `codegen/generate.py`, one module per API area ([`pods`], [`serverless`],
//!   [`templates`], ...), each split into `operations`, `models`, `enums`,
//!   `requests` and `responses`. Every type is also re-exported here at the
//!   root, so `runpod::Pod` and `runpod::pods::Pod` are the same thing.
//! - `logs/`: the two Server-Sent Events log streams, hand-written.
//! - `error.rs`, `rate_limit.rs`: what calls fail with, and quota headers.

mod api;
mod client;
mod error;
mod logs;
mod rate_limit;

pub use api::*;
pub use client::{Client, ClientBuilder, DEFAULT_BASE_URL};
pub use error::{ApiError, Error};
pub use logs::{LogEvent, LogStream, LogStreamParams};
pub use rate_limit::{RateLimit, RateLimitPolicy, RateLimitWindow};
