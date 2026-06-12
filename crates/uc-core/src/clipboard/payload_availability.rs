/// Explicit state machine for clipboard payload availability.
///
/// Key principle: This enum ONLY expresses state, never carries data.
/// Data carriers are inline_data and blob_id on PersistedClipboardRepresentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PayloadAvailability {
    /// Small content stored inline - inline_data=Some, blob_id=None
    Inline,

    /// Blob materialized and ready - inline_data=None, blob_id=Some
    BlobReady,

    /// Large content awaiting worker - inline_data=None, blob_id=None
    /// Data should be available in cache or spool
    Staged,

    /// Worker is processing - inline_data=None, blob_id=None
    Processing,

    /// Worker failed - inline_data=None, blob_id=None
    Failed { last_error: String },

    /// Data permanently lost - inline_data=None, blob_id=None
    Lost,
}

impl PayloadAvailability {
    /// Whether this state requires inline_data to be Some.
    pub fn requires_inline_data(&self) -> bool {
        matches!(self, PayloadAvailability::Inline)
    }

    /// Whether this state requires blob_id to be Some.
    pub fn requires_blob_id(&self) -> bool {
        matches!(self, PayloadAvailability::BlobReady)
    }

    /// String representation for persistence.
    pub fn as_str(&self) -> &'static str {
        match self {
            PayloadAvailability::Inline => "Inline",
            PayloadAvailability::BlobReady => "BlobReady",
            PayloadAvailability::Staged => "Staged",
            PayloadAvailability::Processing => "Processing",
            PayloadAvailability::Failed { .. } => "Failed",
            PayloadAvailability::Lost => "Lost",
        }
    }

    /// Whether data should be available in cache or spool.
    pub fn is_cache_or_spool_expected(&self) -> bool {
        matches!(
            self,
            PayloadAvailability::Staged | PayloadAvailability::Processing
        )
    }
}
