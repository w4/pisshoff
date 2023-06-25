#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use crate::{config::Args, server::Server};
use clap::Parser;
use futures::FutureExt;
use std::sync::Arc;
use thrussh::MethodSet;
use tokio::{signal::unix::SignalKind, sync::watch};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

mod audit;
mod command;
mod config;
mod server;
mod state;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        error!("Failed to run {}: {}", env!("CARGO_CRATE_NAME"), e);
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    std::env::set_var("RUST_LOG", args.verbosity());

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    info!(
        "{} listening on {}",
        env!("CARGO_CRATE_NAME"),
        args.config.listen_address
    );

    let keys = vec![thrussh_keys::key::KeyPair::generate_ed25519().unwrap()];

    let thrussh_config = Arc::new(thrussh::server::Config {
        methods: MethodSet::PASSWORD | MethodSet::PUBLICKEY | MethodSet::KEYBOARD_INTERACTIVE,
        keys,
        auth_rejection_time: std::time::Duration::from_secs(1),
        ..thrussh::server::Config::default()
    });

    let (reload_send, reload_recv) = watch::channel(());

    let (audit_send, audit_handle) = audit::start_audit_writer(args.config.clone(), reload_recv);
    let mut audit_handle = audit_handle.fuse();

    let server = Server::new(args.config.clone(), audit_send);
    let listen_address = args.config.listen_address.to_string();

    let fut = thrussh::server::run(thrussh_config, &listen_address, server);

    let reload_watcher = watch_for_reloads(reload_send);

    tokio::select! {
        res = fut => res?,
        res = &mut audit_handle => res??,
        res = reload_watcher => res?,
        _ = tokio::signal::ctrl_c() => {
            info!("Received ctrl-c, initiating shutdown");
        }
    }

    info!("Finishing audit log writes");
    audit_handle.await??;
    info!("Audit log writes finished");

    Ok(())
}

async fn watch_for_reloads(send: watch::Sender<()>) -> Result<(), anyhow::Error> {
    let mut signal = tokio::signal::unix::signal(SignalKind::hangup())?;

    while let Some(()) = signal.recv().await {
        info!("Received SIGHUP, broadcasting reload");
        let _res = send.send(());
    }

    Ok(())
}
