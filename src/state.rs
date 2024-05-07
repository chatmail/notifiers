use std::io::Seek;
use std::path::Path;
use std::time::Duration;

use a2::{Client, Endpoint};
use anyhow::{Context as _, Result};
use async_std::sync::Arc;

use crate::metrics::Metrics;
use crate::schedule::Schedule;

#[derive(Debug, Clone)]
pub struct State {
    inner: Arc<InnerState>,
}

#[derive(Debug)]
pub struct InnerState {
    schedule: Schedule,

    fcm_client: reqwest::Client,

    production_client: Client,

    sandbox_client: Client,

    topic: Option<String>,

    metrics: Metrics,

    /// Heartbeat notification interval.
    interval: Duration,

    fcm_api_key: Option<String>,
}

impl State {
    pub fn new(
        db: &Path,
        mut certificate: std::fs::File,
        password: &str,
        topic: Option<String>,
        metrics: Metrics,
        interval: Duration,
        fcm_api_key: Option<String>,
    ) -> Result<Self> {
        let schedule = Schedule::new(db)?;
        let fcm_client = reqwest::Client::new();

        let production_client =
            Client::certificate(&mut certificate, password, Endpoint::Production)
                .context("Failed to create production client")?;
        certificate.rewind()?;
        let sandbox_client = Client::certificate(&mut certificate, password, Endpoint::Sandbox)
            .context("Failed to create sandbox client")?;

        Ok(State {
            inner: Arc::new(InnerState {
                schedule,
                fcm_client,
                production_client,
                sandbox_client,
                topic,
                metrics,
                interval,
                fcm_api_key,
            }),
        })
    }

    pub fn schedule(&self) -> &Schedule {
        &self.inner.schedule
    }

    pub fn fcm_client(&self) -> &reqwest::Client {
        &self.inner.fcm_client
    }

    pub fn fcm_api_key(&self) -> Option<&str> {
        self.inner.fcm_api_key.as_deref()
    }

    pub fn production_client(&self) -> &Client {
        &self.inner.production_client
    }

    pub fn sandbox_client(&self) -> &Client {
        &self.inner.sandbox_client
    }

    pub fn topic(&self) -> Option<&str> {
        self.inner.topic.as_deref()
    }

    pub fn metrics(&self) -> &Metrics {
        &self.inner.metrics
    }

    pub fn interval(&self) -> Duration {
        self.inner.interval
    }
}
