use crate::app::AppState;
use crate::dashboard_handlers::session_helpers::require_admin;
use crate::error::{AppError, AppResult};
use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};

/// admin-dashboard.spec.md AD-1..AD-5: one admin-only aggregate snapshot of
/// node/system status, replica state, user usage ranking, and channel health.
pub async fn get_admin_overview(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> AppResult<Json<Value>> {
    require_admin(&headers, &state).await?;

    let now = chrono::Utc::now();
    let started_at = state.started_at;
    let uptime_seconds = now
        .signed_duration_since(started_at)
        .to_std()
        .map(|duration| duration.as_secs())
        .unwrap_or(0);

    let database_backend = if state.db_pool.is_sqlite() {
        "sqlite"
    } else if state.db_pool.is_postgres() {
        "postgres"
    } else {
        "unknown"
    };
    let dsn_redacted = redact_dsn(&state.runtime.database_dsn);
    let role = state.node.role.as_str();

    let (spool_pending_count, spool_pending_bytes) = match (role, state.metering.as_ref()) {
        ("replica", Some(metering)) => {
            let (count, bytes) = metering.delta_spool().pending_stats();
            (count, bytes)
        }
        _ => (0usize, 0u64),
    };

    let sse_connections: usize = state
        .sse_connections
        .iter()
        .map(|entry| entry.value().load(std::sync::atomic::Ordering::Relaxed))
        .sum::<usize>()
        .min(usize::MAX);

    let ranking_window_from = (now - chrono::Duration::hours(24)).to_rfc3339();
    let ranking = state
        .user_store
        .get_users_usage_ranking(&ranking_window_from, &now.to_rfc3339(), 20)
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", e))?;
    let users_ranking: Vec<Value> = ranking
        .into_iter()
        .map(|row| {
            json!({
                "user_id": row.user_id,
                "username": row.username,
                "call_count": row.call_count,
                "cost_nano_usd": row.cost_nano_usd.to_string(),
            })
        })
        .collect();

    let providers = state
        .monoize_store
        .list_providers()
        .await
        .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", e))?;
    let health = state.channel_health.lock().await;
    let now_ms = crate::handlers::routing::now_ts();
    let mut channel_health: Vec<Value> = Vec::new();
    for provider in providers {
        for channel in provider.channels {
            let state_entry = health.get(&channel.id);
            channel_health.push(json!({
                "provider_id": provider.id,
                "provider_name": provider.name,
                "channel_id": channel.id,
                "channel_name": channel.name,
                "enabled": channel.enabled,
                "weight": channel.weight,
                "session_affinity_auto": channel.session_affinity_auto.unwrap_or(false),
                "healthy": state_entry.map(|entry| entry.healthy).unwrap_or(true),
                "last_success_at": state_entry.and_then(|entry| entry.last_success_at),
                "cooldown_until": state_entry.and_then(|entry| entry.cooldown_until),
                "probe_success_count": state_entry.map(|entry| entry.probe_success_count).unwrap_or(0),
                "last_probe_at": state_entry.and_then(|entry| entry.last_probe_at),
                "cooldown_active": state_entry.and_then(|entry| entry.cooldown_until).is_some_and(|until| until > now_ms),
            }));
        }
    }
    drop(health);

    Ok(Json(json!({
        "node": {
            "role": role,
            "version": env!("CARGO_PKG_VERSION"),
            "started_at": started_at.to_rfc3339(),
            "uptime_seconds": uptime_seconds,
            "listen": state.runtime.listen,
            "metrics_path": state.runtime.metrics_path,
            "database_backend": database_backend,
            "database_dsn_redacted": dsn_redacted,
            "upstream_proxy_url": state.node.upstream_proxy_url,
        },
        "replica": {
            "ingest_enabled": state.metering_token_digest.is_some(),
            "spool_pending_count": spool_pending_count,
            "spool_pending_bytes": spool_pending_bytes,
        },
        "system": {
            "pending_request_logs": state.pending_request_logs.len(),
            "sse_connections": sse_connections,
            "channel_health_entries": state.channel_health.lock().await.len(),
            "channel_affinity_entries": state.channel_affinity.lock().await.len(),
            "routing_config_revision": state.routing_config_revision
                .load(std::sync::atomic::Ordering::Relaxed)
                .to_string(),
        },
        "users_ranking": users_ranking,
        "channel_health": channel_health,
    })))
}

fn redact_dsn(dsn: &str) -> String {
    if let Some(at_pos) = dsn.find('@')
        && let Some(scheme_end) = dsn.find("://")
    {
        return format!("{}://***@{}", &dsn[..scheme_end], &dsn[at_pos + 1..]);
    }
    if dsn.starts_with("sqlite") {
        return dsn.to_string();
    }
    "***".to_string()
}