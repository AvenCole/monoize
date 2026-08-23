//! `primary-replica-deployment.spec.md` test matrix (T2/T3/T4/T7).

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use monoize::db_cache::{LastUsedBatcher, RequestLogBatcher};
use monoize::replica::metering::{
    BalanceDelta, DeltaSpool, ReplicaMetering, apply_metering_batch, drain_delta_spool_to_local_db,
};
use tempfile::TempDir;
use tokio::sync::broadcast;

fn test_runtime(database_dsn: String) -> monoize::app::RuntimeConfig {
    monoize::app::RuntimeConfig {
        listen: "127.0.0.1:0".to_string(),
        metrics_path: "/metrics".to_string(),
        database_dsn,
        request_log_spool_dir: None,
        node: monoize::node_config::NodeSettings::primary_default(),
    }
}

fn delta(kind: &str, user_id: &str, api_key_id: Option<&str>, amount: i128) -> BalanceDelta {
    BalanceDelta {
        delta_id: uuid::Uuid::new_v4().to_string(),
        kind: kind.to_string(),
        user_id: user_id.to_string(),
        api_key_id: api_key_id.map(str::to_string),
        amount_nano_usd: amount.to_string(),
        meta_json: serde_json::json!({ "request_id": "req-1" }),
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

async fn boot() -> (TempDir, monoize::app::AppState) {
    let temp = TempDir::new().unwrap();
    let dsn = format!("sqlite://{}", temp.path().join("m.db").display());
    let state = monoize::app::load_state_with_runtime(test_runtime(dsn))
        .await
        .expect("state loads");
    (temp, state)
}

#[tokio::test]
async fn ingest_applies_balance_delta_idempotently() {
    let (_temp, state) = boot().await;
    let user = state
        .user_store
        .create_user("delta_user", "pw", monoize::users::UserRole::User, &[])
        .await
        .expect("user");
    state
        .user_store
        .update_user(
            &user.id,
            None,
            None,
            None,
            None,
            Some("5000000000"),
            None,
            None,
            None,
        )
        .await
        .expect("seed balance");

    let batch = monoize::replica::metering::MeteringBatch {
        request_logs: vec![],
        last_used: vec![],
        balance_deltas: vec![delta("request_charge", &user.id, None, 1_000_000_000)],
    };

    // T2 first delivery: one ledger row, balance reduced.
    let ack1 = apply_metering_batch(&state.db_pool, &batch)
        .await
        .expect("apply");
    assert_eq!(ack1.applied_balance_deltas, 1);
    let balance = state
        .user_store
        .get_user_balance(&user.id)
        .await
        .expect("balance")
        .expect("row");
    assert_eq!(balance.balance_nano_usd, 4_000_000_000);

    // I6 replay: nothing changes, counts report zero new applies.
    let ack2 = apply_metering_batch(&state.db_pool, &batch)
        .await
        .expect("replay");
    assert_eq!(ack2.applied_balance_deltas, 0);
    assert_eq!(ack2.applied_request_logs, 0);
    let balance2 = state
        .user_store
        .get_user_balance(&user.id)
        .await
        .expect("balance")
        .unwrap();
    assert_eq!(balance2.balance_nano_usd, 4_000_000_000);
}

#[tokio::test]
async fn ingest_allows_negative_result_and_counts_unlimited_as_applied_without_update() {
    let (_temp, state) = boot().await;
    let limited = state
        .user_store
        .create_user("limited", "pw", monoize::users::UserRole::User, &[])
        .await
        .expect("limited");
    let unlimited = state
        .user_store
        .create_user("unl", "pw", monoize::users::UserRole::User, &[])
        .await
        .expect("unlimited");
    state
        .user_store
        .update_user(
            &unlimited.id,
            None,
            None,
            None,
            None,
            Some("0"),
            Some(true),
            None,
            None,
        )
        .await
        .expect("make unlimited");

    // T3 negative result allowed on the limited user.
    let batch = monoize::replica::metering::MeteringBatch {
        request_logs: vec![],
        last_used: vec![],
        balance_deltas: vec![delta("request_charge", &limited.id, None, 100)],
    };
    let ack = apply_metering_batch(&state.db_pool, &batch)
        .await
        .expect("apply");
    assert_eq!(ack.applied_balance_deltas, 1);
    let bal = state
        .user_store
        .get_user_balance(&limited.id)
        .await
        .expect("bal")
        .unwrap();
    assert_eq!(bal.balance_nano_usd, -100);

    // T3 unlimited owner: ledger event recorded but balance untouched.
    let batch_u = monoize::replica::metering::MeteringBatch {
        request_logs: vec![],
        last_used: vec![],
        balance_deltas: vec![delta("request_charge", &unlimited.id, None, 77)],
    };
    let ack_u = apply_metering_batch(&state.db_pool, &batch_u)
        .await
        .expect("apply u");
    assert_eq!(ack_u.applied_balance_deltas, 1);
    let bal_u = state
        .user_store
        .get_user_balance(&unlimited.id)
        .await
        .expect("bal u")
        .unwrap();
    assert_eq!(bal_u.balance_nano_usd, 0);
}

/// T4: shipment releases spool data only after an HTTP 200 and retains it otherwise.
#[tokio::test]
async fn shipper_acks_delete_and_failures_retain() {
    let temp = TempDir::new().unwrap();
    let spool_dir = temp.path().join("metering");

    // Failing primary first.
    let failing = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let fail_flag = failing.clone();
    let fail_app = Router::new().route(
        monoize::replica::metering::METERING_INGEST_PATH,
        post(move |body: axum::body::Bytes| async move {
            fail_flag.fetch_add(1, Ordering::SeqCst);
            let batch: monoize::replica::metering::MeteringBatch =
                serde_json::from_slice(&body).unwrap();
            assert_eq!(batch.balance_deltas.len(), 1);
            Err::<axum::response::Response, _>((
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "boom",
            ))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, fail_app).await.unwrap() });

    let metering =
        ReplicaMetering::new(spool_dir.clone(), 1024 * 1024, &addr, "token", 10).expect("metering");
    metering
        .enqueue_balance_delta("request_charge", "u-1", None, 1234, &serde_json::json!({}))
        .await
        .expect("enqueue");
    assert_eq!(
        metering.pending().outstanding("u-1"),
        1234,
        "M3 pending counter increments on enqueue"
    );

    let log_batcher = RequestLogBatcher::new_with_limits(
        8,
        temp.path().join("rl-spool"),
        64 * 1024 * 1024,
        8 * 1024 * 1024,
        broadcast::channel(4).0,
        Arc::new(dashmap::DashMap::new()),
    );
    let last_used = LastUsedBatcher::with_capacity(16);

    metering.ship_once(&log_batcher, &last_used).await;
    assert_eq!(failing.load(Ordering::SeqCst), 1, "one POST attempt made");
    assert_eq!(
        metering.pending().outstanding("u-1"),
        1234,
        "M5 failure retains the pending counter"
    );
    assert_eq!(
        metering.delta_spool().pending_files(),
        1,
        "M5 failure retains the durable delta file"
    );

    // Succeeding primary next: same data ships and is released only after 200.
    let ok_app = Router::new().route(
        monoize::replica::metering::METERING_INGEST_PATH,
        post(|body: axum::body::Bytes| async move {
            let batch: monoize::replica::metering::MeteringBatch =
                serde_json::from_slice(&body).unwrap();
            assert_eq!(batch.balance_deltas[0].amount_nano_usd, "1234");
            axum::Json(monoize::replica::metering::MeteringAck {
                applied_request_logs: batch.request_logs.len() as u64,
                applied_last_used: batch.last_used.len() as u64,
                applied_balance_deltas: batch.balance_deltas.len() as u64,
            })
        }),
    );
    let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr2 = format!("http://{}", listener2.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener2, ok_app).await.unwrap() });

    let metering_ok =
        ReplicaMetering::new(spool_dir, 1024 * 1024, &addr2, "token", 10).expect("metering 2");
    // Re-key pending counter onto the second instance for assertion symmetry.
    metering_ok.pending().add("u-1", 1234);
    metering_ok.ship_once(&log_batcher, &last_used).await;
    assert_eq!(
        metering_ok.delta_spool().pending_files(),
        0,
        "successful ack deletes the durable file"
    );
    assert_eq!(
        metering_ok.pending().outstanding("u-1"),
        0,
        "successful ack clears the pending deduction"
    );
}

use std::sync::atomic::Ordering;

/// T7: a promoted node drains leftover deltas into its own database before serving.
#[tokio::test]
async fn promotion_drain_applies_leftover_deltas_locally() {
    let temp = TempDir::new().unwrap();
    let dsn = format!("sqlite://{}", temp.path().join("m.db").display());
    let state = monoize::app::load_state_with_runtime(test_runtime(dsn))
        .await
        .expect("state");
    let user = state
        .user_store
        .create_user("drain_user", "pw", monoize::users::UserRole::User, &[])
        .await
        .expect("user");

    let spool_dir = temp.path().join("leftover-metering");
    std::fs::create_dir_all(&spool_dir).unwrap();
    let spool = DeltaSpool::new(spool_dir.clone(), 1024 * 1024).unwrap();
    spool
        .enqueue(&delta("request_charge", &user.id, None, 250))
        .await
        .expect("enqueue leftover");
    assert_eq!(spool.pending_files(), 1);

    drain_delta_spool_to_local_db(&state.db_pool, &spool)
        .await
        .expect("drain");
    assert_eq!(spool.pending_files(), 0, "PRP9 drain empties the spool");
    let balance = state
        .user_store
        .get_user_balance(&user.id)
        .await
        .expect("bal")
        .unwrap();
    assert_eq!(balance.balance_nano_usd, -250);
}
