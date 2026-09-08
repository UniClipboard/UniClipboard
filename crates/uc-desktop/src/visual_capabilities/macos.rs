use super::GraphicsCapability;
use objc2_metal::{MTLCopyAllDevices, MTLDevice, MTLGPUFamily};

// Metal discovery requires CoreGraphics framework registration. No window API
// is called here, and enumerating devices does not request a discrete-GPU switch.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {}

pub(super) fn detect_graphics() -> GraphicsCapability {
    let devices = MTLCopyAllDevices();
    if devices.is_empty() {
        return GraphicsCapability::Unavailable;
    }
    // Assess the weakest available GPU, rather than assuming the WebView uses
    // the most capable adapter on a switchable/multi-GPU Mac.
    if (0..devices.len()).all(|index| {
        devices
            .objectAtIndex(index)
            .supportsFamily(MTLGPUFamily::Mac2)
    }) {
        GraphicsCapability::ModernHardware
    } else {
        GraphicsCapability::LegacyHardware
    }
}
