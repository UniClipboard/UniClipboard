//! `LocalActiveRegisterAdvancer` — advances the cross-device
//! active-clipboard register for writes that originate on this device
//! (local capture / restore).
//!
//! The active-clipboard register tracks which content is the current OS
//! clipboard content as a last-writer-wins register that converges across
//! devices. A local write stamps the activation with this device's id and
//! the current wall-clock, so the device that just put content on its own
//! clipboard becomes the latest writer.
//!
//! Advancing the register is a best-effort side channel: a storage hiccup
//! is logged and swallowed so it never fails the user-visible clipboard
//! write it trails.

use std::sync::Arc;

use tracing::warn;

use uc_core::clipboard::ActiveClipboardState;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::AdvanceActiveClipboardPort;
use uc_core::ports::{ClockPort, DeviceIdentityPort};

use super::MobileConsumabilityProbe;

/// Advances the active-clipboard register on behalf of locally-originated
/// writes, stamping `(now, this_device)` as the activation key.
#[derive(Clone)]
pub struct LocalActiveRegisterAdvancer {
    register: Arc<dyn AdvanceActiveClipboardPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    clock: Arc<dyn ClockPort>,
    mobile_consumability: MobileConsumabilityProbe,
}

impl LocalActiveRegisterAdvancer {
    pub fn new(
        register: Arc<dyn AdvanceActiveClipboardPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        clock: Arc<dyn ClockPort>,
        mobile_consumability: MobileConsumabilityProbe,
    ) -> Self {
        Self {
            register,
            device_identity,
            clock,
            mobile_consumability,
        }
    }

    /// Record that `snapshot_hash` (held locally as `entry_id`) is now the
    /// active clipboard content, activated on this device at the current
    /// wall-clock. Best-effort: a register storage failure is logged and
    /// swallowed.
    ///
    /// Returns the [`ActiveClipboardState`] stamped for this activation so the
    /// caller can hand it on (e.g. to a broadcaster) without re-deriving the
    /// `(now, this_device)` key. The state is returned regardless of whether
    /// the register storage write succeeded — the local OS clipboard already
    /// holds this content, so it is the activation of record either way.
    pub async fn advance_local(
        &self,
        snapshot_hash: String,
        entry_id: EntryId,
    ) -> ActiveClipboardState {
        let state = ActiveClipboardState::new(
            snapshot_hash,
            entry_id,
            self.clock.now_ms(),
            self.device_identity.current_device_id(),
        );
        let mobile_consumable = self
            .mobile_consumability
            .is_mobile_consumable(&state.entry_id)
            .await;
        match self.register.advance(&state, mobile_consumable).await {
            Ok(advanced) => {
                tracing::debug!(
                    snapshot_hash = %state.snapshot_hash,
                    entry_id = %state.entry_id,
                    advanced,
                    "active register: local advance"
                );
            }
            Err(e) => {
                warn!(
                    error = %e,
                    snapshot_hash = %state.snapshot_hash,
                    entry_id = %state.entry_id,
                    "active register: local advance failed (best-effort, ignored)"
                );
            }
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::ids::DeviceId;
    use uc_core::ports::clipboard::ActiveClipboardRegisterError;

    use crate::test_support::{empty_directory_file_set, FixedFileSets};

    #[derive(Default)]
    struct RecordingRegister(Mutex<Vec<bool>>);

    #[async_trait]
    impl AdvanceActiveClipboardPort for RecordingRegister {
        async fn advance(
            &self,
            _state: &ActiveClipboardState,
            mobile_consumable: bool,
        ) -> Result<bool, ActiveClipboardRegisterError> {
            self.0.lock().unwrap().push(mobile_consumable);
            Ok(true)
        }
    }

    struct FixedClock;

    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            1
        }
    }

    struct FixedDevice;

    impl DeviceIdentityPort for FixedDevice {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("device")
        }
    }

    #[tokio::test]
    async fn local_advance_marks_directories_non_consumable() {
        let register = Arc::new(RecordingRegister::default());
        let advancer = LocalActiveRegisterAdvancer::new(
            register.clone(),
            Arc::new(FixedDevice),
            Arc::new(FixedClock),
            MobileConsumabilityProbe::new(Arc::new(FixedFileSets(Ok(Some(
                empty_directory_file_set(),
            ))))),
        );

        advancer
            .advance_local("blake3v1:directory".into(), EntryId::from("directory"))
            .await;

        assert_eq!(register.0.lock().unwrap().as_slice(), &[false]);
    }
}
