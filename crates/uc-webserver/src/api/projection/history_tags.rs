//! Engine history-tag summaries → transport DTOs.

use uc_engine::{
    HistoryEntryTagSummary, HistoryTagApplicationSummary, HistoryTagBatchSummary,
    HistoryTagCreatedSummary, HistoryTagDeletedSummary, HistoryTagMergeSummary,
    HistoryTagRenameSummary, HistoryTagSummary,
};

use super::IntoApiDto;
use crate::api::dto::history_tags::{
    HistoryEntryTagSummaryDto, HistoryTagApplicationDto, HistoryTagBatchResultDto,
    HistoryTagCreatedDto, HistoryTagDeletedDto, HistoryTagDto, HistoryTagMergeResultDto,
    HistoryTagRenameResultDto,
};

impl IntoApiDto<HistoryTagDto> for HistoryTagSummary {
    fn into_api_dto(self) -> HistoryTagDto {
        HistoryTagDto {
            tag_id: self.tag_id,
            name: self.name,
            created_at_ms: self.created_at_ms,
            entry_count: self.entry_count,
        }
    }
}

impl IntoApiDto<HistoryTagCreatedDto> for HistoryTagCreatedSummary {
    fn into_api_dto(self) -> HistoryTagCreatedDto {
        HistoryTagCreatedDto {
            tag: self.tag.into_api_dto(),
            created: self.created,
        }
    }
}

impl IntoApiDto<HistoryTagRenameResultDto> for HistoryTagRenameSummary {
    fn into_api_dto(self) -> HistoryTagRenameResultDto {
        match self {
            Self::Renamed(tag) => HistoryTagRenameResultDto::Renamed {
                tag: tag.into_api_dto(),
            },
            Self::NameConflict { existing_tag_id } => {
                HistoryTagRenameResultDto::NameConflict { existing_tag_id }
            }
        }
    }
}

impl IntoApiDto<HistoryTagBatchResultDto> for HistoryTagBatchSummary {
    fn into_api_dto(self) -> HistoryTagBatchResultDto {
        HistoryTagBatchResultDto {
            changed: self.changed,
            unchanged: self.unchanged,
            missing_entry_ids: self.missing_entry_ids,
        }
    }
}

impl IntoApiDto<HistoryTagApplicationDto> for HistoryTagApplicationSummary {
    fn into_api_dto(self) -> HistoryTagApplicationDto {
        HistoryTagApplicationDto {
            tag_id: self.tag_id,
            applied: self.applied,
        }
    }
}

impl IntoApiDto<HistoryEntryTagSummaryDto> for HistoryEntryTagSummary {
    fn into_api_dto(self) -> HistoryEntryTagSummaryDto {
        HistoryEntryTagSummaryDto {
            selected: self.selected,
            tags: self
                .tags
                .into_iter()
                .map(IntoApiDto::into_api_dto)
                .collect(),
        }
    }
}

impl IntoApiDto<HistoryTagMergeResultDto> for HistoryTagMergeSummary {
    fn into_api_dto(self) -> HistoryTagMergeResultDto {
        HistoryTagMergeResultDto {
            moved: self.moved,
            already_on_target: self.already_on_target,
        }
    }
}

impl IntoApiDto<HistoryTagDeletedDto> for HistoryTagDeletedSummary {
    fn into_api_dto(self) -> HistoryTagDeletedDto {
        HistoryTagDeletedDto {
            detached: self.detached,
        }
    }
}
