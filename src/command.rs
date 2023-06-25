use itertools::Itertools;
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
        other => {
            // TODO: fix stderr displaying out of order
            session.data(
                channel,
                format!("bash: {other}: command not found\n").into(),
            );
        }
    }
}
