use std::time::{Duration, SystemTime};

use a2::{
    Client, DefaultNotificationBuilder, Error::ResponseError, NotificationBuilder,
    NotificationOptions, Priority,
};
use anyhow::{bail, Context as _, Result};
use log::*;

use crate::metrics::Metrics;
use crate::schedule::Schedule;
use crate::server::NotificationToken;
use crate::state::State;

pub async fn start(state: State, interval: std::time::Duration) -> Result<()> {
    let schedule = state.schedule();
    let metrics = state.metrics();
    let production_client = state.production_client();
    let sandbox_client = state.sandbox_client();
    let topic = state.topic();

    info!(
        "Waking up devices every {}",
        humantime::format_duration(interval)
    );

    loop {
        metrics.heartbeat_tokens.set(schedule.token_count() as i64);

        let Some((timestamp, token)) = schedule.pop()? else {
            info!("No tokens to notify, sleeping for a minute.");
            tokio::time::sleep(Duration::from_secs(60)).await;
            continue;
        };

        // Sleep until we need to notify the token.
        let now = SystemTime::now();
        let timestamp: SystemTime = SystemTime::UNIX_EPOCH
            .checked_add(Duration::from_secs(timestamp))
            .unwrap_or(now);
        let timestamp = std::cmp::min(timestamp, now);
        let delay = timestamp
            .checked_add(interval)
            .unwrap_or(now)
            .duration_since(now)
            .unwrap_or_default();

        if !delay.is_zero() {
            info!(
                "Sleeping for {} before next notification.",
                humantime::format_duration(delay)
            );
            tokio::time::sleep(delay).await;
        }

        if let Err(err) = wakeup(
            schedule,
            metrics,
            production_client,
            sandbox_client,
            topic,
            token,
        )
        .await
        {
            error!("Failed to notify token: {err:#}");

            // Sleep to avoid busy looping and flooding APNS
            // with requests in case of database errors.
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }
}

async fn wakeup(
    schedule: &Schedule,
    metrics: &Metrics,
    production_client: &Client,
    sandbox_client: &Client,
    topic: Option<&str>,
    key_device_token: String,
) -> Result<()> {
    info!("notify: {}", key_device_token);

    let device_token: NotificationToken = key_device_token.as_str().parse()?;

    let (client, device_token) = match device_token {
        NotificationToken::Fcm { .. } | NotificationToken::UBports(..) => {
            // Only APNS tokens can be registered for periodic notifications.
            info!("Removing FCM token {key_device_token}");
            schedule
                .remove_token(&key_device_token)
                .with_context(|| format!("Failed to remove {}", &key_device_token))?;
            return Ok(());
        }
        NotificationToken::ApnsSandbox(token) => (sandbox_client, token),
        NotificationToken::ApnsProduction(token) => (production_client, token),
    };

    // Send silent notification.
    // According to <https://developer.apple.com/documentation/usernotifications/generating-a-remote-notification>
    // to send a silent notification you need to set background notification flag `content-available` to 1
    // and don't include `alert`, `badge` or `sound`.
    let payload = DefaultNotificationBuilder::new()
        .set_content_available()
        .build(
            &device_token,
            NotificationOptions {
                // Normal priority (5) means
                // "send the notification based on power considerations on the user’s device".
                // <https://developer.apple.com/documentation/usernotifications/sending-notification-requests-to-apns>
                apns_priority: Some(Priority::Normal),
                apns_topic: topic,
                ..Default::default()
            },
        );

    match client.send(payload).await {
        Ok(res) => match res.code {
            200 => {
                info!("delivered notification for {}", device_token);
                schedule
                    .insert_token_now(&key_device_token)
                    .context("Failed to update latest notification timestamp")?;
                metrics.heartbeat_notifications_total.inc();
            }
            _ => {
                bail!("unexpected status: {:?}", res);
            }
        },
        Err(ResponseError(res)) => {
            info!(
                "Removing token {} due to error {:?}.",
                &key_device_token, res
            );
            schedule
                .remove_token(&key_device_token)
                .with_context(|| format!("Failed to remove {}", &key_device_token))?;
        }
        Err(err) => {
            // Update notification time regardless of success
            // to avoid busy looping.
            schedule
                .insert_token_now(&key_device_token)
                .with_context(|| format!("Failed to update token timestamp: {err:?}"))?;
        }
    }
    Ok(())
}
