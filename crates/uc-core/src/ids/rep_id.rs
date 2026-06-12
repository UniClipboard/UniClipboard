use serde::{Deserialize, Serialize};

use super::id_macro::impl_id;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepresentationId(String);

impl_id!(RepresentationId);
