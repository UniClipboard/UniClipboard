use crate::{ids::DeviceId, ids::EventId, ObservedClipboardRepresentation};
use anyhow::Result;

#[async_trait::async_trait]
pub trait ClipboardEventRepositoryPort: Send + Sync {
    async fn get_representation(
        &self,
        id: &EventId,
        representation_id: &str,
    ) -> Result<ObservedClipboardRepresentation>;

    /// 返回该 event 的来源设备 id。`None` 表示 event 不存在;调用方应据此
    /// 把派生信息降级为"来源不可信",不得当作"本机产生"处理。
    async fn get_source_device(&self, event_id: &EventId) -> Result<Option<DeviceId>>;
}
