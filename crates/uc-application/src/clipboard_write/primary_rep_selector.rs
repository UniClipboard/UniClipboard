//! Narrow a multi-representation `SystemClipboardSnapshot` down to one
//! representation before calling `SystemClipboardPort::write_snapshot`.
//!
//! ## Why this lives here
//!
//! `uc-platform`'s `write_snapshot` contract is "exactly one representation"
//! (see `uc-platform/src/clipboard/common.rs` `ensure!(snapshot.representations.len() == 1)`
//! and the TODO around `NSPasteboardItem` atomic multi-rep writes). Inbound
//! sync's V3 envelope routinely carries multiple reps (text/plain +
//! text/html + text/rtf + image/png + ...). Deciding which rep to push is
//! **policy**, not a platform concern (`uc-platform/AGENTS.md` §6.1), so the
//! choice belongs in the application layer.
//!
//! ## Policy reuse
//!
//! We do not re-invent a MIME priority table. `CaptureClipboardUseCase`
//! already runs `SelectRepresentationPolicyPort::select` on every captured
//! snapshot and stores the resulting `ClipboardSelection.paste_rep_id`
//! as the preferred-for-paste representation. Re-using that same policy
//! here keeps "what goes into the OS clipboard when the user pastes" and
//! "what we push to the OS clipboard when we apply an inbound frame"
//! aligned — changing paste priority in one place updates both.

use thiserror::Error;

use uc_core::clipboard::SystemClipboardSnapshot;
use uc_core::ports::SelectRepresentationPolicyPort;

#[derive(Debug, Error)]
pub enum PrimaryRepError {
    #[error("snapshot has no representations")]
    Empty,
    #[error("representation policy failed: {0}")]
    Policy(String),
    #[error("policy selected paste_rep_id not present in the snapshot")]
    MissingPasteRep,
}

/// Narrow `snapshot` down to a single representation by consulting
/// `policy.select(...).paste_rep_id`. Single-rep snapshots are returned
/// unchanged to avoid a redundant policy call.
pub fn narrow_to_primary(
    snapshot: SystemClipboardSnapshot,
    policy: &dyn SelectRepresentationPolicyPort,
) -> Result<SystemClipboardSnapshot, PrimaryRepError> {
    if snapshot.representations.is_empty() {
        return Err(PrimaryRepError::Empty);
    }
    if snapshot.representations.len() == 1 {
        return Ok(snapshot);
    }

    let selection = policy
        .select(&snapshot)
        .map_err(|e| PrimaryRepError::Policy(e.to_string()))?;
    let paste_id = selection.paste_rep_id.clone();
    let chosen_idx = snapshot
        .representations
        .iter()
        .position(|rep| rep.id == paste_id)
        .ok_or(PrimaryRepError::MissingPasteRep)?;

    let ts_ms = snapshot.ts_ms;
    let mut reps = snapshot.representations;
    let chosen = reps.remove(chosen_idx);
    Ok(SystemClipboardSnapshot {
        ts_ms,
        representations: vec![chosen],
        file_content_digests: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use uc_core::clipboard::SelectRepresentationPolicyV1;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::{MimeType, ObservedClipboardRepresentation};

    fn rep(fmt: &str, mime: Option<&str>, bytes: &[u8]) -> ObservedClipboardRepresentation {
        ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from(fmt),
            mime.map(|m| MimeType(m.to_string())),
            bytes.to_vec(),
        )
    }

    #[test]
    fn empty_snapshot_returns_empty_error() {
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![],

            file_content_digests: Vec::new(),
        };
        let policy = SelectRepresentationPolicyV1::default();
        match narrow_to_primary(snapshot, &policy) {
            Err(PrimaryRepError::Empty) => {}
            other => panic!("expected Empty, got {other:?}"),
        }
    }

    #[test]
    fn single_rep_passthrough() {
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![rep("text", Some("text/plain"), b"hello")],

            file_content_digests: Vec::new(),
        };
        let original_id = snapshot.representations[0].id.clone();
        let policy = SelectRepresentationPolicyV1::default();
        let narrowed = narrow_to_primary(snapshot, &policy).expect("passthrough ok");
        assert_eq!(narrowed.representations.len(), 1);
        assert_eq!(narrowed.representations[0].id, original_id);
    }

    #[test]
    fn multi_rep_narrows_to_policies_paste_rep() {
        // text/plain + text/html + image/png. V1's DefaultPaste ranks
        // RichText (html) above PlainText above Image (see
        // `uc-core::clipboard::policy::v1::SelectRepresentationPolicyV1::score`),
        // so the html rep should win — preserving formatting on paste.
        // Pins both "narrow actually narrows" and "narrow uses the
        // paste-priority, not preview-priority" since UiPreview would
        // have picked plain.
        let plain = rep("text", Some("text/plain"), b"hi");
        let html = rep("html", Some("text/html"), b"<p>hi</p>");
        let image = rep("image", Some("image/png"), b"\x89PNG\r\n");
        let html_id = html.id.clone();
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 2,
            representations: vec![plain, html, image],

            file_content_digests: Vec::new(),
        };
        let policy = SelectRepresentationPolicyV1::default();
        let narrowed = narrow_to_primary(snapshot, &policy).expect("policy narrow ok");
        assert_eq!(narrowed.representations.len(), 1);
        assert_eq!(narrowed.representations[0].id, html_id);
    }

    #[test]
    fn ts_ms_preserved_across_narrow() {
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 42_000,
            representations: vec![
                rep("text", Some("text/plain"), b"a"),
                rep("html", Some("text/html"), b"<p>a</p>"),
            ],
            file_content_digests: Vec::new(),
        };
        let policy = SelectRepresentationPolicyV1::default();
        let narrowed = narrow_to_primary(snapshot, &policy).expect("ok");
        assert_eq!(narrowed.ts_ms, 42_000);
    }
}
