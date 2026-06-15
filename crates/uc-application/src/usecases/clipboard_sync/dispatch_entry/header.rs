//! `OutboundHeaderFactory` — builds the [`ClipboardHeader`] stamped on
//! every fanned-out frame: protocol version, content hash, capture time,
//! origin device id / name, payload version, and the cross-device
//! `flow_id` that lets the inbound peer join the same Sentry trace.

use std::sync::Arc;

use tracing::warn;
use uc_core::ids::DeviceId;
use uc_core::ports::{ClipboardHeader, ClockPort, LocalIdentityPort, SettingsPort};
use uc_observability::FlowId;

use super::DispatchClipboardEntryInput;

pub(crate) struct OutboundHeaderFactory {
    settings: Arc<dyn SettingsPort>,
    local_identity: Arc<dyn LocalIdentityPort>,
    clock: Arc<dyn ClockPort>,
}

impl OutboundHeaderFactory {
    pub(crate) fn new(
        settings: Arc<dyn SettingsPort>,
        local_identity: Arc<dyn LocalIdentityPort>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            settings,
            local_identity,
            clock,
        }
    }

    /// Build the outbound header once per dispatch; cloned per target on
    /// the wire. `local_device` is resolved once by the caller.
    pub(crate) async fn build(
        &self,
        input: &DispatchClipboardEntryInput,
        flow_id: &FlowId,
        local_device: &DeviceId,
    ) -> ClipboardHeader {
        let origin_device_name = self.load_origin_device_name().await;
        ClipboardHeader {
            version: ClipboardHeader::CURRENT_VERSION,
            content_hash: input.content_hash.clone(),
            captured_at_ms: self.clock.now_ms(),
            origin_device_id: local_device.as_str().to_string(),
            origin_device_name,
            payload_version: input.payload_version,
            flow_id: Some(flow_id.to_string()),
        }
    }

    /// Load the device's own display name to embed in the outbound header
    /// so the peer can show "from <Alice's Laptop>". Falls back to the
    /// fingerprint if settings are unreadable or empty.
    async fn load_origin_device_name(&self) -> String {
        match self.settings.load().await {
            Ok(settings) => {
                if let Some(name) = settings.general.device_name {
                    if !name.is_empty() {
                        return name;
                    }
                }
            }
            Err(err) => {
                warn!(error = %err, "dispatch: settings load failed; using fingerprint fallback");
            }
        }
        match self.local_identity.get_current_fingerprint().await {
            Ok(Some(fp)) => fp.as_display().to_string(),
            _ => "unknown-device".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    fn factory(
        settings: MockSettings_,
        local_identity: MockLocalIdentity,
    ) -> OutboundHeaderFactory {
        OutboundHeaderFactory::new(
            Arc::new(settings),
            Arc::new(local_identity),
            Arc::new(FixedClock(1_700_000_000_000)),
        )
    }

    /// Stamps protocol version, content hash, payload version, clock-sourced
    /// capture time, origin id, and the flow id — and prefers the settings
    /// device name when it is present and non-empty.
    #[tokio::test]
    async fn build_stamps_header_and_uses_settings_device_name() {
        let mut settings = MockSettings_::new();
        settings
            .expect_load()
            .returning(|| Ok(settings_with_device_name("Alice Laptop")));
        // Fingerprint is the fallback; it must not be consulted here.
        let local_identity = MockLocalIdentity::new();

        let factory = factory(settings, local_identity);
        let input = dispatch_input();
        let flow = FlowId::generate();
        let header = factory.build(&input, &flow, &dev("self-device")).await;

        assert_eq!(header.version, ClipboardHeader::CURRENT_VERSION);
        assert_eq!(header.content_hash, input.content_hash);
        assert_eq!(header.payload_version, input.payload_version);
        assert_eq!(header.captured_at_ms, 1_700_000_000_000);
        assert_eq!(header.origin_device_id, "self-device");
        assert_eq!(header.origin_device_name, "Alice Laptop");
        assert_eq!(header.flow_id, Some(flow.to_string()));
    }

    /// No usable settings name (absent) ⇒ fall back to the fingerprint
    /// display.
    #[tokio::test]
    async fn build_falls_back_to_fingerprint_when_settings_name_absent() {
        let mut settings = MockSettings_::new();
        settings.expect_load().returning(|| Ok(default_settings()));
        let mut local_identity = MockLocalIdentity::new();
        local_identity
            .expect_get_current_fingerprint()
            .returning(|| Ok(Some(fp(7))));

        let factory = factory(settings, local_identity);
        let header = factory
            .build(&dispatch_input(), &FlowId::generate(), &dev("self-device"))
            .await;

        assert_eq!(header.origin_device_name, fp(7).as_display().to_string());
    }

    /// An empty settings name is treated as "absent" — the fingerprint
    /// fallback still kicks in.
    #[tokio::test]
    async fn build_falls_back_to_fingerprint_when_settings_name_empty() {
        let mut settings = MockSettings_::new();
        settings
            .expect_load()
            .returning(|| Ok(settings_with_device_name("")));
        let mut local_identity = MockLocalIdentity::new();
        local_identity
            .expect_get_current_fingerprint()
            .returning(|| Ok(Some(fp(7))));

        let factory = factory(settings, local_identity);
        let header = factory
            .build(&dispatch_input(), &FlowId::generate(), &dev("self-device"))
            .await;

        assert_eq!(header.origin_device_name, fp(7).as_display().to_string());
    }

    /// Settings load failure also degrades to the fingerprint fallback
    /// (best-effort header naming must never abort the dispatch).
    #[tokio::test]
    async fn build_falls_back_to_fingerprint_when_settings_load_errors() {
        let mut settings = MockSettings_::new();
        settings
            .expect_load()
            .returning(|| Err(anyhow::anyhow!("settings unavailable")));
        let mut local_identity = MockLocalIdentity::new();
        local_identity
            .expect_get_current_fingerprint()
            .returning(|| Ok(Some(fp(3))));

        let factory = factory(settings, local_identity);
        let header = factory
            .build(&dispatch_input(), &FlowId::generate(), &dev("self-device"))
            .await;

        assert_eq!(header.origin_device_name, fp(3).as_display().to_string());
    }

    /// No settings name AND no fingerprint ⇒ the `"unknown-device"`
    /// last-resort literal.
    #[tokio::test]
    async fn build_uses_unknown_device_when_no_name_and_no_fingerprint() {
        let mut settings = MockSettings_::new();
        settings.expect_load().returning(|| Ok(default_settings()));
        let mut local_identity = MockLocalIdentity::new();
        local_identity
            .expect_get_current_fingerprint()
            .returning(|| Ok(None));

        let factory = factory(settings, local_identity);
        let header = factory
            .build(&dispatch_input(), &FlowId::generate(), &dev("self-device"))
            .await;

        assert_eq!(header.origin_device_name, "unknown-device");
    }
}
