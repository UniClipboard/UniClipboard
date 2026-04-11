//! Daemon-owned search integration layer.
//!
//! This module contains:
//! - `projection`: the single authority for building `SearchPipelineInput` from
//!   live and persisted clipboard sources.
//! - `coordinator`: the daemon owner for index rebuild lifecycle, reason codes,
//!   and WebSocket progress forwarding.

pub mod coordinator;
pub mod projection;
