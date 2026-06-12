use crate::ids::id_macro::impl_id;
use serde::{Deserialize, Serialize};

/// Represents a unique identifier for a blob.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobId(String);

impl_id!(BlobId);
