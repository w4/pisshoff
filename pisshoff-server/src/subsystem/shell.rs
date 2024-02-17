mod parser;

use async_trait::async_trait;
use pisshoff_types::audit::{AuditLogAction, ExecCommandEvent};
use thrussh::{server::Session, ChannelId};
use tracing::info;

use crate::{
    command::{CommandResult, ConcreteCommand},
    server::{ConnectionState, EitherSession, StdoutCaptureSession},
    subsystem::{
        shell::parser::{tokenize, IterState, ParsedPart},
        Subsystem,
    },
};

pub const SHELL_PROMPT: &str = "bash-5.1$ ";

type IResult<I, O> = nom::IResult<I, O, nom_supreme::error::ErrorTree<I>>;

#[derive(Debug)]
pub struct Shell {
    interactive: bool,
    state: State,
}

impl Shell {
    pub fn new(interactive: bool, channel: ChannelId, session: &mut Session) -> Self {
        if interactive {
            session.data(channel, SHELL_PROMPT.to_string().into());
        }

        Self {
            interactive,
            state: State::Prompt,
        }
    }

    fn handle_command_result(
        &self,
        command_result: CommandResult<ExecutingCommand>,
    ) -> (State, bool) {
        match (command_result, self.interactive) {
            (CommandResult::ReadStdin(cmd), _) => (State::Running(cmd), true),
            (CommandResult::Exit(exit_status), true) => (State::Exit(exit_status), false),
            (CommandResult::Exit(exit_status), false) | (CommandResult::Close(exit_status), _) => {
                (State::Quit(exit_status), false)
            }
        }
    }
}

#[async_trait]
impl Subsystem for Shell {
    const NAME: &'static str = "shell";

    async fn data(
        &mut self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) {
        loop {
            let (next, end) = match std::mem::take(&mut self.state) {
                State::Prompt => {
                    connection
                        .audit_log()
                        .push_action(AuditLogAction::ExecCommand(ExecCommandEvent {
                            args: Box::from(vec![String::from_utf8_lossy(data).to_string()]),
                        }));

                    match tokenize(data) {
                        Ok((_unparsed, args)) => {
                            let cmd = parser::Iter::new(
                                args.into_iter().map(ParsedPart::into_owned).collect(),
                            );
                            self.handle_command_result(
                                ExecutingCommand::new(cmd, connection, channel, session).await,
                            )
                        }
                        Err(e) => {
                            // TODO
                            info!("Invalid syntax: {e}");
                            session.data(channel, "bash: syntax error\n".to_string().into());
                            (State::Prompt, true)
                        }
                    }
                }
                State::Running(command) => self
                    .handle_command_result(command.stdin(connection, channel, data, session).await),
                State::Exit(exit_status) => {
                    session.exit_status_request(channel, exit_status);
                    (State::Prompt, true)
                }
                State::Quit(exit_status) => {
                    session.exit_status_request(channel, exit_status);
                    session.close(channel);
                    break;
                }
            };

            self.state = next;

            if end {
                break;
            }
        }

        if matches!(self.state, State::Prompt) {
            session.data(channel, SHELL_PROMPT.to_string().into());
        }
    }
}

#[derive(Debug)]
pub struct ExecutingCommand {
    iter: parser::Iter<'static>,
    current: ConcreteCommand,
    buf: Option<Vec<u8>>,
}

impl ExecutingCommand {
    async fn new(
        iter: parser::Iter<'static>,
        connection: &mut ConnectionState,
        channel: ChannelId,
        session: &mut Session,
    ) -> CommandResult<Self> {
        Self::new_inner(Vec::new(), iter, connection, channel, session).await
    }

    async fn new_inner(
        mut buf: Vec<u8>,
        mut iter: parser::Iter<'static>,
        connection: &mut ConnectionState,
        channel: ChannelId,
        session: &mut Session,
    ) -> CommandResult<Self> {
        loop {
            let (has_next, current) = match iter.step(
                connection.environment(),
                Some(std::mem::take(&mut buf)).filter(|v| !v.is_empty()),
            ) {
                IterState::Expand(cmd) => (true, cmd),
                IterState::Ready(cmd) => (false, cmd),
            };

            let mut session = if has_next {
                EitherSession::L(StdoutCaptureSession::new(&mut buf))
            } else {
                EitherSession::R(&mut *session)
            };

            match (
                current
                    .into_concrete_command(connection, channel, &mut session)
                    .await,
                has_next,
            ) {
                (CommandResult::ReadStdin(cmd), has_next) => {
                    break CommandResult::ReadStdin(Self {
                        iter,
                        current: cmd,
                        buf: has_next.then_some(buf),
                    })
                }
                (CommandResult::Exit(_status), true) => {
                    continue;
                }
                (CommandResult::Exit(status), false) => {
                    break CommandResult::Exit(status);
                }
                (CommandResult::Close(status), _) => {
                    break CommandResult::Close(status);
                }
            }
        }
    }

    async fn stdin(
        mut self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> CommandResult<Self> {
        let mut sess = if let Some(buf) = &mut self.buf {
            EitherSession::L(StdoutCaptureSession::new(buf))
        } else {
            EitherSession::R(&mut *session)
        };

        match self
            .current
            .stdin(connection, channel, data, &mut sess)
            .await
        {
            CommandResult::ReadStdin(cmd) => CommandResult::ReadStdin(Self {
                iter: self.iter,
                current: cmd,
                buf: self.buf,
            }),
            CommandResult::Exit(_) => {
                Self::new_inner(
                    self.buf.unwrap_or_default(),
                    self.iter,
                    connection,
                    channel,
                    session,
                )
                .await
            }
            CommandResult::Close(status) => CommandResult::Close(status),
        }
    }
}

#[derive(Debug, Default)]
enum State {
    #[default]
    Prompt,
    Running(ExecutingCommand),
    Exit(u32),
    Quit(u32),
}
