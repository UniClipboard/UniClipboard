// Re-export from uc-core — canonical definitions now live there (ADR-008 P5).
pub use uc_core::ports::observability::{
    extract_trace, OptionalTrace, TraceMetadata, TraceParseError,
};
