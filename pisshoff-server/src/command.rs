use itertools::Itertools;
use std::{borrow::Cow, f32, str::FromStr, time::Duration};
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
            // todo: move this out to its own module
            let out: Cow<'static, str> = match args.get(1).map(String::as_str) {
                None | Some("-s" | "--kernel-name") => "Linux\n".into(),
                Some("-a" | "--all") => "Linux cd5079c0d642 5.15.49 #1 SMP PREEMPT Tue Sep 13 07:51:32 UTC 2022 x86_64 x86_64 x86_64 GNU/Linux\n".into(),
                Some("-n" | "--nodename") => "cd5079c0d642\n".into(),
                Some("-r" | "--kernel-release") => "5.15.49\n".into(),
                Some("-v" | "--kernel-version") => "#1 SMP PREEMPT Tue Sep 13 07:51:32 UTC 2022\n".into(),
                Some("-m" | "--machine" | "-p" | "--processor" | "-i" | "--hardware-platform") => "x86_64\n".into(),
                Some("-o" | "--operating-system") => "GNU/Linux\n".into(),
                Some("--version") => "uname (GNU coreutils) 8.32
Copyright (C) 2020 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by David MacKenzie.\n".into(),
                Some("--help") => "Usage: uname [OPTION]...
Print certain system information.  With no OPTION, same as -s.

  -a, --all                print all information, in the following order,
                             except omit -p and -i if unknown:
  -s, --kernel-name        print the kernel name
  -n, --nodename           print the network node hostname
  -r, --kernel-release     print the kernel release
  -v, --kernel-version     print the kernel version
  -m, --machine            print the machine hardware name
  -p, --processor          print the processor type (non-portable)
  -i, --hardware-platform  print the hardware platform (non-portable)
  -o, --operating-system   print the operating system
      --help     display this help and exit
      --version  output version information and exit

GNU coreutils online help: <https://www.gnu.org/software/coreutils/>
Report any translation bugs to <https://translationproject.org/team/>
Full documentation <https://www.gnu.org/software/coreutils/uname>
or available locally via: info '(coreutils) uname invocation'\n".into(),
                Some("-") => "uname: extra operand '-'\nTry 'uname --help' for more information.\n".into(),
                Some(v) => format!(
                    "uname: invalid option -- '{}'\nTry 'uname --help' for more information.\n",
                    if v.starts_with("-") && !v.starts_with("--") {
                        &v[1..]
                    } else {
                        v
                    }
                ).into(),
            };

            session.data(channel, out.into_owned().into());
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
