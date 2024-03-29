use std::{
    borrow::Cow,
    fmt::{Debug, Formatter},
    net::SocketAddr,
    time::{Duration, Instant},
};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use strum::IntoStaticStr;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct AuditLog {
    pub connection_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub ts: OffsetDateTime,
    pub peer_address: Option<SocketAddr>,
    pub host: Cow<'static, str>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub environment_variables: Vec<(Box<str>, Box<str>)>,
    pub events: Vec<AuditLogEvent>,
    #[serde(skip, default = "Instant::now")]
    pub start: Instant,
}

impl Default for AuditLog {
    fn default() -> Self {
        Self {
            connection_id: Uuid::default(),
            ts: OffsetDateTime::now_utc(),
            host: Cow::Borrowed(""),
            peer_address: None,
            environment_variables: vec![],
            events: vec![],
            start: Instant::now(),
        }
    }
}

#[allow(clippy::missing_fields_in_debug)]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct AuditLogEvent {
    pub start_offset: Duration,
    pub action: AuditLogAction,
}

#[derive(Debug, Serialize, Deserialize, IntoStaticStr)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
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
    Mkdir(MkdirEvent),
    WriteFile(WriteFileEvent),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MkdirEvent {
    pub path: Box<str>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WriteFileEvent {
    pub path: Box<str>,
    pub content: Bytes,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExecCommandEvent {
    pub args: Box<[String]>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WindowAdjustedEvent {
    pub new_size: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubsystemRequestEvent {
    pub name: Box<str>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignalEvent {
    pub name: Box<str>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "credential-type", rename_all = "kebab-case")]
pub enum LoginAttemptEvent {
    UsernamePassword {
        username: Box<str>,
        password: Box<str>,
    },
    PublicKey {
        kind: Cow<'static, str>,
        fingerprint: Box<str>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PtyRequestEvent {
    pub term: Box<str>,
    pub col_width: u32,
    pub row_height: u32,
    pub pix_width: u32,
    pub pix_height: u32,
    pub modes: Box<[(u8, u32)]>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenX11Event {
    pub originator_address: Box<str>,
    pub originator_port: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct X11RequestEvent {
    pub single_connection: bool,
    pub x11_auth_protocol: Box<str>,
    pub x11_auth_cookie: Box<str>,
    pub x11_screen_number: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenDirectTcpIpEvent {
    pub host_to_connect: Box<str>,
    pub port_to_connect: u32,
    pub originator_address: Box<str>,
    pub originator_port: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WindowChangeRequestEvent {
    pub col_width: u32,
    pub row_height: u32,
    pub pix_width: u32,
    pub pix_height: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TcpIpForwardEvent {
    pub address: Box<str>,
    pub port: u32,
}
