use crate::db::models::clipboard_selection::{ClipboardSelectionRow, NewClipboardSelectionRow};
use crate::db::ports::{InsertMapper, RowMapper};
use anyhow::Result;
use uc_core::clipboard::ClipboardSelectionDecision;

pub struct ClipboardSelectionRowMapper;

impl InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>
    for ClipboardSelectionRowMapper
{
    fn to_row(&self, domain: &ClipboardSelectionDecision) -> Result<NewClipboardSelectionRow> {
        let secondary_rep_ids = domain
            .selection
            .secondary_rep_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        Ok(NewClipboardSelectionRow {
            entry_id: domain.entry_id.to_string(),
            primary_rep_id: domain.selection.primary_rep_id.to_string(),
            secondary_rep_ids,
            preview_rep_id: domain.selection.preview_rep_id.to_string(),
            paste_rep_id: domain.selection.paste_rep_id.to_string(),
            policy_version: domain.selection.policy_version.to_string(),
        })
    }
}

impl RowMapper<ClipboardSelectionRow, ClipboardSelectionDecision> for ClipboardSelectionRowMapper {
    fn to_domain(&self, row: &ClipboardSelectionRow) -> Result<ClipboardSelectionDecision> {
        use uc_core::{
            clipboard::ClipboardSelection,
            ids::{EntryId, RepresentationId},
        };

        // Parse secondary_rep_ids from comma-separated string with strict validation.
        // Empty string is valid (no secondary representations), but empty tokens are not.
        let secondary_rep_ids: Vec<RepresentationId> = if row.secondary_rep_ids.trim().is_empty() {
            Vec::new()
        } else {
            row.secondary_rep_ids
                .split(',')
                .enumerate()
                .map(|(i, s)| {
                    let trimmed = s.trim();
                    if trimmed.is_empty() {
                        Err(anyhow::anyhow!(
                            "Empty token at position {} in secondary_rep_ids for entry {}",
                            i,
                            row.entry_id
                        ))
                    } else {
                        Ok(RepresentationId::from(trimmed.to_string()))
                    }
                })
                .collect::<Result<Vec<_>>>()?
        };

        // Parse policy_version
        let policy_version = row.policy_version.parse().map_err(|_| {
            anyhow::anyhow!(
                "Invalid policy_version '{}' for entry {}",
                row.policy_version,
                row.entry_id
            )
        })?;

        let selection = ClipboardSelection {
            primary_rep_id: RepresentationId::from(row.primary_rep_id.clone()),
            secondary_rep_ids,
            preview_rep_id: RepresentationId::from(row.preview_rep_id.clone()),
            paste_rep_id: RepresentationId::from(row.paste_rep_id.clone()),
            policy_version,
        };

        Ok(ClipboardSelectionDecision::new(
            EntryId::from(row.entry_id.clone()),
            selection,
        ))
    }
}
