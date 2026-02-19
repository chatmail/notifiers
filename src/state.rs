use std::io::{Read, Seek};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use a2::{Client, Endpoint};
use anyhow::{Context as _, Result};

use crate::debouncer::Debouncer;
use crate::metrics::Metrics;
use crate::openpgp::PgpDecryptor;
use crate::schedule::Schedule;

#[derive(Clone)]
pub struct State {
    inner: Arc<InnerState>,
}

pub struct InnerState {
    schedule: Schedule,

    http_client: reqwest::Client,

    apns_production_client: Client,

    apns_sandbox_client: Client,

    topic: Option<String>,

    metrics: Metrics,

    /// Heartbeat notification interval.
    interval: Duration,

    fcm_authenticator: yup_oauth2::authenticator::DefaultAuthenticator,

    /// Decryptor for incoming tokens
    /// storing the secret keyring inside.
    openpgp_decryptor: PgpDecryptor,

    debouncer: Debouncer,
}

impl State {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        db: &Path,
        mut certificate: std::fs::File,
        password: &str,
        topic: Option<String>,
        metrics: Metrics,
        interval: Duration,
        fcm_key_path: String,
        openpgp_keyring_path: String,
    ) -> Result<Self> {
        let schedule = Schedule::new(db)?;
        let http_client = reqwest::ClientBuilder::new()
            .timeout(Duration::from_secs(60))
            .build()
            .context("Failed to build HTTP client (FCM/UBPort)")?;

        let fcm_key: yup_oauth2::ServiceAccountKey =
            yup_oauth2::read_service_account_key(fcm_key_path)
                .await
                .context("Failed to read key")?;
        let fcm_authenticator = yup_oauth2::ServiceAccountAuthenticator::builder(fcm_key)
            .build()
            .await
            .context("Failed to create authenticator")?;

        let apns_production_client =
            Client::certificate(&mut certificate, password, Endpoint::Production)
                .context("Failed to create production client")?;
        certificate.rewind()?;
        let apns_sandbox_client =
            Client::certificate(&mut certificate, password, Endpoint::Sandbox)
                .context("Failed to create sandbox client")?;

        let mut keyring_file = std::fs::File::open(openpgp_keyring_path)?;
        let mut keyring = String::new();
        keyring_file.read_to_string(&mut keyring)?;
        let openpgp_decryptor = PgpDecryptor::new(&keyring)?;

        Ok(State {
            inner: Arc::new(InnerState {
                schedule,
                http_client,
                apns_production_client,
                apns_sandbox_client,
                topic,
                metrics,
                interval,
                fcm_authenticator,
                openpgp_decryptor,
                debouncer: Default::default(),
            }),
        })
    }

    pub fn schedule(&self) -> &Schedule {
        &self.inner.schedule
    }

    pub fn fcm_client(&self) -> &reqwest::Client {
        &self.inner.http_client
    }

    pub async fn fcm_token(&self) -> Result<Option<String>> {
        let token = self
            .inner
            .fcm_authenticator
            .token(&["https://www.googleapis.com/auth/firebase.messaging"])
            .await?
            .token()
            .map(|s| s.to_string());
        Ok(token)
    }

    pub fn production_client(&self) -> &Client {
        &self.inner.apns_production_client
    }

    pub fn sandbox_client(&self) -> &Client {
        &self.inner.apns_sandbox_client
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

    pub fn openpgp_decryptor(&self) -> &PgpDecryptor {
        &self.inner.openpgp_decryptor
    }

    pub(crate) fn debouncer(&self) -> &Debouncer {
        &self.inner.debouncer
    }
}
