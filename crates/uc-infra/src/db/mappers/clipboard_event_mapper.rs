use crate::db::models::clipboard_event::NewClipboardEventRow;
use crate::db::ports::InsertMapper;
use anyhow::Result;
use uc_core::clipboard::ClipboardEvent;

pub struct ClipboardEventRowMapper;

impl InsertMapper<ClipboardEvent, NewClipboardEventRow> for ClipboardEventRowMapper {
    fn to_row(&self, domain: &ClipboardEvent) -> Result<NewClipboardEventRow> {
        Ok(NewClipboardEventRow {
            event_id: domain.event_id.clone().into(),
            captured_at_ms: domain.captured_at_ms,
            source_device: domain.source_device.as_str().to_string(),
            snapshot_hash: domain.snapshot_hash.to_string(),
        })
    }
}
