use crate::server::ConnectionState;
use async_trait::async_trait;
use thrussh::server::Session;
use thrussh::ChannelId;

pub mod sftp;
pub mod shell;

#[async_trait]
pub trait Subsystem {
    const NAME: &'static str;

    async fn data(
        &mut self,
        connection: &mut ConnectionState,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    );
}
