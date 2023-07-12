use crate::{
    command::{Command, CommandResult},
    server::Connection,
};
use async_trait::async_trait;
use std::str::FromStr;
use thrussh::{server::Session, ChannelId};

#[derive(Debug, Clone)]
pub struct Exit {}

#[async_trait]
impl Command for Exit {
    async fn new(
        _connection: &mut Connection,
        params: &[String],
        _channel: ChannelId,
        _session: &mut Session,
    ) -> CommandResult<Self> {
        let exit_status = params
            .get(0)
            .map(String::as_str)
            .map_or(Ok(0), u32::from_str)
            .unwrap_or(2);

        CommandResult::Close(exit_status)
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
