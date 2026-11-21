# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Releases are automatic: when a commit on `main` changes the version in
`Cargo.toml` and this file has a section for that version,
`.github/workflows/release.yml` tags `v<version>` and publishes a GitHub
Release with that section as its notes. `python3 scripts/changelog.py release
<version>` makes both edits. Consumers pin a tag:
`runpod = { git = "https://github.com/safewords/rs-runpod-sdk", tag = "v0.1.0" }`.

Add what you change under `[Unreleased]` in the same commit as the change.

## [Unreleased]

## [0.1.0] - 2026-11-20

### Added

- An async client for every operation in Runpod's REST API v2 OpenAPI
  document (`https://api.runpod.io/v2/openapi.json`, 2.0.0): Pods and queued
  Pods, Clusters, Serverless endpoints with their workers, releases, builds
  and metrics, templates, network and global volumes, container registries
  and ECR delegations, the catalog, cost centers, tags, audit logs, billing,
  the account, secrets and SSH keys, the organization, and signup.
- `Client::openapi_document()`: Runpod's own OpenAPI document
  (`GET /v2/openapi.json`), which the API overview offers but the document
  does not list among its paths.
- Pod and Serverless worker logs as a `Stream` of events, resumable with
  `Last-Event-ID`.
- `*_all` variants of every paginated list that follow the cursor to the end.
- Retries with jittered backoff: reads and idempotent writes on network
  errors, 429 and 5xx; creates only on 429 and refused connections.
  `Retry-After` is honoured, and a spent `RateLimit` window is waited out
  before the next request instead of spending it on a 429.
- `Other(String)` on every enum, so values Runpod adds later still decode.
- `Debug` that prints `<redacted>` for secret values, registry passwords,
  verification codes, issued API keys and `env`.
- The types and operations are generated from a checked-in copy of the
  OpenAPI document by `codegen/generate.py`, one module per API area.

[Unreleased]: https://github.com/safewords/rs-runpod-sdk/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/safewords/rs-runpod-sdk/releases/tag/v0.1.0
