use bytes::BytesMut;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

const CAP_API_ENDPOINT_ENV: &str = "MONOIZE_CAP_API_ENDPOINT";
const CAP_SECRET_KEY_ENV: &str = "MONOIZE_CAP_SECRET_KEY";
const VERIFY_RESPONSE_MAX_BYTES: usize = 4096;
const CAPTCHA_TOKEN_MAX_BYTES: usize = 4096;
const VERIFY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct CapVerifier {
    configured: Option<Arc<ConfiguredCap>>,
}

struct ConfiguredCap {
    api_endpoint: reqwest::Url,
    verify_endpoint: reqwest::Url,
    secret_key: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapVerifyError {
    Required,
    Invalid,
    Unavailable,
}

#[derive(Serialize)]
struct SiteVerifyRequest<'a> {
    secret: &'a str,
    response: &'a str,
}

#[derive(Deserialize)]
struct SiteVerifyResponse {
    success: bool,
}

impl CapVerifier {
    pub fn unconfigured() -> Self {
        Self { configured: None }
    }

    pub fn from_env() -> Result<Self, String> {
        let endpoint = nonempty_env(CAP_API_ENDPOINT_ENV);
        let secret = nonempty_env(CAP_SECRET_KEY_ENV);
        match (endpoint, secret) {
            (None, None) => Ok(Self::unconfigured()),
            (Some(endpoint), Some(secret)) => Self::configured(&endpoint, secret),
            _ => Err(format!(
                "{CAP_API_ENDPOINT_ENV} and {CAP_SECRET_KEY_ENV} must be configured together"
            )),
        }
    }

    pub fn configured(api_endpoint: &str, secret_key: String) -> Result<Self, String> {
        let api_endpoint = normalize_api_endpoint(api_endpoint)?;
        let secret_key = secret_key.trim().to_string();
        if secret_key.is_empty() {
            return Err(format!("{CAP_SECRET_KEY_ENV} must not be empty"));
        }
        let verify_endpoint = api_endpoint
            .join("siteverify")
            .map_err(|error| format!("failed to construct Cap siteverify URL: {error}"))?;
        let http = reqwest::Client::builder()
            .no_proxy()
            .timeout(VERIFY_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| format!("failed to build Cap verification client: {error}"))?;
        Ok(Self {
            configured: Some(Arc::new(ConfiguredCap {
                api_endpoint,
                verify_endpoint,
                secret_key,
                http,
            })),
        })
    }

    pub fn public_api_endpoint(&self) -> Option<&str> {
        self.configured
            .as_ref()
            .map(|configured| configured.api_endpoint.as_str())
    }

    pub fn api_origin(&self) -> Option<String> {
        let endpoint = &self.configured.as_ref()?.api_endpoint;
        let origin = endpoint.origin().ascii_serialization();
        (origin != "null").then_some(origin)
    }

    pub async fn verify(&self, token: &str) -> Result<(), CapVerifyError> {
        let token = token.trim();
        if token.is_empty() || token.len() > CAPTCHA_TOKEN_MAX_BYTES {
            return Err(CapVerifyError::Required);
        }
        let configured = self
            .configured
            .as_ref()
            .ok_or(CapVerifyError::Unavailable)?;
        let response = configured
            .http
            .post(configured.verify_endpoint.clone())
            .json(&SiteVerifyRequest {
                secret: &configured.secret_key,
                response: token,
            })
            .send()
            .await
            .map_err(|_| CapVerifyError::Unavailable)?;
        if !response.status().is_success() {
            return Err(CapVerifyError::Unavailable);
        }
        let body = read_limited_body(response, VERIFY_RESPONSE_MAX_BYTES).await?;
        let verification: SiteVerifyResponse =
            serde_json::from_slice(&body).map_err(|_| CapVerifyError::Unavailable)?;
        if verification.success {
            Ok(())
        } else {
            Err(CapVerifyError::Invalid)
        }
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_api_endpoint(raw: &str) -> Result<reqwest::Url, String> {
    let mut endpoint = reqwest::Url::parse(raw.trim())
        .map_err(|error| format!("{CAP_API_ENDPOINT_ENV} is not a valid URL: {error}"))?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(format!(
            "{CAP_API_ENDPOINT_ENV} must use the http or https scheme"
        ));
    }
    if endpoint.host().is_none() || !endpoint.username().is_empty() || endpoint.password().is_some()
    {
        return Err(format!(
            "{CAP_API_ENDPOINT_ENV} must contain a host and must not contain credentials"
        ));
    }
    if endpoint.query().is_some() || endpoint.fragment().is_some() {
        return Err(format!(
            "{CAP_API_ENDPOINT_ENV} must not contain a query string or fragment"
        ));
    }
    if !endpoint.path().ends_with('/') {
        let normalized_path = format!("{}/", endpoint.path());
        endpoint.set_path(&normalized_path);
    }
    Ok(endpoint)
}

async fn read_limited_body(
    response: reqwest::Response,
    max_bytes: usize,
) -> Result<bytes::Bytes, CapVerifyError> {
    if response
        .content_length()
        .is_some_and(|length| length > max_bytes as u64)
    {
        return Err(CapVerifyError::Unavailable);
    }
    let mut body = BytesMut::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| CapVerifyError::Unavailable)?;
        if chunk.len() > max_bytes.saturating_sub(body.len()) {
            return Err(CapVerifyError::Unavailable);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body.freeze())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::response::Response;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::{Value, json};
    use std::sync::Mutex;

    #[derive(Clone)]
    struct TestSiteverify {
        status: StatusCode,
        body: String,
        requests: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    }

    async fn siteverify(
        State(state): State<TestSiteverify>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        state.requests.lock().unwrap().push((headers, body));
        Response::builder()
            .status(state.status)
            .header("content-type", "application/json")
            .body(Body::from(state.body))
            .unwrap()
    }

    async fn test_verifier(
        status: StatusCode,
        body: impl Into<String>,
    ) -> (CapVerifier, TestSiteverify) {
        let state = TestSiteverify {
            status,
            body: body.into(),
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server_state = state.clone();
        tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/site/siteverify", post(siteverify))
                    .with_state(server_state),
            )
            .await
            .unwrap();
        });
        let verifier = CapVerifier::configured(
            &format!("http://{address}/site/"),
            "site-secret".to_string(),
        )
        .unwrap();
        (verifier, state)
    }

    #[test]
    fn endpoint_is_normalized_and_secret_is_not_public() {
        let verifier = CapVerifier::configured(
            "https://cap.example.test/site-key",
            "secret-value".to_string(),
        )
        .unwrap();
        assert_eq!(
            verifier.public_api_endpoint(),
            Some("https://cap.example.test/site-key/")
        );
        assert_eq!(
            verifier.api_origin().as_deref(),
            Some("https://cap.example.test")
        );
        assert!(!verifier.public_api_endpoint().unwrap().contains("secret"));
    }

    #[test]
    fn endpoint_rejects_unsupported_or_ambiguous_urls() {
        for endpoint in [
            "/site-key/",
            "file:///site-key/",
            "https://user:password@cap.example.test/site-key/",
            "https://cap.example.test/site-key/?mode=test",
            "https://cap.example.test/site-key/#fragment",
        ] {
            assert!(CapVerifier::configured(endpoint, "secret".to_string()).is_err());
        }
        assert!(
            CapVerifier::configured("https://cap.example.test/site/", "  ".to_string()).is_err()
        );
    }

    #[tokio::test]
    async fn missing_or_oversized_tokens_are_rejected_before_configuration() {
        let verifier = CapVerifier::unconfigured();
        assert_eq!(verifier.verify("  ").await, Err(CapVerifyError::Required));
        assert_eq!(
            verifier
                .verify(&"x".repeat(CAPTCHA_TOKEN_MAX_BYTES + 1))
                .await,
            Err(CapVerifyError::Required)
        );
        assert_eq!(
            verifier.verify("present").await,
            Err(CapVerifyError::Unavailable)
        );
    }

    #[tokio::test]
    async fn successful_verification_sends_only_secret_and_token_once() {
        let (verifier, state) = test_verifier(StatusCode::OK, r#"{"success":true}"#).await;
        verifier.verify(" solved-token ").await.unwrap();

        let requests = state.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let (headers, body) = &requests[0];
        assert_eq!(
            body,
            &json!({"secret": "site-secret", "response": "solved-token"})
        );
        for name in ["x-forwarded-for", "x-real-ip", "cf-connecting-ip"] {
            assert!(!headers.contains_key(name));
        }
    }

    #[tokio::test]
    async fn verifier_distinguishes_invalid_tokens_from_service_failures() {
        let (invalid, invalid_state) = test_verifier(StatusCode::OK, r#"{"success":false}"#).await;
        assert_eq!(invalid.verify("token").await, Err(CapVerifyError::Invalid));
        assert_eq!(invalid_state.requests.lock().unwrap().len(), 1);

        let (failed, failed_state) =
            test_verifier(StatusCode::INTERNAL_SERVER_ERROR, "failure").await;
        assert_eq!(
            failed.verify("token").await,
            Err(CapVerifyError::Unavailable)
        );
        assert_eq!(failed_state.requests.lock().unwrap().len(), 1);

        let (oversized, oversized_state) =
            test_verifier(StatusCode::OK, "x".repeat(VERIFY_RESPONSE_MAX_BYTES + 1)).await;
        assert_eq!(
            oversized.verify("token").await,
            Err(CapVerifyError::Unavailable)
        );
        assert_eq!(oversized_state.requests.lock().unwrap().len(), 1);
    }
}
