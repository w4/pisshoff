use crate::command::Arg;
use bitflags::bitflags;

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

pub fn execute(params: &[String]) -> String {
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
            Arg::Long("help") => return HELP_STRING.to_string(),
            Arg::Long("version") => return VERSION_STRING.to_string(),
            Arg::Operand(operand) => {
                return format!(
                    "uname: extra operand '{operand}'\nTry 'uname --help' for more information.\n"
                );
            }
            Arg::Short(s) => {
                return format!(
                    "uname: invalid option -- '{s}'\nTry 'uname --help' for more information.\n"
                );
            }
            Arg::Long(s) => return format!(
                "uname: unrecognized option '--{s}'\nTry 'uname --help' for more information.\n"
            ),
        };
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

    out
}

#[cfg(test)]
mod test {
    use crate::command::uname::execute;
    use test_case::test_case;

    #[test_case("-a"; "all")]
    #[test_case("-snrvmpio"; "all separate")]
    #[test_case("-asnrvmpio"; "all separate with all")]
    #[test_case("-sn"; "subset")]
    #[test_case("-sn --fake"; "unknown long arg param")]
    #[test_case("-sn -z"; "unknown short arg param")]
    #[test_case("-sn oper"; "unknown operand")]
    fn snapshot(input: &str) {
        let input_parsed = shlex::split(input).unwrap();
        let output = execute(&input_parsed);

        insta::assert_display_snapshot!(input, output);
    }
}
