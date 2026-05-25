//! Diagnostic data attached by clipboard backends to write-failure errors
//! when contention can be attributed to a specific foreign process.
//!
//! This is **observability metadata**, not a domain concept — pids and
//! executable names exist only inside a running OS process and have no
//! meaning to the business layer. It lives in `uc-platform` (the layer
//! that can probe OS-level clipboard ownership) and rides up to consumers
//! via `anyhow::Error`'s causal chain. Consumers that care about
//! contention diagnostics walk the chain and downcast; consumers that
//! don't see it ignore it.

/// Diagnostic record describing a foreign process that holds the system
/// clipboard at the moment a write attempt fails because of ownership
/// contention.
///
/// ## Contract
///
/// Backends that can identify the contending peer attach this value to the
/// error chain of [`uc_core::ports::SystemClipboardPort::write_snapshot`];
/// consumers recover it via
/// `anyhow::Error::chain().find_map(|e| e.downcast_ref::<ClipboardHolderInfo>())`.
/// Backends that cannot identify the peer simply omit the attachment — the
/// absence of the diagnostic is itself information (probe race lost,
/// ownership not introspectable on this platform, etc.).
///
/// ## Layering
///
/// To preserve the original error message as the outer `Display` (so log
/// fields like `error = %err` keep showing the OS-error text), attach via
/// `anyhow::Error::new(ClipboardHolderInfo { … }).context(human_message)`.
/// `.context(...)` puts the new layer on top of `Display`, so the holder
/// must be at the bottom of the chain (the source) and the human message
/// on top (the context). Reversing this layering shadows the message with
/// "clipboard held by pid=… exe=…" and silently regresses logs.
///
/// ## Semantics
///
/// The record reflects ownership *at the moment of probing*, which by
/// contract should be as close as the backend can practically get to the
/// original conflict. It is **not** guaranteed to be the same process by
/// the time the error surfaces upstream; treat it as a strong hint, not a
/// proof.
///
/// Implements [`std::error::Error`] so it can ride along in
/// `anyhow::Error`'s causal chain via `.context(...)`. The error chain is
/// the canonical transport — do not add bespoke fields to the
/// `SystemClipboardPort` trait surface for this.
#[derive(Debug, Clone)]
pub struct ClipboardHolderInfo {
    /// OS-level identifier of the contending process.
    pub holder_pid: u32,
    /// Executable name of the contending process (e.g. its file-name
    /// component). When the backend obtains a pid but cannot resolve the
    /// executable, it records a sentinel value (such as `"<access denied>"`)
    /// so a partial result remains observable instead of being silently lost.
    pub holder_exe: String,
}

impl std::fmt::Display for ClipboardHolderInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "clipboard held by pid={} exe={}",
            self.holder_pid, self.holder_exe
        )
    }
}

impl std::error::Error for ClipboardHolderInfo {}

#[cfg(test)]
mod tests {
    //! Contract tests for [`ClipboardHolderInfo`]'s anyhow transport.
    //!
    //! These pin the two invariants consumers depend on:
    //!
    //! 1. A backend can attach `ClipboardHolderInfo` to an error such that
    //!    the human-facing message is preserved as the outer `Display`
    //!    (so `error = %err` log fields don't get shadowed by the holder
    //!    record).
    //! 2. The holder can be recovered by walking `anyhow::Error::chain()`
    //!    and `downcast_ref`, regardless of how deeply nested it is.
    //!
    //! Without these tests the contract is purely a doc-comment claim —
    //! and the layering is subtle enough that "context puts the new layer
    //! on top of Display" is easy to flip by accident.
    use super::*;

    /// The recipe documented above: holder at the bottom
    /// (`anyhow::Error::new`), human message on top (`.context(...)`). The
    /// outer `Display` must be the message — not "clipboard held by pid=…".
    #[test]
    fn holder_via_context_preserves_outer_display_and_remains_downcastable() {
        let err = anyhow::Error::new(ClipboardHolderInfo {
            holder_pid: 4242,
            holder_exe: "Ditto.exe".to_string(),
        })
        .context("OS clipboard write failed: ERROR_ACCESS_DENIED");

        // Outer Display = the human message (not the holder record).
        assert_eq!(
            err.to_string(),
            "OS clipboard write failed: ERROR_ACCESS_DENIED",
        );

        // Downcast still finds the holder somewhere in the chain.
        let holder = err
            .chain()
            .find_map(|e| e.downcast_ref::<ClipboardHolderInfo>())
            .expect("holder must be reachable via chain downcast");
        assert_eq!(holder.holder_pid, 4242);
        assert_eq!(holder.holder_exe, "Ditto.exe");
    }

    /// Errors without a holder must still be ergonomic for the consumer —
    /// the downcast simply returns `None`, no panic, no surprise.
    #[test]
    fn chain_without_holder_returns_none_on_downcast() {
        let err = anyhow::anyhow!("OS clipboard write failed");
        assert!(err
            .chain()
            .find_map(|e| e.downcast_ref::<ClipboardHolderInfo>())
            .is_none());
    }

    /// Alternate `{:#}` formatting includes both the message and the
    /// holder — useful for full-context dumps without losing structured
    /// downcast access.
    #[test]
    fn alternate_display_includes_holder_message() {
        let err = anyhow::Error::new(ClipboardHolderInfo {
            holder_pid: 7,
            holder_exe: "X.exe".to_string(),
        })
        .context("top message");
        let combined = format!("{:#}", err);
        assert!(combined.contains("top message"), "got: {combined}");
        assert!(combined.contains("pid=7"), "got: {combined}");
        assert!(combined.contains("X.exe"), "got: {combined}");
    }
}
