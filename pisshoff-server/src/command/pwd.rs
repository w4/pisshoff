use crate::{
    command::{Command, CommandResult},
    server::{ConnectionState, ThrusshSession},
};
use async_trait::async_trait;
use thrussh::ChannelId;

#[derive(Debug, Clone)]
pub struct Pwd {}

#[async_trait]
impl Command for Pwd {
    async fn new<S: ThrusshSession + Send>(
        connection: &mut ConnectionState,
        _params: &[String],
        channel: ChannelId,
        session: &mut S,
    ) -> CommandResult<Self> {
        session.data(
            channel,
            format!("{}\n", connection.file_system().pwd().display()).into(),
        );

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
        command::{pwd::Pwd, Command, CommandResult},
        server::{
            test::{fake_channel_id, predicate::eq_string},
            ConnectionState, MockThrusshSession,
        },
    };
    use mockall::predicate::always;

    #[tokio::test]
    async fn works() {
        let mut session = MockThrusshSession::default();

        session
            .expect_data()
            .once()
            .with(always(), eq_string("/root\n"))
            .returning(|_, _| ());

        let out = Pwd::new(
            &mut ConnectionState::mock(),
            [].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::Exit(0)), "{out:?}");
    }
}
