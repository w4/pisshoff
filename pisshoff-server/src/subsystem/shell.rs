use crate::{
    command::{CommandResult, ConcreteCommand},
    server::Connection,
    subsystem::Subsystem,
};
use async_trait::async_trait;
use pisshoff_types::audit::{AuditLogAction, ExecCommandEvent};
use thrussh::{server::Session, ChannelId};

pub const SHELL_PROMPT: &str = "bash-5.1$ ";

#[derive(Clone, Debug)]
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
        command_result: CommandResult<ConcreteCommand>,
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
        connection: &mut Connection,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) {
        loop {
            let (next, terminal) = match std::mem::take(&mut self.state) {
                State::Prompt => {
                    let Some(args) = shlex::split(String::from_utf8_lossy(data).as_ref()) else {
                        return;
                    };

                    connection
                        .audit_log()
                        .push_action(AuditLogAction::ExecCommand(ExecCommandEvent {
                            args: Box::from(args.clone()),
                        }));

                    self.handle_command_result(
                        ConcreteCommand::new(connection, &args, channel, session).await,
                    )
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

            if terminal {
                break;
            }
        }

        if matches!(self.state, State::Prompt) {
            session.data(channel, SHELL_PROMPT.to_string().into());
        }
    }
}

#[derive(Debug, Clone, Default)]
enum State {
    #[default]
    Prompt,
    Running(ConcreteCommand),
    Exit(u32),
    Quit(u32),
}
