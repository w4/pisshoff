#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use crate::{config::Args, server::Server};
use anyhow::anyhow;
use clap::Parser;
use futures::FutureExt;
use std::sync::Arc;
use thrussh::MethodSet;
use tokio::{
    signal::unix::SignalKind,
    sync::{oneshot, watch},
};
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

    let hostname = Box::leak(
        nix::unistd::gethostname()?
            .into_string()
            .map_err(|_| anyhow!("invalid hostname"))?
            .into_boxed_str(),
    );
    let keys = vec![thrussh_keys::key::KeyPair::generate_ed25519().unwrap()];

    let thrussh_config = Arc::new(thrussh::server::Config {
        server_id: args.config.server_id.to_string(),
        methods: MethodSet::PASSWORD | MethodSet::PUBLICKEY | MethodSet::KEYBOARD_INTERACTIVE,
        keys,
        auth_rejection_time: std::time::Duration::from_secs(1),
        ..thrussh::server::Config::default()
    });

    let (reload_send, reload_recv) = watch::channel(());
    let (shutdown_send, shutdown_recv) = oneshot::channel();

    let (audit_send, audit_handle) =
        audit::start_audit_writer(args.config.clone(), reload_recv, shutdown_recv);
    let mut audit_handle = audit_handle.fuse();

    let server = Server::new(hostname, args.config.clone(), audit_send);
    let listen_address = args.config.listen_address.to_string();

    // TODO: needs clean shutdowns on clients
    let fut = thrussh::server::run(thrussh_config, &listen_address, server);

    let shutdown_watcher = watch_for_shutdown(shutdown_send);
    let reload_watcher = watch_for_reloads(reload_send);

    tokio::select! {
        res = fut => res?,
        res = &mut audit_handle => res??,
        res = shutdown_watcher => res?,
        res = reload_watcher => res?,
    }

    info!("Finishing audit log writes");
    audit_handle.await??;
    info!("Audit log writes finished");

    Ok(())
}

async fn watch_for_shutdown(send: oneshot::Sender<()>) -> Result<(), anyhow::Error> {
    tokio::signal::ctrl_c().await?;
    info!("Received ctrl-c, initiating shutdown");

    let _res = send.send(());

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
