#![deny(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

use std::sync::Arc;

use clap::Parser;
use deadpool_postgres::{
    tokio_postgres::{NoTls, Statement, Transaction},
    GenericClient, Runtime,
};
use futures::{StreamExt, TryFutureExt};
use pisshoff_types::audit::{AuditLog, AuditLogEvent};
use tokio::net::{UnixListener, UnixStream};
use tokio_util::codec::{Decoder, LinesCodec};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::config::Args;

mod config;

mod embedded {
    use refinery::embed_migrations;
    embed_migrations!();
}

pub struct Context {
    db: deadpool_postgres::Pool,
}

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

    let db = args.config.pg.create_pool(Some(Runtime::Tokio1), NoTls)?;
    let context = Arc::new(Context { db });

    embedded::migrations::runner()
        .run_async(&mut **context.db.get().await?)
        .await?;

    spawn_listener(&args, context).await
}

async fn spawn_listener(args: &Args, context: Arc<Context>) -> anyhow::Result<()> {
    let listener = UnixListener::bind(&args.config.socket_path)?;

    loop {
        let (stream, remote) = listener.accept().await?;

        info!(?remote, "Accepted incoming connection");

        let context = context.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream, context).await {
                error!("Connection failed: {e}");
            }
        });
    }
}

async fn handle_connection(stream: UnixStream, context: Arc<Context>) -> anyhow::Result<()> {
    let mut framed = LinesCodec::new().framed(stream);

    while let Some(line) = framed.next().await.transpose()? {
        let context = context.clone();

        tokio::spawn(
            ingest_log(context, line).inspect_err(|e| error!("Failed to ingest log: {e}")),
        );
    }

    Ok(())
}

async fn ingest_log(context: Arc<Context>, line: String) -> anyhow::Result<()> {
    let line: AuditLog = serde_json::from_str(&line)?;

    let Some(peer_address) = line.peer_address else {
        return Ok(());
    };

    let mut connection = context.db.get().await?;
    let tx = connection.transaction().await?;

    tokio::try_join!(
        async {
            tx
                .execute(
                    "INSERT INTO audit (timestamp, connection_id, peer_address, host) VALUES ($1, $2, $3, $4)",
                    &[&line.ts, &line.connection_id, &peer_address.to_string(), &line.host],
                )
                .await
                .map_err(anyhow::Error::from)
        },
        async {
            let prepared = tx.prepare("INSERT INTO audit_environment_variables (connection_id, name, value) VALUES ($1, $2, $3)").await?;

            futures::future::try_join_all(line.environment_variables.iter().map(
                |(key, value)| async {
                    tx.execute(&prepared, &[&line.connection_id, key, value])
                        .await
                },
            ))
            .await
            .map_err(anyhow::Error::from)
        },
        async {
            let prepared = tx.prepare("INSERT INTO audit_events (timestamp, connection_id, type, content) VALUES ($1, $2, $3, $4)").await?;

            futures::future::try_join_all(
                line.events
                    .iter()
                    .map(|event| insert_event(&tx, &prepared, &line, event)),
            )
            .await
        }
    )?;

    tx.commit().await?;

    Ok(())
}

async fn insert_event(
    tx: &Transaction<'_>,
    prepared: &Statement,
    line: &AuditLog,
    event: &AuditLogEvent,
) -> anyhow::Result<()> {
    let ts = line.ts + event.start_offset;

    tx.execute(
        prepared,
        &[
            &ts,
            &line.connection_id,
            &<&'static str>::from(&event.action),
            &serde_json::to_value(&event.action)?,
        ],
    )
    .await?;

    Ok(())
}
