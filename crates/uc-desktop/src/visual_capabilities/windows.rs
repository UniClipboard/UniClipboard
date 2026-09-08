use super::GraphicsCapability;
use windows::Win32::{
    Foundation::{E_INVALIDARG, HMODULE},
    Graphics::{
        Direct3D::{
            D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_10_0,
            D3D_FEATURE_LEVEL_10_1, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
            D3D_FEATURE_LEVEL_12_0, D3D_FEATURE_LEVEL_9_3,
        },
        Direct3D11::{D3D11CreateDevice, D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION},
        Dxgi::IDXGIAdapter,
    },
};

fn hardware_feature_level(
    levels: &[D3D_FEATURE_LEVEL],
) -> windows::core::Result<D3D_FEATURE_LEVEL> {
    let mut level = D3D_FEATURE_LEVEL_9_3;
    // SAFETY: all input/output references remain valid for this synchronous call.
    // A null adapter with HARDWARE selects the default hardware adapter. Null
    // device/context outputs validate support without retaining graphics resources.
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_FLAG(0),
            Some(levels),
            D3D11_SDK_VERSION,
            None,
            Some(&mut level),
            None,
        )?;
    }
    Ok(level)
}

pub(super) fn detect_graphics() -> GraphicsCapability {
    let modern = [
        D3D_FEATURE_LEVEL_12_0,
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_1,
        D3D_FEATURE_LEVEL_10_0,
        D3D_FEATURE_LEVEL_9_3,
    ];
    let result = hardware_feature_level(&modern).or_else(|error| {
        if error.code() != E_INVALIDARG {
            return Err(error);
        }
        // Older runtimes reject unknown feature levels. Retry legacy levels to
        // distinguish an older hardware GPU from unavailable graphics; never use WARP.
        hardware_feature_level(&modern[2..])
    });
    match result {
        Ok(level) if level.0 >= D3D_FEATURE_LEVEL_12_0.0 => GraphicsCapability::ModernHardware,
        Ok(_) => GraphicsCapability::LegacyHardware,
        Err(_) => GraphicsCapability::Unavailable,
    }
}
