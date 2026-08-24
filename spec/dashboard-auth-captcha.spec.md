# Dashboard Authentication CAPTCHA Specification

## 0. Scope

This specification defines Cap challenge configuration and verification for the public dashboard login and registration endpoints.

## 1. Configuration

CAP-C1. `MONOIZE_CAP_API_ENDPOINT` MUST contain the public Cap site endpoint used by the browser, including the site key path. The value MUST be an absolute `http` or `https` URL with a host. Monoize MUST normalize its path to end with `/`. The value MUST NOT contain credentials, a query string, or a fragment.

CAP-C2. `MONOIZE_CAP_SECRET_KEY` MUST contain the secret key for the site identified by `MONOIZE_CAP_API_ENDPOINT`. Monoize MUST NOT return this value through any API or log it.

CAP-C3. If both variables are absent or empty, Monoize MAY start, but login and registration MUST fail closed as specified by CAP-V4. If exactly one variable is non-empty, or if the endpoint is invalid, startup MUST fail with code `cap_config_invalid`.

CAP-C4. `GET /api/dashboard/settings/public` MUST add `cap_api_endpoint`. The value MUST equal the normalized `MONOIZE_CAP_API_ENDPOINT` when Cap is configured and MUST be `null` otherwise. The settings store MUST continue to query only the four database keys listed in `database-configuration.spec.md` DB23e.

## 2. Authentication request contract

CAP-A1. `POST /api/dashboard/auth/login` and `POST /api/dashboard/auth/register` MUST accept `captcha_token: string` in the JSON request body.

CAP-A2. A missing token, an empty token after trimming, or a token longer than 4096 bytes MUST return HTTP `400` with code `captcha_required`. The handler MUST NOT query or mutate user, session, or settings state after this rejection.

CAP-A3. Monoize MUST NOT derive an authentication admission decision from the client IP address. The authentication request path MUST NOT contain an IP-keyed rate limiter, IP-key capacity, IP-key cleanup task, or `MONOIZE_AUTH_RATE_LIMIT_MAX_KEYS` compatibility setting.

## 3. Server verification

CAP-V1. Before username, password, account, registration-state, or session processing, Monoize MUST send one JSON request to `<cap_api_endpoint>siteverify`:

```json
{
  "secret": "<MONOIZE_CAP_SECRET_KEY>",
  "response": "<captcha_token>"
}
```

Monoize MUST NOT include a client IP field or a forwarded client IP header in this request.

CAP-V2. The verification request timeout MUST be 5 seconds. Its response body MUST be limited to 4096 bytes. Monoize MUST NOT follow redirects from the verification endpoint.

CAP-V3. A `2xx` verification response with JSON field `success: true` MUST authorize the handler to continue. A `2xx` response without boolean `success: true` MUST return HTTP `400` with code `captcha_invalid`.

CAP-V4. Missing Cap configuration, a transport error, a timeout, a non-`2xx` response, an over-limit response body, or invalid response JSON MUST return HTTP `503` with code `captcha_unavailable`.

CAP-V5. The handler MUST verify each request token exactly once. It MUST NOT retry verification. Cap tokens are single-use; after any post-verification authentication error, the client MUST obtain a new token before another submission.

## 4. Dashboard client

CAP-U1. The login page MUST load the Cap widget from the pinned frontend package and MUST set `data-cap-api-endpoint` to the public `cap_api_endpoint` value.

CAP-U2. The frontend MUST bundle the pinned Cap WASM solver and its pako decompression fallback as same-origin build assets. The widget MUST use those assets through `CAP_CUSTOM_WASM_URL` and `CAP_PAKO_URL`; it MUST NOT depend on a runtime CDN request for either asset.

CAP-U3. While public settings are loading, the login form MUST show a skeleton in the widget position and MUST disable submission. If `cap_api_endpoint` is `null`, the page MUST show a configuration error and MUST disable submission.

CAP-U4. The client MUST enable submission only after the widget emits a non-empty token. The client MUST send that token as `captcha_token` for login or registration.

CAP-U5. After a failed login or registration request, the client MUST clear the stored token and reset the widget. Switching between login and registration MUST also clear the token and reset the widget.

CAP-U6. Widget solve and widget error messages MUST use the active dashboard locale. Supported dashboard locales MUST remain English, Simplified Chinese, Traditional Chinese, and Japanese.

## 5. Content Security Policy

CAP-S1. The response Content Security Policy MUST allow `connect-src` only from `'self'` and the configured Cap endpoint origin. An unconfigured deployment MUST keep `connect-src 'self'`.

CAP-S2. The policy MUST allow Cap solver workers from same-origin URLs and `blob:` URLs.

CAP-S3. Each HTTP response MUST receive a fresh script nonce. Embedded SPA entry responses MUST expose that nonce through a non-script metadata element. The dashboard client MUST set `CAP_SCRIPT_NONCE` to that value before loading the Cap widget. The policy MUST NOT add `'unsafe-inline'` to `script-src`.
