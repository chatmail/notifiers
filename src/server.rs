use a2::{
    DefaultNotificationBuilder, Error::ResponseError, NotificationBuilder, NotificationOptions,
    Priority, PushType,
};
use anyhow::{bail, Error, Result};
use log::*;
use serde::Deserialize;
use std::str::FromStr;

use crate::metrics::Metrics;
use crate::state::State;

pub async fn start(state: State, server: String, port: u16) -> Result<()> {
    let mut app = tide::with_state(state);
    app.at("/").get(|_| async { Ok("Hello, world!") });
    app.at("/register").post(register_device);
    app.at("/notify").post(notify_device);

    info!("Listening on {server}:port");
    app.listen((server, port)).await?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceQuery {
    token: String,
}

/// Registers a device for heartbeat notifications.
async fn register_device(mut req: tide::Request<State>) -> tide::Result<tide::Response> {
    let query: DeviceQuery = req.body_json().await?;
    info!("register_device {}", query.token);

    let schedule = req.state().schedule();
    schedule.insert_token_now(&query.token)?;

    // Flush database to ensure we don't lose this token in case of restart.
    schedule.flush().await?;

    req.state().metrics().heartbeat_registrations_total.inc();

    Ok(tide::Response::new(tide::StatusCode::Ok))
}

enum NotificationToken {
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
        } else if let Some(token) = s.strip_prefix("sandbox:") {
            Ok(Self::ApnsSandbox(token.to_string()))
        } else {
            Ok(Self::ApnsProduction(s.to_string()))
        }
    }
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
) -> tide::Result<tide::Response> {
    let Some(fcm_api_key) = fcm_api_key else {
        warn!("Cannot notify FCM because key is not set");
        return Ok(tide::Response::new(tide::StatusCode::InternalServerError));
    };

    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':' || c == '-')
    {
        return Ok(tide::Response::new(tide::StatusCode::Gone));
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
        return Ok(tide::Response::new(tide::StatusCode::Gone));
    }
    if status.is_server_error() {
        warn!("Internal server error while attempting to deliver FCM notification to {token}");
        return Ok(tide::Response::new(tide::StatusCode::InternalServerError));
    }
    info!("Delivered notification to FCM token {token}");
    metrics.fcm_notifications_total.inc();
    Ok(tide::Response::new(tide::StatusCode::Ok))
}

async fn notify_apns(
    req: tide::Request<State>,
    client: a2::Client,
    device_token: String,
) -> tide::Result<tide::Response> {
    let schedule = req.state().schedule();
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
                apns_topic: req.state().topic(),
                apns_push_type: Some(PushType::Alert),
                ..Default::default()
            },
        );

    match client.send(payload).await {
        Ok(res) => {
            match res.code {
                200 => {
                    info!("delivered notification for {}", device_token);
                    req.state().metrics().direct_notifications_total.inc();
                }
                _ => {
                    warn!("unexpected status: {:?}", res);
                }
            }

            Ok(tide::Response::new(tide::StatusCode::Ok))
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
                Ok(tide::Response::new(tide::StatusCode::Gone))
            } else {
                Ok(tide::Response::new(tide::StatusCode::InternalServerError))
            }
        }
        Err(err) => {
            error!("failed to send notification: {}, {:?}", device_token, err);
            Ok(tide::Response::new(tide::StatusCode::InternalServerError))
        }
    }
}

/// Notifies a single device with a visible notification.
async fn notify_device(mut req: tide::Request<State>) -> tide::Result<tide::Response> {
    let device_token = req.body_string().await?;
    info!("Got direct notification for {device_token}.");

    let device_token: NotificationToken = device_token.as_str().parse()?;

    match device_token {
        NotificationToken::Fcm {
            package_name,
            token,
        } => {
            let client = req.state().fcm_client().clone();
            let Ok(fcm_token) = req.state().fcm_token().await else {
                return Ok(tide::Response::new(tide::StatusCode::InternalServerError));
            };
            let metrics = req.state().metrics();
            notify_fcm(
                &client,
                fcm_token.as_deref(),
                &package_name,
                &token,
                metrics,
            )
            .await
        }
        NotificationToken::ApnsSandbox(token) => {
            let client = req.state().sandbox_client().clone();
            notify_apns(req, client, token).await
        }
        NotificationToken::ApnsProduction(token) => {
            let client = req.state().production_client().clone();
            notify_apns(req, client, token).await
        }
    }
}
