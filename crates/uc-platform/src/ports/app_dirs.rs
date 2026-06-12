use uc_core::app_dirs::AppDirs;
use uc_core::ports::errors::AppDirsError;

pub trait AppDirsPort: Send + Sync {
    fn get_app_dirs(&self) -> Result<AppDirs, AppDirsError>;
}
