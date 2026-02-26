use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use uc_core::network::PairedDevice;

static STAGED_PAIRED_DEVICES: OnceLock<Mutex<HashMap<String, PairedDevice>>> = OnceLock::new();

fn staged_devices() -> &'static Mutex<HashMap<String, PairedDevice>> {
    STAGED_PAIRED_DEVICES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn stage(session_id: &str, device: PairedDevice) {
    if let Ok(mut staged) = staged_devices().lock() {
        staged.insert(session_id.to_string(), device);
    }
}

pub(crate) fn take_by_peer_id(peer_id: &str) -> Option<PairedDevice> {
    let mut staged = staged_devices().lock().ok()?;
    let session_id = staged.iter().find_map(|(session_id, device)| {
        (device.peer_id.as_str() == peer_id).then(|| session_id.clone())
    })?;
    staged.remove(&session_id)
}

pub(crate) fn get_by_peer_id(peer_id: &str) -> Option<PairedDevice> {
    let staged = staged_devices().lock().ok()?;
    let session_id = staged.iter().find_map(|(session_id, device)| {
        (device.peer_id.as_str() == peer_id).then(|| session_id.clone())
    })?;
    staged.get(&session_id).cloned()
}

#[cfg(test)]
pub(crate) fn clear() {
    if let Ok(mut staged) = staged_devices().lock() {
        staged.clear();
    }
}
