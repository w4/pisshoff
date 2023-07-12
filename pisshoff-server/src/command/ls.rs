use crate::{
    command::{Command, CommandResult},
    server::Connection,
};
use async_trait::async_trait;
use std::fmt::Write;
use thrussh::{server::Session, ChannelId};

#[derive(Debug, Clone)]
pub struct Ls {}

#[async_trait]
impl Command for Ls {
    async fn new(
        connection: &mut Connection,
        params: &[String],
        channel: ChannelId,
        session: &mut Session,
    ) -> CommandResult<Self> {
        let resp = if params.is_empty() {
            connection.file_system().ls(None).join("  ")
        } else if params.len() == 1 {
            connection
                .file_system()
                .ls(Some(params.get(0).unwrap()))
                .join("  ")
        } else {
            let mut out = String::new();

            for dir in params {
                if !out.is_empty() {
                    out.push_str("\n\n");
                }

                write!(out, "{dir}:").unwrap();
                out.push_str(&connection.file_system().ls(Some(dir)).join("  "));
            }

            out
        };

        if !resp.is_empty() {
            session.data(channel, format!("{resp}\n").into());
        }

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
