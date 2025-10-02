//! Prometheus (OpenMetrics) metrics server.
//!
//! It is listening on its own address
//! to allow exposting it on a private network only
//! independently of the main service.

use std::sync::atomic::AtomicI64;

use anyhow::Result;
use axum::http::{header, HeaderMap};
use axum::response::IntoResponse;
use axum::routing::get;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;

use crate::state::State;

#[derive(Debug, Default)]
pub struct Metrics {
    pub registry: Registry,

    /// Number of successfully sent visible APNS notifications.
    pub direct_notifications_total: Counter,

    /// Number of successfully sent visible FCM notifications.
    pub fcm_notifications_total: Counter,

    /// Number of successfully sent visible UBports notifications.
    pub ubports_notifications_total: Counter,

    /// Number of successfully sent heartbeat notifications.
    pub heartbeat_notifications_total: Counter,

    /// Number of heartbeat token registrations.
    pub heartbeat_registrations_total: Counter,

    /// Number of tokens registered for heartbeat notifications.
    pub heartbeat_tokens: Gauge<i64, AtomicI64>,

    /// Number of decryption failures for encrypted tokens.
    pub openpgp_decryption_failures_total: Counter,
}

impl Metrics {
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let direct_notifications_total = Counter::default();
        registry.register(
            "direct_notifications",
            "Number of direct APNS notifications",
            direct_notifications_total.clone(),
        );

        let fcm_notifications_total = Counter::default();
        registry.register(
            "fcm_notifications",
            "Number of FCM notifications",
            fcm_notifications_total.clone(),
        );

        let ubports_notifications_total = Counter::default();
        registry.register(
            "ubports_notifications",
            "Number of UBports notifications",
            ubports_notifications_total.clone(),
        );

        let heartbeat_notifications_total = Counter::default();
        registry.register(
            "heartbeat_notifications",
            "Number of heartbeat notifications",
            heartbeat_notifications_total.clone(),
        );

        let heartbeat_registrations_total = Counter::default();
        registry.register(
            "heartbeat_registrations",
            "Number of heartbeat registrations",
            heartbeat_registrations_total.clone(),
        );

        let heartbeat_tokens = Gauge::<i64, AtomicI64>::default();
        registry.register(
            "heartbeat_tokens",
            "Number of tokens registered for heartbeat notifications",
            heartbeat_tokens.clone(),
        );

        let openpgp_decryption_failures_total = Counter::default();
        registry.register(
            "openpgp_decryption_failures",
            "Number of failures to decrypt OpenPGP-encrypted token",
            openpgp_decryption_failures_total.clone(),
        );

        Self {
            registry,
            direct_notifications_total,
            fcm_notifications_total,
            ubports_notifications_total,
            heartbeat_notifications_total,
            heartbeat_registrations_total,
            heartbeat_tokens,
            openpgp_decryption_failures_total,
        }
    }
}

pub async fn start(state: State, server: String) -> Result<()> {
    let app = axum::Router::new()
        .route("/metrics", get(metrics))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(server).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn metrics(axum::extract::State(state): axum::extract::State<State>) -> impl IntoResponse {
    let mut encoded = String::new();
    encode(&mut encoded, &state.metrics().registry).unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/openmetrics-text; version=1.0.0; charset=utf-8"
            .parse()
            .unwrap(),
    );
    (headers, encoded)
}
