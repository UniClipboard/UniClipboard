//! Daemon-owned search integration layer.
//!
//! This module contains the daemon owner for index rebuild lifecycle, reason
//! codes, and WebSocket progress forwarding. Search projection rules live in
//! `uc-application`.

pub mod coordinator;
