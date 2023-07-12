use crate::{
    command::{run_command, ConcreteLongRunningCommand},
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
        let next = match std::mem::take(&mut self.state) {
            State::Prompt => {
                let Some(args) = shlex::split(String::from_utf8_lossy(data).as_ref()) else {
                    return;
                };

                connection
                    .audit_log()
                    .push_action(AuditLogAction::ExecCommand(ExecCommandEvent {
                        args: Box::from(args.clone()),
                    }));

                run_command(&args, channel, session, connection)
                    .await
                    .map_or(State::Prompt, State::Running)
            }
            State::Running(command) => command
                .data(connection, channel, data, session)
                .await
                .map_or(State::Prompt, State::Running),
        };

        if matches!(next, State::Prompt) {
            if self.interactive {
                session.data(channel, SHELL_PROMPT.to_string().into());
            } else {
                session.exit_status_request(channel, 0);
                session.close(channel);
            }
        }

        self.state = next;
    }
}

#[derive(Debug, Clone, Default)]
enum State {
    #[default]
    Prompt,
    Running(ConcreteLongRunningCommand),
}
