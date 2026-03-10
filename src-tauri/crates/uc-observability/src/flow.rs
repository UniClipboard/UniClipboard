/// Flow correlation ID for clipboard capture pipeline tracing.
///
/// Wraps a UUID v7 (time-ordered) to provide monotonic, unique identifiers
/// that can be attached as tracing span fields via the `Display` impl.
use std::fmt;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowId(Uuid);

impl FlowId {
    /// Generate a new flow ID using UUID v7 (time-ordered).
    pub fn generate() -> Self {
        Self(Uuid::now_v7())
    }
}

impl fmt::Display for FlowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn generate_returns_uuid_v7_string() {
        let flow_id = FlowId::generate();
        let s = flow_id.to_string();
        // UUID format: 8-4-4-4-12 hex chars = 36 chars total
        assert_eq!(s.len(), 36, "FlowId display should be 36-char UUID string");
        // UUID v7: version nibble (char at index 14) should be '7'
        assert_eq!(
            s.chars().nth(14),
            Some('7'),
            "FlowId should be UUID v7 (version nibble = 7)"
        );
    }

    #[test]
    fn display_and_debug_work() {
        let flow_id = FlowId::generate();
        let display = format!("{}", flow_id);
        let debug = format!("{:?}", flow_id);
        assert!(!display.is_empty());
        assert!(!debug.is_empty());
        // Display should be the bare UUID, Debug includes struct name
        assert_ne!(display, debug);
    }

    #[test]
    fn clone_and_eq() {
        let a = FlowId::generate();
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn hash_works() {
        let a = FlowId::generate();
        let b = a.clone();
        let mut set = HashSet::new();
        set.insert(a.clone());
        assert!(set.contains(&b));
    }

    #[test]
    fn unique_ids() {
        let a = FlowId::generate();
        let b = FlowId::generate();
        assert_ne!(a, b);
    }
}
