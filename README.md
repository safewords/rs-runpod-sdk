# rs-runpod-sdk

An unofficial, third-party async Rust client for the
[Runpod](https://www.runpod.io) REST API v2. Not affiliated with or endorsed by
Runpod.

It covers every operation in Runpod's OpenAPI document
(`https://api.runpod.io/v2/openapi.json`, version 2.0.0): 121 operations
across Pods and queued Pods, Clusters, Serverless endpoints (workers,
releases, builds, metrics, logs), templates, network and global volumes,
container registries, the catalog, cost centers, tags, audit logs, billing,
the account, the organization, and signup.

```toml
[dependencies]
runpod = { git = "https://github.com/safewords/rs-runpod-sdk", tag = "v0.1.0" }
```

```rust
use runpod::{Client, CreateGpuConfig, CreatePodRequest, CreatePodResponse, PodExecRequest};

let runpod = Client::new(std::env::var("RUNPOD_API_KEY")?)?;

let pod = match runpod
    .create_pod(&CreatePodRequest {
        name: "worker".into(),
        image: Some("runpod/base:0.6.2-cuda12.4.1".into()),
        gpu: Some(CreateGpuConfig { id: "NVIDIA GeForce RTX 4090".into(), count: Some(1), ..Default::default() }),
        ..Default::default()
    })
    .await?
{
    CreatePodResponse::Created(pod) => pod,
    CreatePodResponse::Accepted(queued) => todo!("queued as {} until capacity opens", queued.id),
};

let out = runpod
    .exec_pod(&pod.id, &PodExecRequest { command: vec!["nvidia-smi".into()], ..Default::default() })
    .await?;
println!("{}", out.stdout);

runpod.delete_pod(&pod.id).await?;
```

## Shape of the API

- **Method names are Runpod's `operationId`s in snake case**: `listPods` is
  `Client::list_pods`, `getEndpointRequestMetrics` is
  `Client::get_endpoint_request_metrics`. Each method's rustdoc ends with its
  `METHOD /path`.
- **Optional query parameters** go in a `<Operation>Params` struct that
  implements `Default`; required ones are arguments.
- **Paginated lists** also have `<operation>_all`, which follows `nextCursor`
  and returns every item.
- **An answer that depends on the status** is an enum: `create_pod` returns
  `CreatePodResponse::Created(Pod)` (201) or `Accepted(QueuedPod)` (202).
  An operation that may answer 204 returns `Option<T>`.
- **Logs** stream as Server-Sent Events: `stream_pod_logs` and
  `stream_worker_logs` return a `Stream` of `LogEvent`s.
- **Every enum has `Other(String)`**, so a value Runpod adds later decodes.
- **Types are re-exported at the crate root** and also live in their area's
  module: `runpod::Pod` and `runpod::pods::Pod` are the same type.

## Retries and rate limits

Reads and idempotent writes (`GET`, `PUT`, `PATCH`, `DELETE`) are retried on
network errors, 429 and 5xx, with jittered exponential backoff, up to four
attempts. A `POST` that creates or acts is retried only on 429 (Runpod refused
it before doing anything) and on a refused connection: retrying after a lost
answer could create a second pod. `Retry-After` is honoured. When Runpod's
`RateLimit` header says a window is spent, the next request waits for it to
reset instead of spending a request on a 429. `Client::rate_limit()` returns
the quota Runpod last reported.

## Examples

```sh
RUNPOD_API_KEY=... cargo run --example catalog   # read-only: account, GPU types and prices
RUNPOD_API_KEY=... cargo run --example pod       # spends money: create, exec, logs, terminate
```

## Layout

```
src/client/     Client, ClientBuilder, transport (auth, retries, rate limits)
src/api/        generated: one module per API area, each split into
                operations, models, enums, requests and responses
src/logs/       the two Server-Sent Events log streams
codegen/        the generator, and openapi/runpod-v2.json is its input
scripts/        changelog.py: validate CHANGELOG.md, cut a release
```

`src/api/` is generated. To pick up a change to Runpod's API, replace
`openapi/runpod-v2.json` with the current document and run
`python3 codegen/generate.py`; CI fails if the two disagree.

## Releases

Automatic. `python3 scripts/changelog.py release 0.2.0` moves `[Unreleased]`
in [`CHANGELOG.md`](CHANGELOG.md) to `[0.2.0]` and bumps `Cargo.toml`; merging
that to `main` makes `.github/workflows/release.yml` run CI, tag `v0.2.0`, and
publish a GitHub Release with that section as its notes. Details in
[`docs/features/testing-and-releases.md`](docs/features/testing-and-releases.md).

## Feature docs

How each part works, with `path:line` references, and the known defects:
[`docs/features/README.md`](docs/features/README.md).

## License

MIT
