//! The preview of the selected entry, drawn in the satellite window.

use crate::ports::{PreviewData, ServiceError};

use super::{timing, Effect, Effects, PanelState};

/// Shown in place of a preview that could not be read.
const UNREADABLE: &str = "无法读取预览，请重试。";

impl PanelState {
    /// Follows the selection with the preview, after a short pause.
    pub fn schedule_preview(&mut self) -> Effects {
        let Some(item) = self.active_item() else {
            return vec![];
        };
        if self.preview.entry.as_deref() == Some(&item.entry_id) {
            return vec![];
        }
        let delay = if self.preview.expanded {
            timing::PREVIEW_DELAY_OPEN
        } else {
            timing::PREVIEW_DELAY_CLOSED
        };
        vec![Effect::SchedulePreview {
            id: item.entry_id.clone(),
            kind: item.content_type.clone(),
            delay,
        }]
    }

    pub(super) fn on_preview_due(&mut self, id: String, kind: String) -> Effects {
        if !self.session.visible {
            return vec![];
        }
        self.preview.entry = Some(id.clone());
        self.preview.text = None;
        self.preview.loading = true;
        self.preview.expanded = true;
        vec![Effect::ShowPreviewWindow, Effect::LoadPreview { id, kind }]
    }

    pub(super) fn on_preview_loaded(
        &mut self,
        id: String,
        result: Result<PreviewData, ServiceError>,
    ) -> Effects {
        if self.preview.entry.as_deref() != Some(&id) {
            return vec![];
        }
        self.preview.loading = false;
        match result {
            Ok(data) => {
                self.preview.text = data.text;
                self.preview.size = data.size;
                match data.image {
                    Some(payload) => vec![Effect::StoreImage {
                        id,
                        payload,
                        size: data.size,
                    }],
                    None => vec![],
                }
            }
            Err(_) => {
                self.preview.text = Some(UNREADABLE.into());
                vec![]
            }
        }
    }
}
