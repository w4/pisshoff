pub mod uname;

use itertools::{Either, Itertools};
use std::{f32, str::FromStr, time::Duration};
use thrussh::{server::Session, ChannelId};

pub async fn run_command(args: &[String], channel: ChannelId, session: &mut Session) {
    let Some(command) = args.get(0) else {
        return;
    };

    match command.as_str() {
        "echo" => {
            session.data(
                channel,
                format!("{}\n", args.iter().skip(1).join(" ")).into(),
            );
        }
        "whoami" => {
            // TODO: grab "logged in" user
            session.data(channel, "root\n".to_string().into());
        }
        "pwd" => {
            // TODO: mock FHS
            session.data(channel, "/root\n".to_string().into());
        }
        "ls" => {
            // pretend /root is empty until we mock the FHS
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
        other => {
            // TODO: fix stderr displaying out of order
            session.data(
                channel,
                format!("bash: {other}: command not found\n").into(),
            );
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
