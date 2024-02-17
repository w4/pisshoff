use std::{collections::VecDeque, path::Path};

use async_trait::async_trait;
use thrussh::ChannelId;

use crate::{
    command::{Command, CommandResult},
    server::{ConnectionState, ThrusshSession},
};

#[derive(Debug, Clone)]
pub struct Cat {
    remaining_params: VecDeque<String>,
    status: u32,
}

impl Cat {
    fn run<S: ThrusshSession + Send>(
        mut self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        session: &mut S,
    ) -> CommandResult<Self> {
        while let Some(param) = self.remaining_params.pop_front() {
            if param == "-" {
                return CommandResult::ReadStdin(self);
            }

            match connection.file_system().read(Path::new(&param)) {
                Ok(content) => {
                    session.data(channel, content.to_vec().into());
                }
                Err(e) => {
                    self.status = 1;
                    // TODO: stderr
                    eprintln!("{e}");
                    session.data(channel, format!("cat: {param}: {e}").into());
                }
            }
        }

        CommandResult::Exit(self.status)
    }
}

#[async_trait]
impl Command for Cat {
    async fn new<S: ThrusshSession + Send>(
        connection: &mut ConnectionState,
        params: &[String],
        channel: ChannelId,
        session: &mut S,
    ) -> CommandResult<Self> {
        let this = Self {
            remaining_params: params.to_vec().into(),
            status: 0,
        };

        if params.is_empty() {
            CommandResult::ReadStdin(this)
        } else {
            this.run(connection, channel, session)
        }
    }

    async fn stdin<S: ThrusshSession + Send>(
        self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        data: &[u8],
        session: &mut S,
    ) -> CommandResult<Self> {
        session.data(channel, data.to_vec().into());
        self.run(connection, channel, session)
    }
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use mockall::predicate::always;

    use crate::{
        command::{cat::Cat, Command, CommandResult},
        server::{
            test::{fake_channel_id, predicate::eq_string},
            ConnectionState, MockThrusshSession,
        },
    };

    #[tokio::test]
    async fn no_args() {
        let mut session = MockThrusshSession::default();

        let out = Cat::new(
            &mut ConnectionState::mock(),
            [].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::ReadStdin(_)), "{out:?}");
    }

    #[tokio::test]
    async fn file_args_with_missing() {
        let mut session = MockThrusshSession::default();
        let mut state = ConnectionState::mock();

        state.file_system().mkdirall(Path::new("/rootdir")).unwrap();

        state
            .file_system()
            .write(Path::new("a"), "hello".as_bytes().into())
            .unwrap();
        state
            .file_system()
            .write(Path::new("/rootdir/c"), "world".as_bytes().into())
            .unwrap();

        session
            .expect_data()
            .once()
            .with(always(), eq_string("hello"))
            .returning(|_, _| ());

        session
            .expect_data()
            .once()
            .with(always(), eq_string("cat: b: No such file or directory"))
            .returning(|_, _| ());

        session
            .expect_data()
            .once()
            .with(always(), eq_string("world"))
            .returning(|_, _| ());

        let out = Cat::new(
            &mut state,
            ["a".to_string(), "b".to_string(), "/rootdir/c".to_string()].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::Exit(1)), "{out:?}");
    }

    #[tokio::test]
    async fn file_args() {
        let mut session = MockThrusshSession::default();
        let mut state = ConnectionState::mock();

        state
            .file_system()
            .write(Path::new("a"), "hello".as_bytes().into())
            .unwrap();
        state
            .file_system()
            .write(Path::new("b"), "world".as_bytes().into())
            .unwrap();

        session
            .expect_data()
            .once()
            .with(always(), eq_string("hello"))
            .returning(|_, _| ());

        session
            .expect_data()
            .once()
            .with(always(), eq_string("world"))
            .returning(|_, _| ());

        let out = Cat::new(
            &mut state,
            ["a".to_string(), "b".to_string()].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await;

        assert!(matches!(out, CommandResult::Exit(0)), "{out:?}");
    }

    #[tokio::test]
    async fn stdin() {
        let mut session = MockThrusshSession::default();
        let mut state = ConnectionState::mock();

        state
            .file_system()
            .write(Path::new("a"), "hello".as_bytes().into())
            .unwrap();

        state
            .file_system()
            .write(Path::new("b"), "world".as_bytes().into())
            .unwrap();

        session
            .expect_data()
            .once()
            .with(always(), eq_string("hello"))
            .returning(|_, _| ());

        session
            .expect_data()
            .once()
            .with(always(), eq_string("the whole"))
            .returning(|_, _| ());

        session
            .expect_data()
            .once()
            .with(always(), eq_string("world"))
            .returning(|_, _| ());

        let out = Cat::new(
            &mut state,
            ["a".to_string(), "-".to_string(), "b".to_string()].as_slice(),
            fake_channel_id(),
            &mut session,
        )
        .await
        .unwrap_stdin();

        let out = out
            .stdin(
                &mut state,
                fake_channel_id(),
                "the whole".as_bytes(),
                &mut session,
            )
            .await;

        assert!(matches!(out, CommandResult::Exit(0)), "{out:?}");
    }
}
