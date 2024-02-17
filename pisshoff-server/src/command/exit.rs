use std::str::FromStr;

use async_trait::async_trait;
use thrussh::ChannelId;

use crate::{
    command::{Command, CommandResult},
    server::{ConnectionState, ThrusshSession},
};

#[derive(Debug, Clone)]
pub struct Exit {}

#[async_trait]
impl Command for Exit {
    async fn new<S: ThrusshSession + Send>(
        _connection: &mut ConnectionState,
        params: &[String],
        _channel: ChannelId,
        _session: &mut S,
    ) -> CommandResult<Self> {
        let exit_status = params
            .first()
            .map(String::as_str)
            .map_or(Ok(0), u32::from_str)
            .unwrap_or(2);

        CommandResult::Close(exit_status)
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
    use test_case::test_case;

    use crate::{
        command::{exit::Exit, Command, CommandResult},
        server::{test::fake_channel_id, ConnectionState, MockThrusshSession},
    };

    #[test_case(&[], 0; "no parameters")]
    #[test_case(&["3"], 3; "with parameter")]
    #[test_case(&["invalid"], 2; "invalid parameter")]
    #[tokio::test]
    async fn test(params: &[&str], expected_exit_code: u32) {
        let mut session = MockThrusshSession::default();

        let out = Exit::new(
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

        assert!(
            matches!(out, CommandResult::Close(v) if v == expected_exit_code),
            "{out:?}"
        );
    }
}
