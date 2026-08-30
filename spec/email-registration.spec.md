# Email Registration Specification

## 0. Scope

This specification defines public dashboard registration using a required verified
email address. Existing username-and-password login remains the authentication method.

## 1. SMTP configuration

ER-C1. Email registration MUST require `MONOIZE_SMTP_HOST`,
`MONOIZE_SMTP_PORT`, `MONOIZE_SMTP_USERNAME`, `MONOIZE_SMTP_PASSWORD`, and
`MONOIZE_SMTP_FROM` when the registration flow sends a message. The optional
`MONOIZE_SMTP_FROM_NAME` sets the display name. `MONOIZE_SMTP_SECURITY` MUST be
`starttls` or `tls` and MUST default to `starttls`. The server MUST reject an
unencrypted SMTP mode.

ER-C2. If `MONOIZE_SMTP_HOST` is absent or empty, email registration MUST be
unavailable and existing login MUST remain available. If any other required SMTP
variable is missing or invalid, startup MUST fail with code `smtp_config_invalid`.

ER-C3. SMTP credentials MUST NOT appear in API responses or logs. SMTP connection
attempts MUST have a finite timeout of 10 seconds.

## 2. Registration initiation

ER-R1. `POST /api/dashboard/auth/register` MUST accept JSON fields
`username`, `password`, `email`, and optional `captcha_token`. The handler MUST
trim `username` and `email`; the email key MUST be lowercase. Email validation MUST
require one `@`, a non-empty local part and host, and no control characters.

ER-R2. The handler MUST validate username, password, email, registration state, and
CAPTCHA before sending mail. A password MUST be at least 8 characters. A username
with the reserved `_monoize_` prefix MUST be rejected.

ER-R3. A valid request MUST hash the password and a randomly generated six-digit
numeric code with Argon2. Plaintext passwords and codes MUST NOT be persisted or
logged. The code MUST expire 15 minutes after issuance. The response MUST be HTTP
`202` with JSON fields `registration_id`, normalized `email`, `expires_at`, and
`resend_after` (an RFC3339 timestamp). The response MUST NOT set a session cookie.

ER-R4. The service MUST persist one pending record per normalized email and one per
username. A new pending request for an existing email MUST replace that pending
record after validation. A pending request MUST NOT replace an existing user.

ER-R5. A normalized email already belonging to any user MUST return HTTP `409` with
code `email_exists`. A username already belonging to any user or another pending
record MUST return HTTP `409` with code `username_exists`.

ER-R6. The database record MUST be committed before SMTP delivery. If delivery fails,
the pending record MUST remain retryable and the endpoint MUST return HTTP `503` with
code `email_send_failed`; the code MUST NOT be returned in the response.

## 3. Resend

ER-S1. `POST /api/dashboard/auth/register/resend-code` MUST accept
`registration_id` and optional `captcha_token`. It MUST require a live pending
record and repeat CAPTCHA validation.

ER-S2. A resend MUST be rejected for 60 seconds after the prior send with HTTP `429`
and code `verification_cooldown`. A resend MUST generate and hash a new code, reset
the expiry to 15 minutes, and return HTTP `202` with the same response shape as ER-R3.

## 4. Verification and account creation

ER-V1. `POST /api/dashboard/auth/register/verify` MUST accept
`registration_id` and `code`. It MUST NOT require CAPTCHA. The code MUST contain
exactly six ASCII digits.

ER-V2. An expired or missing pending record MUST return HTTP `410` with code
`verification_expired`. A wrong code MUST increment the attempt counter and return
HTTP `400` with code `verification_invalid`. After five wrong attempts, the record
MUST be deleted and subsequent requests MUST return HTTP `429` with code
`verification_attempts_exceeded`.

ER-V3. Successful verification MUST atomically recheck registration state, global
email uniqueness, username uniqueness, and the first-user role rule, then insert the
user with the normalized verified email and delete the pending record in one write
transaction. No user row MAY exist before this transaction succeeds.

ER-V4. Successful verification MUST create the normal dashboard session and set the
`monoize_session` cookie defined by `dashboard-session-authentication.spec.md`.
The response MUST use the existing `AuthResponse` shape. A failed transaction MUST
create neither a user nor a session.

## 5. Persistence and cleanup

ER-P1. The `pending_registrations` table MUST store only the registration id,
normalized email and email key, username, password hash, code hash, issue and expiry
timestamps, attempt count, and created/updated timestamps. The table MUST have unique
indexes on email key and username and an expiry index.

ER-P2. A primary process MUST remove expired pending records periodically. Cleanup
MUST never remove a non-expired record. Replicas MUST NOT mutate this table.

ER-P3. User email uniqueness MUST be case-insensitive and whitespace-insensitive for
non-empty values. The migration MUST fail rather than silently rewrite legacy
duplicate user emails.
