use a2::{
    DefaultNotificationBuilder, Error::ResponseError, NotificationBuilder, NotificationOptions,
    Priority, PushType,
};
use anyhow::Result;
use log::*;
use serde::Deserialize;

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

/// Notifies a single device with a visible notification.
async fn notify_device(mut req: tide::Request<State>) -> tide::Result<tide::Response> {
    let device_token = req.body_string().await?;
    info!("Got direct notification for {device_token}.");

    let (client, device_token) = if let Some(sandbox_token) = device_token.strip_prefix("sandbox:")
    {
        (req.state().sandbox_client(), sandbox_token)
    } else {
        (req.state().production_client(), device_token.as_str())
    };

    let schedule = req.state().schedule();
    let payload = DefaultNotificationBuilder::new()
        .set_title("New messages")
        .set_title_loc_key("new_messages") // Localization key for the title.
        .set_body("You have new messages")
        .set_loc_key("new_messages_body") // Localization key for the body.
        .set_sound("default")
        .set_mutable_content()
        .build(
            device_token,
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
                if let Err(err) = schedule.remove_token(device_token) {
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
