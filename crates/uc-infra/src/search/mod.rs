//! `uc-infra::search` — persistence foundation for local encrypted search.
//!
//! This module owns:
//! - `constants`: authoritative `CURRENT_INDEX_VERSION` and field-mask bit positions.
//! - `rows`: adapter-owned Diesel row types with `profile_id` and domain conversion helpers.
//!
//! Profile scoping (`profile_id`) is a persistence concern owned here.
//! It is NOT added to `uc-core` search domain structs.

pub mod constants;
pub mod pipeline;
pub mod rows;
pub mod search_key_derivation;
pub mod sqlite_index;
pub mod text_extractor;
pub mod tokenizer;

pub use constants::*;
pub use pipeline::*;
pub use rows::*;
pub use search_key_derivation::*;
pub use sqlite_index::*;
pub use text_extractor::*;
pub use tokenizer::*;
