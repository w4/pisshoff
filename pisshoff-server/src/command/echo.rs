use crate::{
    command::{Command, CommandResult},
    server::Connection,
};
use async_trait::async_trait;
use itertools::Itertools;
use thrussh::{server::Session, ChannelId};

#[derive(Debug, Clone)]
pub struct Echo {}

#[async_trait]
impl Command for Echo {
    async fn new(
        _connection: &mut Connection,
        params: &[String],
        channel: ChannelId,
        session: &mut Session,
    ) -> CommandResult<Self> {
        session.data(channel, format!("{}\n", params.iter().join(" ")).into());

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
