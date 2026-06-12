use crate::db::models::clipboard_representation_thumbnail::{
    ClipboardRepresentationThumbnailRow, NewClipboardRepresentationThumbnailRow,
};
use crate::db::ports::{InsertMapper, RowMapper};
use anyhow::Result;
use std::str::FromStr;
use uc_core::clipboard::{ThumbnailMetadata, TimestampMs};
use uc_core::ids::RepresentationId;
use uc_core::{BlobId, MimeType};

pub struct ThumbnailRowMapper;

impl InsertMapper<ThumbnailMetadata, NewClipboardRepresentationThumbnailRow>
    for ThumbnailRowMapper
{
    fn to_row(&self, domain: &ThumbnailMetadata) -> Result<NewClipboardRepresentationThumbnailRow> {
        Ok(NewClipboardRepresentationThumbnailRow {
            representation_id: domain.representation_id.to_string(),
            thumbnail_blob_id: domain.thumbnail_blob_id.to_string(),
            thumbnail_mime_type: domain.thumbnail_mime_type.to_string(),
            original_width: domain.original_width,
            original_height: domain.original_height,
            original_size_bytes: domain.original_size_bytes,
            created_at_ms: domain.created_at_ms.map(|ts| ts.as_millis()),
        })
    }
}

impl RowMapper<ClipboardRepresentationThumbnailRow, ThumbnailMetadata> for ThumbnailRowMapper {
    fn to_domain(&self, row: &ClipboardRepresentationThumbnailRow) -> Result<ThumbnailMetadata> {
        let representation_id = RepresentationId::from(row.representation_id.clone());
        let thumbnail_blob_id = BlobId::from(row.thumbnail_blob_id.clone());
        let thumbnail_mime_type = MimeType::from_str(&row.thumbnail_mime_type).map_err(|e| {
            anyhow::anyhow!(
                "invalid thumbnail mime type '{}': {}",
                row.thumbnail_mime_type,
                e
            )
        })?;
        let created_at_ms = row.created_at_ms.map(TimestampMs::from_epoch_millis);

        Ok(ThumbnailMetadata::new(
            representation_id,
            thumbnail_blob_id,
            thumbnail_mime_type,
            row.original_width,
            row.original_height,
            row.original_size_bytes,
            created_at_ms,
        ))
    }
}
