use crate::{
    command::{Command, CommandResult},
    server::Connection,
};
use async_trait::async_trait;
use thrussh::{server::Session, ChannelId};

#[derive(Debug, Clone)]
pub struct Pwd {}

#[async_trait]
impl Command for Pwd {
    async fn new(
        connection: &mut Connection,
        _params: &[String],
        channel: ChannelId,
        session: &mut Session,
    ) -> CommandResult<Self> {
        session.data(
            channel,
            format!("{}\n", connection.file_system().pwd().display()).into(),
        );

        CommandResult::Exit(0)
    }

    async fn stdin(
        self,
        _connection: &mut Connection,
        _channel: ChannelId,
        _data: &[u8],
        _session: &mut Session,
    ) -> CommandResult<Self> {
        CommandResult::Exit(0)
    }
}
