//! Parsed setup state — typed interpretation of [`SetupStateResponseDto`].
//!
//! Centralizes all remote state string parsing so CLI layer only works with typed enums.

use serde_json::Value;
use uc_daemon_contract::api::dto::setup::SetupStateResponseDto;

// ── Enums ────────────────────────────────────────────────────────────

/// Hint derived from `next_step_hint` field — what the CLI should do next.
#[derive(Debug, Clone, PartialEq)]
pub enum SetupHint {
    Idle,
    Completed,
    HostConfirmPeer,
    JoinSelectPeer,
    JoinEnterPassphrase,
    /// Joiner has selected a peer and is waiting for the host to accept/reject.
    JoinWaitingForHost,
    Unknown(String),
}

impl SetupHint {
    pub fn from_hint_string(s: &str) -> Self {
        match s {
            "idle" => SetupHint::Idle,
            "completed" => SetupHint::Completed,
            "host-confirm-peer" => SetupHint::HostConfirmPeer,
            "join-select-peer" => SetupHint::JoinSelectPeer,
            "join-enter-passphrase" => SetupHint::JoinEnterPassphrase,
            "join-waiting-for-host" => SetupHint::JoinWaitingForHost,
            other => SetupHint::Unknown(other.to_string()),
        }
    }
}

/// Variant derived from `state` field — backend pairing session type.
#[derive(Debug, Clone, PartialEq)]
pub enum SetupVariant {
    Idle,
    JoinSpaceConfirmPeer,
    JoinSpaceInputPassphrase,
    Completed,
    Unknown(String),
}

impl SetupVariant {
    /// Parse SetupVariant from the `state: Value` field of SetupStateResponseDto.
    pub fn from_state_value(state: &Value) -> Self {
        match state {
            Value::String(s) => Self::from_str(s),
            Value::Object(map) if map.len() == 1 => {
                let key = map.keys().next().map(String::as_str).unwrap_or("<none>");
                Self::from_str(key)
            }
            _ => SetupVariant::Unknown("<none>".to_string()),
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "Idle" => SetupVariant::Idle,
            "JoinSpaceConfirmPeer" => SetupVariant::JoinSpaceConfirmPeer,
            "JoinSpaceInputPassphrase" => SetupVariant::JoinSpaceInputPassphrase,
            "Completed" => SetupVariant::Completed,
            other => SetupVariant::Unknown(other.to_string()),
        }
    }
}

// ── ParsedSetupState ────────────────────────────────────────────────

/// Combined parsed state from a [`SetupStateResponseDto`].
///
/// This is the single output type that CLI flows work with — no more raw string matching.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSetupState {
    /// Parsed hint (what CLI should do)
    pub hint: SetupHint,
    /// Parsed variant (backend session type)
    pub variant: SetupVariant,
    /// Current session ID (if any)
    pub session_id: Option<String>,
    /// Whether space setup has completed
    pub has_completed: bool,
    /// Short verification code (extracted from JoinSpaceConfirmPeer payload)
    pub short_code: Option<String>,
    /// Formatted peer label for display
    pub selected_peer_label: Option<String>,
    /// Error code from passphrase verification failure
    pub error_code: Option<String>,
}

/// Parse a [`SetupStateResponseDto`] into a typed [`ParsedSetupState`].
///
/// This is the main entry point — call this once per poll iteration instead of
/// matching on raw strings throughout the CLI code.
#[must_use]
pub fn parse_setup_state(dto: &SetupStateResponseDto) -> ParsedSetupState {
    let hint = SetupHint::from_hint_string(&dto.next_step_hint);
    let variant = SetupVariant::from_state_value(&dto.state);
    let short_code = extract_short_code(&dto.state, &variant);
    let error_code = extract_error_code(&dto.state, &variant);
    let selected_peer_label = format_peer_label(&dto.selected_peer_id, &dto.selected_peer_name);

    ParsedSetupState {
        hint,
        variant,
        session_id: dto.session_id.clone(),
        has_completed: dto.has_completed,
        short_code,
        error_code,
        selected_peer_label,
    }
}

// ── Internal helpers ────────────────────────────────────────────────

fn extract_short_code(state: &Value, variant: &SetupVariant) -> Option<String> {
    let variant_name = match variant {
        SetupVariant::JoinSpaceConfirmPeer => "JoinSpaceConfirmPeer",
        _ => return None,
    };

    let payload = match state {
        Value::Object(map) => map.get(variant_name)?,
        _ => return None,
    };

    payload.get("short_code")?.as_str().map(String::from)
}

fn extract_error_code(state: &Value, variant: &SetupVariant) -> Option<String> {
    let variant_name = match variant {
        SetupVariant::JoinSpaceInputPassphrase => "JoinSpaceInputPassphrase",
        _ => return None,
    };
    let payload = match state {
        Value::Object(map) => map.get(variant_name)?,
        _ => return None,
    };
    payload.get("error")?.as_str().map(String::from)
}

fn format_peer_label(peer_id: &Option<String>, peer_name: &Option<String>) -> Option<String> {
    let peer_id = peer_id.as_deref();
    let peer_name = peer_name.as_deref().map(str::trim);

    match (peer_name, peer_id) {
        (Some(name), Some(peer_id)) if !name.is_empty() => {
            Some(format!("{name} ({})", format_peer_id_suffix(peer_id)))
        }
        (Some(name), None) if !name.is_empty() => Some(name.to_string()),
        (_, Some(peer_id)) => Some(format_peer_id_suffix(peer_id)),
        _ => None,
    }
}

/// Return the last 8 characters of a peer ID for compact display.
///
/// If the peer ID is already 8 chars or shorter, it is returned as-is.
pub fn format_peer_id_suffix(peer_id: &str) -> String {
    if peer_id.len() <= 8 {
        peer_id.to_string()
    } else {
        peer_id[peer_id.len() - 8..].to_string()
    }
}

// ── Tests ───────────────────────────────────────────────────────────
