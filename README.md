<p align="center">
    <img src="https://i.imgur.com/76FWBbY.png" width="100px">
</p>

<h1 align="center">pisshoff</h1>

A very simple SSH server using [thrussh][] that exposes mocked versions of a `bash` shell, some
commands and SSH subsystems to act as a honeypot for would-be crackers.

All actions undertaken on the connection by the client are recorded in JSON format in an audit log
file.

[thrussh]: https://crates.io/crates/thrussh

## What does the server expose?

### Commands

- echo
- exit
- ls
- pwd
- scp
- uname
- whoami

### Subsystems

- shell
- sftp

### How?

None of the commands or utilities shell out or otherwise interact with your operating system,
you can essentially consider the honeypot "airgapped". Although for all intents and purposes
it _feels_ like you're connecting to an actual server, you're actually interacting with very
simple partial reimplementations of common commands and utilities that don't do anything but
return the expected output and write to an audit log.

### Example

```
$ ssh root@127.0.0.1
bash-5.1$ pwd
/root
bash-5.1$ echo test
test
bash-5.1$ uname -a
Linux cd5079c0d642 5.15.49 #1 SMP PREEMPT Tue Sep 13 07:51:32 UTC 2022 x86_64 GNU/Linux
bash-5.1$ whoami
root
bash-5.1$ exit
$ echo test > test
$ scp test root@127.0.0.1:test
(root@127.0.0.1) Password:
test                                                                                                      100%    5     0.1KB/s   00:00
```

```json
$ cat audit.log | tail -n 2 | jq
{
  "connection_id": "464d87c9-e8fc-4d24-ab6f-34ee67b094f5",
  "ts": "2023-08-10T20:46:09.837165036Z",
  "peer_address": "127.0.0.1:31732",
  "host": "my-cool-honeypot.dev",
  "environment_variables": [
    ["LC_TERMINAL_VERSION", "4.5.20"],
    ["LANG", "en_GB.UTF-8"],
    ["LC_TERMINAL", "iTerm2"]
  ],
  "events": [
    {
      "start_offset": {
        "secs": 1,
        "nanos": 362803172
      },
      "action": {
        "type": "login-attempt",
        "credential-type": "public-key",
        "kind": "ssh-ed25519",
        "fingerprint": "AAAAC3NzaC1lZDI1NTE5AAAAIK3kwN10QmXsnt7jlZ7mYWXdwjfBmgK3fIp5rji"
      }
    },
    {
      "start_offset": {
        "secs": 7,
        "nanos": 85973767
      },
      "action": {
        "type": "login-attempt",
        "credential-type": "username-password",
        "username": "root",
        "password": "root"
      }
    },
    {
      "start_offset": {
        "secs": 7,
        "nanos": 190169895
      },
      "action": {
        "type": "shell-requested"
      }
    },
    {
      "start_offset": {
        "secs": 11,
        "nanos": 153124524
      },
      "action": {
        "type": "exec-command",
        "args": ["pwd"]
      }
    },
    {
      "start_offset": {
        "secs": 14,
        "nanos": 342192712
      },
      "action": {
        "type": "exec-command",
        "args": ["echo", "test"]
      }
    },
    {
      "start_offset": {
        "secs": 63,
        "nanos": 599852779
      },
      "action": {
        "type": "exec-command",
        "args": ["uname", "-a"]
      }
    },
    {
      "start_offset": {
        "secs": 67,
        "nanos": 368327325
      },
      "action": {
        "type": "exec-command",
        "args": ["whoami"]
      }
    },
    {
      "start_offset": {
        "secs": 166,
        "nanos": 208707438
      },
      "action": {
        "type": "exec-command",
        "args": ["exit"]
      }
    }
  ]
}
{
  "...": "...",
  "events": [
    "...",
    {
      "start_offset": {
        "secs": 4,
        "nanos": 196898172
      },
      "action": {
        "type": "subsystem-request",
        "name": "sftp"
      }
    },
    {
      "start_offset": {
        "secs": 4,
        "nanos": 404745407
      },
      "action": {
        "type": "write-file",
        "path": "test",
        "content": [116, 101, 115, 116, 10] // test
      }
    }
  ]
}
```

## Running the server

An [example configuration][] is provided within the repository, running the server is as simple
as building the binary using [`cargo build --release`][] and calling `./pisshoff-server -c config.toml`.

[example configuration]: https://github.com/w4/pisshoff/blob/master/pisshoff-server/config.toml
[`cargo build --release`]: https://www.rust-lang.org/
