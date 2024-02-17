use async_trait::async_trait;
use bitflags::bitflags;
use thrussh::ChannelId;

use crate::{
    command::{Arg, Command, CommandResult},
    server::{ConnectionState, ThrusshSession},
};

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    struct ToPrint: u8 {
        const KERNEL_NAME      = 0b0000_0001;
        const NODE_NAME        = 0b0000_0010;
        const KERNEL_RELEASE   = 0b0000_0100;
        const KERNEL_VERSION   = 0b0000_1000;
        const MACHINE          = 0b0001_0000;
        const PROCESSOR        = 0b0010_0000;
        const PLATFORM         = 0b0100_0000;
        const OPERATING_SYSTEM = 0b1000_0000;
    }
}

const VERSION_STRING: &str = "uname (GNU coreutils) 8.32
Copyright (C) 2020 Free Software Foundation, Inc.
License GPLv3+: GNU GPL version 3 or later <https://gnu.org/licenses/gpl.html>.
This is free software: you are free to change and redistribute it.
There is NO WARRANTY, to the extent permitted by law.

Written by David MacKenzie.
";

pub const HELP_STRING: &str = "Usage: uname [OPTION]...
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
or available locally via: info '(coreutils) uname invocation'
";

#[derive(Debug, Clone)]
pub struct Uname {}

#[async_trait]
impl Command for Uname {
    async fn new<S: ThrusshSession + Send>(
        _connection: &mut ConnectionState,
        params: &[String],
        channel: ChannelId,
        session: &mut S,
    ) -> CommandResult<Self> {
        let (out, exit_code) = execute(params);

        session.data(channel, out.into());
        CommandResult::Exit(exit_code)
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

pub fn execute(params: &[String]) -> (String, u32) {
    let mut to_print = ToPrint::empty();
    let mut filter_unknown = false;

    for param in super::argparse(params) {
        to_print |= match param {
            Arg::Short('a') | Arg::Long("all") => {
                filter_unknown = true;
                ToPrint::all()
            }
            Arg::Short('s') | Arg::Long("kernel-name") => ToPrint::KERNEL_NAME,
            Arg::Short('n') | Arg::Long("nodename") => ToPrint::NODE_NAME,
            Arg::Short('r') | Arg::Long("kernel-release") => ToPrint::KERNEL_RELEASE,
            Arg::Short('v') | Arg::Long("kernel-version") => ToPrint::KERNEL_VERSION,
            Arg::Short('m') | Arg::Long("machine") => ToPrint::MACHINE,
            Arg::Short('p') | Arg::Long("processor") => ToPrint::PROCESSOR,
            Arg::Short('i') | Arg::Long("hardware-platform") => ToPrint::PLATFORM,
            Arg::Short('o') | Arg::Long("operating-system") => ToPrint::OPERATING_SYSTEM,
            Arg::Long("help") => return (HELP_STRING.to_string(), 0),
            Arg::Long("version") => return (VERSION_STRING.to_string(), 0),
            Arg::Operand(operand) => {
                return (
                    format!(
                    "uname: extra operand '{operand}'\nTry 'uname --help' for more information.\n"
                ),
                    1,
                );
            }
            Arg::Short(s) => {
                return (
                    format!(
                    "uname: invalid option -- '{s}'\nTry 'uname --help' for more information.\n"
                ),
                    1,
                );
            }
            Arg::Long(s) => {
                return (
                    format!(
                    "uname: unrecognized option '--{s}'\nTry 'uname --help' for more information.\n"
                ),
                    1,
                );
            }
        };
    }

    if to_print.is_empty() {
        to_print |= ToPrint::KERNEL_NAME;
    }

    let mut out = String::with_capacity(105);

    macro_rules! write {
        ($v:expr) => {
            if !out.is_empty() {
                out.push(' ');
            }

            out.push_str($v);
        };
    }

    if to_print.contains(ToPrint::KERNEL_NAME) {
        write!("Linux");
    }

    if to_print.contains(ToPrint::NODE_NAME) {
        write!("cd5079c0d642");
    }

    if to_print.contains(ToPrint::KERNEL_RELEASE) {
        write!("5.15.49");
    }

    if to_print.contains(ToPrint::KERNEL_VERSION) {
        write!("#1 SMP PREEMPT Tue Sep 13 07:51:32 UTC 2022");
    }

    if to_print.contains(ToPrint::MACHINE) {
        write!("x86_64");
    }

    if to_print.contains(ToPrint::PROCESSOR) && !filter_unknown {
        write!("unknown");
    }

    if to_print.contains(ToPrint::PLATFORM) && !filter_unknown {
        write!("unknown");
    }

    if to_print.contains(ToPrint::OPERATING_SYSTEM) {
        write!("GNU/Linux");
    }

    out.push('\n');

    (out, 0)
}

#[cfg(test)]
mod test {
    use test_case::test_case;

    use crate::command::uname::execute;

    #[test_case("", 0; "none")]
    #[test_case("-a", 0; "all")]
    #[test_case("-snrvmpio", 0; "all separate")]
    #[test_case("-asnrvmpio", 0; "all separate with all")]
    #[test_case("-sn", 0; "subset")]
    #[test_case("-sn --fake", 1; "unknown long arg param")]
    #[test_case("-sn -z", 1; "unknown short arg param")]
    #[test_case("-sn oper", 1; "unknown operand")]
    fn snapshot(input: &str, expected_exit_code: u32) {
        let input_parsed = shlex::split(input).unwrap();
        let (output, actual_exit_code) = execute(&input_parsed);

        insta::assert_display_snapshot!(input, output);
        assert_eq!(actual_exit_code, expected_exit_code);
    }
}
