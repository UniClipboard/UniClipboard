use serde::{Deserialize, Serialize};

use super::id_macro::impl_id;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpaceId(String);

impl_id!(SpaceId);
