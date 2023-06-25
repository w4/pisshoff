use crate::config::Config;
use serde::Serialize;
use std::{
    fmt::{Debug, Formatter},
    io::ErrorKind,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use time::OffsetDateTime;
use tokio::{
    fs::OpenOptions,
    io::{AsyncWriteExt, BufWriter},
    task::JoinHandle,
};
use tracing::debug;
use uuid::Uuid;

pub fn start_audit_writer(
    config: Arc<Config>,
) -> (
    tokio::sync::mpsc::UnboundedSender<AuditLog>,
    JoinHandle<Result<(), std::io::Error>>,
) {
    let (send, mut recv) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(async move {
        let file = OpenOptions::default()
            .create(true)
            .append(true)
            .open(&config.audit_output_file)
            .await?;
        let mut writer = BufWriter::new(file);
        let mut shutdown = false;

        loop {
            tokio::select! {
                log = recv.recv(), if !shutdown => {
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
                _ = tokio::time::sleep(Duration::from_secs(5)), if !writer.buffer().is_empty() && !shutdown => {
                    debug!("Flushing audits to disk");
                    writer.flush().await?;
                }
                else => break,
            }
        }

        writer.flush().await?;

        Ok(())
    });

    (send, handle)
}

#[derive(Serialize)]
pub struct AuditLog {
    pub connection_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub peer_address: Option<SocketAddr>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub environment_variables: Vec<(Box<str>, Box<str>)>,
    pub events: Vec<AuditLogEvent>,
    #[serde(skip)]
    pub start: Instant,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self {
            connection_id: Uuid::default(),
            ts: OffsetDateTime::now_utc(),
            peer_address: None,
            environment_variables: vec![],
            events: vec![],
            start: Instant::now(),
        }
    }
}

impl Debug for AuditLog {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditLog")
            .field("connection_id", &self.connection_id)
            .field("peer_address", &self.peer_address)
            .field("environment_variables", &self.environment_variables)
            .field("events", &self.events)
            .finish()
    }
}

impl AuditLog {
    pub fn push_action(&mut self, action: AuditLogAction) {
        self.events.push(AuditLogEvent {
            start_offset: self.start.elapsed(),
            action,
        });
    }
}

#[derive(Debug, Serialize)]
pub struct AuditLogEvent {
    pub start_offset: Duration,
    pub action: AuditLogAction,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AuditLogAction {
    LoginAttempt(LoginAttemptEvent),
    PtyRequest(PtyRequestEvent),
    X11Request(X11RequestEvent),
    OpenX11(OpenX11Event),
    OpenDirectTcpIp(OpenDirectTcpIpEvent),
    ExecCommand(ExecCommandEvent),
    WindowAdjusted(WindowAdjustedEvent),
    ShellRequested,
    SubsystemRequest(SubsystemRequestEvent),
    WindowChangeRequest(WindowChangeRequestEvent),
    Signal(SignalEvent),
    TcpIpForward(TcpIpForwardEvent),
    CancelTcpIpForward(TcpIpForwardEvent),
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub struct ExecCommandEvent {
    pub args: Box<[String]>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub struct WindowAdjustedEvent {
    pub new_size: usize,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub struct SubsystemRequestEvent {
    pub name: Box<str>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub struct SignalEvent {
    pub name: Box<str>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LoginAttemptEvent {
    UsernamePassword {
        username: Box<str>,
        password: Box<str>,
    },
    PublicKey {
        kind: &'static str,
        fingerprint: Box<str>,
    },
}

#[derive(Debug, Serialize)]
pub struct PtyRequestEvent {
    pub term: Box<str>,
    pub col_width: u32,
    pub row_height: u32,
    pub pix_width: u32,
    pub pix_height: u32,
    pub modes: Box<[(u8, u32)]>,
}

#[derive(Debug, Serialize)]
pub struct OpenX11Event {
    pub originator_address: Box<str>,
    pub originator_port: u32,
}

#[derive(Debug, Serialize)]
pub struct X11RequestEvent {
    pub single_connection: bool,
    pub x11_auth_protocol: Box<str>,
    pub x11_auth_cookie: Box<str>,
    pub x11_screen_number: u32,
}

#[derive(Debug, Serialize)]
pub struct OpenDirectTcpIpEvent {
    pub host_to_connect: Box<str>,
    pub port_to_connect: u32,
    pub originator_address: Box<str>,
    pub originator_port: u32,
}

#[derive(Debug, Serialize)]
pub struct WindowChangeRequestEvent {
    pub col_width: u32,
    pub row_height: u32,
    pub pix_width: u32,
    pub pix_height: u32,
}

#[derive(Debug, Serialize)]
pub struct TcpIpForwardEvent {
    pub address: Box<str>,
    pub port: u32,
}
