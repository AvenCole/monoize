# Runtime Resource Bounds Specification

## 0. Status

- **Purpose:** Bound process-local maps and buffers that accept attacker-influenced identities or payloads.
- **Scope:** Applies to `src/rate_limit.rs`, `src/bounded_response.rs`, and the resource-bound APIs referenced by the database, capture, WebSocket, provider-discovery, and image-transform specifications.

## 1. Rate-limit keys

RRB-RL1. A rate-limit key derived from a client IP MUST be constructed from `std::net::IpAddr` and serialized with `IpAddr::to_string()`. Equivalent textual IPv6 forms MUST map to one key.

RRB-RL2. The public typed-key check API MUST accept a canonical `RateLimitKey`. A compatibility string API MUST parse the string as `IpAddr`; every invalid string MUST map to one shared invalid-input key rather than creating an attacker-selected map entry.

RRB-RL3. The distinct-key capacity MUST default to `10000` and be configurable by `MONOIZE_AUTH_RATE_LIMIT_MAX_KEYS`. An unseen key MUST be rejected without insertion when the capacity is full. Admission of an unseen key MUST NOT scan existing keys. Stale-key removal MUST occur only in the explicit periodic `cleanup` operation.

RRB-RL4. The timestamp vector for one key MUST never contain more than `max_requests` entries. `cleanup` MUST remove expired timestamps and empty keys.

## 2. Configuration parsing

RRB-C1. Each resource-bound environment value in this specification and the linked subsystem specifications MUST accept only positive base-10 integers. Missing, zero, invalid, or overflowing values MUST use the documented default.

RRB-C2. All limits are process-local. This version requires no multi-instance cache coherence or distributed quota coordination.

## 3. Upstream discovery response bodies

RRB-UD1. `MONOIZE_UPSTREAM_DISCOVERY_MAX_BYTES` MUST select the maximum response-body byte length yielded by the HTTP client for every upstream provider-model discovery and model-metadata discovery request. Its default MUST be `16777216`. Parsing MUST follow RRB-C1.

RRB-UD2. Before reading a discovery response body, Monoize MUST compare a valid HTTP `Content-Length` value with the selected limit. If `Content-Length` exceeds the limit, Monoize MUST reject the response without reading a body chunk.

RRB-UD3. Monoize MUST read a chunked response or a response without `Content-Length` incrementally. Before appending each chunk, Monoize MUST reject the response if the accumulated byte length plus that chunk would exceed the selected limit. Monoize MUST stop polling the body after this rejection.

RRB-UD4. A response whose yielded body length equals the selected limit MUST be accepted. Empty response bodies MUST be accepted by the byte reader.

RRB-UD5. Discovery code MUST parse JSON or construct error text only from bytes returned by the bounded response reader. Discovery code MUST NOT call `reqwest::Response::json`, `reqwest::Response::text`, or `reqwest::Response::bytes` directly.

RRB-UD6. A body rejected by RRB-UD2 or RRB-UD3 MUST produce an error whose message states the configured byte limit. Dashboard provider-model discovery MUST return HTTP `502` with code `upstream_discovery_response_too_large`. A body transport failure within the limit MUST return the subsystem's existing upstream-fetch error.
