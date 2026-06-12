use crate::clipboard::{ClipboardSelection, PolicyError, SystemClipboardSnapshot};

pub trait SelectRepresentationPolicyPort: Send + Sync {
    fn select(&self, snapshot: &SystemClipboardSnapshot)
        -> Result<ClipboardSelection, PolicyError>;
}
