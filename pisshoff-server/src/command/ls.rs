use crate::{
    command::{Command, CommandResult},
    server::{ConnectionState, ThrusshSession},
};
use async_trait::async_trait;
use std::fmt::Write;
use thrussh::ChannelId;

#[derive(Debug, Clone)]
pub struct Ls {}

#[async_trait]
impl Command for Ls {
    async fn new<S: ThrusshSession + Send>(
        connection: &mut ConnectionState,
        params: &[String],
        channel: ChannelId,
        session: &mut S,
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

    async fn stdin<S: ThrusshSession + Send>(
        self,
        _connection: &mut ConnectionState,
        _channel: ChannelId,
        _data: &[u8],
        _session: &mut S,
    ) -> CommandResult<Self> {
        CommandResult::Exit(0)
    }
}

#[cfg(test)]
mod test {
    use crate::{
        command::{ls::Ls, Command, CommandResult},
        server::{
            test::{fake_channel_id, predicate::eq_string},
            ConnectionState, MockThrusshSession,
        },
    };
    use mockall::predicate::always;

    #[tokio::test]
    async fn empty_pwd() {
        let mut session = MockThrusshSession::default();

        let out = Ls::new(
            &mut ConnectionState::mock(),
            [].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::Exit(0)), "{out:?}");
    }

    #[tokio::test]
    async fn multiple_empty_directories() {
        let mut session = MockThrusshSession::default();

        session
            .expect_data()
            .once()
            .with(always(), eq_string("a:\n\nb:\n"))
            .returning(|_, _| ());

        let out = Ls::new(
            &mut ConnectionState::mock(),
            ["a".to_string(), "b".to_string()].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::Exit(0)), "{out:?}");
    }
}
