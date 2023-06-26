use crate::config::Config;
pub use pisshoff_types::audit::*;
use std::{io::ErrorKind, sync::Arc, time::Duration};
use tokio::{
    fs::OpenOptions,
    io::{AsyncWriteExt, BufWriter},
    sync::{oneshot, watch},
    task::JoinHandle,
};
use tracing::{debug, info};

pub fn start_audit_writer(
    config: Arc<Config>,
    mut reload: watch::Receiver<()>,
    mut shutdown_recv: oneshot::Receiver<()>,
) -> (
    tokio::sync::mpsc::UnboundedSender<AuditLog>,
    JoinHandle<Result<(), std::io::Error>>,
) {
    let (send, mut recv) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let open_writer = || async {
            let file = OpenOptions::default()
                .create(true)
                .append(true)
                .open(&config.audit_output_file)
                .await?;
            Ok::<_, std::io::Error>(BufWriter::new(file))
        };

        let mut writer = open_writer().await?;
        let mut shutdown = false;

        while !shutdown {
            tokio::select! {
                log = recv.recv() => {
                    match log {
                        Some(log) => {
                            let log = serde_json::to_vec(&log)
                                .map_err(|e| std::io::Error::new(ErrorKind::Other, e))?;
                            writer.write_all(&log).await?;
                            writer.write_all("\n".as_bytes()).await?;
                        }
                        None => {
                            shutdown = true;
                        }
                    }
                }
                _ = &mut shutdown_recv => {
                    shutdown = true;
                }
                _ = tokio::time::sleep(Duration::from_secs(5)), if !writer.buffer().is_empty() => {
                    debug!("Flushing audits to disk");
                    writer.flush().await?;
                }
                Ok(()) = reload.changed() => {
                    info!("Flushing audits to disk");
                    writer.flush().await?;

                    info!("Reopening handle to log file");
                    writer = open_writer().await?;

                    info!("Successfully re-opened log file");
                }
                else => break,
            }
        }

        writer.flush().await?;

        Ok(())
    });

    (send, handle)
}
