use crate::clipboard::ClipboardSelection;
use crate::ids::EntryId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardSelectionDecision {
    pub entry_id: EntryId,
    pub selection: ClipboardSelection,
}

impl ClipboardSelectionDecision {
    pub fn new(entry_id: EntryId, selection: ClipboardSelection) -> Self {
        Self {
            entry_id,
            selection,
        }
    }
}
