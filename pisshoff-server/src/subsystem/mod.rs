use async_trait::async_trait;
use pisshoff_types::audit::AuditLog;
use thrussh::server::Session;
use thrussh::ChannelId;

pub mod sftp;

#[async_trait]
pub trait Subsystem {
    const NAME: &'static str;

    async fn data(
        &mut self,
        audit_log: &mut AuditLog,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    );
}
