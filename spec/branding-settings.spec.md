# Branding Settings Specification

## 0. Scope

This specification defines administrator-managed dashboard branding assets. Text site
name and description continue to use the system settings contract.

## 1. Logo upload API

BR-L1. `POST /api/dashboard/branding/logo` MUST require an administrator session and
accept one multipart file field named `logo`. The raw upload MUST be no larger than
1 MiB. Accepted media types are PNG, JPEG, and WebP. SVG and all other formats MUST
be rejected with HTTP `400` and code `invalid_logo`.

BR-L2. The server MUST decode the image, reject malformed data, and reject images
larger than 2048 pixels on either edge or 4,000,000 total pixels. It MUST re-encode
the accepted image as PNG before persistence. The stored bytes MUST therefore contain
no executable markup.

BR-L3. A successful upload MUST replace the existing logo atomically and return JSON
`{ "configured": true, "url": "/api/dashboard/branding/logo" }`. The API MUST NOT
return the image bytes in this response.

BR-L4. `DELETE /api/dashboard/branding/logo` MUST require an administrator session,
remove the stored logo, and return `{ "configured": false }`. Deleting an absent logo
is successful and idempotent.

## 2. Public logo retrieval

BR-G1. `GET /api/dashboard/branding/logo` MUST be available without authentication.
When configured, it MUST return the persisted PNG with media type `image/png` and a
`Cache-Control: no-cache` header. When no logo is configured it MUST return HTTP `404`
with code `logo_not_configured`.

BR-G2. The endpoint MUST be routed before the frontend fallback so that a missing
logo response is never replaced by `index.html`.

## 3. Dashboard behavior

BR-U1. Login and dashboard navigation MUST read the public logo endpoint and render
the image when it returns `200`. On `404`, load failure, or image decode failure they
MUST render the existing built-in Monoize mark.

BR-U2. The configured site name MUST be used as the logo alternate text, document
title, and every visible dashboard brand label, including the first line of the
top-left `site name + Console` lockup. Empty or whitespace-only site names MUST use
the existing `Monoize` fallback.

BR-U2a. The document favicon MUST probe `/api/dashboard/branding/logo` and use the
persisted PNG when available. A missing, unavailable, or undecodable logo MUST
restore `/monoize.svg` without replacing the built-in fallback mark.

BR-U3. The settings page MUST provide upload, preview, and reset controls in the
existing site-information category. Upload and reset mutations MUST update the
visible preview without a manual page reload. All controls and errors MUST use the
four supported dashboard locales.

BR-U4. The browser MUST not persist uploaded image bytes outside the normal HTTP
cache. The fallback mark MUST remain available when the endpoint is unavailable.
