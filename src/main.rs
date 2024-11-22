use std::path::PathBuf;

use anyhow::{Context, Result};
use structopt::StructOpt;

use notifiers::{metrics, notifier, server, state};

#[derive(Debug, StructOpt)]
struct Opt {
    /// Path to the certificate file PKS12.
    #[structopt(long, parse(from_os_str))]
    certificate_file: PathBuf,
    /// Password for the certificate file.
    #[structopt(long)]
    password: String,
    /// The topic for the notification.
    #[structopt(long)]
    topic: Option<String>,
    /// The host on which to start the server.
    #[structopt(long, default_value = "127.0.0.1")]
    host: String,
    /// The port on which to start the server.
    #[structopt(long, default_value = "9000")]
    port: u16,
    /// The host and port on which to start the metrics server.
    /// For example, `127.0.0.1:9001`.
    #[structopt(long)]
    metrics: Option<String>,
    /// The path to the database file.
    #[structopt(long, default_value = "notifiers.db", parse(from_os_str))]
    db: PathBuf,
    #[structopt(long, default_value = "20m", parse(try_from_str = humantime::parse_duration))]
    interval: std::time::Duration,

    /// Path to FCM private key.
    #[structopt(long)]
    fcm_key_path: String,

    /// Path to the OpenPGP private keyring.
    ///
    /// OpenPGP keys are used to decrypt tokens
    /// so [chatmail](https://github.com/deltachat/chatmail) servers don't
    /// see the tokens in plaintext and cannot tell if the user
    /// is an Apple or Google (FCM) user.
    ///
    /// The file should contain ASCII armored keys
    /// delimited by `-----BEGIN PGP PRIVATE KEY BLOCK-----`
    /// and `-----END PGP PRIVATE KEY BLOCK-----`.
    #[structopt(long)]
    openpgp_keyring_path: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    femme::start();

    let opt = Opt::from_args();
    let certificate = std::fs::File::open(&opt.certificate_file).context("invalid certificate")?;

    let metrics_state = metrics::Metrics::new();

    let state = state::State::new(
        &opt.db,
        certificate,
        &opt.password,
        opt.topic.clone(),
        metrics_state,
        opt.interval,
        opt.fcm_key_path,
        opt.openpgp_keyring_path,
    )
    .await?;

    let host = opt.host.clone();
    let port = opt.port;
    let interval = opt.interval;

    if let Some(metrics_address) = opt.metrics.clone() {
        let state = state.clone();
        tokio::task::spawn(async move { metrics::start(state, metrics_address).await });
    }

    // Setup mulitple parallel notifiers.
    // This is needed to utilize HTTP/2 pipelining.
    // Notifiers take tokens for notifications from the same schedule
    // and use the same HTTP/2 clients, one for production and one for sandbox server.
    for _ in 0..50 {
        let state = state.clone();
        tokio::task::spawn(async move { notifier::start(state, interval).await });
    }

    server::start(state, host, port).await?;

    Ok(())
}
