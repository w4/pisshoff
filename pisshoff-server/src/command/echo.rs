use crate::{
    command::{Command, CommandResult},
    server::{ConnectionState, ThrusshSession},
};
use async_trait::async_trait;
use itertools::Itertools;
use thrussh::ChannelId;

#[derive(Debug, Clone)]
pub struct Echo {}

#[async_trait]
impl Command for Echo {
    async fn new<S: ThrusshSession + Send>(
        _connection: &mut ConnectionState,
        params: &[String],
        channel: ChannelId,
        session: &mut S,
    ) -> CommandResult<Self> {
        let suffix = if session.redirected() { "" } else { "\n" };

        session.data(
            channel,
            format!("{}{suffix}", params.iter().join(" ")).into(),
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
        command::{echo::Echo, Command, CommandResult},
        server::{
            test::{fake_channel_id, predicate::eq_string},
            ConnectionState, MockThrusshSession,
        },
    };
    use mockall::predicate::always;
    use test_case::test_case;

    #[test_case(&[], "\n"; "no parameters")]
    #[test_case(&["hello"], "hello\n"; "single parameter")]
    #[test_case(&["hello", "world"], "hello world\n"; "multiple parameters")]
    #[tokio::test]
    async fn test(params: &[&str], output: &'static str) {
        let mut session = MockThrusshSession::default();

        session
            .expect_data()
            .once()
            .with(always(), eq_string(output))
            .returning(|_, _| ());

        session.expect_redirected().returning(|| false);

        let out = Echo::new(
            &mut ConnectionState::mock(),
            params
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::Exit(0)), "{out:?}");
    }
}
