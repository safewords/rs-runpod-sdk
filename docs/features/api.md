# API areas: the generated operations and types

> **Status:** PARTIAL (verified against the spec, not yet against live Runpod)
> **Owners (code):** `src/api/` (generated), `openapi/runpod-v2.json`
> **Related docs:** [codegen.md](./codegen.md), [client.md](./client.md)
> **Last verified against code:** 2026-11-20 @ the initial commit

## Overview

`src/api/` holds one module per tag in Runpod's OpenAPI document, plus
`common` for types several areas share and none can claim by name:

| Module | Operations | |
|---|---|---|
| `account` | account, limits, SSH keys, secrets | |
| `audit_logs` | audit log search | |
| `billing` | balance, and billing history by resource kind | |
| `catalog` | GPU and CPU types, data centers, public templates | |
| `clusters` | multi-node clusters and their pods | |
| `cost_centers` | cost centers and resource attribution | |
| `global_volumes`, `network_volumes` | storage | |
| `organization` | the organization, groups, members, invites, roles | operations split per sub-resource |
| `pods` | pods, actions, exec, queued pods | logs are in `src/logs/` |
| `registries` | registry credentials, ECR delegations | |
| `serverless` | endpoints, workers, releases, builds, metrics | operations split per sub-resource; worker logs are in `src/logs/` |
| `signup` | email-code sign-in that issues an API key | no API key needed |
| `tags` | tags and their resources | |
| `templates` | templates and READMEs | |

Each module has `operations.rs` (or `operations/` per sub-resource) adding
methods to `Client`, and `models.rs`, `enums.rs`, `requests.rs`,
`responses.rs` as needed. The module's `mod.rs` lists its operations as
`METHOD /path`. Everything is re-exported at the crate root.

## How it works today

### Shapes

- **Arguments**: path parameters, then required query/header parameters,
  then `params: &<Op>Params` for the optional ones, then `body: &<Request>`.
- **Query encoding**: arrays the spec marks `explode: false` are
  comma-joined; `filters` is URL-encoded JSON, as Runpod specifies
  (checked in `tests/operations.rs`).
- **Answers**: one JSON type → `T`; JSON or 204 → `Option<T>`; several
  statuses with different bodies → a `<Op>Response` enum by status;
  Markdown READMEs → `String`; 204 → `()`.
- **Pagination**: list operations with a cursor have `<op>_all`, following
  `pagination.nextCursor` while `hasNextPage`
  (`codegen/runpod_codegen/operations.py:282`).
- **Serialization**: unset optional fields are omitted, never sent as
  `null`; Runpod's update operations are PATCHes where omitted means "leave
  it".

### Credentials under `{:?}`

Structs with a field named `password`, `apiKey`, `secret`, `token`,
`codeVerifier` or `env`, a `value` on a secret, or `code` on a verify
request print `<redacted>` for it (`codegen/runpod_codegen/schemas.py:9`).
`env` is included because people put tokens there, which hides ordinary
environment variables in `Pod`'s debug output too.

### Edge cases & gotchas

- A field the spec marks required is not an `Option`; if live Runpod omits
  it, decoding the whole answer fails.
- `create_pod` can queue: `CreatePodResponse::Accepted(QueuedPod)` when there
  is no capacity and the request asked for a queue.
- See the known defects in [README.md](./README.md).

## History & references

- Source document: `https://api.runpod.io/v2/openapi.json`, version 2.0.0,
  fetched 2026-11-20. Human docs: `https://docs.runpod.io/api-reference-v2/overview`.
