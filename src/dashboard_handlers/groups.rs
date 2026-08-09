use crate::app::AppState;
use crate::dashboard_handlers::session_helpers::get_current_user;
use crate::error::{AppError, AppResult};
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct DashboardGroupsResponse {
    pub groups: Vec<String>,
}

pub async fn list_dashboard_groups(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<DashboardGroupsResponse>> {
    get_current_user(&headers, &state).await?;

    let groups = state
        .monoize_store
        .list_dashboard_group_labels()
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", e))?;

    Ok(Json(DashboardGroupsResponse { groups }))
}

#[cfg(test)]
mod tests {
    use super::{DashboardGroupsResponse, list_dashboard_groups};
    use crate::app::{RuntimeConfig, load_state_with_runtime};
    use crate::users::UserRole;
    use axum::Json;
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue};
    use sea_orm::ConnectionTrait;

    #[tokio::test]
    async fn dashboard_groups_endpoint_returns_aggregated_labels_for_authenticated_user() {
        let state = load_state_with_runtime(RuntimeConfig {
            listen: "127.0.0.1:0".to_string(),
            metrics_path: "/metrics".to_string(),
            database_dsn: "sqlite::memory:".to_string(),
            request_log_spool_dir: None,
        })
        .await
        .expect("state loads");

        let user = state
            .user_store
            .create_user(
                "dashboard_reader",
                "password123",
                UserRole::User,
                &[" Team-A ".to_string(), "".to_string()],
            )
            .await
            .expect("user created");
        let session = state
            .user_store
            .create_session(&user.id, 7)
            .await
            .expect("session created");
        let api_key_owner = state
            .user_store
            .create_user("api_owner", "password123", UserRole::User, &[])
            .await
            .expect("api owner created");
        state
            .user_store
            .create_api_key_extended(
                &api_key_owner.id,
                crate::users::CreateApiKeyInput {
                    name: "reader key".to_string(),
                    expires_in_days: None,
                    sub_account_enabled: false,
                    sub_account_balance_nano_usd: None,
                    model_limits_enabled: false,
                    model_limits: Vec::new(),
                    ip_whitelist: Vec::new(),
                    allowed_groups: vec!["gamma".to_string(), "Beta".to_string()],
                    max_multiplier: None,
                    transforms: Vec::new(),
                    model_redirects: Vec::new(),
                    reasoning_envelope_enabled: true,
                    request_capture_mode: crate::users::RequestCaptureMode::Off,
                },
                false,
            )
            .await
            .expect("api key created");

        state
            .monoize_store
            .create_provider(crate::monoize_routing::CreateMonoizeProviderInput {
                name: "provider".to_string(),
                enabled: true,
                priority: Some(0),
                max_retries: -1,
                channel_max_retries: 0,
                channel_retry_interval_ms: 0,
                circuit_breaker_enabled: true,
                per_model_circuit_break: false,
                groups: vec!["beta".to_string(), " delta ".to_string()],
                channels: vec![crate::monoize_routing::CreateMonoizeChannelInput {
                    id: None,
                    name: "ch".to_string(),
                    provider_type: crate::monoize_routing::MonoizeProviderType::Responses,
                    base_url: "https://example.com".to_string(),
                    api_key: Some("secret".to_string()),
                    weight: 1,
                    enabled: true,
                    passive_failure_count_threshold_override: None,
                    passive_window_seconds_override: None,
                    passive_cooldown_seconds_override: None,
                    passive_rate_limit_cooldown_seconds_override: None,
                    models: std::collections::HashMap::from([(
                        "gpt-5".to_string(),
                        crate::monoize_routing::MonoizeModelEntry {
                            redirect: None,
                            multiplier: crate::exact_decimal::Multiplier::ONE,
                        },
                    )]),
                    active_probe_enabled_override: None,
                    active_probe_interval_seconds_override: None,
                    active_probe_success_threshold_override: None,
                    active_probe_model_override: None,
                    affinity_enabled_override: None,
                    affinity_idle_ttl_seconds_override: None,
                    affinity_failback_mode_override: None,
                    affinity_failback_delay_seconds_override: None,
                }],
                transforms: Vec::new(),
                api_type_overrides: Vec::new(),
                active_probe_enabled_override: None,
                active_probe_interval_seconds_override: None,
                active_probe_success_threshold_override: None,
                active_probe_model_override: None,
                request_timeout_ms_override: None,
                extra_fields_whitelist: None,
                strip_cross_protocol_nested_extra: None,
            })
            .await
            .expect("provider created");

        state
            .user_store
            .db
            .write()
            .await
            .execute(state.user_store.db.stmt(
                "UPDATE users SET allowed_groups = $1 WHERE id = $2",
                vec!["not-json".into(), api_key_owner.id.clone().into()],
            ))
            .await
            .expect("corrupt user groups");
        state
            .user_store
            .db
            .write()
            .await
            .execute(state.user_store.db.stmt(
                "UPDATE api_keys SET allowed_groups = $1",
                vec![r#"["Gamma"," epsilon ",""]"#.into()],
            ))
            .await
            .expect("override api key groups");
        state
            .user_store
            .db
            .write()
            .await
            .execute(state.user_store.db.stmt(
                "UPDATE monoize_providers SET groups = $1",
                vec![r#"["beta"," delta ","BETA"]"#.into()],
            ))
            .await
            .expect("override provider groups");

        assert_eq!(
            state
                .monoize_store
                .list_dashboard_group_labels_with_batch_size(2)
                .await
                .expect("bounded group scan succeeds"),
            vec![
                "beta".to_string(),
                "delta".to_string(),
                "epsilon".to_string(),
                "gamma".to_string(),
                "team-a".to_string(),
            ]
        );

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {}", session.token)).expect("header value"),
        );

        let response = list_dashboard_groups(State(state), headers)
            .await
            .expect("handler succeeds");
        let Json(body) = response;
        let body: DashboardGroupsResponse = body;

        assert_eq!(
            body.groups,
            vec![
                "beta".to_string(),
                "delta".to_string(),
                "epsilon".to_string(),
                "gamma".to_string(),
                "team-a".to_string(),
            ]
        );
    }
}
