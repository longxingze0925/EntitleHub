# Client SDK

Rust helper library for client activation, customer login, session caching, signed device requests, update verification, script verification, device key rotation, and client-side AI gateway calls.

The SDK is intentionally transport-agnostic. Your application owns HTTP, secure local storage, clock source, nonce generation, and retry policy. The SDK builds request payloads, validates backend response envelopes, signs protected requests, and serializes cache state.

## Responsibilities

Use the SDK for:

- Generating and loading `DeviceIdentity`.
- Building activation and customer-login request payloads.
- Building transport-agnostic request parts for every `/api/client/...` endpoint.
- Parsing wrapped backend `ApiResponse` JSON.
- Managing in-memory session refresh state.
- Building `Authorization` and device-signature headers for protected client APIs.
- Building OpenAI-compatible client AI gateway payloads without exposing upstream provider keys.
- Storing and restoring `SdkCacheEnvelope`.
- Applying device key rotation responses atomically to cache state.
- Verifying update packages and secure scripts with JWKS-backed Ed25519 keys.

The host application must provide:

- HTTPS HTTP client.
- Durable encrypted storage for `SdkCacheEnvelope`.
- A monotonic enough Unix timestamp source.
- Fresh nonces for every signed request.
- The actual refresh-token API call when the SDK asks for a refresh.
- Crash-safe persistence after auth, refresh, logout, JWKS update, and key rotation.

## Module Map

```text
access_token  JWT access-token validation helpers
ai            AI gateway payload/response helpers
auth          activation, customer login, heartbeat, verify, logout models
cache         durable SDK cache envelope
client        endpoint paths and high-level request part builders
device        machine id, device key generation, device key rotation payloads
jwks          JWKS parsing/cache and EdDSA key lookup
request       authorized signed request header builders
response      backend ApiResponse envelope parsing
script        secure script model and verification helpers
session       session state and refresh handling
signing       Ed25519 device request signing helpers
update        release/update model and verification helpers
```

## Endpoint Coverage

The `client` module exposes constants and request builders for the supported client surface:

```text
POST   /api/client/auth/activate
POST   /api/client/auth/login
POST   /api/client/auth/refresh
POST   /api/client/auth/heartbeat
POST   /api/client/auth/verify
POST   /api/client/auth/logout
POST   /api/client/auth/email/verify/request
POST   /api/client/auth/email/verify/confirm
POST   /api/client/auth/password/reset/confirm
GET    /api/client/apps/{app_key}/jwks
GET    /api/client/releases/latest
GET    /api/client/releases/download/{file_name}
GET    /api/client/secure-scripts/versions
POST   /api/client/secure-scripts/fetch
DELETE /api/client/devices/self
POST   /api/client/devices/self/rotate-key
GET    /api/client/ai/v1/models
POST   /api/client/ai/v1/chat/completions
POST   /api/client/ai/v1/images/generations
POST   /api/client/ai/v1/videos/generations
POST   /api/client/ai/v1/embeddings
```

OpenAI-compatible `/v1/...` routes are for API-key callers. A normal logged-in client should use `/api/client/ai/v1/...`; the SDK signs those requests with the client session and device key, and no customer-side AI API key is needed.

## Basic Flow

Generate or load a device identity before activation/login:

```rust
use client_sdk::device::DeviceIdentity;

let device = DeviceIdentity::generate("app_key", &["machine-fingerprint"])?;
```

Build an activation payload and send it to `POST /api/client/auth/activate` with your HTTP client:

```rust
use client_sdk::auth::ClientBootstrap;

let bootstrap = ClientBootstrap::new(device.clone())?;
let payload = bootstrap.activation_request(
    "app_key",
    "license_key",
    Some("Workstation"),
    Some("Windows"),
    Some("1.0.0"),
)?;
let body = serde_json::to_vec(&payload)?;
```

Parse the backend response with the wrapped-response parser, then create a cache envelope:

```rust
use client_sdk::{
    cache::SdkCacheEnvelope,
    jwks::JwksCache,
    session::ClientAuthSessionResponse,
};

let auth = ClientAuthSessionResponse::from_api_response_json(&response_body)?;
let device_key_id = auth.device_key_id.clone();
let session = bootstrap.apply_auth_response(auth, now_unix)?;

let cache = SdkCacheEnvelope::new_with_device_key_id(
    "app_key",
    device,
    device_key_id.as_deref(),
    Some(session),
    &JwksCache::default(),
    now_unix,
)?;

let persisted = cache.to_json()?;
```

Store the serialized cache in encrypted local storage. On next launch, restore with `SdkCacheEnvelope::from_json`.

## Protected Requests

For protected client APIs, restore the cache, create a session manager from it, and ask the SDK to build request parts:

```rust
use client_sdk::client::{
    heartbeat_request,
    ProtectedClientRequestContext,
};

let cache = SdkCacheEnvelope::from_json(&persisted_cache_json)?;
let session_manager = cache.session_manager();

let request = heartbeat_request(
    ProtectedClientRequestContext {
        cache: &cache,
        session_manager: &session_manager,
        timestamp: now_unix,
        nonce: "unique-random-nonce",
        refresh_before_seconds: 60,
    },
    Some("1.0.0"),
    |current_session| {
        // Call POST /api/client/auth/refresh with current_session.refresh_token,
        // parse the backend response, and return SessionRefresh.
        refresh_session(current_session)
    },
)?;
```

Send `request.method`, `request.path`, `request.body`, and every returned header with your HTTP client:

```text
Authorization
X-Device-Id
X-Device-Key-Id
X-Timestamp
X-Nonce
X-Body-SHA256
X-Signature
```

If the refresh closure succeeds, `SessionManager` updates its in-memory session. Persist a new `SdkCacheEnvelope` after a refresh if your HTTP layer observes that tokens changed.

## Client AI Gateway

Client AI calls use the signed client routes and return raw provider-compatible JSON on success. Failed EntitleHub checks can still return the normal backend error envelope.

```rust
use client_sdk::{
    ai::{AiGatewayJsonResponse, AiModelListResponse},
    client::{ai_chat_completions_request, ai_models_request, ai_video_generations_request, ProtectedClientRequestContext},
};

let context = ProtectedClientRequestContext {
    cache: &cache,
    session_manager: &session_manager,
    timestamp: now_unix,
    nonce: "unique-random-nonce",
    refresh_before_seconds: 60,
};

let models_request = ai_models_request(context, |session| refresh_session(session))?;
// Send GET models_request.path with models_request.headers.
let models = AiModelListResponse::from_json(&models_response_body)?;

let chat_request = ai_chat_completions_request(
    context,
    &serde_json::json!({
        "model": "gpt-test",
        "messages": [{ "role": "user", "content": "hello" }]
    }),
    Some("idempotency-key-1"),
    |session| refresh_session(session),
)?;
// Send POST chat_request.path with chat_request.body and chat_request.headers.
let chat = AiGatewayJsonResponse::from_json_with_usage_id(
    &chat_response_body,
    response_headers.get("x-entitlehub-usage-id"),
)?;

let video_request = ai_video_generations_request(
    context,
    &serde_json::json!({
        "model": "video-test",
        "prompt": "short intro",
        "duration": 8
    }),
    Some("idempotency-key-2"),
    |session| refresh_session(session),
)?;
// Send POST video_request.path with video_request.body and video_request.headers.
```

The backend handles balance checks, subscription gating, provider forwarding, and image/video caching. For image responses, `ai::image_urls_from_response` extracts URLs after the backend has replaced provider assets with EntitleHub asset URLs. For video responses, `ai::video_urls_from_response` extracts cached EntitleHub asset URLs from common provider response shapes.

For Web products, do not put EntitleHub Server Keys in the browser. Keep the Server Key in your business backend and call the server-side async generation routes:

```text
POST /api/server/ai/v1/images/jobs
POST /api/server/ai/v1/videos/jobs
GET  /api/server/ai/v1/jobs/{job_id}
```

Those routes are designed for async providers such as image/video task APIs. EntitleHub reserves wallet balance when the job is created, polls the provider result, caches generated assets under `/api/ai/assets/{id}`, and only captures the charge after the assets are ready.

## Device Key Rotation

Rotation uses the old active device key to sign the rotation request and stores the new key only after the backend confirms it.

```rust
use client_sdk::device::{
    build_rotate_device_key_request,
    RotateDeviceKeyResponse,
};

let next_device = cache.device.rotate_key()?;
let payload = build_rotate_device_key_request(&next_device)?;
let body = serde_json::to_vec(&payload)?;

let headers = build_authorized_cached_device_request(
    &cache,
    &session_manager,
    CachedAuthorizedDeviceRequestInput {
        method: "post",
        path: "/api/client/devices/self/rotate-key",
        body: &body,
        timestamp: now_unix,
        nonce: "unique-random-nonce",
        refresh_before_seconds: 60,
    },
    |current_session| refresh_session(current_session),
)?;

// Send body and headers to POST /api/client/devices/self/rotate-key.
let rotate = RotateDeviceKeyResponse::from_api_response_json(&response_body)?;
cache.apply_device_key_rotation(next_device, &rotate.device_key_id, now_unix)?;
persist(cache.to_json()?);
```

Do not overwrite the stored cache with `next_device` before the backend response succeeds. If the request fails, keep using the previous cached device identity and `device_key_id`.

## Logout

Default logout behavior clears the session while keeping the device identity, current `device_key_id`, and JWKS cache:

```rust
use client_sdk::cache::LogoutClearOptions;

let next_cache = cache.into_logout_cache(LogoutClearOptions::default(), now_unix)?;
```

If `next_cache` is `Some`, persist it. If it is `None`, delete the SDK cache from local storage.

## Response Parsing

Prefer `from_api_response_json` for backend responses because the backend returns this envelope:

```json
{
  "code": 0,
  "message": "ok",
  "data": {},
  "request_id": "req_..."
}
```

On non-zero `code`, SDK parsing returns `SdkError::ApiError(code, message)`.

## Verification

Run the SDK checks from this directory:

```bash
cargo fmt --check
cargo test
cargo run --example recommended_flow
```

The integration-style unit test in `src/lib.rs` documents the recommended full flow across activation, cache, signed protected requests, response parsing, device key rotation, and logout.

The executable example in `examples/recommended_flow.rs` demonstrates the same flow with fake backend JSON and no HTTP dependency.

## Live Backend Smoke

The ignored tests in `tests/live_backend.rs` verify the SDK against a running backend. They cover activation, refresh-token rotation, JWKS-backed access-token validation, signed heartbeat headers, heartbeat response parsing, customer login, and AI subscription gating.

From the repository root, prefer the ops wrapper because it creates short-lived test licenses, customers, and subscriptions through the Admin API without printing secrets:

```powershell
pwsh -File ops/smoke-init-owner.ps1 -RunMigrations
pwsh -File ops/smoke-client-sdk.ps1
```

To run the activation ignored test directly, set `SDK_SMOKE_BACKEND_URL`, `SDK_SMOKE_APP_KEY`, `SDK_SMOKE_LICENSE_KEY`, and `SDK_SMOKE_JWT_ISSUER`. `SDK_SMOKE_MACHINE_ID` and `SDK_SMOKE_JWT_AUDIENCE` are optional. To run the customer login AI gate test directly, also set `SDK_SMOKE_CUSTOMER_EMAIL`, `SDK_SMOKE_CUSTOMER_PASSWORD`, and `SDK_SMOKE_AI_EXPECT_SUBSCRIPTION`.
