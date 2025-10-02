use a2::{
    DefaultNotificationBuilder, Error::ResponseError, NotificationBuilder, NotificationOptions,
    Priority, PushType,
};
use anyhow::{bail, Error, Result};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use chrono::{Local, TimeDelta};
use log::*;
use serde::Deserialize;
use std::str::FromStr;

use crate::metrics::Metrics;
use crate::state::State;

pub async fn start(state: State, server: String, port: u16) -> Result<()> {
    let app = axum::Router::new()
        .route("/", get(|| async { "Hello, world!" }))
        .route("/register", post(register_device))
        .route("/notify", post(notify_device))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind((server, port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceQuery {
    token: String,
}

struct AppError(anyhow::Error);

impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

/// Registers a device for heartbeat notifications.
async fn register_device(
    axum::extract::State(state): axum::extract::State<State>,
    body: String,
) -> Result<(), AppError> {
    let query: DeviceQuery = serde_json::from_str(&body)?;

    let mut device_token = query.token;
    if let Some(openpgp_device_token) = device_token.strip_prefix("openpgp:") {
        device_token = state.openpgp_decryptor().decrypt(openpgp_device_token)?;
    }

    info!("Registering device {:?}.", device_token);

    let schedule = state.schedule();
    schedule.insert_token_now(&device_token)?;

    // Flush database to ensure we don't lose this token in case of restart.
    schedule.flush().await?;

    state.metrics().heartbeat_registrations_total.inc();

    Ok(())
}

pub(crate) enum NotificationToken {
    /// Ubuntu touch app
    UBports(String),

    /// Android App.
    Fcm {
        /// Package name such as `chat.delta`.
        package_name: String,

        /// Token.
        token: String,
    },

    /// APNS sandbox token.
    ApnsSandbox(String),

    /// APNS production token.
    ApnsProduction(String),
}

impl FromStr for NotificationToken {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        if let Some(s) = s.strip_prefix("fcm-") {
            if let Some((package_name, token)) = s.split_once(':') {
                Ok(Self::Fcm {
                    package_name: package_name.to_string(),
                    token: token.to_string(),
                })
            } else {
                bail!("Invalid FCM token");
            }
        } else if let Some(s) = s.strip_prefix("ubports-") {
            Ok(Self::UBports(s.to_string()))
        } else if let Some(token) = s.strip_prefix("sandbox:") {
            Ok(Self::ApnsSandbox(token.to_string()))
        } else {
            Ok(Self::ApnsProduction(s.to_string()))
        }
    }
}

/// Notify the UBports push server
///
/// API documentation is available at
/// <https://docs.ubports.com/en/latest/appdev/guides/pushnotifications.html>
async fn notify_ubports(
    client: &reqwest::Client,
    token: &str,
    metrics: &Metrics,
) -> Result<StatusCode> {
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '-')
    {
        return Ok(StatusCode::GONE);
    }

    let url = "https://push.ubports.com/notify";
    let expire_on = (Local::now() + TimeDelta::weeks(1)).to_rfc3339();
    let body = format!(
        r#"{{"expire_on":"{expire_on}","appid":"deltatouch.lotharketterer_deltatouch","token":"{token}","data":{{"notification":{{"tag":"sent_by_chatmail_server","card":{{"popup":true,"persist":true,"summary":"New message","body":"You have a new message"}},"sound":true,"vibrate":{{"pattern":[200],"duration":200,"repeat":1}} }},"sent-by":"Chatmail Server"}} }}"#
    );
    let res = client
        .post(url)
        .body(body.clone())
        .header("Content-Type", "application/json")
        .send()
        .await?;
    let status = res.status();
    if status.is_client_error() {
        warn!("Failed to deliver UBports notification to {token}");
        warn!("BODY: {body:?}");
        warn!("RES: {res:?}");
        return Ok(StatusCode::GONE);
    }
    if status.is_server_error() {
        warn!("Internal server error while attempting to deliver UBports notification to {token}");
        return Ok(StatusCode::INTERNAL_SERVER_ERROR);
    }
    info!("Delivered notification to UBports token {token}");
    metrics.ubports_notifications_total.inc();
    Ok(StatusCode::OK)
}

/// Notifies a single FCM token.
///
/// API documentation is available at
/// <https://firebase.google.com/docs/cloud-messaging/send-message#rest>
async fn notify_fcm(
    client: &reqwest::Client,
    fcm_api_key: Option<&str>,
    _package_name: &str,
    token: &str,
    metrics: &Metrics,
) -> Result<StatusCode> {
    let Some(fcm_api_key) = fcm_api_key else {
        warn!("Cannot notify FCM because key is not set");
        return Ok(StatusCode::INTERNAL_SERVER_ERROR);
    };

    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '-')
    {
        return Ok(StatusCode::GONE);
    }

    let url = "https://fcm.googleapis.com/v1/projects/delta-chat-fcm/messages:send";
    let body =
        format!("{{\"message\":{{\"token\":\"{token}\",\"data\":{{\"level\": \"awesome\"}} }} }}");
    let res = client
        .post(url)
        .body(body.clone())
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {fcm_api_key}"))
        .send()
        .await?;
    let status = res.status();
    if status.is_client_error() {
        warn!("Failed to deliver FCM notification to {token}");
        warn!("BODY: {body:?}");
        warn!("RES: {res:?}");
        return Ok(StatusCode::GONE);
    }
    if status.is_server_error() {
        warn!("Internal server error while attempting to deliver FCM notification to {token}");
        return Ok(StatusCode::INTERNAL_SERVER_ERROR);
    }
    info!("Delivered notification to FCM token {token}");
    metrics.fcm_notifications_total.inc();
    Ok(StatusCode::OK)
}

async fn notify_apns(state: State, client: a2::Client, device_token: String) -> Result<StatusCode> {
    let schedule = state.schedule();
    let payload = DefaultNotificationBuilder::new()
        .set_title("New messages")
        .set_title_loc_key("new_messages") // Localization key for the title.
        .set_body("You have new messages")
        .set_loc_key("new_messages_body") // Localization key for the body.
        .set_sound("default")
        .set_mutable_content()
        .build(
            &device_token,
            NotificationOptions {
                // High priority (10).
                // <https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns>
                apns_priority: Some(Priority::High),
                apns_topic: state.topic(),
                apns_push_type: Some(PushType::Alert),
                ..Default::default()
            },
        );

    match client.send(payload).await {
        Ok(res) => {
            match res.code {
                200 => {
                    info!("delivered notification for {}", device_token);
                    state.metrics().direct_notifications_total.inc();
                }
                _ => {
                    warn!("unexpected status: {:?}", res);
                }
            }

            Ok(StatusCode::OK)
        }
        Err(ResponseError(res)) => {
            info!("Removing token {} due to error {:?}.", &device_token, res);
            if res.code == 410 {
                // 410 means that "The device token is no longer active for the topic."
                // <https://developer.apple.com/documentation/usernotifications/handling-notification-responses-from-apns>
                //
                // Unsubscribe invalid token from heartbeat notification if it is subscribed.
                if let Err(err) = schedule.remove_token(&device_token) {
                    error!("failed to remove {}: {:?}", &device_token, err);
                }
                // Return 410 Gone response so email server can remove the token.
                Ok(StatusCode::GONE)
            } else {
                Ok(StatusCode::INTERNAL_SERVER_ERROR)
            }
        }
        Err(err) => {
            error!("failed to send notification: {}, {:?}", device_token, err);
            Ok(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// Notifies a single device with a visible notification.
async fn notify_device(
    axum::extract::State(state): axum::extract::State<State>,
    mut device_token: String,
) -> Result<StatusCode, AppError> {
    // Decrypt the token if it is OpenPGP-encrypted.
    if let Some(openpgp_device_token) = device_token.strip_prefix("openpgp:") {
        match state.openpgp_decryptor().decrypt(openpgp_device_token) {
            Ok(decrypted_device_token) => {
                device_token = decrypted_device_token;
            }
            Err(err) => {
                error!("Failed to decrypt device token: {:#}.", err);

                let metrics = state.metrics();
                metrics.openpgp_decryption_failures_total.inc();

                // Return 410 Gone response so email server can remove the token.
                return Ok(StatusCode::GONE);
            }
        }
    }

    info!("Got direct notification for {device_token}.");
    let device_token: NotificationToken = device_token.as_str().parse()?;

    let status_code = match device_token {
        NotificationToken::UBports(token) => {
            let client = state.fcm_client().clone();
            let metrics = state.metrics();
            notify_ubports(&client, &token, metrics).await?
        }
        NotificationToken::Fcm {
            package_name,
            token,
        } => {
            let client = state.fcm_client().clone();
            let Ok(fcm_token) = state.fcm_token().await else {
                return Ok(StatusCode::INTERNAL_SERVER_ERROR);
            };
            let metrics = state.metrics();
            notify_fcm(
                &client,
                fcm_token.as_deref(),
                &package_name,
                &token,
                metrics,
            )
            .await?
        }
        NotificationToken::ApnsSandbox(token) => {
            let client = state.sandbox_client().clone();
            notify_apns(state, client, token).await?
        }
        NotificationToken::ApnsProduction(token) => {
            let client = state.production_client().clone();
            notify_apns(state, client, token).await?
        }
    };
    Ok(status_code)
}
