use crate::file_ports::LocalDeviceIdentity;
use crate::paths::DesktopPaths;

pub struct GuiClientDeps {
    pub device_id: String,
    pub storage_paths: DesktopPaths,
}

pub fn build_gui_client_context() -> anyhow::Result<GuiClientDeps> {
    let storage_paths = DesktopPaths::resolve()?;
    let device_identity =
        LocalDeviceIdentity::load_or_create(storage_paths.app_data_root_dir.clone())?;

    Ok(GuiClientDeps {
        device_id: device_identity.current_device_id().to_string(),
        storage_paths,
    })
}
