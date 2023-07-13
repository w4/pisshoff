use crate::{server::ConnectionState, subsystem::Subsystem};
use async_trait::async_trait;
use bytes::Bytes;
use nom::{
    bytes::complete::take,
    combinator::{map_res, opt},
    error::ErrorKind,
    number::complete::{be_u32, be_u64, be_u8},
    IResult,
};
use pisshoff_types::audit::{AuditLogAction, MkdirEvent, WriteFileEvent};
use std::{collections::HashMap, io::Write, mem::size_of, str::FromStr};
use strum::FromRepr;
use thrussh::{server::Session, ChannelId};
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

// https://datatracker.ietf.org/doc/html/draft-ietf-secsh-filexfer-13
#[derive(Default, Clone, Debug)]
pub struct Sftp {
    open_files: HashMap<Uuid, String>,
    pending_data: bytes::BytesMut,
}

#[async_trait]
impl Subsystem for Sftp {
    const NAME: &'static str = "sftp";

    #[allow(clippy::too_many_lines)]
    async fn data(
        &mut self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) {
        self.pending_data.extend_from_slice(data);

        loop {
            let data = self.pending_data.split();

            let packet = match WirePacket::parse(&data) {
                Ok((rest, packet)) => {
                    self.pending_data.extend_from_slice(rest);
                    packet
                }
                Err(e) if e.is_incomplete() => {
                    self.pending_data.unsplit(data);
                    break;
                }
                Err(nom::Err::Error(nom::error::Error {
                    code: ErrorKind::Eof,
                    ..
                })) => {
                    self.pending_data.unsplit(data);
                    break;
                }
                Err(e) => {
                    error!("Bad SFTP packet {e:?}");
                    break;
                }
            };

            match packet.typ {
                PacketType::Init => {
                    // the version the client sent us is in `request_id`, lets just echo it back
                    // to them, bounded by the version of the rfc we developed this barebones
                    // implementation against
                    session.data(
                        channel,
                        WirePacket::new(PacketType::Version, packet.request_id.min(6), &[])
                            .to_bytes()
                            .into(),
                    );
                }
                PacketType::Stat | PacketType::Lstat => {
                    let (_data, stat) = StatPacket::parse(packet.data).unwrap();

                    trace!("SFTP stat packet: {stat:?}");

                    session.data(
                        channel,
                        StatusResponse {
                            code: StatusCode::NoSuchFile,
                            message: "No such file or directory",
                        }
                        .to_packet(packet.request_id)
                        .into(),
                    );
                }
                PacketType::Open => {
                    let (_data, open) = OpenPacket::parse(packet.data).unwrap();

                    trace!("SFTP open packet: {open:?}");

                    let uuid = Uuid::new_v4();
                    self.open_files.insert(uuid, open.path.to_string());

                    session.data(
                        channel,
                        HandleResponse(uuid).to_packet(packet.request_id).into(),
                    );
                }
                PacketType::FSetStat | PacketType::SetStat => {
                    let (_data, set_stat) = FSetStatPacket::parse(packet.data).unwrap();

                    trace!("SFTP fsetstat packet: {set_stat:?}");

                    session.data(
                        channel,
                        StatusResponse {
                            code: StatusCode::Ok,
                            message: "",
                        }
                        .to_packet(packet.request_id)
                        .into(),
                    );
                }
                PacketType::Write => {
                    let (_data, write_packet) = WritePacket::parse(packet.data).unwrap();

                    let path = self
                        .open_files
                        .get(&Uuid::from_str(write_packet.handle).unwrap())
                        .unwrap();

                    debug!(
                        "Received write for {path} at offset {}: {:?}",
                        write_packet.offset, write_packet.data
                    );

                    connection
                        .audit_log()
                        .push_action(AuditLogAction::WriteFile(WriteFileEvent {
                            path: path.to_string().into_boxed_str(),
                            content: Bytes::copy_from_slice(write_packet.data.as_bytes()),
                        }));

                    session.data(
                        channel,
                        StatusResponse {
                            code: StatusCode::Ok,
                            message: "",
                        }
                        .to_packet(packet.request_id)
                        .into(),
                    );
                }
                PacketType::Close => {
                    let (_data, close_packet) = ClosePacket::parse(packet.data).unwrap();

                    trace!("SFTP close packet: {close_packet:?}");

                    self.open_files
                        .remove(&Uuid::from_str(close_packet.handle).unwrap())
                        .unwrap();

                    session.data(
                        channel,
                        StatusResponse {
                            code: StatusCode::Ok,
                            message: "",
                        }
                        .to_packet(packet.request_id)
                        .into(),
                    );
                }
                PacketType::RealPath => {
                    let (_data, real_path) = RealPathPacket::parse(packet.data).unwrap();

                    trace!("SFTP realpath packet: {real_path:?}");

                    #[allow(clippy::wildcard_in_or_patterns)]
                    match real_path.control {
                        // SSH_FXP_REALPATH_STAT_ALWAYS
                        Some(2) => {
                            session.data(
                                channel,
                                StatusResponse {
                                    code: StatusCode::NoSuchFile,
                                    message: "No such file or directory",
                                }
                                .to_packet(packet.request_id)
                                .into(),
                            );
                        }
                        // SSH_FXP_REALPATH_NO_CHECK | SSH_FXP_REALPATH_STAT_IF
                        Some(0 | 1) | _ => {
                            session.data(
                                channel,
                                NameResponse {
                                    files: &[NameResponseFile {
                                        name: real_path.path,
                                        long_name: real_path.path,
                                        attrs: FileAttrs {
                                            typ: FileType::Unknown,
                                        },
                                    }],
                                }
                                .to_packet(packet.request_id)
                                .into(),
                            );
                        }
                    }
                }
                PacketType::Mkdir => {
                    let (_data, mkdir) = MkdirPacket::parse(packet.data).unwrap();

                    trace!("SFTP mkdir packet: {mkdir:?}");

                    connection
                        .audit_log()
                        .push_action(AuditLogAction::Mkdir(MkdirEvent {
                            path: mkdir.path.to_string().into_boxed_str(),
                        }));

                    session.data(
                        channel,
                        StatusResponse {
                            code: StatusCode::Ok,
                            message: "",
                        }
                        .to_packet(packet.request_id)
                        .into(),
                    );
                }
                _ => {
                    // TODO: return SSH_FX_OP_UNSUPPORTED
                    warn!("Unknown SFTP packet {packet:?}");
                }
            }
        }

        session.channel_success(channel);
        session.flush_pending(channel);
    }
}

fn take_length_delimited_string(rest: &[u8]) -> IResult<&[u8], &str> {
    let (rest, length) = be_u32(rest)?;
    map_res(take(length), std::str::from_utf8)(rest)
}

#[derive(Debug)]
struct MkdirPacket<'a> {
    path: &'a str,
    // TODO: fileattrs
}

impl<'a> MkdirPacket<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, path) = take_length_delimited_string(rest)?;

        Ok((rest, Self { path }))
    }
}

#[derive(Debug)]
struct RealPathPacket<'a> {
    path: &'a str,
    control: Option<u8>,
}

impl<'a> RealPathPacket<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, path) = take_length_delimited_string(rest)?;
        let (rest, control) = opt(be_u8)(rest)?;

        Ok((rest, Self { path, control }))
    }
}

#[derive(Debug)]
struct WritePacket<'a> {
    handle: &'a str,
    offset: u64,
    data: &'a str,
}

impl<'a> WritePacket<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, handle) = take_length_delimited_string(rest)?;
        let (rest, offset) = be_u64(rest)?;
        let (rest, data) = take_length_delimited_string(rest)?;

        Ok((
            rest,
            Self {
                handle,
                offset,
                data,
            },
        ))
    }
}

#[derive(Debug)]
struct ClosePacket<'a> {
    handle: &'a str,
}

impl<'a> ClosePacket<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, handle) = take_length_delimited_string(rest)?;

        Ok((rest, Self { handle }))
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct OpenPacket<'a> {
    path: &'a str,
    desired_access: u32,
    flags: u32,
}

impl<'a> OpenPacket<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, path) = take_length_delimited_string(rest)?;
        let (rest, desired_access) = be_u32(rest)?;
        let (rest, flags) = be_u32(rest)?;

        Ok((
            rest,
            Self {
                path,
                desired_access,
                flags,
            },
        ))
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct FSetStatPacket<'a> {
    handle: &'a str,
}

impl<'a> FSetStatPacket<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, handle) = take_length_delimited_string(rest)?;

        Ok((rest, Self { handle }))
    }
}

#[derive(Debug)]
#[allow(dead_code)]
struct StatPacket<'a> {
    path: &'a str,
    flags: u32,
}

impl<'a> StatPacket<'a> {
    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, path) = take_length_delimited_string(rest)?;
        let (rest, flags) = opt(be_u32)(rest)?;

        Ok((
            rest,
            Self {
                path,
                flags: flags.unwrap_or(0),
            },
        ))
    }
}

#[derive(Debug)]
struct WirePacket<'a> {
    length: u32,
    typ: PacketType,
    request_id: u32,
    data: &'a [u8],
}

impl<'a> WirePacket<'a> {
    fn new(typ: PacketType, request_id: u32, data: &'a [u8]) -> Self {
        Self {
            length: u32::try_from(size_of::<u8>() + size_of::<u32>() + data.len()).unwrap(),
            typ,
            request_id,
            data,
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            size_of::<u32>() + size_of::<u8>() + size_of::<u32>() + self.data.len(),
        );
        out.extend_from_slice(&self.length.to_be_bytes());
        out.push(self.typ as u8);
        out.extend_from_slice(&self.request_id.to_be_bytes());
        out.extend_from_slice(self.data);
        out
    }

    fn parse(rest: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (rest, length) = be_u32(rest)?;
        let (rest, typ) = be_u8(rest)?;
        let (rest, request_id) = be_u32(rest)?;
        let (rest, data) = take(
            length - u32::try_from(size_of::<u8>() + size_of::<u32>()).unwrap_or(u32::MAX),
        )(rest)?;

        let Some(typ) = PacketType::from_repr(typ) else {
           return Err(nom::Err::Failure(nom::error::Error::new(rest, nom::error::ErrorKind::Verify)));
        };

        Ok((
            rest,
            Self {
                length,
                typ,
                request_id,
                data,
            },
        ))
    }
}

#[derive(Copy, Clone, Debug, FromRepr)]
#[repr(u8)]
pub enum PacketType {
    Init = 1,
    Version = 2,
    Open = 3,
    Close = 4,
    Read = 5,
    Write = 6,
    Lstat = 7,
    Fstat = 8,
    SetStat = 9,
    FSetStat = 10,
    OpenDir = 11,
    ReadDir = 12,
    Remove = 13,
    Mkdir = 14,
    Rmdir = 15,
    RealPath = 16,
    Stat = 17,
    Rename = 18,
    ReadLink = 19,
    Link = 21,
    Block = 22,
    Unblock = 23,
    Status = 101,
    Handle = 102,
    Data = 103,
    Name = 104,
    Attrs = 105,
    Extended = 200,
    ExtendedReply = 201,
}

pub struct StatusResponse<'a> {
    code: StatusCode,
    message: &'a str,
    // language_tag: &'a str,
}

impl Response for StatusResponse<'_> {
    const TYPE: PacketType = PacketType::Status;

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(size_of::<u32>() + size_of::<u32>() + self.message.len());
        out.extend_from_slice(&(self.code as u32).to_be_bytes());
        out.extend_from_slice(
            &u32::try_from(self.message.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(self.message.as_bytes());
        out
    }
}

pub struct HandleResponse(Uuid);

impl Response for HandleResponse {
    const TYPE: PacketType = PacketType::Handle;

    fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(size_of::<u32>() + 36);
        out.extend_from_slice(&36_u32.to_be_bytes());
        write!(out, "{}", self.0).unwrap();
        out
    }
}

pub struct NameResponse<'a> {
    files: &'a [NameResponseFile<'a>],
}

impl Response for NameResponse<'_> {
    const TYPE: PacketType = PacketType::Name;

    fn to_bytes(&self) -> Vec<u8> {
        // TODO: include nameresponsefile size
        let mut out = Vec::with_capacity(size_of::<u32>());
        out.extend_from_slice(
            &u32::try_from(self.files.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );

        for file in self.files {
            out.extend_from_slice(&file.to_bytes());
        }

        out.push(1);

        out
    }
}

pub struct NameResponseFile<'a> {
    name: &'a str,
    long_name: &'a str,
    attrs: FileAttrs,
}

impl NameResponseFile<'_> {
    fn to_bytes(&self) -> Vec<u8> {
        // TODO: include FileAttrs size
        let mut out = Vec::with_capacity(
            size_of::<u32>() + self.name.len() + size_of::<u32>() + self.long_name.len(),
        );
        out.extend_from_slice(
            &u32::try_from(self.name.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(self.name.as_bytes());
        out.extend_from_slice(
            &u32::try_from(self.long_name.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        out.extend_from_slice(self.long_name.as_bytes());
        out.extend_from_slice(&self.attrs.to_bytes());
        out
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(u8)]
#[allow(dead_code)]
enum FileType {
    Regular = 1,
    Directory = 2,
    Symlink = 3,
    Special = 4,
    Unknown = 5,
    Socket = 6,
    CharDevice = 7,
    BlockDevice = 8,
    Fifo = 9,
}

#[derive(Copy, Clone, Debug)]
struct FileAttrs {
    typ: FileType,
}

impl FileAttrs {
    fn to_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(size_of::<u32>() + size_of::<u8>());
        out.extend_from_slice(&0_u32.to_be_bytes());
        out.push(self.typ as u8);
        out
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(u32)]
#[allow(dead_code)]
enum StatusCode {
    Ok = 0,
    Eof = 1,
    NoSuchFile = 2,
    PermissionDenied = 3,
    Failure = 4,
    BadMessage = 5,
    NoConnection = 6,
    ConnectionLost = 7,
    OpUnsupported = 8,
    InvalidHandle = 9,
    NoSuchPath = 10,
    FileAlreadyExists = 11,
    WriteProtect = 12,
    NoMedia = 13,
    NoSpaceOnFilesystem = 14,
    QuotaExceeded = 15,
    UnknownPrincipal = 16,
    LockConflict = 17,
    DirNotEmpty = 18,
    NotADirectory = 19,
    InvalidFilename = 20,
    LinkLoop = 21,
    CannotDelete = 22,
    InvalidParameter = 23,
    FileIsADirectory = 24,
    ByteRangeLockConflict = 25,
    ByteRangeLockRefused = 26,
    DeletePending = 27,
    FileCorrupt = 28,
    OwnerInvalid = 29,
    GroupInvalid = 30,
    NoMatchingByteRangeLock = 31,
}

trait Response {
    const TYPE: PacketType;

    fn to_bytes(&self) -> Vec<u8>;

    fn to_packet(&self, request_id: u32) -> Vec<u8> {
        WirePacket::new(Self::TYPE, request_id, &self.to_bytes()).to_bytes()
    }
}
