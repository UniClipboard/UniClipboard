use crate::db::models::snapshot_representation::{
    NewSnapshotRepresentationRow, SnapshotRepresentationRow,
};
use crate::db::ports::{InsertMapper, RowMapper};
use anyhow::Result;
use uc_core::{
    clipboard::{PayloadAvailability, PersistedClipboardRepresentation},
    ids::{EventId, FormatId, RepresentationId},
    BlobId, MimeType,
};

pub struct RepresentationRowMapper;

impl InsertMapper<(PersistedClipboardRepresentation, EventId), NewSnapshotRepresentationRow>
    for RepresentationRowMapper
{
    fn to_row(
        &self,
        domain: &(PersistedClipboardRepresentation, EventId),
    ) -> Result<NewSnapshotRepresentationRow> {
        let (rep, event_id) = domain;
        Ok(NewSnapshotRepresentationRow {
            id: rep.id.to_string(),
            event_id: event_id.to_string(),
            format_id: rep.format_id.to_string(),
            mime_type: rep.mime_type.as_ref().map(|m| m.to_string()),
            size_bytes: rep.size_bytes,
            inline_data: rep.inline_data.clone(),
            blob_id: rep.blob_id.as_ref().map(|id| id.to_string()),
            payload_state: rep.payload_state.as_str().to_string(),
            last_error: match &rep.payload_state {
                PayloadAvailability::Failed { last_error } => Some(last_error.clone()),
                _ => rep.last_error.clone(),
            },
        })
    }
}

// Blanket implementation for references: if we can map from owned values,
// we can also map from references by dereferencing
impl<'a>
    InsertMapper<(&'a PersistedClipboardRepresentation, &'a EventId), NewSnapshotRepresentationRow>
    for RepresentationRowMapper
where
    Self: InsertMapper<(PersistedClipboardRepresentation, EventId), NewSnapshotRepresentationRow>,
{
    fn to_row(
        &self,
        domain: &(&'a PersistedClipboardRepresentation, &'a EventId),
    ) -> Result<NewSnapshotRepresentationRow> {
        let (rep, event_id) = domain;
        // Convert references to owned values for the owned implementation
        let owned_domain = ((**rep).clone(), (**event_id).clone());
        <Self as InsertMapper<
            (PersistedClipboardRepresentation, EventId),
            NewSnapshotRepresentationRow,
        >>::to_row(self, &owned_domain)
    }
}

impl RowMapper<SnapshotRepresentationRow, uc_core::clipboard::PersistedClipboardRepresentation>
    for RepresentationRowMapper
{
    fn to_domain(
        &self,
        row: &SnapshotRepresentationRow,
    ) -> Result<uc_core::clipboard::PersistedClipboardRepresentation> {
        let payload_state = parse_payload_state(row)?;
        let last_error = match &payload_state {
            PayloadAvailability::Failed { last_error } => Some(last_error.clone()),
            _ => row.last_error.clone(),
        };

        uc_core::clipboard::PersistedClipboardRepresentation::new_with_state(
            RepresentationId::from(row.id.clone()),
            FormatId::from(row.format_id.clone()),
            row.mime_type.as_ref().map(|s| MimeType(s.clone())),
            row.size_bytes,
            row.inline_data.clone(),
            row.blob_id.as_ref().map(|s| BlobId::from(s.clone())),
            payload_state,
            last_error,
        )
    }
}

fn parse_payload_state(row: &SnapshotRepresentationRow) -> Result<PayloadAvailability> {
    match row.payload_state.as_str() {
        "Inline" => Ok(PayloadAvailability::Inline),
        "BlobReady" => Ok(PayloadAvailability::BlobReady),
        "Staged" => Ok(PayloadAvailability::Staged),
        "Processing" => Ok(PayloadAvailability::Processing),
        "Failed" => {
            let last_error = row
                .last_error
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("payload_state Failed requires last_error"))?;
            Ok(PayloadAvailability::Failed {
                last_error: last_error.to_string(),
            })
        }
        "Lost" => Ok(PayloadAvailability::Lost),
        other => Err(anyhow::anyhow!("unknown payload_state: {}", other)),
    }
}
