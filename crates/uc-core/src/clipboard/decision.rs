#[derive(Debug, Clone)]
pub enum ClipboardContentActionDecision {
    Allow,
    Reject { reason: RejectReason },
}

#[derive(Debug, Clone)]
pub enum RejectReason {
    NotFound,
    Expired,
    Sensitive,
    PolicyDenied,
    InternalError,
}

#[derive(Debug, Clone)]
pub enum DuplicationHint {
    New,
    Repeated,
    // Future: Support for repeated content with a timestamp
    // Repeated {
    //     previous_at: chrono::DateTime<chrono::Utc>,
    // },
}
