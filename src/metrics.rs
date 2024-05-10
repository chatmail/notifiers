//! Prometheus (OpenMetrics) metrics server.
//!
//! It is listening on its own address
//! to allow exposting it on a private network only
//! independently of the main service.

use std::sync::atomic::AtomicI64;

use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;

use anyhow::Result;

use crate::state::State;

#[derive(Debug, Default)]
pub struct Metrics {
    pub registry: Registry,

    /// Number of successfully sent visible APNS notifications.
    pub direct_notifications_total: Counter,

    /// Number of successfully sent visible FCM notifications.
    pub fcm_notifications_total: Counter,

    /// Number of successfully sent heartbeat notifications.
    pub heartbeat_notifications_total: Counter,

    /// Number of heartbeat token registrations.
    pub heartbeat_registrations_total: Counter,

    /// Number of tokens registered for heartbeat notifications.
    pub heartbeat_tokens: Gauge<i64, AtomicI64>,
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

        Self {
            registry,
            fcm_notifications_total,
            direct_notifications_total,
            heartbeat_notifications_total,
            heartbeat_registrations_total,
            heartbeat_tokens,
        }
    }
}

pub async fn start(state: State, server: String) -> Result<()> {
    let mut app = tide::with_state(state);
    app.at("/metrics").get(metrics);
    app.listen(server).await?;
    Ok(())
}

async fn metrics(req: tide::Request<State>) -> tide::Result<tide::Response> {
    let mut encoded = String::new();
    encode(&mut encoded, &req.state().metrics().registry).unwrap();
    let response = tide::Response::builder(tide::StatusCode::Ok)
        .body(encoded)
        .content_type("application/openmetrics-text; version=1.0.0; charset=utf-8")
        .build();
    Ok(response)
}
