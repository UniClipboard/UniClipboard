pub mod autostart;
pub mod clipboard;
pub mod encryption;
pub mod error;
pub mod lifecycle;
pub mod pairing;
pub mod settings;
pub mod setup;
pub mod startup;
pub mod tray;
pub mod updater;

use tracing::Span;
use uc_core::ports::observability::TraceMetadata;

// Re-export commonly used types
pub use autostart::*;
pub use clipboard::*;
pub use encryption::*;
pub use lifecycle::*;
pub use pairing::*;
pub use settings::*;
pub use setup::*;
pub use startup::*;
pub use updater::*;

pub use error::map_err;

pub(crate) fn record_trace_fields(span: &Span, trace: &Option<TraceMetadata>) {
    if let Some(metadata) = trace.as_ref() {
        span.record("trace_id", tracing::field::display(&metadata.trace_id));
        span.record("trace_ts", metadata.timestamp);
    }
}

#[cfg(test)]
mod tests {
    use super::record_trace_fields;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use tracing::info_span;
    use tracing_subscriber::fmt::MakeWriter;
    use uc_core::ports::observability::TraceMetadata;

    #[derive(Clone)]
    struct BufferWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    struct BufferGuard {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl<'a> MakeWriter<'a> for BufferWriter {
        type Writer = BufferGuard;

        fn make_writer(&'a self) -> Self::Writer {
            BufferGuard {
                buffer: self.buffer.clone(),
            }
        }
    }

    impl Write for BufferGuard {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            if let Ok(mut buffer) = self.buffer.lock() {
                buffer.extend_from_slice(buf);
            }
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn records_trace_fields_on_span() {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = BufferWriter {
            buffer: buffer.clone(),
        };
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_ansi(false)
            .with_target(false)
            .without_time()
            .finish();

        let trace = TraceMetadata {
            trace_id: Default::default(),
            timestamp: 1737100000000,
        };

        tracing::subscriber::with_default(subscriber, || {
            let span = info_span!(
                "command.test",
                trace_id = tracing::field::Empty,
                trace_ts = tracing::field::Empty,
            );
            record_trace_fields(&span, &Some(trace));
            let _guard = span.enter();
            tracing::info!("test event");
        });

        let output =
            String::from_utf8(buffer.lock().expect("buffer lock").clone()).expect("utf8 output");
        assert!(
            output.contains("trace_id=00000000-0000-0000-0000-000000000000"),
            "missing trace_id: {}",
            output
        );
        assert!(
            output.contains("trace_ts=1737100000000"),
            "missing trace_ts"
        );
    }
}
