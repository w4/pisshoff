use std::{fmt::Write, path::Path};

use async_trait::async_trait;
use thrussh::ChannelId;

use crate::{
    command::{Command, CommandResult},
    server::{ConnectionState, ThrusshSession},
};

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
        let mut error = false;

        let resp = if params.is_empty() {
            match connection.file_system().ls(None) {
                Ok(v) => v.join("  "),
                Err(e) => {
                    error = true;
                    format!("ls: {}: {e}", connection.file_system().pwd().display())
                }
            }
        } else if params.len() == 1 {
            match connection
                .file_system()
                .ls(Some(Path::new(params.first().unwrap())))
            {
                Ok(v) => v.join("  "),
                Err(e) => {
                    error = true;
                    format!("ls: {}: {e}", params.first().unwrap())
                }
            }
        } else {
            let mut out = String::new();

            for dir in params {
                if !out.is_empty() {
                    out.push('\n');
                }

                match connection.file_system().ls(Some(Path::new(dir))) {
                    Ok(v) => {
                        write!(out, "{dir}:\n{}", v.join("  ")).unwrap();
                    }
                    Err(e) => {
                        error = true;
                        write!(out, "ls: {dir}: {e}").unwrap();
                    }
                }
            }

            out
        };

        if !resp.is_empty() {
            let resp = resp.trim();
            session.data(channel, format!("{resp}\n").into());
        }

        CommandResult::Exit(u32::from(error))
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
    use std::path::Path;

    use mockall::predicate::always;

    use crate::{
        command::{ls::Ls, Command, CommandResult},
        server::{
            test::{fake_channel_id, predicate::eq_string},
            ConnectionState, MockThrusshSession,
        },
    };

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

        let mut state = ConnectionState::mock();
        state.file_system().mkdirall(Path::new("/root/a")).unwrap();
        state.file_system().mkdirall(Path::new("/root/b")).unwrap();

        let out = Ls::new(
            &mut state,
            ["a".to_string(), "b".to_string()].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::Exit(0)), "{out:?}");
    }
}
