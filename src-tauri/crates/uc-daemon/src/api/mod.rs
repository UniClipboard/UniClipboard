//! Transport-facing daemon API modules.

pub mod auth;
pub mod blob;
pub mod clipboard;
pub mod conversion;
pub mod device;
pub mod dto;
pub mod encryption;
pub mod event_emitter;
pub mod lifecycle;
pub mod openapi;
pub mod pairing;
pub mod projection;
pub mod query;
pub mod routes;
pub mod search;
pub mod server;
pub mod settings;
pub mod setup;
pub mod storage;
pub mod types;
pub mod ws;

#[cfg(debug_assertions)]
pub mod dev;

#[cfg(debug_assertions)]
pub use dev::{dev_token_handler, ApiDocDev, DevTokenResponse};
