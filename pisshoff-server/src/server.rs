use crate::{
    audit::{
        AuditLog, AuditLogAction, LoginAttemptEvent, OpenDirectTcpIpEvent, OpenX11Event,
        PtyRequestEvent, X11RequestEvent,
    },
    audit::{
        SignalEvent, SubsystemRequestEvent, TcpIpForwardEvent, WindowAdjustedEvent,
        WindowChangeRequestEvent,
    },
    config::Config,
    file_system::FileSystem,
    state::State,
    subsystem::{self, shell::Shell, Subsystem as SubsystemTrait},
};
use futures::{
    future::{BoxFuture, InspectErr},
    FutureExt, TryFutureExt,
};
use std::{
    borrow::Cow,
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use thrussh::{
    server::{Auth, Response, Session},
    ChannelId, Pty, Sig,
};
use thrussh_keys::key::PublicKey;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;
use tracing::{debug, error, info, info_span, instrument::Instrumented, Instrument, Span};

pub static KEYBOARD_INTERACTIVE_PROMPT: &[(Cow<'static, str>, bool)] =
    &[(Cow::Borrowed("Password: "), false)];

#[derive(Clone)]
pub struct Server {
    config: Arc<Config>,
    state: Arc<State>,
    hostname: &'static str,
    audit_send: UnboundedSender<AuditLog>,
}

impl Server {
    pub fn new(
        hostname: &'static str,
        config: Arc<Config>,
        audit_send: UnboundedSender<AuditLog>,
    ) -> Self {
        Self {
            config,
            hostname,
            state: Arc::new(State::default()),
            audit_send,
        }
    }
}

impl thrussh::server::Server for Server {
    type Handler = Connection;

    fn new(&mut self, peer_addr: Option<SocketAddr>) -> Self::Handler {
        let connection_id = uuid::Uuid::new_v4();

        Connection {
            span: info_span!("connection", ?peer_addr, %connection_id),
            server: self.clone(),
            audit_log: AuditLog {
                connection_id,
                host: Cow::Borrowed(self.hostname),
                peer_address: peer_addr,
                ..AuditLog::default()
            },
            username: None,
            file_system: None,
            subsystem: HashMap::new(),
        }
    }
}

pub struct Connection {
    span: Span,
    server: Server,
    audit_log: AuditLog,
    username: Option<String>,
    file_system: Option<FileSystem>,
    subsystem: HashMap<ChannelId, Arc<Mutex<Subsystem>>>,
}

impl Connection {
    pub fn username(&self) -> &str {
        self.username.as_deref().unwrap_or("root")
    }

    pub fn file_system(&mut self) -> &mut FileSystem {
        if self.file_system.is_none() {
            self.file_system = Some(FileSystem::new(self.username()));
        }

        self.file_system.as_mut().unwrap()
    }

    pub fn audit_log(&mut self) -> &mut AuditLog {
        &mut self.audit_log
    }

    fn try_login(&mut self, user: &str, password: &str) -> bool {
        self.username = Some(user.to_string());

        let res = if self
            .server
            .state
            .previously_accepted_passwords
            .seen(user, password)
        {
            info!(user, password, "Accepted login due to it being used before");
            true
        } else if fastrand::f64() <= self.server.config.access_probability {
            info!(user, password, "Accepted login randomly");
            self.server
                .state
                .previously_accepted_passwords
                .store(user, password);
            true
        } else {
            info!(?user, ?password, "Rejected login");
            false
        };

        self.audit_log.push_action(AuditLogAction::LoginAttempt(
            LoginAttemptEvent::UsernamePassword {
                username: Box::from(user),
                password: Box::from(password),
            },
        ));

        res
    }
}

impl thrussh::server::Handler for Connection {
    type Error = anyhow::Error;
    type FutureAuth = HandlerFuture<Auth>;
    type FutureUnit = HandlerFuture<Session>;
    type FutureBool =
        ServerFuture<Self::Error, BoxFuture<'static, Result<(Self, Session, bool), Self::Error>>>;

    fn finished_auth(self, auth: Auth) -> Self::FutureAuth {
        let span = info_span!(parent: &self.span, "finished_auth");
        futures::future::ok((self, auth)).boxed().wrap(span)
    }

    fn finished_bool(self, b: bool, session: Session) -> Self::FutureBool {
        let span = info_span!(parent: &self.span, "finished_bool");
        let _entered = span.enter();

        futures::future::ok((self, session, b))
            .boxed()
            .wrap(Span::current())
    }

    fn finished(self, session: Session) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "finished");
        let _entered = span.enter();

        futures::future::ok((self, session))
            .boxed()
            .wrap(Span::current())
    }

    fn auth_none(self, _user: &str) -> Self::FutureAuth {
        let span = info_span!(parent: &self.span, "auth_none");

        self.finished_auth(Auth::UnsupportedMethod)
            .boxed()
            .wrap(span)
    }

    fn auth_password(mut self, user: &str, password: &str) -> Self::FutureAuth {
        let span = info_span!(parent: &self.span, "auth_password");
        let _entered = span.enter();

        let res = if self.try_login(user, password) {
            Auth::Accept
        } else {
            Auth::Reject
        };

        self.finished_auth(res)
    }

    fn auth_publickey(mut self, _user: &str, public_key: &PublicKey) -> Self::FutureAuth {
        let span = info_span!(parent: &self.span, "auth_publickey");
        let _entered = span.enter();

        let kind = public_key.name();
        let fingerprint = public_key.fingerprint();

        self.audit_log
            .push_action(AuditLogAction::LoginAttempt(LoginAttemptEvent::PublicKey {
                kind: Cow::Borrowed(kind),
                fingerprint: Box::from(fingerprint),
            }));

        self.finished_auth(Auth::Reject)
            .boxed()
            .wrap(Span::current())
    }

    fn auth_keyboard_interactive(
        mut self,
        user: &str,
        _submethods: &str,
        mut response: Option<Response>,
    ) -> Self::FutureAuth {
        let span = info_span!(parent: &self.span, "auth_keyboard_interactive");
        let _entered = span.enter();

        let result = if let Some(password) = response
            .as_mut()
            .and_then(Response::next)
            .map(String::from_utf8_lossy)
        {
            if self.try_login(user, password.as_ref()) {
                Auth::Accept
            } else {
                Auth::Reject
            }
        } else {
            debug!("Client is attempting keyboard-interactive, obliging");

            Auth::Partial {
                name: "".into(),
                instructions: "".into(),
                prompts: KEYBOARD_INTERACTIVE_PROMPT.into(),
            }
        };

        self.finished_auth(result)
    }

    fn channel_close(self, channel: ChannelId, mut session: Session) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "channel_close");
        let _entered = span.enter();

        session.channel_success(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn channel_eof(mut self, channel: ChannelId, mut session: Session) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "channel_eof");
        let _entered = span.enter();

        if self.subsystem.remove(&channel).is_some() {
            session.exit_status_request(channel, 0);
            session.channel_success(channel);
        } else {
            session.channel_failure(channel);
        }

        session.close(channel);

        self.finished(session).boxed().wrap(Span::current())
    }

    fn channel_open_session(self, channel: ChannelId, mut session: Session) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "channel_open_session");
        let _entered = span.enter();

        session.channel_success(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn channel_open_x11(
        mut self,
        channel: ChannelId,
        originator_address: &str,
        originator_port: u32,
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "channel_open_x11");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::OpenX11(OpenX11Event {
                originator_address: Box::from(originator_address),
                originator_port,
            }));

        session.channel_failure(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn channel_open_direct_tcpip(
        mut self,
        channel: ChannelId,
        host_to_connect: &str,
        port_to_connect: u32,
        originator_address: &str,
        originator_port: u32,
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "channel_open_direct_tcpip");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::OpenDirectTcpIp(OpenDirectTcpIpEvent {
                host_to_connect: Box::from(host_to_connect),
                port_to_connect,
                originator_address: Box::from(originator_address),
                originator_port,
            }));

        session.channel_failure(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn data(mut self, channel: ChannelId, data: &[u8], mut session: Session) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "data");
        let _entered = span.enter();

        // TODO: don't unwrap
        let subsystem = self.subsystem.get(&channel).unwrap().clone();
        let data = data.to_vec();

        async move {
            let mut subsystem = subsystem.lock().await;

            match &mut *subsystem {
                Subsystem::Shell(ref mut inner) => {
                    inner.data(&mut self, channel, &data, &mut session).await;
                }
                Subsystem::Sftp(ref mut inner) => {
                    inner.data(&mut self, channel, &data, &mut session).await;
                }
            }

            self.finished(session).await
        }
        .boxed()
        .wrap(Span::current())
    }

    fn extended_data(
        self,
        _channel: ChannelId,
        _code: u32,
        _data: &[u8],
        session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "extended_data");
        let _entered = span.enter();

        self.finished(session).boxed().wrap(Span::current())
    }

    fn window_adjusted(
        mut self,
        _channel: ChannelId,
        new_window_size: usize,
        session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "window_adjusted");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::WindowAdjusted(WindowAdjustedEvent {
                new_size: new_window_size,
            }));

        self.finished(session).boxed().wrap(Span::current())
    }

    fn adjust_window(&mut self, _channel: ChannelId, current: u32) -> u32 {
        let span = info_span!(parent: &self.span, "adjust_window");
        let _entered = span.enter();

        current
    }

    fn pty_request(
        mut self,
        channel: ChannelId,
        term: &str,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        modes: &[(Pty, u32)],
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "pty_request");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::PtyRequest(PtyRequestEvent {
                term: Box::from(term),
                col_width,
                row_height,
                pix_width,
                pix_height,
                modes: Box::from(
                    modes
                        .iter()
                        .copied()
                        .map(|(pty, val)| (pty as u8, val))
                        .collect::<Vec<_>>(),
                ),
            }));

        session.channel_failure(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn x11_request(
        mut self,
        channel: ChannelId,
        single_connection: bool,
        x11_auth_protocol: &str,
        x11_auth_cookie: &str,
        x11_screen_number: u32,
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "x11_request");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::X11Request(X11RequestEvent {
                single_connection,
                x11_auth_protocol: Box::from(x11_auth_protocol),
                x11_auth_cookie: Box::from(x11_auth_cookie),
                x11_screen_number,
            }));

        session.channel_failure(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn env_request(
        mut self,
        channel: ChannelId,
        variable_name: &str,
        variable_value: &str,
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "env_request");
        let _entered = span.enter();

        self.audit_log
            .environment_variables
            .push((Box::from(variable_name), Box::from(variable_value)));

        session.channel_success(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn shell_request(mut self, channel: ChannelId, mut session: Session) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "shell_request");
        let _entered = span.enter();

        self.audit_log.push_action(AuditLogAction::ShellRequested);

        let shell = Shell::new(true, channel, &mut session);
        self.subsystem
            .insert(channel, Arc::new(Mutex::new(Subsystem::Shell(shell))));

        session.channel_success(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn exec_request(
        mut self,
        channel: ChannelId,
        data: &[u8],
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "exec_request");
        let _entered = span.enter();

        let data = data.to_vec();

        async move {
            let mut shell = Shell::new(false, channel, &mut session);
            shell.data(&mut self, channel, &data, &mut session).await;

            self.subsystem
                .insert(channel, Arc::new(Mutex::new(Subsystem::Shell(shell))));

            session.channel_success(channel);
            self.finished(session).await
        }
        .boxed()
        .wrap(Span::current())
    }

    fn subsystem_request(
        mut self,
        channel: ChannelId,
        name: &str,
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "subsystem_request");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::SubsystemRequest(SubsystemRequestEvent {
                name: Box::from(name),
            }));

        let subsystem = match name {
            subsystem::sftp::Sftp::NAME => Some(Subsystem::Sftp(subsystem::sftp::Sftp::default())),
            _ => None,
        };

        if let Some(subsystem) = subsystem {
            self.subsystem
                .insert(channel, Arc::new(Mutex::new(subsystem)));
            session.channel_success(channel);
        } else {
            session.channel_failure(channel);
        }

        self.finished(session).boxed().wrap(Span::current())
    }

    fn window_change_request(
        mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        pix_width: u32,
        pix_height: u32,
        mut session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "window_change_request");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::WindowChangeRequest(
                WindowChangeRequestEvent {
                    col_width,
                    row_height,
                    pix_width,
                    pix_height,
                },
            ));

        session.channel_success(channel);
        self.finished(session).boxed().wrap(Span::current())
    }

    fn signal(
        mut self,
        _channel: ChannelId,
        signal_name: Sig,
        session: Session,
    ) -> Self::FutureUnit {
        let span = info_span!(parent: &self.span, "signal");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::Signal(SignalEvent {
                name: format!("{signal_name:?}").into(),
            }));

        self.finished(session).boxed().wrap(Span::current())
    }

    fn tcpip_forward(mut self, address: &str, port: u32, session: Session) -> Self::FutureBool {
        let span = info_span!(parent: &self.span, "tcpip_forward");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::TcpIpForward(TcpIpForwardEvent {
                address: Box::from(address),
                port,
            }));

        self.finished_bool(false, session)
            .boxed()
            .wrap(Span::current())
    }

    fn cancel_tcpip_forward(
        mut self,
        address: &str,
        port: u32,
        session: Session,
    ) -> Self::FutureBool {
        let span = info_span!(parent: &self.span, "cancel_tcpip_forward");
        let _entered = span.enter();

        self.audit_log
            .push_action(AuditLogAction::CancelTcpIpForward(TcpIpForwardEvent {
                address: Box::from(address),
                port,
            }));

        self.finished_bool(false, session)
            .boxed()
            .wrap(Span::current())
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let span = info_span!(parent: &self.span, "drop");
        let _entered = span.enter();

        info!("Connection closed");

        let _res = self
            .server
            .audit_send
            .send(std::mem::take(&mut self.audit_log));
    }
}

#[derive(Debug, Clone)]
pub enum Subsystem {
    Shell(subsystem::shell::Shell),
    Sftp(subsystem::sftp::Sftp),
}

type HandlerResult<T> = Result<T, <Connection as thrussh::server::Handler>::Error>;
type HandlerFuture<T> = ServerFuture<
    <Connection as thrussh::server::Handler>::Error,
    BoxFuture<'static, HandlerResult<(Connection, T)>>,
>;

/// Wraps a future, providing logging and instrumentation. This provides a newtype over the future
/// (`ServerFuture`) in order to enforce usage within the `thrussh::server::Handler` impl.
pub trait WrapFuture: Sized {
    type Ok;
    type Err;

    fn wrap(self, span: Span) -> ServerFuture<Self::Err, Self>;
}

impl<T, F: Future<Output = Result<T, anyhow::Error>>> WrapFuture for F {
    type Ok = T;
    type Err = anyhow::Error;

    fn wrap(self, span: Span) -> ServerFuture<Self::Err, Self> {
        ServerFuture(
            self.inspect_err(log_err as fn(&anyhow::Error))
                .instrument(span),
        )
    }
}

/// Logs an error from a future result.
fn log_err(e: &anyhow::Error) {
    error!("Connection closed due to: {}", e);
}

/// A wrapped future, providing logging ad instrumentation.
#[allow(clippy::type_complexity)]
pub struct ServerFuture<E, F>(Instrumented<InspectErr<F, fn(&E)>>);

impl<T, E, F: Future<Output = Result<T, E>> + Unpin> Future for ServerFuture<E, F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}
