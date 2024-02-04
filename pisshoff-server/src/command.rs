mod echo;
mod exit;
mod ls;
mod pwd;
mod scp;
mod uname;
mod whoami;

use crate::server::{ConnectionState, ThrusshSession};
use async_trait::async_trait;
use itertools::Either;
use std::borrow::Cow;
use std::fmt::Debug;
use thrussh::ChannelId;

#[derive(Debug)]
pub enum CommandResult<T> {
    /// Wait for stdin
    ReadStdin(T),
    /// Exit process
    Exit(u32),
    /// Close session
    Close(u32),
}

impl<T: Debug> CommandResult<T> {
    fn map<N>(self, f: fn(T) -> N) -> CommandResult<N> {
        match self {
            Self::ReadStdin(val) => CommandResult::ReadStdin(f(val)),
            Self::Exit(v) => CommandResult::Exit(v),
            Self::Close(v) => CommandResult::Close(v),
        }
    }

    #[cfg(test)]
    pub fn unwrap_stdin(self) -> T {
        match self {
            Self::ReadStdin(val) => val,
            v => panic!("got {v:?}, expected ReadStdin"),
        }
    }
}

#[async_trait]
pub trait Command: Sized {
    async fn new<S: ThrusshSession + Send>(
        connection: &mut ConnectionState,
        params: &[String],
        channel: ChannelId,
        session: &mut S,
    ) -> CommandResult<Self>;

    async fn stdin<S: ThrusshSession + Send>(
        self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        data: &[u8],
        session: &mut S,
    ) -> CommandResult<Self>;
}

#[derive(PartialEq, Eq, Debug)]
pub struct PartialCommand<'a> {
    exec: Option<Cow<'a, [u8]>>,
    params: Vec<Cow<'a, [u8]>>,
}

impl<'a> PartialCommand<'a> {
    pub fn new(exec: Option<Cow<'a, [u8]>>, params: Vec<Cow<'a, [u8]>>) -> Self {
        Self { exec, params }
    }

    pub async fn into_concrete_command<S: ThrusshSession + Send>(
        self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        session: &mut S,
    ) -> CommandResult<ConcreteCommand> {
        // TODO: make commands take byte slices
        let args = self
            .params
            .iter()
            .map(|v| String::from_utf8_lossy(v).to_string())
            .collect::<Vec<_>>();

        ConcreteCommand::new(connection, self.exec.as_deref(), &args, channel, session).await
    }
}

macro_rules! define_commands {
    ($($name:ident($ty:ty) = $command:expr),*) => {
        #[derive(Debug, Clone)]
        pub enum ConcreteCommand {
            $($name($ty)),*
        }

        impl ConcreteCommand {
            pub async fn new<S: ThrusshSession + Send>(
                connection: &mut ConnectionState,
                exec: Option<&[u8]>,
                params: &[String],
                channel: ChannelId,
                session: &mut S,
            ) -> CommandResult<Self> {
                let Some(command) = exec else {
                    return CommandResult::Exit(0);
                };

                match command {
                    $($command => <$ty as Command>::new(connection, &params, channel, session).await.map(Self::$name),)*
                    other => {
                        // TODO: fix stderr displaying out of order
                        session.data(
                            channel,
                            format!("bash: {}: command not found\n", String::from_utf8_lossy(other)).into(),
                        );
                        CommandResult::Exit(1)
                    }
                }
            }

            pub async fn stdin<S: ThrusshSession + Send>(
                self,
                connection: &mut ConnectionState,
                channel: ChannelId,
                data: &[u8],
                session: &mut S,
            ) -> CommandResult<Self> {
                match self {
                    $(Self::$name(cmd) => {
                        cmd
                            .stdin(connection, channel, data, session)
                            .await
                            .map(Self::$name)
                    }),*
                }
            }
        }
    }
}

define_commands! {
    Echo(echo::Echo) = b"echo",
    Exit(exit::Exit) = b"exit",
    Ls(ls::Ls) = b"ls",
    Pwd(pwd::Pwd) = b"pwd",
    Scp(scp::Scp) = b"scp",
    Uname(uname::Uname) = b"uname",
    Whoami(whoami::Whoami) = b"whoami"
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Arg<'a> {
    Operand(&'a str),
    Long(&'a str),
    Short(char),
}

fn argparse(args: &[String]) -> impl Iterator<Item = Arg<'_>> {
    args.iter().flat_map(|rest| {
        if let Some(rest) = rest.strip_prefix("--") {
            Either::Left(std::iter::once(Arg::Long(rest)))
        } else if let Some(rest) = rest.strip_prefix('-').filter(|v| !v.is_empty()) {
            Either::Right(rest.chars().map(Arg::Short))
        } else {
            Either::Left(std::iter::once(Arg::Operand(rest)))
        }
    })
}

#[cfg(test)]
mod test {
    use super::Arg;
    use test_case::test_case;

    #[test_case("-a", &[Arg::Short('a')]; "single short parameter")]
    #[test_case("-abc", &[Arg::Short('a'), Arg::Short('b'), Arg::Short('c')]; "multiple short parameter")]
    #[test_case("-a --long operand -b -", &[Arg::Short('a'), Arg::Long("long"), Arg::Operand("operand"), Arg::Short('b'), Arg::Operand("-")]; "full hit")]
    fn argparse(input: &str, expected: &[Arg<'static>]) {
        let input = shlex::split(input).unwrap();
        let output = super::argparse(&input).collect::<Vec<_>>();
        assert_eq!(output, expected);
    }
}
