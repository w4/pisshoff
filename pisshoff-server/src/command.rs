mod echo;
mod exit;
mod ls;
mod pwd;
mod scp;
mod uname;
mod whoami;

use crate::server::Connection;
use async_trait::async_trait;
use itertools::Either;
use thrussh::{server::Session, ChannelId};

pub enum CommandResult<T> {
    ReadStdin(T),
    Exit(u32),
    Close(u32),
}

impl<T> CommandResult<T> {
    fn map<N>(self, f: fn(T) -> N) -> CommandResult<N> {
        match self {
            Self::ReadStdin(val) => CommandResult::ReadStdin(f(val)),
            Self::Exit(v) => CommandResult::Exit(v),
            Self::Close(v) => CommandResult::Close(v),
        }
    }
}

#[async_trait]
pub trait Command: Sized {
    async fn new(
        connection: &mut Connection,
        params: &[String],
        channel: ChannelId,
        session: &mut Session,
    ) -> CommandResult<Self>;

    async fn stdin(
        self,
        connection: &mut Connection,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> CommandResult<Self>;
}

macro_rules! define_commands {
    ($($name:ident($ty:ty) = $command:expr),*) => {
        #[derive(Debug, Clone)]
        pub enum ConcreteCommand {
            $($name($ty)),*
        }

        impl ConcreteCommand {
            pub async fn new(
                connection: &mut Connection,
                params: &[String],
                channel: ChannelId,
                session: &mut Session,
            ) -> CommandResult<Self> {
                let Some(command) = params.get(0) else {
                    return CommandResult::Exit(0);
                };

                match command.as_str() {
                    $($command => <$ty as Command>::new(connection, &params[1..], channel, session).await.map(Self::$name),)*
                    other => {
                        // TODO: fix stderr displaying out of order
                        session.data(
                            channel,
                            format!("bash: {other}: command not found\n").into(),
                        );
                        CommandResult::Exit(1)
                    }
                }
            }

            pub async fn stdin(
                self,
                connection: &mut Connection,
                channel: ChannelId,
                data: &[u8],
                session: &mut Session,
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
    Echo(echo::Echo) = "echo",
    Exit(exit::Exit) = "exit",
    Ls(ls::Ls) = "ls",
    Pwd(pwd::Pwd) = "pwd",
    Scp(scp::Scp) = "scp",
    Uname(uname::Uname) = "uname",
    Whoami(whoami::Whoami) = "whoami"
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
