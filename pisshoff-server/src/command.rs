pub mod scp;
pub mod uname;

use crate::{command::scp::Scp, server::Connection};
use async_trait::async_trait;
use itertools::{Either, Itertools};
use std::{f32, fmt::Write, str::FromStr, time::Duration};
use thrussh::{server::Session, ChannelId};

pub async fn run_command(
    args: &[String],
    channel: ChannelId,
    session: &mut Session,
    conn: &mut Connection,
) -> Option<ConcreteLongRunningCommand> {
    let Some(command) = args.get(0) else {
        return None;
    };

    match command.as_str() {
        "echo" => {
            session.data(
                channel,
                format!("{}\n", args.iter().skip(1).join(" ")).into(),
            );
        }
        "whoami" => {
            session.data(channel, format!("{}\n", conn.username()).into());
        }
        "pwd" => {
            session.data(
                channel,
                format!("{}\n", conn.file_system().pwd().display()).into(),
            );
        }
        "ls" => {
            let resp = if args.len() == 1 {
                conn.file_system().ls(None).join("  ")
            } else if args.len() == 2 {
                conn.file_system().ls(Some(args.get(1).unwrap())).join("  ")
            } else {
                let mut out = String::new();

                for dir in args.iter().skip(1) {
                    if !out.is_empty() {
                        out.push_str("\n\n");
                    }

                    write!(out, "{dir}:").unwrap();
                    out.push_str(&conn.file_system().ls(Some(dir)).join("  "));
                }

                out
            };

            if !resp.is_empty() {
                session.data(channel, format!("{resp}\n").into());
            }
        }
        "cd" => {
            if args.len() > 2 {
                session.data(
                    channel,
                    "-bash: cd: too many arguments\n".to_string().into(),
                );
                return None;
            }

            conn.file_system().cd(args.get(1).map(String::as_str));
        }
        "exit" => {
            let exit_status = args
                .get(1)
                .map(String::as_str)
                .map_or(Ok(0), u32::from_str)
                .unwrap_or(2);

            session.exit_status_request(channel, exit_status);
            session.close(channel);
        }
        "sleep" => {
            if let Some(Ok(secs)) = args.get(1).map(String::as_str).map(f32::from_str) {
                tokio::time::sleep(Duration::from_secs_f32(secs)).await;
            }
        }
        "uname" => {
            let out = uname::execute(&args[1..]);
            session.data(channel, out.into());
        }
        "scp" => match Scp::new(&args[1..], channel, session) {
            Ok(v) => return Some(ConcreteLongRunningCommand::Scp(v)),
            Err(e) => session.data(channel, e.to_string().into()),
        },
        other => {
            // TODO: fix stderr displaying out of order
            session.data(
                channel,
                format!("bash: {other}: command not found\n").into(),
            );
        }
    }

    None
}

#[async_trait]
pub trait LongRunningCommand: Sized {
    fn new(
        params: &[String],
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<Self, &'static str>;

    async fn data(
        self,
        connection: &mut Connection,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Option<Self>;
}

#[derive(Debug, Clone)]
pub enum ConcreteLongRunningCommand {
    Scp(Scp),
}

impl ConcreteLongRunningCommand {
    pub async fn data(
        self,
        connection: &mut Connection,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Option<Self> {
        match self {
            Self::Scp(cmd) => cmd
                .data(connection, channel, data, session)
                .await
                .map(Self::Scp),
        }
    }
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
