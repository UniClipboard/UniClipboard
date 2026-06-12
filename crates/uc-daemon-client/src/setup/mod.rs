//! Typed setup state parsing.
//!
//! See [`parsed_state`](parsed_state) module for details.

pub use parsed_state::{
    format_peer_id_suffix, parse_setup_state, ParsedSetupState, SetupHint, SetupVariant,
};

pub mod parsed_state;
