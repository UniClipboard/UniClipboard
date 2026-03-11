//! Shared span-field collection logic for tracing formatters.
//!
//! Extracts span fields from a tracing context into a `BTreeMap`, reusable
//! by both `FlatJsonFormat` and `CLEFFormat`.

use std::collections::BTreeMap;
use tracing::Subscriber;
use tracing_subscriber::fmt::format::FormatFields;
use tracing_subscriber::fmt::FmtContext;
use tracing_subscriber::registry::LookupSpan;

/// Collect span fields from the tracing context into a `BTreeMap`.
///
/// Walks from root to leaf span, parsing `JsonFields` extensions into
/// key-value pairs. Later (child) spans overwrite earlier (parent) spans
/// for the same key.
///
/// Returns `(leaf_span_name, fields_map)`.
pub fn collect_span_fields<S, N>(
    ctx: &FmtContext<'_, S, N>,
) -> (Option<String>, BTreeMap<String, serde_json::Value>)
where
    S: Subscriber + for<'lookup> LookupSpan<'lookup>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    let mut span_fields = BTreeMap::new();
    let mut leaf_span_name: Option<String> = None;

    if let Some(scope) = ctx.event_scope() {
        let spans: Vec<_> = scope.collect();

        // Leaf span name (first in scope = leaf since scope iterates leaf-to-root)
        if let Some(leaf) = spans.first() {
            leaf_span_name = Some(leaf.name().to_string());
        }

        // Walk root-to-leaf to collect span fields
        for span in spans.iter().rev() {
            let extensions = span.extensions();
            if let Some(fields) = extensions.get::<tracing_subscriber::fmt::FormattedFields<N>>() {
                if let Ok(parsed) =
                    serde_json::from_str::<BTreeMap<String, serde_json::Value>>(fields.as_ref())
                {
                    for (key, value) in parsed {
                        span_fields.insert(key, value);
                    }
                }
            }
        }
    }

    (leaf_span_name, span_fields)
}
