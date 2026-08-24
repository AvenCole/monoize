# Dashboard Session Authentication Specification

## 0. Scope

This specification defines browser storage and transport of dashboard sessions.

## 1. Session cookie

DSA1. Successful login and registration MUST set `monoize_session` with attributes `HttpOnly`, `Secure`, `SameSite=Strict`, and `Path=/`.

DSA2. Dashboard browser requests MUST send cookies with `credentials: "include"`.

DSA3. The dashboard browser MUST NOT store the dashboard session token in `localStorage`, `sessionStorage`, or IndexedDB.

DSA4. The dashboard browser MUST NOT read the dashboard session token from browser storage.

DSA5. The dashboard browser MUST NOT add an `Authorization` header for dashboard session authentication. This rule applies to REST and SSE requests.

DSA6. The dashboard browser MUST determine its authenticated state by calling `GET /api/dashboard/auth/me` with the session cookie.

DSA7. Logout MUST invalidate the server session identified by the cookie and MUST expire the `monoize_session` cookie.

## 2. Non-browser clients

DSA8. The backend MAY accept `Authorization: Bearer <session-token>` for non-browser dashboard clients. This compatibility MUST NOT cause the dashboard browser to expose or persist the token.
