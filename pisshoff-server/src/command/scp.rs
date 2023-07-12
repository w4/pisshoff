use crate::{
    command::{Arg, LongRunningCommand},
    server::Connection,
};
use async_trait::async_trait;
use bytes::{Buf, BytesMut};
use nom::{
    bytes::complete::{tag, take, take_until},
    character::complete::{digit1, u64},
    combinator::{map, map_res},
    IResult,
};
use pisshoff_types::audit::{AuditLogAction, WriteFileEvent};
use std::{path::PathBuf, str::FromStr};
use thrussh::{server::Session, ChannelId};
use tracing::warn;

const HELP: &str = "usage: scp [-346ABCOpqRrsTv] [-c cipher] [-D sftp_server_path] [-F ssh_config]
           [-i identity_file] [-J destination] [-l limit] [-o ssh_option]
           [-P port] [-S program] [-X sftp_option] source ... target\n";

const AMBIGUOUS_TARGET: &str = "scp: ambiguous target\n";

const SUCCESS: &str = "\0";

// https://web.archive.org/web/20170215184048/https://blogs.oracle.com/janp/entry/how_the_scp_protocol_works
#[derive(Debug, Clone)]
pub struct Scp {
    path: PathBuf,
    pending_data: BytesMut,
    state: State,
}

#[async_trait]
impl LongRunningCommand for Scp {
    fn new(
        params: &[String],
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<Self, &'static str> {
        let mut path = None;
        let mut transfer = false;

        for param in super::argparse(params) {
            match param {
                Arg::Short('t') => {
                    transfer = true;
                }
                Arg::Short('r' | 'v') => {
                    // this is an allowed param, do nothing
                }
                Arg::Operand(p) => {
                    path = Some(p);
                }
                _ => {
                    return Err(HELP);
                }
            }
        }

        let Some(path) = path else {
            return Err(AMBIGUOUS_TARGET);
        };

        if !transfer {
            return Err(HELP);
        }

        // signal to the client we've started listening
        session.data(channel, SUCCESS.to_string().into());

        Ok(Self {
            path: PathBuf::new().join(path),
            pending_data: BytesMut::new(),
            state: State::Waiting,
        })
    }

    async fn data(
        mut self,
        connection: &mut Connection,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Option<Self> {
        self.pending_data.extend_from_slice(data);

        let mut exit = false;
        while !self.pending_data.is_empty() && !exit {
            let next_state = match self.state {
                State::Waiting => {
                    match Receive::parse(&self.pending_data) {
                        Ok((rest, res)) => {
                            let mut state = State::Waiting;

                            match res {
                                Receive::FileCopy {
                                    length, file_name, ..
                                } => {
                                    state = State::ReceivingFile(length, self.path.join(file_name));
                                }
                                Receive::DirectoryCopy { directory_name, .. } => {
                                    self.path.push(directory_name);
                                }
                                Receive::EndDirectory => {
                                    self.path.pop();
                                }
                                Receive::AccessTime { .. } => {}
                            }

                            self.pending_data
                                .advance(self.pending_data.len() - rest.len());

                            // signal to the client we received their message and we're now listening for
                            // more data
                            session.data(channel, SUCCESS.to_string().into());

                            state
                        }
                        Err(error) => {
                            warn!(%error, "Rejecting scp modes payload");
                            return None;
                        }
                    }
                }
                State::ReceivingFile(length, path) => {
                    if self.pending_data.len() < length {
                        // keep waiting for more data...
                        exit = true;
                        State::ReceivingFile(length, path)
                    } else {
                        // we've received the whole file, lets print and start waiting again
                        let data = self.pending_data.split_to(length);

                        connection
                            .audit_log()
                            .push_action(AuditLogAction::WriteFile(WriteFileEvent {
                                path: Box::from(path.to_string_lossy().into_owned()),
                                content: data.freeze(),
                            }));

                        State::AwaitingSeparator
                    }
                }
                State::AwaitingSeparator => {
                    if self.pending_data.starts_with(&[0]) {
                        self.pending_data.advance(1);

                        // signal to the client we received their message and we're now listening for
                        // more data
                        session.data(channel, SUCCESS.to_string().into());
                    }

                    State::Waiting
                }
            };

            self.state = next_state;
        }

        Some(self)
    }
}

#[derive(Clone, Debug)]
enum State {
    Waiting,
    ReceivingFile(usize, PathBuf),
    AwaitingSeparator,
}

#[derive(Debug)]
#[allow(dead_code)]
enum Receive<'a> {
    FileCopy {
        mode: &'a str,
        length: usize,
        file_name: &'a str,
    },
    DirectoryCopy {
        mode: &'a str,
        length: u64,
        directory_name: &'a str,
    },
    EndDirectory,
    AccessTime {
        modified_time: u64,
        modified_time_micros: u64,
        access_time: u64,
        access_time_micros: u64,
    },
}

enum ReceiveType {
    FileCopy,
    DirectoryCopy,
    EndDirectory,
    AccessTime,
}

impl<'a> Receive<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Receive<'a>> {
        let (rest, typ) = nom::branch::alt((
            map(tag("C"), |_| ReceiveType::FileCopy),
            map(tag("D"), |_| ReceiveType::DirectoryCopy),
            map(tag("E"), |_| ReceiveType::EndDirectory),
            map(tag("T"), |_| ReceiveType::AccessTime),
        ))(rest)?;

        match typ {
            ReceiveType::FileCopy => {
                let (rest, mode) = map_res(take(4_usize), std::str::from_utf8)(rest)?;
                let (rest, _) = tag(" ")(rest)?;
                let (rest, length) =
                    map_res(map_res(digit1, std::str::from_utf8), usize::from_str)(rest)?;
                let (rest, _) = tag(" ")(rest)?;
                let (rest, file_name) = map_res(take_until("\n"), std::str::from_utf8)(rest)?;
                let (rest, _) = tag("\n")(rest)?;

                Ok((
                    rest,
                    Receive::FileCopy {
                        mode,
                        length,
                        file_name,
                    },
                ))
            }
            ReceiveType::DirectoryCopy => {
                let (rest, mode) = map_res(take(4_usize), std::str::from_utf8)(rest)?;
                let (rest, _) = tag(" ")(rest)?;
                let (rest, length) = u64(rest)?;
                let (rest, _) = tag(" ")(rest)?;
                let (rest, directory_name) = map_res(take_until("\n"), std::str::from_utf8)(rest)?;
                let (rest, _) = tag("\n")(rest)?;

                Ok((
                    rest,
                    Receive::DirectoryCopy {
                        mode,
                        length,
                        directory_name,
                    },
                ))
            }
            ReceiveType::EndDirectory => {
                let (rest, _) = tag("\n")(rest)?;
                Ok((rest, Receive::EndDirectory))
            }
            ReceiveType::AccessTime => {
                let (rest, modified_time) =
                    map_res(map_res(digit1, std::str::from_utf8), u64::from_str)(rest)?;
                let (rest, _) = tag(" ")(rest)?;
                let (rest, modified_time_micros) =
                    map_res(map_res(digit1, std::str::from_utf8), u64::from_str)(rest)?;
                let (rest, _) = tag(" ")(rest)?;
                let (rest, access_time) =
                    map_res(map_res(digit1, std::str::from_utf8), u64::from_str)(rest)?;
                let (rest, _) = tag(" ")(rest)?;
                let (rest, access_time_micros) =
                    map_res(map_res(digit1, std::str::from_utf8), u64::from_str)(rest)?;
                let (rest, _) = tag("\n")(rest)?;

                Ok((
                    rest,
                    Receive::AccessTime {
                        modified_time,
                        modified_time_micros,
                        access_time,
                        access_time_micros,
                    },
                ))
            }
        }
    }
}